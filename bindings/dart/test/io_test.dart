// connectChat against a fake Kick on localhost: timeouts, close(), user
// callbacks that throw, token refresh failures and terminal closes.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:kicksdk/io.dart';
import 'package:test/test.dart';

const feed = 'chatrooms.1.v2';

/// Kick's realtime endpoints and Centrifugo server, scriptable.
class FakeKick {
  late final HttpServer _server;
  var connections = 0;
  var tokens = 0;
  final sockets = <WebSocket>[];
  final received = <String>[];

  /// Never answer the connection request / the WebSocket upgrade.
  var silentHttp = false;
  var silentSocket = false;

  /// Whether the n-th token request (from 1) fails with a 500.
  bool Function(int n) tokenFails = (_) => false;
  String connectReply = '{"client":"c","ping":25,"pong":true}';

  Map<String, String> get endpoints => {'realtime': 'http://127.0.0.1:${_server.port}'};
  String get _ws => 'ws://127.0.0.1:${_server.port}/ws';

  static Future<FakeKick> start() async {
    final k = FakeKick();
    k._server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    k._server.listen(k._handle);
    return k;
  }

  Future<void> stop() => _server.close(force: true);

  Future<void> _handle(HttpRequest req) async {
    switch (req.uri.path) {
      case '/connection':
        connections++;
        if (silentHttp) return; // held open, unanswered
        req.response.write(
          jsonEncode({
            'data': {
              'connections': [
                {
                  'provider': 'centrifugo',
                  'credentials': {'url': _ws},
                },
              ],
            },
          }),
        );
      case '/auth/connection':
        tokens++;
        if (tokenFails(tokens)) {
          req.response.statusCode = 500;
          req.response.write('{}');
        } else {
          req.response.write('{"data":{"token":"t$tokens"}}');
        }
      case '/ws':
        if (silentSocket) return;
        final ws = await WebSocketTransformer.upgrade(req);
        sockets.add(ws);
        ws.listen((f) => _frame(ws, f as String), onError: (_) {});
        return;
      default:
        req.response.statusCode = 404;
    }
    await req.response.close();
  }

  void _frame(WebSocket ws, String frame) {
    received.add(frame);
    for (final line in frame.split('\n')) {
      if (line == '{}') continue;
      final m = jsonDecode(line) as Map<String, dynamic>;
      final id = m['id'];
      if (m.containsKey('connect')) ws.add('{"id":$id,"connect":$connectReply}');
      if (m.containsKey('subscribe')) ws.add('{"id":$id,"subscribe":{}}');
      if (m.containsKey('refresh')) ws.add('{"id":$id,"refresh":{"expires":true,"ttl":3600}}');
    }
  }
}

String chat(String content) => jsonEncode({
  'push': {
    'channel': feed,
    'pub': {
      'data': {
        'event': r'App\Events\ChatMessageEvent',
        'data': jsonEncode({
          'id': content,
          'chatroom_id': 1,
          'content': content,
          'type': 'message',
          'sender': {'id': 2, 'username': 'somechannel', 'slug': 'somechannel'},
        }),
      },
    },
  },
});

Future<void> until(bool Function() ok, [Duration within = const Duration(seconds: 10)]) async {
  final end = DateTime.now().add(within);
  while (!ok()) {
    if (DateTime.now().isAfter(end)) fail('timed out waiting');
    await Future<void>.delayed(const Duration(milliseconds: 10));
  }
}

void main() {
  late FakeKick kick;
  final drops = <String>[];
  final errors = <Object>[];
  final events = <String>[];
  var subscribed = false;
  late ChatConnection chat_;

  setUp(() async {
    kick = await FakeKick.start();
    drops.clear();
    errors.clear();
    events.clear();
    subscribed = false;
  });
  tearDown(() async {
    chat_.close();
    await kick.stop();
  });

  ChatConnection connect({
    void Function(Map<String, dynamic>)? onEvent,
    void Function(bool, List<String>)? onState,
    Duration timeout = const Duration(seconds: 5),
  }) => chat_ = connectChat(
    chatroomId: 1,
    channels: [feed],
    endpoints: kick.endpoints,
    timeout: timeout,
    backoff: Backoff(initial: const Duration(milliseconds: 50), max: const Duration(milliseconds: 50)),
    onEvent: onEvent ?? (e) => events.add(e['event']['content'] as String),
    onState:
        onState ??
        (connected, subs) {
          subscribed = subs.contains(feed);
        },
    onDrop: drops.add,
    onError: (e, _) => errors.add(e),
  );

  test('a request with no answer times out, and is retried', () async {
    kick.silentHttp = true;
    connect(timeout: const Duration(milliseconds: 200));
    await until(() => drops.length >= 2);
    expect(drops.first, contains('TimeoutException'));
    expect(kick.connections, greaterThanOrEqualTo(2));
  });

  test('a WebSocket handshake with no answer times out, and is retried', () async {
    kick.silentSocket = true;
    connect(timeout: const Duration(milliseconds: 200));
    await until(() => drops.length >= 2);
    expect(drops.first, contains('no handshake'));
  });

  test('nothing is called back after close()', () async {
    kick.silentHttp = true;
    final c = connect(timeout: const Duration(milliseconds: 300));
    await until(() => kick.connections == 1);
    c.close();
    await c.done;
    await Future<void>.delayed(const Duration(milliseconds: 600));
    expect(drops, isEmpty);
    expect(kick.connections, 1);

    kick.silentHttp = false;
    final d = connect();
    await until(() => subscribed);
    d.close();
    await Future<void>.delayed(const Duration(milliseconds: 200));
    expect(drops, isEmpty);
    expect(errors, isEmpty);
  });

  test('a callback that throws is reported, and the batch goes on', () async {
    var states = 0;
    connect(
      onEvent: (e) {
        events.add(e['event']['content'] as String);
        throw StateError('user bug ${events.length}');
      },
      onState: (connected, subs) {
        states++;
        subscribed = subs.contains(feed);
        throw StateError('state bug');
      },
    );
    await until(() => subscribed);
    final before = kick.received.length;
    // One frame: two events, then the server's ping, which wants a pong.
    kick.sockets.single.add('${chat('one')}\n${chat('two')}\n{}');
    await until(() => kick.received.length > before);
    expect(events, ['one', 'two']);
    expect(kick.received.last, '{}');
    expect(
      errors.map((e) => '$e'),
      containsAll(['Bad state: user bug 1', 'Bad state: user bug 2', 'Bad state: state bug']),
    );
    expect(states, greaterThanOrEqualTo(2));
    expect(drops, isEmpty);
  });

  test('a failed token refresh is reported and retried; the connection stays', () async {
    // A 2 s token: the session asks for a new one after 1 s.
    kick.connectReply = '{"client":"c","ping":25,"pong":true,"expires":true,"ttl":2}';
    kick.tokenFails = (n) => n == 2 || n == 3;
    connect();
    await until(() => kick.received.any((f) => f.contains('"refresh"')));
    expect(errors, hasLength(2));
    expect(errors.first, isA<KickException>().having((e) => e.status, 'status', 500));
    expect(kick.tokens, 4);
    expect(kick.received.last, contains('"token":"t4"'));
    expect(kick.connections, 1);
    expect(drops, isEmpty);
  });

  test('a terminal disconnect from Kick ends the connection', () async {
    final c = connect();
    await until(() => subscribed);
    kick.sockets.single.add('{"push":{"disconnect":{"code":3501,"reason":"bad request"}}}');
    await c.done;
    expect(drops.single, contains('3501'));
    expect(subscribed, isFalse);
    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(kick.connections, 1);
  });

  test('a socket closed with a terminal code ends it; another code reconnects', () async {
    final c = connect();
    await until(() => subscribed);
    await kick.sockets.single.close(3000, 'later');
    await until(() => kick.sockets.length == 2 && subscribed);
    expect(drops.single, 'socket closed: 3000 later');
    await kick.sockets.last.close(4501, 'banned');
    await c.done;
    expect(drops.last, 'socket closed: 4501 banned');
    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(kick.connections, 2);
  });
}
