// The recorder's anonymizer on made-up answers that carry every kind of
// leak, and its commands that need no network.
//
//   node --test tools/record-api.test.mjs

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const TOOL = fileURLToPath(new URL('./record-api.mjs', import.meta.url));
const FIXTURES = fileURLToPath(new URL('../fixtures/', import.meta.url));
const count = () => ['public_api', 'id'].map((d) => readdirSync(join(FIXTURES, d)).length);
const before = count();
const { createAnonymizer, with127Workaround } = await import('./record-api.mjs');

// The real values: none may survive (case-insensitively).
const REAL = {
  access: 'ACCESS-tok-9f8e7d6c5b4a', refresh: 'REFRESH-tok-1a2b3c4d5e6f', clientId: '01CLIENTIDXYZ0000000000000',
  secret: 'client-secret-shhh-123456', code: 'AUTHCODE-abcdef-987654', verifier: 'verifier-' + 'v'.repeat(40),
  state: 'state-q1w2e3r4t5y6', username: 'Real_Streamer', slug: 'real-streamer', viewer: 'SomeViewer99',
  email: 'real.person@gmail.com', title: 'my private stream title', chat: 'hello from a real person',
  reason: 'spamming links', bio: 'I live in Springfield', prompt: 'tell me your secret wish',
  input: 'my user input text', rewardTitle: 'Hydrate!', tag: 'MyCustomTag', avatar: 'https://files.kick.com/images/user/42/avatar.webp',
  cursor: 'eyJpZCI6IjEyMzQ1In0', uuid: '9b2f4c1e-7d3a-4f8b-a1c2-3e4f5a6b7c8d', ulid: '01KCTMJ2X3DXT814CVX8FR9T1D',
  streamKey: 'sk_us-west-2_ABCDEF123456', unknownField: 'Mystery Value', cookie: 'kick_session=abcdef0123456789'
};
const REAL_IDS = [4815162, 23423, 1010101];

function assertClean(text, extra = []) {
  const low = text.toLowerCase();
  for (const [k, v] of Object.entries(REAL)) assert.ok(!low.includes(v.toLowerCase()), `${k} leaked: ${text}`);
  for (const v of extra) assert.ok(!low.includes(v.toLowerCase()), `${v} leaked: ${text}`);
  for (const id of REAL_IDS) assert.ok(!new RegExp(`(?<![0-9])${id}(?![0-9])`).test(text), `id ${id} leaked: ${text}`);
}

const req = (method, url, { headers = [], body = null } = {}) => ({ method, url, headers, body });
const json = (o) => JSON.stringify(o);

test('tokens, codes, the client secret and the verifier are redacted everywhere', () => {
  const a = createAnonymizer();
  a.secret(REAL.secret);
  const code = a.anonExchange({
    req: req('POST', 'https://id.kick.com/oauth/token', {
      headers: [['content-type', 'application/x-www-form-urlencoded']],
      body: new URLSearchParams({ grant_type: 'authorization_code', code: REAL.code, client_id: REAL.clientId,
        client_secret: REAL.secret, redirect_uri: 'http://127.0.0.1:8765/callback', code_verifier: REAL.verifier }).toString()
    }),
    status: 200,
    headers: [['content-type', 'application/json'], ['set-cookie', REAL.cookie], ['x-request-id', 'abc']],
    body: json({ access_token: REAL.access, refresh_token: REAL.refresh, expires_in: 7200, refresh_expires_in: 2592000,
      token_type: 'Bearer', scope: 'user:read chat:write moderation:chat_message:manage' })
  });
  const text = json(code.fixture);
  assertClean(text);
  assert.equal(code.hits, 0);
  const f = code.fixture;
  assert.deepEqual(Object.keys(f), ['elapsed_ms', 'kind', 'recorded_at', 'request', 'response']);
  assert.equal(f.request.form.grant_type, 'authorization_code');
  assert.equal(f.request.form.code, '[redacted]');
  assert.equal(f.request.form.client_secret, '[redacted]');
  assert.equal(f.request.form.code_verifier, '[redacted]');
  assert.match(f.request.form.redirect_uri, /^https:\/\/example\.invalid\/asset\/\d+$/);
  const body = JSON.parse(f.response.body);
  assert.equal(body.access_token, '[redacted]');
  assert.equal(body.scope, 'user:read chat:write moderation:chat_message:manage');
  assert.equal(body.expires_in, 7200);
  assert.deepEqual(f.response.headers, [['content-type', 'application/json']]);

  // The token in a revoke URL's query; the bearer token never written.
  const revoke = a.anonExchange({
    req: req('POST', `https://id.kick.com/oauth/revoke?token=${REAL.refresh}&token_type_hint=refresh_token`,
      { headers: [['authorization', `Bearer ${REAL.access}`]] }),
    status: 200, body: 'OK'
  });
  assertClean(json(revoke.fixture));
  assert.deepEqual(revoke.fixture.request.params, { token: '[redacted]', token_type_hint: 'refresh_token' });
  assert.equal(revoke.fixture.request.url, 'https://id.kick.com/oauth/revoke');
  assert.equal(revoke.fixture.response.body, 'OK');

  // A secret echoed somewhere unexpected (an error's description, a body that is not JSON).
  const err = a.anonExchange({ req: req('POST', 'https://id.kick.com/oauth/token'), status: 400,
    body: json({ error: 'invalid_grant', error_description: `code ${REAL.code} expired` }) });
  assertClean(json(err.fixture));
  assert.equal(JSON.parse(err.fixture.response.body).error, 'invalid_grant');
  assert.equal(err.hits, 1);
  const plain = a.anonExchange({ req: req('GET', 'https://api.kick.com/public/v1/users'), status: 500,
    body: `<html>token ${REAL.access}</html>` });
  assert.equal(plain.fixture.response.body, '[unparsed]');
});

test('names, ids, emails, URLs, free text and cursors become stable pseudonyms', () => {
  const a = createAnonymizer();
  const page = a.anonExchange({
    req: req('GET', 'https://api.kick.com/public/v2/livestreams?category_id=15&language_code=en&limit=25'),
    status: 200,
    body: json({ data: [{
      broadcaster_user: { id: REAL_IDS[0], username: REAL.username, profile_picture: REAL.avatar },
      channel: { slug: REAL.slug }, category: { id: 15, name: 'Just Chatting', thumbnail: 'https://files.kick.com/c.webp' },
      id: REAL.uuid, title: REAL.title, language_code: 'en', has_mature_content: false, started_at: '2026-11-15T10:00:00Z',
      tags: [REAL.tag], thumbnail: null, viewer_count: 321, mystery: REAL.unknownField, nested: { deep: { who: REAL.viewer } }
    }], pagination: { next_cursor: REAL.cursor }, message: 'OK' })
  });
  assertClean(json(page.fixture));
  const s = JSON.parse(page.fixture.response.body);
  const d = s.data[0];
  assert.equal(d.broadcaster_user.id, 900000001);
  assert.equal(d.broadcaster_user.username, 'user0001');
  assert.equal(d.channel.slug, 'user0001', 'a slug and its username share a pseudonym');
  assert.deepEqual(d.category, { id: 15, name: 'Just Chatting', thumbnail: 'https://example.invalid/asset/2' });
  assert.equal(d.id, '00000000-0000-4000-8000-000000000001');
  assert.equal(d.title, 'text-1');
  assert.deepEqual(d.tags, ['text-2']);
  assert.equal(d.language_code, 'en');
  assert.equal(d.started_at, '2026-11-15T10:00:00Z');
  assert.equal(d.viewer_count, 321);
  assert.equal(d.has_mature_content, false);
  assert.match(d.mystery, /^str-\d+$/, 'a field nobody listed is replaced');
  assert.match(d.nested.deep.who, /^str-\d+$/);
  assert.equal(s.pagination.next_cursor, 'cursor-1');
  assert.equal(s.message, 'OK');
  assert.deepEqual(page.fixture.request.params, { category_id: 15, language_code: 'en', limit: 25 });

  // The next page's request carries the same cursor; the same user, the same pseudonyms.
  const p2 = a.anonExchange({
    req: req('GET', `https://api.kick.com/public/v1/users?id=${REAL_IDS[0]}&id=${REAL_IDS[1]}&cursor=${REAL.cursor}`),
    status: 200,
    body: json({ data: [
      { user_id: REAL_IDS[0], name: REAL.username, email: REAL.email, profile_picture: REAL.avatar },
      { user_id: REAL_IDS[1], name: REAL.viewer, email: null, profile_picture: '' }], message: 'OK' })
  });
  assertClean(json(p2.fixture));
  assert.deepEqual(p2.fixture.request.params, { id: [900000001, 900000002], cursor: 'cursor-1' });
  const u = JSON.parse(p2.fixture.response.body).data;
  assert.equal(u[0].name, 'user0001');
  assert.equal(u[0].email, 'user0001@example.invalid', 'an email takes its owner\'s pseudonym');
  assert.equal(u[0].profile_picture, 'https://example.invalid/asset/1');
  assert.equal(u[1].user_id, 900000002);
  assert.equal(u[1].profile_picture, '');
});

test('the acting endpoints: chat, moderation, channel, rewards', () => {
  const a = createAnonymizer();
  const jsonReq = (method, url, body) => req(method, url, { headers: [['content-type', 'application/json']], body: json(body) });
  const out = [];
  const x = (r, status, body) => {
    const e = a.anonExchange({ req: r, status, body: body === undefined ? '' : json(body) });
    out.push(e);
    return e.fixture;
  };
  const sent = x(jsonReq('POST', 'https://api.kick.com/public/v1/chat',
    { broadcaster_user_id: REAL_IDS[0], content: REAL.chat, type: 'user', reply_to_message_id: REAL.uuid }),
  200, { data: { is_sent: true, message_id: REAL.uuid }, message: 'OK' });
  assert.deepEqual(sent.request.json, { broadcaster_user_id: 900000001, content: 'text-1', type: 'user',
    reply_to_message_id: '00000000-0000-4000-8000-000000000001' });
  const del = x(req('DELETE', `https://api.kick.com/public/v1/chat/${REAL.uuid}`), 204);
  assert.equal(del.request.url, 'https://api.kick.com/public/v1/chat/00000000-0000-4000-8000-000000000001');
  const ban = x(jsonReq('POST', 'https://api.kick.com/public/v1/moderation/bans',
    { broadcaster_user_id: REAL_IDS[0], user_id: REAL_IDS[2], duration: 1, reason: REAL.reason }),
  400, { data: {}, message: 'Invalid request' });
  assert.deepEqual(ban.request.json, { broadcaster_user_id: 900000001, user_id: 900000002, duration: 1, reason: 'text-2' });
  assert.equal(ban.response.body, '{"data":{},"message":"Invalid request"}');
  const patch = x(jsonReq('PATCH', 'https://api.kick.com/public/v1/channels',
    { category_id: 15, custom_tags: [REAL.tag], stream_title: REAL.title }), 204);
  assert.deepEqual(patch.request.json, { category_id: 15, custom_tags: ['text-3'], stream_title: 'text-4' });
  const ch = x(req('GET', `https://api.kick.com/public/v1/channels?broadcaster_user_id=${REAL_IDS[0]}`), 200,
    { data: [{ broadcaster_user_id: REAL_IDS[0], slug: REAL.slug, stream_title: REAL.title, channel_description: REAL.bio,
      banner_picture: REAL.avatar, stream: { key: REAL.streamKey, url: 'rtmps://fa723fc1b171.global-contribute.live-video.net',
        is_live: false, viewer_count: 0, custom_tags: [REAL.tag] }, category: { id: 15, name: 'Just Chatting' } }], message: 'OK' });
  const c = JSON.parse(ch.response.body).data[0];
  assert.equal(c.stream_title, 'text-4', 'the same text, the same placeholder');
  assert.equal(c.stream.key, '[redacted]');
  assert.match(c.stream.url, /^https:\/\/example\.invalid\/asset\//);
  const reward = { id: REAL.ulid, title: REAL.rewardTitle, description: REAL.bio, prompt: REAL.prompt, cost: 100,
    background_color: '#00e701', is_enabled: true, is_paused: false };
  const created = x(jsonReq('POST', 'https://api.kick.com/public/v1/channels/rewards', { title: REAL.rewardTitle, cost: 1 }),
    200, { data: reward, message: 'OK' });
  const r = JSON.parse(created.response.body).data;
  assert.equal(r.id, '00000000000000000000000001');
  assert.equal(r.cost, 100);
  assert.equal(r.background_color, '#00e701');
  assert.match(r.prompt, /^text-\d+$/);
  const upd = x(jsonReq('PATCH', `https://api.kick.com/public/v1/channels/rewards/${REAL.ulid}`, { title: 'x' }), 200, { data: reward });
  assert.equal(upd.request.url, 'https://api.kick.com/public/v1/channels/rewards/00000000000000000000000001');
  const redemptions = x(req('GET', `https://api.kick.com/public/v1/channels/rewards/redemptions?status=pending&reward_id=${REAL.ulid}`), 200,
    { data: [{ reward: { ...reward, can_manage: true }, redemptions: [{ id: '01KCTMJ2X3DXT814CVX8FR9T1E', status: 'pending',
      redeemed_at: '2025-12-19T06:26:36Z', redeemer: { user_id: REAL_IDS[1] }, user_input: REAL.input }] }],
    pagination: { next_cursor: REAL.cursor }, message: 'OK' });
  assert.deepEqual(redemptions.request.params, { status: 'pending', reward_id: '00000000000000000000000001' });
  const rd = JSON.parse(redemptions.response.body).data[0].redemptions[0];
  assert.equal(rd.status, 'pending');
  assert.match(rd.user_input, /^text-\d+$/);
  const failed = x(jsonReq('POST', 'https://api.kick.com/public/v1/channels/rewards/redemptions/accept', { ids: [REAL.ulid] }),
    200, { data: [{ id: REAL.ulid, reason: 'NOT_PENDING' }], message: 'OK' });
  assert.deepEqual(JSON.parse(failed.response.body).data, [{ id: '00000000000000000000000001', reason: 'NOT_PENDING' }]);
  const kicks = x(req('GET', 'https://api.kick.com/public/v1/kicks/leaderboard?top=10'), 200,
    { data: { lifetime: [{ user_id: REAL_IDS[1], username: REAL.viewer, rank: 1, gifted_amount: 500 }], month: [], week: [] } });
  assert.deepEqual(JSON.parse(kicks.response.body).data.lifetime[0], { user_id: 900000003, username: 'user0002', rank: 1, gifted_amount: 500 });
  const adErr = x(req('GET', 'https://api.kick.com/public/v1/ads/ad-break-status'), 400, { data: 'CHANNEL_IS_NOT_LIVE', message: 'Bad Request' });
  assert.equal(JSON.parse(adErr.response.body).data, 'CHANNEL_IS_NOT_LIVE');
  for (const e of out) {
    assertClean(json(e.fixture));
    assert.equal(e.hits, 0, json(e.fixture));
  }
});

test('categories keep their catalogue data; a real name echoed in a kept message is caught', () => {
  const a = createAnonymizer();
  const cats = a.anonExchange({ catalog: true,
    req: req('GET', 'https://api.kick.com/public/v2/categories?name=Just%20Chatting&id=15&limit=5'), status: 200,
    body: json({ data: [{ id: 15, name: 'Just Chatting', thumbnail: 'https://files.kick.com/c.webp', tags: ['IRL'] }],
      pagination: { next_cursor: REAL.cursor }, message: 'OK' }) });
  assert.deepEqual(cats.fixture.request.params, { name: 'Just Chatting', id: 15, limit: 5 });
  assert.deepEqual(JSON.parse(cats.fixture.response.body).data[0],
    { id: 15, name: 'Just Chatting', thumbnail: 'https://example.invalid/asset/1', tags: ['IRL'] });

  a.anonExchange({ req: req('GET', `https://api.kick.com/public/v1/channels?slug=${REAL.slug}&broadcaster_user_id=${REAL_IDS[0]}`), status: 200, body: '' });
  const echo = a.anonExchange({ req: req('GET', 'https://api.kick.com/public/v1/channels'), status: 400,
    body: json({ data: {}, message: `channel ${REAL.slug} not found`, id_number: REAL_IDS[0] }) });
  assertClean(json(echo.fixture));
  assert.ok(echo.hits >= 1);
});

test('the 127.0.0.1 workaround puts a throwaway parameter before redirect_uri', () => {
  const url = 'https://id.kick.com/oauth/authorize?response_type=code&client_id=X&redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcb&scope=user%3Aread';
  const out = new URL(with127Workaround(url));
  assert.deepEqual([...out.searchParams.keys()], ['response_type', 'client_id', 'redirect', 'redirect_uri', 'scope']);
  assert.equal(out.searchParams.get('redirect'), '127.0.0.1');
  assert.equal(out.searchParams.get('redirect_uri'), 'http://127.0.0.1:8765/cb');
});

test('status and the actions plan need no network nor credentials', () => {
  const dir = mkdtempSync(join(tmpdir(), 'record-api-test-'));
  try {
    const env = { PATH: process.env.PATH, XDG_RUNTIME_DIR: dir };
    const st = spawnSync(process.execPath, [TOOL, 'status'], { env, encoding: 'utf8' });
    assert.equal(st.status, 0, st.stderr);
    assert.match(st.stdout, /private dir: .*kick-record-\d+ \(not created yet\)/);
    assert.match(st.stdout, /user-reads +not run/);
    const plan = spawnSync(process.execPath, [TOOL, 'actions', '--target', 'someone'], { env, encoding: 'utf8' });
    assert.equal(plan.status, 0, plan.stderr);
    assert.match(plan.stdout, /find someone's user id/);
    assert.match(plan.stdout, /Not recorded .*ads ad-break, ads enroll, drops/s);
    assert.match(plan.stdout, /Nothing sent/);
    const noCreds = spawnSync(process.execPath, [TOOL, 'app'], { env, encoding: 'utf8' });
    assert.equal(noCreds.status, 2);
    assert.match(noCreds.stderr, /KICK_CLIENT_ID_FILE must name a file/);
    // The private dir, once made, is the user's alone.
    const priv = join(dir, `kick-record-${process.getuid()}`);
    assert.equal(statSync(priv).mode & 0o777, 0o700);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('importing the recorder recorded nothing', () => {
  assert.deepEqual(count(), before);
});
