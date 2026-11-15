// The user OAuth flow and the user-token (and drops) endpoints against
// fixtures/docs/: each `actions-*` file holds the request Kick documents and
// an answer, as crates/kicksdk/tests/public_api_actions.rs reads them.
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:kicksdk/kicksdk.dart';
import 'package:test/test.dart';

final docs = Directory.current.uri.resolve('../../fixtures/docs/');

Map<String, dynamic> fixture(String name) =>
    jsonDecode(File.fromUri(docs.resolve('public_api/$name.json')).readAsStringSync()) as Map<String, dynamic>;

(int, String) answer(Map<String, dynamic> f) {
  final r = f['response'] as Map;
  return (r['status'] as int, r['body'] as String);
}

String oauth(String name) => File.fromUri(docs.resolve('oauth/$name')).readAsStringSync();

/// The request is the documented one: method, URL, query, JSON body, token.
void same(KickRequest r, Map<String, dynamic> f) {
  final want = f['request'] as Map;
  expect(r.method, want['method']);
  expect('${r.url.origin}${r.url.path}', want['url']);
  expect(r.url.queryParameters, {for (final MapEntry(:key, :value) in (want['params'] as Map).entries) key: '$value'});
  expect(r.body == null ? null : jsonDecode(r.body!), want['json']);
  expect(r.headers, contains(('authorization', 'Bearer t')));
}

void main() {
  const rewardId = '01HZ8X9K2M4N6P8Q0R2S4T6V8W0Y2Z4';
  const messageId = '828f83bb-e391-4c78-9e91-af35a172840e';

  group('OAuth', () {
    test('PKCE and state are drawn fresh, 43 characters of base64url', () {
      final a = OAuth.newPkce(), b = OAuth.newPkce();
      expect(a['verifier'], hasLength(43));
      expect(a['verifier'], isNot(b['verifier']));
      expect(OAuth.pkceFromVerifier(a['verifier'] as String), a);
      expect(OAuth.newState(), matches(RegExp(r'^[A-Za-z0-9_-]{43}$')));
      expect(() => OAuth.pkce(base64Url.encode(List.filled(16, 0))), throwsA(isA<KickException>()));
    });

    test('the challenge is the verifier\'s SHA-256, base64url without padding', () {
      final verifier = OAuth.newPkce()['verifier'] as String;
      final digest = base64Url.encode(sha256.convert(utf8.encode(verifier)).bytes).replaceAll('=', '');
      expect(OAuth.pkceFromVerifier(verifier)['challenge'], digest);
      expect(() => OAuth.pkceFromVerifier('short'), throwsA(isA<KickException>()));
    });

    test('the authorize URL carries the scopes, state and challenge', () {
      final p = OAuth.newPkce();
      final url = Uri.parse(
        OAuth.authorizeUrl('cid', 'http://localhost/cb', ['user:read', 'chat:write'], 's1', p['verifier'] as String),
      );
      expect(url.origin, 'https://id.kick.com');
      expect(url.queryParameters, containsPair('scope', 'user:read chat:write'));
      expect(url.queryParameters, containsPair('code_challenge', p['challenge']));
      expect(url.queryParameters, containsPair('state', 's1'));
      expect(OAuth.scopes(), allOf(hasLength(13), contains('chat:write')));
    });

    test('token, revoke and introspect requests and answers', () {
      final code = OAuth.codeRequest('cid', 'secret', 'http://localhost/cb', 'c0de', 'v' * 43);
      expect((code.method, code.url.toString()), ('POST', 'https://id.kick.com/oauth/token'));
      expect(Uri.splitQueryString(code.body!), containsPair('grant_type', 'authorization_code'));
      final refresh = OAuth.refreshRequest('cid', 'secret', 'r', endpoints: {'id': 'http://x'});
      expect(refresh.url.toString(), 'http://x/oauth/token');
      expect(Uri.splitQueryString(refresh.body!), containsPair('refresh_token', 'r'));
      expect(OAuth.parseUserToken(200, oauth('token-spec.json')), containsPair('expires_in', 7200));
      expect(
        () => OAuth.parseUserToken(400, oauth('error.json')),
        throwsA(isA<KickException>().having((e) => e.status, 'status', 400)),
      );

      final revoke = OAuth.revokeRequest('t', tokenTypeHint: 'refresh_token');
      expect(revoke.url.queryParameters, containsPair('token_type_hint', 'refresh_token'));
      expect(OAuth.revokeRequest('sekrit').toString(), 'POST https://id.kick.com/oauth/revoke?token=[redacted]');
      OAuth.parseRevoke(200, oauth('revoke.txt'));
      expect(() => OAuth.revokeRequest('t', tokenTypeHint: 'id_token'), throwsA(isA<KickException>()));

      expect(OAuth.introspectRequest('t').method, 'POST');
      final i = OAuth.parseIntrospect(200, oauth('introspect.json'));
      expect(i, allOf(containsPair('active', true), containsPair('scopes', ['user:read', 'channel:read'])));
    });
  });

  test('PublicApi: users, categories, stats, Kicks leaderboard', () {
    final users = fixture('v1-users');
    expect(PublicApi.usersRequest('t', userIds: [123]).url.toString(), 'https://api.kick.com/public/v1/users?id=123');
    expect(PublicApi.usersRequest('t').url.toString(), 'https://api.kick.com/public/v1/users');
    final (us, ub) = answer(users);
    expect(PublicApi.parseUsers(us, ub).single, containsPair('email', 'john.doe@example.com'));

    final cats = PublicApi.categoriesRequest('t', names: ['Rust'], limit: 5);
    expect(cats.url.queryParameters, {'name': 'Rust', 'limit': '5'});
    final (cs, cb) = answer(fixture('v2-categories'));
    final page = PublicApi.parseCategories(cs, cb);
    expect(((page['data'] as List).single as Map)['name'], 'Rust');
    expect(page['next_cursor'], isNotNull);

    expect(PublicApi.livestreamStatsRequest('t').url.path, '/public/v1/livestreams/stats');
    final (ss, sb) = answer(fixture('v1-livestreams-stats'));
    expect(PublicApi.parseLivestreamStats(ss, sb), {'total_count': 100});

    expect(PublicApi.kicksLeaderboardRequest('t', top: 1).url.query, 'top=1');
    expect(() => PublicApi.kicksLeaderboardRequest('t', top: 101), throwsA(isA<KickException>()));
    final (ks, kb) = answer(fixture('v1-kicks-leaderboard'));
    expect((PublicApi.parseKicksLeaderboard(ks, kb)['week'] as List).single, containsPair('username', 'john-doe'));

    PublicApi.parseDone(204, '');
    expect(() => PublicApi.parseDone(401, '{}'), throwsA(isA<KickException>()));
  });

  test('Chat', () {
    final f = fixture('actions-chat-send');
    same(Chat.sendRequest('t', 123, 'Pog', replyToMessageId: messageId), f);
    final (s, b) = answer(f);
    expect(Chat.parseSend(s, b), {'is_sent': true, 'message_id': messageId});
    expect(jsonDecode(Chat.sendRequest('t', null, 'hi').body!), {'content': 'hi', 'type': 'bot'});
    expect(() => Chat.sendRequest('t', null, ''), throwsA(isA<KickException>()));

    final d = fixture('actions-chat-delete');
    same(Chat.deleteRequest('t', messageId), d);
    final (ds, db) = answer(d);
    PublicApi.parseDone(ds, db);
  });

  test('Moderation', () {
    final f = fixture('actions-moderation-timeout');
    same(Moderation.timeoutRequest('t', 123, 333, 10, reason: 'Not vibing'), f);
    final (s, b) = answer(f);
    PublicApi.parseDone(s, b);
    same(Moderation.unbanRequest('t', 123, 333), fixture('actions-moderation-unban'));
    expect(jsonDecode(Moderation.banRequest('t', 123, 333).body!), {'broadcaster_user_id': 123, 'user_id': 333});
    expect(() => Moderation.timeoutRequest('t', 123, 333, 20000), throwsA(isA<KickException>()));
  });

  test('Channels', () {
    final f = fixture('actions-channels-update');
    same(Channels.updateRequest('t', categoryId: 123, customTags: ['pog', 'chad'], streamTitle: 'My first stream'), f);
    final (s, b) = answer(f);
    PublicApi.parseDone(s, b);
    expect(jsonDecode(Channels.updateRequest('t', customTags: []).body!), {'custom_tags': []});
    expect(() => Channels.updateRequest('t'), throwsA(isA<KickException>()));
  });

  test('Rewards', () {
    final list = fixture('actions-rewards-list');
    same(Rewards.listRequest('t'), list);
    final (ls, lb) = answer(list);
    expect(Rewards.parseRewards(ls, lb).single, containsPair('id', rewardId));

    for (final (name, update) in [('actions-rewards-create', false), ('actions-rewards-update', true)]) {
      final f = fixture(name);
      final fields = ((f['request'] as Map)['json'] as Map).cast<String, Object?>();
      same(update ? Rewards.updateRequest('t', rewardId, fields) : Rewards.createRequest('t', fields), f);
      final (s, b) = answer(f);
      expect(Rewards.parseReward(s, b), allOf(containsPair('id', rewardId), containsPair('title', 'Song Request')));
    }
    expect(() => Rewards.createRequest('t', {'titel': 'x'}), throwsA(isA<KickException>()));

    final del = fixture('actions-rewards-delete');
    same(Rewards.deleteRequest('t', rewardId), del);
    final (ds, db) = answer(del);
    PublicApi.parseDone(ds, db);

    final red = fixture('actions-rewards-redemptions');
    same(Rewards.redemptionsRequest('t', status: 'pending'), red);
    final (rs, rb) = answer(red);
    final page = Rewards.parseRedemptions(rs, rb);
    final group = (page['rewards'] as List).first as Map;
    expect((group['reward'] as Map)['title'], 'Unban Request');
    expect((group['redemptions'] as List).first, containsPair('user_input', 'sorry mr strimer'));
    expect(page['next_cursor'], isNotNull);
    expect(Rewards.redemptionsByIdsRequest('t', ['a', 'b'], cursor: 'c').url.query, 'id=a&id=b&cursor=c');

    const ids = ['01KEWZ2RKYZZFGD154K50Y3F8D', '01KEWZ2YC27070XWMHKFJR6C87'];
    for (final (name, r) in [
      ('actions-rewards-accept', Rewards.acceptRequest('t', ids)),
      ('actions-rewards-reject', Rewards.rejectRequest('t', ids)),
    ]) {
      final f = fixture(name);
      same(r, f);
      final (s, b) = answer(f);
      expect(Rewards.parseFailures(s, b).first, {'id': '01KERB68MZZ04551D6X5TNRFYV', 'reason': 'NOT_OWNED'});
    }
  });

  test('Ads', () {
    const uuid = '123e4567-e89b-12d3-a456-426614174000';
    final f = fixture('actions-ads-ad-break');
    same(Ads.adBreakRequest('t', uuid, 30), f);
    final (s, b) = answer(f);
    expect(
      Ads.parseAdBreak(s, b),
      allOf(containsPair('ad_break_id', 'Xq3LmP9aB2cD'), containsPair('remaining_ad_breaks', 2)),
    );
    final (rs, rb) = answer(fixture('actions-ads-ad-break-refused'));
    expect(() => Ads.parseAdBreak(rs, rb), throwsA(isA<KickException>().having((e) => e.status, 'status', 400)));

    final st = fixture('actions-ads-status');
    same(Ads.statusRequest('t'), st);
    final (ss, sb) = answer(st);
    expect(Ads.parseStatus(ss, sb), allOf(containsPair('opted_in', true), containsPair('remaining_ad_breaks', 2)));

    final en = fixture('actions-ads-enroll');
    same(Ads.enrollRequest('t'), en);
    final (es, eb) = answer(en);
    PublicApi.parseDone(es, eb);
  });

  test('Drops', () {
    final f = fixture('actions-drops-claims');
    same(
      Drops.claimsRequest(
        't',
        campaignId: '01JAXK8N4QWRTY5PM7ZEBVJDS8S',
        userId: 1234,
        limit: 10,
        cursor: '01K0TDDR08Q5ZNWXK92VRXD7GW',
      ),
      f,
    );
    final (s, b) = answer(f);
    final claims = Drops.parseClaims(s, b);
    expect(claims['claims'], hasLength(1));
    expect(claims['cursor'], '01K0TDDR08Q5ZNWXK92SDH7SDH');
    expect(Drops.claimsRequest('t').url.query, '');

    final u = fixture('actions-drops-update');
    same(
      Drops.updateClaimsRequest('t', [
        ('01KAAFHJ2PNXS48NG8XXPGWKCZ', 'processed'),
        ('01KAAFHJ2PNXS48NG8XXPGWKCA', 'initiated'),
      ]),
      u,
    );
    final (us, ub) = answer(u);
    PublicApi.parseDone(us, ub);

    final (xs, xb) = answer(fixture('actions-drops-unauthorized'));
    expect(() => Drops.parseClaims(xs, xb), throwsA(isA<KickException>().having((e) => e.status, 'status', 401)));
  });
}
