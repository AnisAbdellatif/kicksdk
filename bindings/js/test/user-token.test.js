// User tokens (OAuth with PKCE) and the official API's user-token calls,
// against the fixtures transcribed from Kick's docs (fixtures/docs/): each
// builder makes the documented request, each parser reads the documented
// answer - the same files crates/kicksdk/tests/public_api_actions.rs reads.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { sdk, FIXTURES } from './load.js';

const docs = (name) => JSON.parse(readFileSync(path.join(FIXTURES, 'docs', name), 'utf8'));
const oauthFile = (name) => readFileSync(path.join(FIXTURES, 'docs/oauth', name), 'utf8');

/** A fixture's request, and its answer as a parser takes it. */
const fixture = (name) => {
  const f = docs(`public_api/${name}.json`);
  return { request: f.request, status: f.response.status, body: f.response.body };
};

/** The request is the one the fixture documents: method, URL, query, JSON body, bearer token. */
const same = (r, f) => {
  assert.equal(r.method, f.request.method);
  const url = new URL(r.url);
  assert.equal(url.origin + url.pathname, f.request.url);
  const params = {};
  for (const k of new Set(url.searchParams.keys())) params[k] = url.searchParams.getAll(k);
  const wanted = {};
  for (const [k, v] of Object.entries(f.request.params ?? {})) wanted[k] = [v].flat().map(String);
  assert.deepEqual(params, wanted, r.url);
  assert.deepEqual(r.body === null ? null : JSON.parse(r.body), f.request.json ?? null, r.url);
  assert.deepEqual(
    r.headers.find(([k]) => k === 'authorization'),
    ['authorization', 'Bearer t']
  );
};

const done = (f) => assert.equal(sdk.parseDone(f.status, f.body), undefined);

test('PKCE pairs come from random bytes or a kept verifier', () => {
  const p = sdk.oauthPkce(new Uint8Array(32));
  assert.equal(p.verifier, 'A'.repeat(43));
  assert.deepEqual(sdk.oauthPkceFromVerifier(p.verifier), p);
  // S256: base64url of the verifier's SHA-256, as node:crypto has it.
  const v = sdk.newPkce().verifier;
  assert.equal(sdk.oauthPkceFromVerifier(v).challenge, createHash('sha256').update(v).digest('base64url'));
  assert.throws(() => sdk.oauthPkce(new Uint8Array(31)), /32 to 96/);
  assert.throws(() => sdk.oauthPkce(new Uint8Array(97)), /32 to 96/);
  assert.throws(() => sdk.oauthPkce([1, 2, 3]), TypeError);
  assert.throws(() => sdk.oauthPkce('x'.repeat(43)), TypeError);
  assert.throws(() => sdk.oauthPkceFromVerifier('short'), /43 to 128/);
});

test('newPkce and newState draw from the CSPRNG', () => {
  const a = sdk.newPkce();
  const b = sdk.newPkce();
  assert.match(a.verifier, /^[A-Za-z0-9_-]{43}$/);
  assert.notEqual(a.verifier, b.verifier);
  assert.deepEqual(sdk.oauthPkceFromVerifier(a.verifier), a);
  const s = sdk.newState();
  assert.match(s, /^[A-Za-z0-9_-]{43}$/);
  assert.notEqual(s, sdk.newState());
});

test('the authorize URL carries the challenge, the scopes and the state', () => {
  const p = sdk.oauthPkce(new Uint8Array(32));
  const url = new URL(
    sdk.oauthAuthorizeUrl('client', 'https://app.example/cb', ['user:read', 'chat:write'], 'st', p.verifier)
  );
  assert.equal(url.origin + url.pathname, 'https://id.kick.com/oauth/authorize');
  assert.deepEqual(Object.fromEntries(url.searchParams), {
    response_type: 'code',
    client_id: 'client',
    redirect_uri: 'https://app.example/cb',
    scope: 'user:read chat:write',
    code_challenge: p.challenge,
    code_challenge_method: 'S256',
    state: 'st'
  });
  const local = sdk.oauthAuthorizeUrl('c', 'r', ['user:read'], 's', p.verifier, { id: 'http://localhost:9' });
  assert.match(local, /^http:\/\/localhost:9\/oauth\/authorize\?/);
  assert.deepEqual(sdk.oauthScopes().length, 13);
  assert.ok(sdk.oauthScopes().includes('moderation:chat_message:manage'));
  assert.throws(() => sdk.oauthAuthorizeUrl('c', 'r', [], 's', p.verifier), /no scopes/);
  assert.throws(() => sdk.oauthAuthorizeUrl('c', 'r', ['a b'], 's', p.verifier), /not one OAuth scope/);
  assert.throws(() => sdk.oauthAuthorizeUrl('c', 'r', 'user:read', 's', p.verifier), TypeError);
  assert.throws(() => sdk.oauthAuthorizeUrl('c', 'r', [1], 's', p.verifier), /scopes\[0\]/);
  assert.throws(() => sdk.oauthAuthorizeUrl('c', 'r', ['user:read'], '', p.verifier), /empty state/);
});

test('token requests, and their answers as the docs and the spec show them', () => {
  const code = sdk.oauthCodeRequest('c', 's', 'https://app.example/cb', 'the-code', 'v');
  assert.equal(code.method, 'POST');
  assert.equal(code.url, 'https://id.kick.com/oauth/token');
  assert.deepEqual(Object.fromEntries(new URLSearchParams(code.body)), {
    grant_type: 'authorization_code',
    code: 'the-code',
    client_id: 'c',
    client_secret: 's',
    redirect_uri: 'https://app.example/cb',
    code_verifier: 'v'
  });
  const refresh = sdk.oauthRefreshRequest('c', 's', 'r');
  assert.deepEqual(Object.fromEntries(new URLSearchParams(refresh.body)), {
    grant_type: 'refresh_token',
    refresh_token: 'r',
    client_id: 'c',
    client_secret: 's'
  });

  const spec = sdk.oauthParseUserToken(200, oauthFile('token-spec.json'));
  assert.equal(spec.token_type, 'Bearer');
  assert.equal(spec.expires_in, 7200);
  assert.equal(spec.refresh_expires_in, 2592000);
  assert.equal(typeof spec.refresh_token, 'string');
  assert.ok(Array.isArray(spec.scopes));
  assert.equal(typeof sdk.oauthParseUserToken(200, oauthFile('token.json')).access_token, 'string');
  assert.throws(() => sdk.oauthParseUserToken(400, oauthFile('error.json')), (e) => e.status === 400);
});

test('revoking and introspecting', () => {
  const r = sdk.oauthRevokeRequest('tok', 'refresh_token');
  assert.equal(r.url, 'https://id.kick.com/oauth/revoke?token=tok&token_type_hint=refresh_token');
  assert.equal(sdk.oauthRevokeRequest('tok').url, 'https://id.kick.com/oauth/revoke?token=tok');
  assert.equal(sdk.oauthRevokeRequest('tok', null).url, 'https://id.kick.com/oauth/revoke?token=tok');
  assert.throws(() => sdk.oauthRevokeRequest('tok', 'refresh'), TypeError);
  assert.throws(() => sdk.oauthRevokeRequest(''), /empty token/);
  assert.equal(sdk.oauthParseRevoke(200, oauthFile('revoke.txt')), undefined);
  assert.throws(() => sdk.oauthParseRevoke(401, ''), (e) => e.status === 401);

  const i = sdk.oauthIntrospectRequest('tok');
  assert.equal(i.url, 'https://id.kick.com/oauth/token/introspect');
  assert.deepEqual(
    i.headers.find(([k]) => k === 'authorization'),
    ['authorization', 'Bearer tok']
  );
  assert.deepEqual(sdk.oauthParseIntrospect(200, oauthFile('introspect.json')), {
    active: true,
    client_id: '',
    exp: 1771046347,
    scopes: ['user:read', 'channel:read'],
    token_type: 'user'
  });
  assert.equal(sdk.oauthParseIntrospect(200, oauthFile('introspect-inactive.json')).active, false);
});

test('users, categories, stats and the Kicks leaderboard', () => {
  let f = docs('public_api/v1-users.json');
  same(sdk.usersRequest('t', [123]), { ...f, request: { ...f.request, json: null } });
  assert.deepEqual(sdk.parseUsers(f.response.status, f.response.body), [
    {
      user_id: 123,
      name: 'John Doe',
      profile_picture: 'https://kick.com/img/default-profile-pictures/default-avatar-2.webp',
      email: 'john.doe@example.com'
    }
  ]);
  assert.equal(sdk.usersRequest('t').url, 'https://api.kick.com/public/v1/users');
  assert.throws(() => sdk.usersRequest('t', Array.from({ length: 51 }, (_, i) => i + 1)), /at most 50/);

  f = docs('public_api/v2-categories.json');
  same(sdk.categoriesRequest('t', ['Rust']), f);
  const page = sdk.parseCategories(f.response.status, f.response.body);
  assert.deepEqual(page.data[0], {
    id: 101,
    name: 'Rust',
    thumbnail: 'https://kick.com/img/categories/rust.jpeg',
    tags: ['FPS', 'Shooter', 'Adventure']
  });
  assert.equal(typeof page.next_cursor, 'string');
  const all = new URL(sdk.categoriesRequest('t', null, ['FPS', 'Co-op'], [1, 2], 10, 'c').url);
  assert.deepEqual(Object.fromEntries(all.searchParams), { tag: 'FPS,Co-op', id: '1,2', limit: '10', cursor: 'c' });
  assert.throws(() => sdk.categoriesRequest('t', ['a,b']), /comma/);

  f = docs('public_api/v1-livestreams-stats.json');
  same(sdk.livestreamStatsRequest('t'), f);
  assert.deepEqual(sdk.parseLivestreamStats(f.response.status, f.response.body), { total_count: 100 });

  f = docs('public_api/v1-kicks-leaderboard.json');
  same(sdk.kicksLeaderboardRequest('t', 1), f);
  const board = sdk.parseKicksLeaderboard(f.response.status, f.response.body);
  assert.deepEqual(board.week, [{ user_id: 123, username: 'john-doe', rank: 1, gifted_amount: 100 }]);
  assert.equal(sdk.kicksLeaderboardRequest('t').url, 'https://api.kick.com/public/v1/kicks/leaderboard');
  assert.throws(() => sdk.kicksLeaderboardRequest('t', 101), /top/);
});

test('chat', () => {
  let f = fixture('actions-chat-send');
  same(sdk.chatSendRequest('t', { user: 123 }, 'Pog', f.request.json.reply_to_message_id), f);
  assert.deepEqual(sdk.chatParseSend(f.status, f.body), {
    is_sent: true,
    message_id: '828f83bb-e391-4c78-9e91-af35a172840e'
  });
  assert.deepEqual(JSON.parse(sdk.chatSendRequest('t', 'bot', 'hi').body), { content: 'hi', type: 'bot' });
  f = fixture('actions-chat-delete');
  same(sdk.chatDeleteRequest('t', '828f83bb-e391-4c78-9e91-af35a172840e'), f);
  done(f);
});

test('moderation', () => {
  let f = fixture('actions-moderation-timeout');
  same(sdk.moderationTimeoutRequest('t', 123, 333, 10, 'Not vibing'), f);
  done(f);
  f = fixture('actions-moderation-unban');
  same(sdk.moderationUnbanRequest('t', 123, 333), f);
  done(f);
  const ban = sdk.moderationBanRequest('t', 123, 333);
  assert.deepEqual(JSON.parse(ban.body), { broadcaster_user_id: 123, user_id: 333 });
  assert.throws(() => sdk.moderationTimeoutRequest('t', 123, 333, 10081), /10080/);
});

test('channel update', () => {
  const f = fixture('actions-channels-update');
  same(sdk.channelsUpdateRequest('t', 123, ['pog', 'chad'], 'My first stream'), f);
  done(f);
  assert.deepEqual(JSON.parse(sdk.channelsUpdateRequest('t', undefined, []).body), { custom_tags: [] });
  assert.throws(() => sdk.channelsUpdateRequest('t'), /nothing to change/);
});

test('rewards and redemptions', () => {
  const id = '01HZ8X9K2M4N6P8Q0R2S4T6V8W0Y2Z4';
  let f = fixture('actions-rewards-list');
  same(sdk.rewardsListRequest('t'), f);
  const list = sdk.rewardsParseRewards(f.status, f.body);
  assert.deepEqual([list.length, list[0].id, list[0].cost], [1, id, 100]);

  f = fixture('actions-rewards-create');
  same(sdk.rewardsCreateRequest('t', f.request.json), f);
  assert.equal(sdk.rewardsParseReward(f.status, f.body).title, 'Song Request');
  f = fixture('actions-rewards-update');
  same(sdk.rewardsUpdateRequest('t', id, f.request.json), f);
  assert.equal(sdk.rewardsParseReward(f.status, f.body).is_paused, false);
  f = fixture('actions-rewards-delete');
  same(sdk.rewardsDeleteRequest('t', id), f);
  done(f);

  f = fixture('actions-rewards-redemptions');
  same(sdk.rewardsRedemptionsRequest('t', undefined, 'pending'), f);
  const page = sdk.rewardsParseRedemptions(f.status, f.body);
  assert.equal(page.rewards[0].reward.title, 'Unban Request');
  assert.deepEqual(page.rewards[0].redemptions[0], {
    id: '01KCTMJ2X3DXT814CVX8FR9T1D',
    status: 'pending',
    redeemed_at: '2025-12-19T06:26:36Z',
    redeemer: 123,
    user_input: 'sorry mr strimer'
  });
  const byIds = new URL(sdk.rewardsRedemptionsByIdsRequest('t', ['a', 'b'], 'c').url);
  assert.deepEqual([...byIds.searchParams], [['id', 'a'], ['id', 'b'], ['cursor', 'c']]);

  const ids = ['01KEWZ2RKYZZFGD154K50Y3F8D', '01KEWZ2YC27070XWMHKFJR6C87'];
  for (const [name, r] of [
    ['actions-rewards-accept', sdk.rewardsAcceptRequest('t', ids)],
    ['actions-rewards-reject', sdk.rewardsRejectRequest('t', ids)]
  ]) {
    f = fixture(name);
    same(r, f);
    assert.deepEqual(sdk.rewardsParseFailures(f.status, f.body)[0], {
      id: '01KERB68MZZ04551D6X5TNRFYV',
      reason: 'NOT_OWNED'
    });
  }
});

test('ads', () => {
  const uuid = '123e4567-e89b-12d3-a456-426614174000';
  let f = fixture('actions-ads-ad-break');
  same(sdk.adsAdBreakRequest('t', uuid, 30), f);
  const b = sdk.adsParseAdBreak(f.status, f.body);
  assert.deepEqual([b.ad_break_id, b.remaining_ad_breaks], ['Xq3LmP9aB2cD', 2]);
  f = fixture('actions-ads-ad-break-refused');
  assert.throws(
    () => sdk.adsParseAdBreak(f.status, f.body),
    (e) => e.status === 400
  );
  f = fixture('actions-ads-status');
  same(sdk.adsStatusRequest('t'), f);
  const s = sdk.adsParseStatus(f.status, f.body);
  assert.deepEqual([s.opted_in, s.limits, s.ad_breaks[0].status], [true, { max: 3, period_seconds: 3600 }, 'inserted']);
  f = fixture('actions-ads-enroll');
  same(sdk.adsEnrollRequest('t'), f);
  done(f);
  assert.throws(() => sdk.adsAdBreakRequest('t', uuid, 6), /7 to 90/);
});

test('drops', () => {
  let f = fixture('actions-drops-claims');
  same(sdk.dropsClaimsRequest('t', f.request.params), f);
  const c = sdk.dropsParseClaims(f.status, f.body);
  assert.deepEqual([c.claims.length, c.claims[0].user_id, c.cursor], [1, null, '01K0TDDR08Q5ZNWXK92SDH7SDH']);
  assert.equal(sdk.dropsClaimsRequest('t').url, 'https://api.kick.com/public/v1/drops/claims');
  f = fixture('actions-drops-update');
  same(sdk.dropsUpdateClaimsRequest('t', f.request.json.claims), f);
  done(f);
  f = fixture('actions-drops-unauthorized');
  assert.throws(() => sdk.dropsParseClaims(f.status, f.body), (e) => e.status === 401);
});

test('parseDone throws with the status', () => {
  assert.equal(sdk.parseDone(204, ''), undefined);
  assert.throws(() => sdk.parseDone(403, '{"message":"Forbidden"}'), (e) => e.status === 403);
});

test("user-token arguments are checked, not converted", () => {
  // Ids.
  for (const bad of [-1, 1.5, NaN, 2 ** 53]) {
    assert.throws(() => sdk.moderationBanRequest('t', bad, 1), RangeError, String(bad));
    assert.throws(() => sdk.moderationBanRequest('t', 1, bad), RangeError, String(bad));
    assert.throws(() => sdk.chatSendRequest('t', { user: bad }, 'hi'), RangeError, String(bad));
    assert.throws(() => sdk.usersRequest('t', [bad]), RangeError, String(bad));
    assert.throws(() => sdk.channelsUpdateRequest('t', bad), RangeError, String(bad));
  }
  assert.throws(() => sdk.moderationBanRequest('t', '1', 1), TypeError);
  assert.throws(() => sdk.moderationTimeoutRequest('t', 1, 2, '10'), TypeError);
  assert.throws(() => sdk.adsAdBreakRequest('t', 'id', '30'), TypeError);
  assert.throws(() => sdk.kicksLeaderboardRequest('t', '10'), TypeError);
  assert.throws(() => sdk.categoriesRequest('t', null, null, null, true), TypeError);
  assert.throws(() => sdk.categoriesRequest('t', null, null, ['1']), /ids\[0\]/);
  assert.throws(() => sdk.categoriesRequest('t', 'Rust'), TypeError);
  assert.throws(() => sdk.usersRequest('t', '1'), TypeError);
  assert.throws(() => sdk.channelsUpdateRequest('t', undefined, 'pog'), TypeError);
  assert.throws(() => sdk.rewardsAcceptRequest('t', 'a'), TypeError);
  assert.throws(() => sdk.rewardsAcceptRequest('t', [1]), /ids\[0\]/);
  assert.throws(() => sdk.rewardsAcceptRequest('t', []), /no ids/);
  assert.throws(() => sdk.rewardsAcceptRequest('t', ['a', 'a']), /twice/);

  // The sender: {user} or 'bot', nothing else.
  for (const bad of ['user', { user: 1, extra: 2 }, { broadcaster: 1 }, [1], 1, null]) {
    assert.throws(() => sdk.chatSendRequest('t', bad, 'hi'), TypeError, JSON.stringify(bad));
  }
  assert.throws(() => sdk.chatSendRequest('t', 'bot', ' '), /empty message/);

  // Plain objects: the core's keys, typed, nothing else.
  assert.throws(() => sdk.rewardsCreateRequest('t', { title: 'x', cost: 1, colour: 'red' }), /unknown field `colour`/);
  assert.throws(() => sdk.rewardsCreateRequest('t', { title: 'x', cost: '1' }), TypeError);
  assert.throws(() => sdk.rewardsCreateRequest('t', { title: 'x', cost: 1.5 }), TypeError);
  assert.throws(() => sdk.rewardsCreateRequest('t', { title: 'x', cost: 1, is_enabled: 1 }), TypeError);
  assert.throws(() => sdk.rewardsCreateRequest('t', ['x', 1]), TypeError);
  assert.throws(() => sdk.rewardsCreateRequest('t', { title: 'x' }), /title and a cost/);
  assert.throws(() => sdk.rewardsUpdateRequest('t', 'id', {}), /nothing to change/);
  assert.equal(JSON.parse(sdk.rewardsUpdateRequest('t', 'id', { title: 'x', cost: undefined }).body).title, 'x');
  assert.throws(() => sdk.dropsClaimsRequest('t', { campaign: 'x' }), /unknown field `campaign`/);
  assert.throws(() => sdk.dropsClaimsRequest('t', { user_id: '1' }), TypeError);
  assert.throws(() => sdk.dropsClaimsRequest('t', { limit: 0 }), /limit 0/);
  assert.throws(() => sdk.dropsClaimsRequest('t', 'x'), TypeError);
  assert.throws(() => sdk.dropsUpdateClaimsRequest('t', [{ claim_id: 'a' }]), /external_status/);
  assert.throws(() => sdk.dropsUpdateClaimsRequest('t', [{ claim_id: 'a', external_status: 'b', x: 1 }]), /unknown/);
  assert.throws(() => sdk.dropsUpdateClaimsRequest('t', [['a', 'b']]), /updates\[0\]/);
  assert.throws(() => sdk.dropsUpdateClaimsRequest('t', {}), TypeError);
  assert.throws(() => sdk.rewardsRedemptionsRequest('t', null, 'done'), TypeError);
  // Endpoints as everywhere.
  assert.throws(() => sdk.adsStatusRequest('t', { apl: 'x' }), /unknown key `apl`/);
  assert.match(sdk.adsStatusRequest('t', { api: 'http://localhost:9' }).url, /^http:\/\/localhost:9\/public\/v1\//);
});
