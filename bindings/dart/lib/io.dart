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

  /// Completes when the connection has ended for good: after [close], or
  /// after Kick closed it with a code that means "do not come back" (a bad
  /// token, a ban; `onDrop` had the reason).
  final Future<void> done;

  ChatConnection._(this._close, this.done);

  /// Disconnects for good. Nothing is called back after this, not even
  /// `onDrop`.
  void close() => _close();
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
    unawaited(pending.then((s) => s.close(), onError: (Object _) {}));
    throw TimeoutException('WebSocket ${Uri.parse(url).replace(query: '')}: no handshake', timeout);
  }
}

/// Connects to one channel's chat and keeps it up until [ChatConnection.close].
///
/// `onEvent` gets `{"channel", "name", "data", "event"}` for each event, the
/// typed one in `event`. `onState` gets whether the socket is connected and
/// which feeds are subscribed, whenever that changes. `onDrop` gets why a
/// connection was lost or could not be made; a reconnect follows, after a
/// jittered [Backoff], unless Kick's code says it would be refused again: then
/// the connection ends, and [ChatConnection.done] completes.
///
/// `onError` gets what the connection carried on through: an exception
/// thrown by `onEvent`, `onState` or `onDrop` (the rest of that batch of
/// frames is still handled), and a token refresh that failed (retried while
/// the connection lasts). Without `onError`, these are printed to stderr.
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
  void Function(Object error, StackTrace stack)? onError,
  Map<String, String>? endpoints,
  Duration timeout = const Duration(seconds: 10),
  Backoff? backoff,
}) {
  final feeds = channels ?? Realtime.chatChannels(chatroomId, channelId);
  final http = HttpClient()..connectionTimeout = timeout;
  final delays = backoff ?? Backoff();
  final random = Random();
  final clock = Stopwatch()..start();
  int now() => clock.elapsedMilliseconds;
  final done = Completer<void>();
  Session? session = Session(feeds);
  var closed = false;
  WebSocket? ws;
  Timer? deadline;
  Timer? retry;
  Timer? refreshRetry;
  var clientId = '';
  // For the socket in `ws`: whether onDrop has had its reason (from a close
  // action), and whether that reason rules out a reconnect.
  var dropped = false;
  var terminal = false;

  void report(Object error, StackTrace stack) {
    final handler = onError;
    if (handler != null) {
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

  void drop(String reason) {
    final handler = onDrop;
    if (!closed && handler != null) guard(() => handler(reason));
  }

  void stop() {
    if (closed) return;
    closed = true;
    deadline?.cancel();
    retry?.cancel();
    refreshRetry?.cancel();
    final socket = ws;
    ws = null;
    socket?.close();
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

  late final void Function() again;
  late final void Function() refreshToken;
  late final void Function(List<Map<String, dynamic>>) apply;

  void schedule() {
    deadline?.cancel();
    deadline = null;
    final s = session;
    if (closed || s == null) return;
    final at = s.deadline;
    if (at == null) return;
    deadline = Timer(Duration(milliseconds: max(0, at - now())), () {
      final s = session;
      if (!closed && s != null) apply(s.tick(now()));
    });
  }

  apply = (actions) {
    if (closed) return;
    var changed = false;
    try {
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
            terminal = a['reconnect'] == false;
            dropped = true;
            drop(a['reason'] as String);
            ws?.close();
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

  void lost(WebSocket socket) {
    if (ws != socket) return;
    ws = null;
    deadline?.cancel();
    refreshRetry?.cancel();
    if (!dropped) {
      final code = socket.closeCode;
      final reason = socket.closeReason ?? '';
      drop(code == null ? 'socket closed' : 'socket closed: $code${reason.isEmpty ? '' : ' $reason'}');
      terminal = code != null && !Realtime.shouldReconnect(code);
    }
    final s = session;
    if (s != null) apply(s.closed());
    state();
    if (terminal) {
      stop();
    } else {
      again();
    }
  }

  Future<void> connect() async {
    if (closed) return;
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
      drop('$e');
      again();
      return;
    }
    final s = session;
    if (closed || s == null) {
      await socket.close();
      return;
    }
    ws = socket;
    dropped = false;
    terminal = false;
    socket.listen(
      (frame) {
        final s = session;
        if (ws == socket && s != null && frame is String) apply(s.receive(frame, now()));
      },
      onError: (Object _) => lost(socket),
      onDone: () => lost(socket),
      cancelOnError: true,
    );
    apply(s.open(token, now()));
  }

  again = () {
    if (closed) return;
    retry?.cancel();
    retry = Timer(delays.nextDelayJittered(random.nextDouble()), () => connect().catchError(report));
  };

  connect().catchError(report);
  return ChatConnection._(stop, done.future);
}
