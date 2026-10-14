// connectChat against a fake Kick on localhost: timeouts, close(), user
// callbacks that throw, token refresh failures, and terminal closes, which
// reconnect after a long wait (shortened here through the backoff).
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

  /// The client id of each token request, in order.
  final clientIds = <String>[];

  /// Close each new socket with this code as soon as it connects, before
  /// any subscribe is answered.
  int? closeOnConnect;

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
        final body = jsonDecode(await utf8.decoder.bind(req).join()) as Map<String, dynamic>;
        clientIds.add(body['client_id'] as String);
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
      if (m.containsKey('connect')) {
        final code = closeOnConnect;
        if (code != null) {
          ws.close(code, 'refused');
          return;
        }
        ws.add('{"id":$id,"connect":$connectReply}');
      }
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

/// Waits 50 ms whatever the real backoff says, and keeps what it said.
class QuickBackoff extends Backoff {
  /// `retryAfterMs` of each call, and the wait the real backoff gave for it.
  final asked = <int>[];
  final waits = <Duration>[];
  var resets = 0;

  @override
  Duration nextDelayFor(int retryAfterMs, double random) {
    asked.add(retryAfterMs);
    waits.add(super.nextDelayFor(retryAfterMs, random));
    return const Duration(milliseconds: 50);
  }

  @override
  void reset() {
    resets++;
    super.reset();
  }
}

Matcher within(int least) => inInclusiveRange(least, least + least ~/ 4);

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
  final infos = <ChatDrop>[];
  final errors = <Object>[];
  final events = <String>[];
  var subscribed = false;
  late ChatConnection chat_;
  late QuickBackoff backoff;

  setUp(() async {
    kick = await FakeKick.start();
    drops.clear();
    infos.clear();
    errors.clear();
    backoff = QuickBackoff();
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
    backoff: backoff,
    onEvent: onEvent ?? (e) => events.add(e['event']['content'] as String),
    onState:
        onState ??
        (connected, subs) {
          subscribed = subs.contains(feed);
        },
    onDrop: drops.add,
    onDropInfo: infos.add,
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

  test('a terminal disconnect waits long, then reconnects as a new client', () async {
    final c = connect();
    var ended = false;
    c.done.then((_) => ended = true);
    await until(() => subscribed);
    expect(backoff.resets, 1);
    // Kick's side stays open: the driver lets go of the socket at once.
    kick.sockets.single.add('{"push":{"disconnect":{"code":3501,"reason":"bad request"}}}');
    await until(() => kick.connections == 2 && subscribed, const Duration(seconds: 2));
    expect(drops.single, contains('3501'));
    expect(infos.single.code, 3501);
    expect(infos.single.terminal, isTrue);
    expect(infos.single.retryIn, const Duration(milliseconds: 50));
    expect(backoff.asked, [300000]);
    expect(backoff.waits.single.inMilliseconds, within(300000));
    expect(kick.clientIds, hasLength(2));
    expect(kick.clientIds.last, isNot(kick.clientIds.first));
    expect(ended, isFalse);
    expect(errors, isEmpty);
    c.close();
    await c.done;
  });

  test('socket close codes: 3000 reconnects soon, 4501 after the long wait', () async {
    final c = connect();
    var ended = false;
    c.done.then((_) => ended = true);
    await until(() => subscribed);
    await kick.sockets.single.close(3000, 'later');
    await until(() => kick.sockets.length == 2 && subscribed);
    expect(drops.single, 'socket closed: 3000 later');
    expect((infos.single.code, infos.single.terminal), (3000, false));
    await kick.sockets.last.close(4501, 'banned');
    await until(() => kick.sockets.length == 3 && subscribed);
    expect(drops.last, 'socket closed: 4501 banned');
    expect((infos.last.code, infos.last.terminal), (4501, true));
    expect(backoff.asked, [0, 300000]);
    expect(backoff.waits.last.inMilliseconds, within(300000));
    expect(kick.connections, 3);
    expect(ended, isFalse);
  });

  test('terminal closes in a row wait longer; the chat feed subscribing starts over', () async {
    kick.closeOnConnect = 4500;
    connect();
    await until(() => backoff.asked.length >= 4);
    kick.closeOnConnect = null;
    await until(() => subscribed);
    expect(backoff.asked.take(4), everyElement(300000));
    expect(
      [for (final w in backoff.waits.take(4)) w.inMilliseconds],
      [within(300000), within(600000), within(1200000), within(1800000)],
    );
    expect(infos.take(4).every((d) => d.terminal && d.code == 4500), isTrue);
    expect(kick.clientIds.toSet(), hasLength(kick.clientIds.length));

    final before = backoff.asked.length;
    await kick.sockets.last.close(4500, 'again');
    await until(() => backoff.asked.length > before);
    expect(backoff.waits.last.inMilliseconds, within(300000));
  });

  test('a callback that closes the connection, then throws, goes to stderr, not onError', () async {
    final printed = _Lines();
    await IOOverrides.runZoned(() async {
      final c = connect(
        onEvent: (e) {
          chat_.close();
          throw StateError('after close');
        },
      );
      await until(() => subscribed);
      kick.sockets.single.add(chat('one'));
      await c.done;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }, stderr: () => printed);
    expect(errors, isEmpty);
    expect(drops, isEmpty);
    expect(printed.lines.single, startsWith('kicksdk: Bad state: after close'));
  });
}

/// A stderr that keeps what is written.
class _Lines implements Stdout {
  final lines = <String>[];

  @override
  void writeln([Object? object = '']) => lines.add('$object');

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
