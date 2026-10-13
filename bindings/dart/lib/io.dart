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
  ChatConnection._(this._close);

  /// Disconnects for good.
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

Future<(int, String)> _send(HttpClient http, KickRequest r) async {
  final req = await http.openUrl(r.method, r.url);
  for (final (k, v) in r.headers) {
    req.headers.set(k, v);
  }
  if (r.body != null) req.write(r.body);
  final res = await req.close();
  return (res.statusCode, await res.transform(utf8.decoder).join());
}

/// Connects to one channel's chat and keeps it up until [ChatConnection.close].
///
/// `onEvent` gets `{"channel", "name", "data", "event"}` for each event, the
/// typed one in `event`. Sends `Origin: https://kick.com`, which Kick's
/// realtime server requires.
ChatConnection connectChat({
  required int chatroomId,
  int? channelId,
  List<String>? channels,
  required void Function(Map<String, dynamic> event) onEvent,
  void Function(bool connected, List<String> subscribed)? onState,
  void Function(String reason)? onDrop,
  Map<String, String>? endpoints,
}) {
  final feeds = channels ?? Realtime.chatChannels(chatroomId, channelId);
  final http = HttpClient();
  final backoff = Backoff();
  final clock = Stopwatch()..start();
  int now() => clock.elapsedMilliseconds;
  var closed = false;
  WebSocket? ws;
  Session? session;
  Timer? timer;
  Timer? retry;
  late String clientId;

  void state() {
    final s = session;
    if (onState == null || s == null) return;
    onState(s.isConnected, [
      for (final f in feeds)
        if (s.isSubscribed(f)) f,
    ]);
  }

  late void Function() again;

  void apply(List<Map<String, dynamic>> actions) {
    final s = session;
    if (closed || s == null) return;
    var changed = false;
    for (final a in actions) {
      switch (a['action']) {
        case 'send':
          ws?.add(a['text'] as String);
        case 'close':
          onDrop?.call(a['reason'] as String);
          ws?.close();
        case 'refresh_token':
          _send(http, Realtime.tokenRequest(clientId, endpoints: endpoints))
              .then((r) => Realtime.parseToken(r.$1, r.$2))
              .then((t) {
                if (!closed && session != null) apply(session!.refresh(t, now()));
              })
              .catchError((_) {});
        case 'subscribed':
          if (a['channel'] == feeds.first) backoff.reset();
          changed = true;
        case 'connected' || 'unsubscribed' || 'subscribe_failed':
          changed = true;
        case 'event':
          onEvent(a);
      }
    }
    if (changed) state();
    timer?.cancel();
    final at = s.deadline;
    if (at != null) {
      timer = Timer(Duration(milliseconds: max(0, at - now())), () {
        if (session != null) apply(session!.tick(now()));
      });
    }
  }

  Future<void> connect() async {
    if (closed) return;
    clientId = _uuid();
    final String url;
    final String token;
    try {
      final where = channelId != null
          ? Realtime.chatConnectionRequest(channelId, clientId, endpoints: endpoints)
          : Realtime.connectionRequest(clientId, endpoints: endpoints);
      final c = await _send(http, where);
      final found = Realtime.parseConnection(c.$1, c.$2).firstWhere(
        (x) => x['provider'] == 'centrifugo',
        orElse: () => throw StateError('Kick offered no Centrifugo server'),
      );
      url = found['url'] as String;
      final t = await _send(http, Realtime.tokenRequest(clientId, endpoints: endpoints));
      token = Realtime.parseToken(t.$1, t.$2);
    } catch (e) {
      onDrop?.call('$e');
      again();
      return;
    }
    if (closed) return;
    session ??= Session(feeds);
    final WebSocket socket;
    try {
      socket = await WebSocket.connect(url, headers: {'Origin': Realtime.requiredOrigin});
    } catch (e) {
      onDrop?.call('$e');
      again();
      return;
    }
    if (closed) {
      await socket.close();
      return;
    }
    ws = socket;
    apply(session!.open(token, now()));
    socket.listen(
      (frame) {
        if (frame is String) apply(session!.receive(frame, now()));
      },
      onDone: () {
        if (ws != socket) return;
        ws = null;
        timer?.cancel();
        if (session != null) apply(session!.closed());
        state();
        again();
      },
      cancelOnError: true,
    );
  }

  again = () {
    if (closed) return;
    retry?.cancel();
    retry = Timer(backoff.next(), connect);
  };

  connect();
  return ChatConnection._(() {
    closed = true;
    timer?.cancel();
    retry?.cancel();
    ws?.close();
    ws = null;
    session?.dispose();
    session = null;
    http.close(force: true);
  });
}
