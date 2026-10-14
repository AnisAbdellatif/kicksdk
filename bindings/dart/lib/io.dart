/// [connectChat]: Kick's chat feed over `dart:io`'s HttpClient and WebSocket.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'kicksdk.dart';

export 'kicksdk.dart';

/// A live connection to one channel's chat, from [connectChat].
class ChatConnection {
  final void Function() _close;

  /// Completes once [close] has been called. Nothing else ends a connection:
  /// a terminal close from Kick (a refused token, a limit, a ban) is waited
  /// out and followed by a reconnect as a new client (docs/KICK.md §6.1).
  final Future<void> done;

  ChatConnection._(this._close, this.done);

  /// Disconnects for good. Nothing is called back after this, not even
  /// `onDrop`. Safe to call more than once, and from inside a callback.
  void close() => _close();
}

/// A connection lost or not made, for `onDropInfo` in [connectChat]. A
/// reconnect follows after [retryIn], unless the connection is closed first.
final class ChatDrop {
  /// What `onDrop` gets.
  final String reason;

  /// The close code: Kick's disconnect code, or its error code for a refused
  /// connect or refresh (0 when the session gave up on a silent socket), or
  /// the code the WebSocket closed with. Null for a failure without one: a
  /// request or the handshake failing, a network error.
  final int? code;

  /// Whether Kick would refuse the same client again right away (a refused
  /// token, a connection limit, a ban: Centrifugo's 3500-3999 and
  /// 4500-4999). The reconnect then waits 5 minutes or more, longer for
  /// each terminal close in a row, and comes back as a new client.
  final bool terminal;

  /// How long until the next attempt.
  final Duration retryIn;

  const ChatDrop(this.reason, {this.code, this.terminal = false, required this.retryIn});

  @override
  String toString() =>
      '$reason (${terminal ? 'terminal, ' : ''}retrying in ${(retryIn.inMilliseconds / 1000).toStringAsFixed(1)} s)';
}

String _uuid() {
  final r = Random.secure();
  final b = List<int>.generate(16, (_) => r.nextInt(256));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  final h = b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();
  return '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
}

/// Sends `r` and reads the whole answer within `timeout`, start to end (the
/// client's connection timeout only covers the TCP connect).
Future<(int, String)> _send(HttpClient http, KickRequest r, Duration timeout) {
  HttpClientRequest? open;
  Future<(int, String)> go() async {
    final req = open = await http.openUrl(r.method, r.url);
    for (final (k, v) in r.headers) {
      req.headers.set(k, v);
    }
    if (r.body != null) req.write(r.body);
    final res = await req.close();
    return (res.statusCode, await res.transform(utf8.decoder).join());
  }

  return go().timeout(
    timeout,
    onTimeout: () {
      open?.abort();
      throw TimeoutException('${r.method} ${r.url.replace(query: '')}: no answer', timeout);
    },
  );
}

/// Opens the WebSocket within `timeout`, handshake included. One that opens
/// after the timeout is closed straight away.
Future<WebSocket> _openSocket(String url, HttpClient http, Duration timeout) async {
  final pending = WebSocket.connect(url, headers: {'Origin': Realtime.requiredOrigin}, customClient: http);
  try {
    return await pending.timeout(timeout);
  } on TimeoutException {
    // Whatever comes of it, the handshake or the close, is of no interest.
    pending.then((s) => s.close()).ignore();
    throw TimeoutException('WebSocket ${Uri.parse(url).replace(query: '')}: no handshake', timeout);
  }
}

/// Closes `socket` without waiting: on a half-open socket, dart:io waits
/// several seconds for the other side's close. Its errors are of no interest.
void _discard(WebSocket socket) {
  try {
    socket.close().ignore();
  } catch (_) {
    // Already closing.
  }
}

/// Connects to one channel's chat and keeps it up until [ChatConnection.close].
///
/// `onEvent` gets `{"channel", "name", "data", "event"}` for each event, the
/// typed one in `event`. `onState` gets whether the socket is connected and
/// which feeds are subscribed, whenever that changes.
///
/// `onDrop` gets why a connection was lost or could not be made, and
/// `onDropInfo` the same with what comes next, as a [ChatDrop]. A reconnect
/// always follows (docs/KICK.md §6.1), after `backoff`'s wait
/// ([Backoff.nextDelayFor], jittered): 1 s doubling to 30 s for an ordinary
/// drop or failure, and 5, 10, 20, then 30 minutes for terminal closes in a
/// row (Kick refused the token or the connection, or closed with a code that
/// means it would refuse the same client again), each followed by a new
/// client id. The chat feed subscribing starts the backoff over. Only
/// [ChatConnection.close] stops it.
///
/// `onError` gets what the connection carried on through: an exception
/// thrown by `onEvent`, `onState`, `onDrop` or `onDropInfo` (the rest of
/// that batch of frames is still handled), and a token refresh that failed
/// (retried while the connection lasts). Without `onError`, or after
/// [ChatConnection.close], these are printed to stderr.
///
/// `timeout` bounds each HTTP request and the WebSocket handshake. Running
/// out is a failed attempt, retried like any other.
///
/// Sends `Origin: https://kick.com`, which Kick's realtime server requires.
ChatConnection connectChat({
  required int chatroomId,
  int? channelId,
  List<String>? channels,
  required void Function(Map<String, dynamic> event) onEvent,
  void Function(bool connected, List<String> subscribed)? onState,
  void Function(String reason)? onDrop,
  void Function(ChatDrop drop)? onDropInfo,
  void Function(Object error, StackTrace stack)? onError,
  Map<String, String>? endpoints,
  Duration timeout = const Duration(seconds: 10),
  Backoff? backoff,
}) {
  final feeds = channels ?? Realtime.chatChannels(chatroomId, channelId);
  Session? session = Session(feeds);
  final http = HttpClient()..connectionTimeout = timeout;
  final delays = backoff ?? Backoff();
  final random = Random();
  final clock = Stopwatch()..start();
  int now() => clock.elapsedMilliseconds;
  final done = Completer<void>();
  var closed = false;
  WebSocket? ws;
  StreamSubscription<dynamic>? frames;
  Timer? deadline;
  Timer? retry;
  Timer? refreshRetry;
  var clientId = '';

  // After close(), nothing is called back, onError included: what still
  // goes wrong (a callback that closed the connection, then threw) goes to
  // stderr.
  void report(Object error, StackTrace stack) {
    final handler = onError;
    if (!closed && handler != null) {
      try {
        handler(error, stack);
        return;
      } catch (e, s) {
        stderr.writeln('kicksdk: onError threw: $e\n$s');
      }
    }
    stderr.writeln('kicksdk: $error\n$stack');
  }

  // Runs a user callback. What it throws is reported, never thrown into the
  // middle of a batch of actions.
  void guard(void Function() callback) {
    try {
      callback();
    } catch (e, s) {
      report(e, s);
    }
  }

  // Lets go of the socket in `ws`, if any, at once: its frames are no longer
  // read, and its close is not waited for.
  WebSocket? detach() {
    final socket = ws;
    ws = null;
    frames?.cancel().ignore();
    frames = null;
    deadline?.cancel();
    refreshRetry?.cancel();
    if (socket != null) _discard(socket);
    return socket;
  }

  void stop() {
    if (closed) return;
    closed = true;
    retry?.cancel();
    detach();
    session?.dispose();
    session = null;
    http.close(force: true);
    done.complete();
  }

  void state() {
    final s = session;
    final handler = onState;
    if (closed || handler == null || s == null) return;
    final connected = s.isConnected;
    final subscribed = [
      for (final f in feeds)
        if (s.isSubscribed(f)) f,
    ];
    guard(() => handler(connected, subscribed));
  }

  late final void Function() refreshToken;
  late final void Function(List<Map<String, dynamic>>) apply;
  late final Future<void> Function() connect;

  void schedule() {
    deadline?.cancel();
    deadline = null;
    final s = session;
    if (closed || s == null || ws == null) return;
    final at = s.deadline;
    if (at == null) return;
    deadline = Timer(Duration(milliseconds: max(0, at - now())), () {
      final s = session;
      if (!closed && s != null) apply(s.tick(now()));
    });
  }

  // The connection in `ws` is lost, or none could be made: report it, and
  // reconnect after the backoff's wait for `retryAfterMs` (KICK.md §6.1).
  // connect() makes a new client id for every connection, which a terminal
  // close needs.
  void lost(String reason, {int? code, bool terminal = false, int retryAfterMs = 0}) {
    final hadSocket = detach() != null;
    if (closed) return;
    // The feeds it had come back unsubscribed, which reports the state.
    final lostFeeds = hadSocket ? session?.closed() ?? const [] : const <Map<String, dynamic>>[];
    final wait = delays.nextDelayFor(retryAfterMs, random.nextDouble());
    final info = ChatDrop(reason, code: code, terminal: terminal, retryIn: wait);
    final handler = onDrop;
    if (handler != null) guard(() => handler(reason));
    final infoHandler = onDropInfo;
    if (!closed && infoHandler != null) guard(() => infoHandler(info));
    if (closed) return;
    if (lostFeeds.isNotEmpty) {
      apply(lostFeeds);
    } else if (hadSocket) {
      state();
    }
    if (closed) return;
    retry?.cancel();
    retry = Timer(wait, () => connect().catchError(report));
  }

  // The socket closed by itself (no close action first): its close code
  // says whether that was terminal.
  void ended(WebSocket socket, [Object? error]) {
    if (ws != socket) return;
    final code = socket.closeCode;
    if (code == null) {
      lost(error == null ? 'socket closed' : 'socket failed: $error');
      return;
    }
    final text = socket.closeReason ?? '';
    var retryAfterMs = 0;
    try {
      retryAfterMs = Realtime.retryAfterMs(code);
    } catch (e, s) {
      report(e, s);
    }
    lost(
      'socket closed: $code${text.isEmpty ? '' : ' $text'}',
      code: code,
      terminal: retryAfterMs > 0,
      retryAfterMs: retryAfterMs,
    );
  }

  apply = (actions) {
    if (closed) return;
    var changed = false;
    try {
      batch:
      for (final a in actions) {
        if (closed) return;
        switch (a['action']) {
          case 'send':
            try {
              ws?.add(a['text'] as String);
            } on StateError {
              // The socket is closing; its onDone takes it from here.
            }
          case 'close':
            // Treated as lost at once: dart:io would hold the socket's
            // onDone for seconds on a half-open connection. What the batch
            // has after it was for that connection.
            changed = false;
            lost(
              a['reason'] as String,
              code: a['code'] as int,
              terminal: a['terminal'] == true,
              retryAfterMs: a['retry_after_ms'] as int,
            );
            break batch;
          case 'refresh_token':
            refreshToken();
          case 'subscribed':
            if (a['channel'] == feeds.first) delays.reset();
            changed = true;
          case 'connected' || 'unsubscribed' || 'subscribe_failed':
            changed = true;
          case 'event':
            guard(() => onEvent(a));
        }
      }
      if (changed) state();
    } finally {
      // Always, so that a batch cut short cannot leave the session without
      // its next tick (silence, token refresh).
      schedule();
    }
  };

  // Fetches a new token for the socket now open. A failure is reported and
  // tried again, with a backoff, while that socket lasts: if it never works,
  // Kick ends the connection when the old token runs out, and the reconnect
  // gets a new one.
  refreshToken = () {
    final socket = ws;
    if (socket == null) return;
    final id = clientId;
    final wait = Backoff();
    Future<void> attempt() async {
      try {
        final r = await _send(http, Realtime.tokenRequest(id, endpoints: endpoints), timeout);
        final token = Realtime.parseToken(r.$1, r.$2);
        final s = session;
        if (!closed && ws == socket && s != null) apply(s.refresh(token, now()));
      } catch (e, st) {
        if (closed || ws != socket) return;
        report(e, st);
        refreshRetry?.cancel();
        refreshRetry = Timer(wait.nextDelayJittered(random.nextDouble()), attempt);
      }
    }

    attempt();
  };

  connect = () async {
    if (closed) return;
    // A new client, so a new guest token, for every connection: after a
    // terminal close, Kick would refuse the old one.
    final id = clientId = _uuid();
    final String token;
    final WebSocket socket;
    try {
      final where = channelId != null
          ? Realtime.chatConnectionRequest(channelId, id, endpoints: endpoints)
          : Realtime.connectionRequest(id, endpoints: endpoints);
      final c = await _send(http, where, timeout);
      final found = Realtime.parseConnection(c.$1, c.$2).firstWhere(
        (x) => x['provider'] == 'centrifugo',
        orElse: () => throw StateError('Kick offered no Centrifugo server'),
      );
      if (closed) return;
      final t = await _send(http, Realtime.tokenRequest(id, endpoints: endpoints), timeout);
      token = Realtime.parseToken(t.$1, t.$2);
      if (closed) return;
      socket = await _openSocket(found['url'] as String, http, timeout);
    } catch (e) {
      // After close(), a request fails because the client was shut: nothing
      // to report then.
      if (closed) return;
      lost('$e');
      return;
    }
    final s = session;
    if (closed || s == null) {
      _discard(socket);
      return;
    }
    ws = socket;
    frames = socket.listen(
      (frame) {
        final s = session;
        if (ws == socket && s != null && frame is String) apply(s.receive(frame, now()));
      },
      onError: (Object e) => ended(socket, e),
      onDone: () => ended(socket),
      cancelOnError: true,
    );
    apply(s.open(token, now()));
  };

  connect().catchError(report);
  return ChatConnection._(stop, done.future);
}
