#!/usr/bin/env node
// Records a session with Kick's realtime chat feed into fixtures/realtime/,
// anonymized as it goes, for the tests to replay.
//
//   node tools/record-realtime.mjs <slug> [seconds=30]
//
// Writes <stamp>-<pseudonym>.jsonl: one line per frame, in and out, with the
// HTTP answers that came before the socket. What is kept as-is: Kick's
// protocol (ids, codes, ping and ttl), event names, feed names (with the ids
// replaced), badge kinds, colours and times. Everything else - names, text,
// URLs, ids - becomes a stable pseudonym (user0001, text-3,
// https://example.invalid/asset/7, 900000001). The token is redacted.
//
// Needs Node 22+ (global WebSocket that takes headers).

import { writeFileSync, mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';

const slug = process.argv[2];
const seconds = Number(process.argv[3] || 30);
if (!slug) {
  console.error('usage: node tools/record-realtime.mjs <slug> [seconds]');
  process.exit(2);
}

const ORIGIN = 'https://kick.com';
// A full browser User-Agent from something that is not that browser gets a 403
// ("Request blocked by security policy") from kick.com/api/v2; this passes.
const UA = 'Mozilla/5.0';
const headers = { 'content-type': 'application/json', accept: 'application/json', origin: ORIGIN, 'user-agent': UA };

// ---- Anonymizer -------------------------------------------------------------

const ids = new Map();
const names = new Map();
const assets = new Map();
let texts = 0;
const fakeId = (n) => ids.get(n) ?? (ids.set(n, 900000001 + ids.size), ids.get(n));
const fakeName = (s) => {
  // A slug is the username lowercased, `_` as `-`: one pseudonym for both.
  const k = String(s).toLowerCase().replace(/_/g, '-');
  if (!names.has(k)) names.set(k, `user${String(names.size + 1).padStart(4, '0')}`);
  return names.get(k);
};
const uuids = new Map();
const fakeUuid = (u) =>
  uuids.get(u) ?? (uuids.set(u, `00000000-0000-4000-8000-${String(uuids.size + 1).padStart(12, '0')}`), uuids.get(u));
const fakeAsset = (u) => assets.get(u) ?? (assets.set(u, `https://example.invalid/asset/${assets.size + 1}`), assets.get(u));

const KEEP = new Set(['type', 'badge_type', 'name', 'color', 'username_color', 'created_at', 'expires_at', 'finish_at',
  'tier', 'event', 'sort_order', 'selected', 'text', 'permanent', 'duration', 'months', 'amount', 'level', 'is_live',
  'started_at', 'ended_at', 'gift_id', 'pinned_time', 'aiModerated', 'number_viewers', 'viewers_count']);
const NAMES = new Set(['username', 'slug', 'host_username', 'gifter_username', 'channel_slug', 'banned_username']);

function anon(v, key) {
  if (Array.isArray(v)) return v.map((x) => anon(x, key === 'gifted_usernames' ? 'username' : key));
  if (v && typeof v === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = anon(x, k);
    return out;
  }
  if (typeof v === 'number') return key && /(^|_)id$|^id$/.test(key) ? fakeId(v) : v;
  if (typeof v !== 'string') return v;
  if (key && /(^|_)id$/.test(key) && /^\d+$/.test(v)) return String(fakeId(Number(v)));
  if (key === 'id' || key === 'message_ref' || key === 'thread_parent_id') return fakeUuid(v);
  if (key && NAMES.has(key)) return fakeName(v);
  if (/^https?:\/\//.test(v)) return fakeAsset(v);
  if (key && KEEP.has(key)) return v;
  if (key === 'content') {
    // Keep the emote tokens' shape; replace what is around them.
    return v.replace(/\[emote:(\d+):[^\]]*\]|[^[]+/g, (m, id) => (id ? `[emote:${fakeId(Number(id))}:emote]` : `text-${++texts}`));
  }
  return `str-${++texts}`;
}

// The ids in a feed name, not the `2` of `.v2`.
const anonChannel = (c) => c.replace(/(?<=[._])\d+/g, (n) => fakeId(Number(n)));

// Kick's envelope keeps its event name; its data is a JSON string to anonymize inside.
function anonFrame(text) {
  return text
    .split('\n')
    .map((line) => {
      if (!line || line === '{}') return line;
      let f;
      try {
        f = JSON.parse(line);
      } catch {
        return line;
      }
      if (f.connect && f.connect.token) f.connect.token = '[redacted]';
      if (f.refresh && f.refresh.token) f.refresh.token = '[redacted]';
      if (f.connect && f.connect.client) f.connect.client = '00000000-0000-4000-8000-000000000000';
      if (f.subscribe && f.subscribe.channel) f.subscribe.channel = anonChannel(f.subscribe.channel);
      const p = f.push;
      if (p) {
        if (p.channel) p.channel = anonChannel(p.channel);
        const env = p.pub && p.pub.data;
        if (env && typeof env.data === 'string') {
          try {
            env.data = JSON.stringify(anon(JSON.parse(env.data)));
          } catch {
            env.data = '[unreadable]';
          }
        } else if (env && env.data) env.data = anon(env.data);
        if (p.pub && p.pub.offset !== undefined) p.pub.offset = 0;
      }
      return JSON.stringify(f);
    })
    .join('\n');
}

// ---- Recording --------------------------------------------------------------

const lines = [];
const t0 = Date.now();
const log = (direction, frame) => lines.push(JSON.stringify({ ms: Date.now() - t0, direction, frame }));

const chRes = await fetch(`https://kick.com/api/v2/channels/${slug}`, { headers: { 'user-agent': UA } });
if (!chRes.ok) throw new Error(`kick.com/api/v2/channels/${slug}: ${chRes.status}`);
const ch = await chRes.json();
const channelId = ch.id;
const chatroomId = ch.chatroom.id;
const clientId = randomUUID();

const post = async (path, body) => {
  const res = await fetch(`https://web.kick.com/api/v1/realtime${path}`, { method: 'POST', headers, body: JSON.stringify(body) });
  return { status: res.status, body: await res.text() };
};
const conn = await post(`/channels/${channelId}/chat/connection`, {
  client: { id: clientId, type: 'web' },
  capabilities: { accepted_providers: [{ provider: 'centrifugo' }] }
});
log('http', { path: `/channels/${fakeId(channelId)}/chat/connection`, ...conn });
const tok = await post('/auth/connection', { client_id: clientId });
const token = JSON.parse(tok.body).data.token;
log('http', { path: '/auth/connection', status: tok.status, body: tok.body.replace(token, '[redacted]') });

const url = JSON.parse(conn.body).data.connections.find((c) => c.provider === 'centrifugo').credentials.url;
const feeds = [`chatrooms.${chatroomId}.v2`, `chatrooms.${chatroomId}`, `chatroom_${chatroomId}`,
  `channel.${channelId}`, `channel_${channelId}`, `private-chatroom_${chatroomId}`];
const ws = new WebSocket(url, { headers: { Origin: ORIGIN, 'User-Agent': UA } });
let id = 1;
const send = (text) => {
  log('out', anonFrame(text));
  ws.send(text);
};
ws.onopen = () => send(JSON.stringify({ id: 1, connect: { token, name: 'js' } }));
ws.onmessage = (e) => {
  const text = String(e.data);
  log('in', anonFrame(text));
  for (const line of text.split('\n')) {
    if (line === '{}') send('{}');
    else if (JSON.parse(line).id === 1) send(feeds.map((c) => JSON.stringify({ id: ++id, subscribe: { channel: c } })).join('\n'));
  }
};
ws.onclose = (e) => log('close', { code: e.code, reason: e.reason });

setTimeout(() => {
  ws.close();
  const stamp = new Date(t0).toISOString().replace(/[-:]/g, '').replace(/\.\d+/, '');
  mkdirSync('fixtures/realtime', { recursive: true });
  const file = `fixtures/realtime/${stamp}-${fakeName(slug)}.jsonl`;
  writeFileSync(file, lines.join('\n') + '\n');
  console.log(`${file}: ${lines.length} frames`);
  process.exit(0);
}, seconds * 1000);
