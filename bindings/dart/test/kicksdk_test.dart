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
    expect(Webhooks.verify(pem, headers, body)['event']['type'], 'followed');
    expect(() => Webhooks.verify(pem, headers, '$body '), throwsA(isA<KickException>()));
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
  });
}
