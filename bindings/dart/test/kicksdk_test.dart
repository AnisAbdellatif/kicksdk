// The Dart package against the repository's shared fixtures.
import 'dart:convert';
import 'dart:io';

import 'package:kicksdk/kicksdk.dart';
import 'package:test/test.dart';

final fixtures = Directory.current.uri.resolve('../../fixtures/');

List<File> files(String dir, [String part = '']) => Directory.fromUri(
  fixtures.resolve('$dir/'),
).listSync().whereType<File>().where((f) => f.path.contains(part)).toList()..sort((a, b) => a.path.compareTo(b.path));

(int, String) response(File f) {
  final r = (jsonDecode(f.readAsStringSync()) as Map)['response'] as Map;
  return (r['status'] as int, r['body'] as String);
}

void main() {
  test('events are typed; unknown ones kept by name', () {
    final host = Events.parse(r'App\Events\StreamHostEvent', '{"host_username":"A","number_viewers":3}');
    expect(host, containsPair('type', 'host'));
    expect(host['viewers'], 3);
    expect(Events.parse('New', '{}'), {'type': 'unknown', 'name': 'New'});
    expect(Content.parse('[emote:1:X] [emote:1:X]', combine: true).single['count'], 2);
  });

  test('requests carry the origin; endpoints can be overridden', () {
    final r = Realtime.chatConnectionRequest(668, 'c1', endpoints: {'realtime': 'http://x/rt'});
    expect(r.method, 'POST');
    expect(r.url.toString(), 'http://x/rt/channels/668/chat/connection');
    expect(r.headers, contains(('origin', 'https://kick.com')));
    expect(PublicApi.livestreamsRequests('t', List.generate(120, (i) => i + 1)), hasLength(3));
  });

  test('errors carry the kind and Kick\'s status', () {
    expect(
      () => Website.parseChannel(404, '{}'),
      throwsA(isA<KickException>().having((e) => e.status, 'status', 404).having((e) => e.kind, 'kind', 'status')),
    );
  });

  test('website channels and livestreams parse', () {
    for (final f in files('v2')) {
      final (s, b) = response(f);
      expect(Website.parseChannel(s, b)['chatroom_id'], greaterThan(0));
    }
    for (final f in files('public_api', 'livestreams-poll')) {
      final (s, b) = response(f);
      for (final l in PublicApi.parseLivestreams(s, b)) {
        expect(l['started_at'], endsWith('Z'));
      }
    }
  });

  test('webhook signatures verify', () {
    final dir = fixtures.resolve('signing/');
    final pem = File.fromUri(dir.resolve('test-public-key.pem')).readAsStringSync();
    final body = File.fromUri(dir.resolve('body.json')).readAsStringSync();
    final headers = [
      ('kick-event-message-id', '01M38M9K3YRKV8E6Q68C18ZWG4'),
      ('kick-event-message-timestamp', '2026-09-24T02:35:42Z'),
      ('kick-event-signature', File.fromUri(dir.resolve('signature.b64')).readAsStringSync().trim()),
      ('kick-event-type', 'channel.followed'),
      ('kick-event-version', '1'),
    ];
    final sent = DateTime.utc(2026, 9, 24, 2, 35, 42);
    final now = sent.add(const Duration(seconds: 100));
    expect(Webhooks.verify(pem, headers, body, now: now)['event']['type'], 'followed');
    expect(Webhooks.verify(pem, headers, utf8.encode(body), now: now)['event']['type'], 'followed');
    expect(Webhooks.verify(pem, headers, body, skipFreshness: true)['event']['type'], 'followed');
    final refused = throwsA(isA<KickException>().having((e) => e.kind, 'kind', 'webhook'));
    expect(() => Webhooks.verify(pem, headers, '$body ', now: now), refused);
    expect(() => Webhooks.verify(pem, headers, utf8.encode('$body '), now: now), refused);
    // Stale: an hour later, or past a tighter tolerance; and the default
    // `now` is this machine's clock, long after the fixture was signed.
    expect(() => Webhooks.verify(pem, headers, body, now: sent.add(const Duration(hours: 1))), refused);
    expect(() => Webhooks.verify(pem, headers, body, now: now, tolerance: const Duration(seconds: 10)), refused);
    expect(() => Webhooks.verify(pem, headers, body), refused);
    expect(() => Webhooks.verify(pem, headers, 42, now: now), throwsArgumentError);
    expect(Webhooks.parse('channel.followed', utf8.encode(body))['type'], 'followed');
    expect(
      () => Webhooks.parse('kicks.gifted', body),
      throwsA(isA<KickException>().having((e) => e.kind, 'kind', 'shape')),
    );
  });

  test('a recorded realtime session replays', () {
    for (final f in files('realtime', '.jsonl')) {
      final records = [for (final l in f.readAsLinesSync()) jsonDecode(l) as Map<String, dynamic>];
      final sent = [
        for (final r in records)
          if (r['direction'] == 'out') r['frame'] as String,
      ];
      final channels = [
        for (final frame in sent)
          for (final line in frame.split('\n'))
            if ((jsonDecode(line) as Map)['subscribe'] case {'channel': final String c}) c,
      ];
      final s = Session(channels);
      final ours = [for (final a in s.open('[redacted]', 0)) a['text'] as String];
      var chat = 0;
      for (final r in records.where((r) => r['direction'] == 'in')) {
        for (final a in s.receive(r['frame'] as String, r['ms'] as int)) {
          if (a['action'] == 'send') ours.add(a['text'] as String);
          if (a['action'] == 'event' && a['event']['type'] == 'chat_message') chat++;
        }
      }
      expect(ours, sent);
      expect(chat, greaterThan(0));
      expect(s.isSubscribed(channels.first), isTrue);
      s.dispose();
    }
  });

  test('backoff doubles to the cap', () {
    final b = Backoff();
    expect([for (var i = 0; i < 7; i++) b.next().inSeconds], [1, 2, 4, 8, 16, 30, 30]);
    b.reset();
    expect(b.next(), const Duration(seconds: 1));
    final zero = Backoff(initial: Duration.zero, max: Duration.zero);
    expect([zero.next(), zero.next()], [const Duration(milliseconds: 1), const Duration(milliseconds: 1)]);
    expect(Backoff(initial: const Duration(seconds: 5), max: const Duration(seconds: 1)).max.inSeconds, 5);
  });

  test('jittered backoff spreads each delay over its second half', () {
    final b = Backoff();
    expect(b.nextDelayJittered(0).inMilliseconds, 500);
    expect(b.nextDelayJittered(0.5).inMilliseconds, 1500);
    expect(b.nextDelayJittered(0.999999).inMilliseconds, 3999);
    expect(b.nextDelayJittered(double.nan).inMilliseconds, 4000);
    expect(b.nextDelayJittered(7).inMilliseconds, 16000);
    expect(b.nextDelayJittered(-1).inMilliseconds, 15000);
    expect(Backoff(initial: const Duration(milliseconds: 1)).nextDelayJittered(0).inMilliseconds, 1);
  });

  test('close codes: terminal ones are not worth a reconnect', () {
    expect(Realtime.shouldReconnect(1006), isTrue);
    expect(Realtime.shouldReconnect(3000), isTrue);
    expect(Realtime.shouldReconnect(3501), isFalse);
    expect(Realtime.shouldReconnect(4499), isTrue);
    expect(Realtime.shouldReconnect(4500), isFalse);
    expect(() => Realtime.shouldReconnect(70000), throwsA(isA<KickException>().having((e) => e.kind, 'kind', 'call')));
  });

  test('arguments JSON cannot hold are refused before the call', () {
    expect(() => Events.parse('x', Object()), throwsA(isA<JsonUnsupportedObjectError>()));
    expect(() => Website.historyRequest(-1), throwsA(isA<KickException>().having((e) => e.kind, 'kind', 'call')));
  });
}
