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
// https://example.invalid/asset/7, 900000001), and any other string `str-N`:
// a field nobody listed is replaced, not kept. Tokens and close reasons are
// redacted; a line that is not JSON is written as [unparsed].
//
// Importing the file records nothing; anonFrame, anonHttp and anonClose are
// exported to try the anonymizer on made-up input.
//
// Needs Node 22+ (global WebSocket that takes headers).

import { writeFileSync, mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { pathToFileURL } from 'node:url';

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

// Payload keys whose strings are kept as they are. Anything not named here,
// not a name, an id or a URL becomes `str-N`.
const KEEP = new Set(['type', 'badge_type', 'color', 'username_color', 'created_at', 'expires_at', 'finish_at',
  'tier', 'event', 'sort_order', 'selected', 'permanent', 'duration', 'months', 'amount', 'level', 'is_live',
  'started_at', 'ended_at', 'gift_id', 'pinned_time', 'aiModerated', 'number_viewers', 'viewers_count']);
// A badge's `name` and `text` are its kind ("level", "Subscriber"); elsewhere
// they are free text.
const BADGE_KEEP = new Set(['name', 'text']);
const NAMES = new Set(['username', 'slug', 'host_username', 'gifter_username', 'channel_slug', 'banned_username']);
// id, user_id, userId, ids, user_ids, userIds.
const ID_KEY = /(^|_)ids?$|[a-z]Ids?$/;
const URL_RE = /^[a-z][a-z0-9+.-]*:\/\//i;

function anon(v, key, badge = false) {
  if (Array.isArray(v)) return v.map((x) => anon(x, key === 'gifted_usernames' ? 'username' : key));
  if (v && typeof v === 'object') {
    const inBadge = key === 'badges' || key === 'badges_v2';
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = anon(x, k, inBadge);
    return out;
  }
  if (typeof v === 'number') return key && ID_KEY.test(key) ? fakeId(v) : v;
  if (typeof v !== 'string') return v;
  if (key && ID_KEY.test(key) && /^\d+$/.test(v)) return String(fakeId(Number(v)));
  if (key === 'id' || key === 'message_ref' || key === 'thread_parent_id') return fakeUuid(v);
  if (key && NAMES.has(key)) return fakeName(v);
  if (URL_RE.test(v)) return fakeAsset(v);
  if (key && KEEP.has(key)) return v;
  if (badge && BADGE_KEEP.has(key)) return v;
  if (key === 'content') {
    // Keep the emote tokens' shape; replace what is around them.
    return v.replace(/\[emote:(\d+):[^\]]*\]|[^[]+/g, (m, id) => (id ? `[emote:${fakeId(Number(id))}:emote]` : `text-${++texts}`));
  }
  return `str-${++texts}`;
}

// The ids in a feed name, not the `2` of `.v2`.
const anonChannel = (c) => c.replace(/(?<=[._])\d+/g, (n) => fakeId(Number(n)));

// Kick's envelope ({event, data}) keeps its event name; its data is a JSON
// string to anonymize inside. Any other data is a payload.
function anonData(x) {
  if (typeof x === 'string') {
    try {
      return JSON.stringify(anon(JSON.parse(x)));
    } catch {
      return '[unreadable]';
    }
  }
  if (x && typeof x === 'object' && !Array.isArray(x)) {
    const out = {};
    for (const [k, y] of Object.entries(x)) out[k] = k === 'data' ? anonData(y) : anon(y, k);
    return out;
  }
  return anon(x, 'data');
}

// Centrifugo's own fields. Numbers and booleans are kept (but ids mapped and
// offsets zeroed); strings only under these keys. Every payload (data, info's
// conn_info and chan_info, tags but the event name) goes through anon.
const PROTO_KEEP = new Set(['version', 'message', 'epoch']);
const ZERO_UUID = '00000000-0000-4000-8000-000000000000';

function proto(v, key) {
  // A list's items, objects or not, are fields of the list's key.
  if (Array.isArray(v)) return v.map((x) => protoField(x, key, key));
  const out = {};
  // connect.subs is keyed by feed name.
  for (const [k, x] of Object.entries(v)) out[key === 'subs' ? anonChannel(k) : k] = protoField(x, k, key);
  return out;
}

function protoField(x, k, parent) {
  if (typeof x === 'string') {
    if (k === 'token') return '[redacted]';
    if (k === 'channel') return anonChannel(x);
    if (k === 'client') return parent === 'connect' ? ZERO_UUID : fakeUuid(x);
    if (k === 'user') return anon(x, 'user_id');
    if (k === 'reason') return x && '[redacted]';
  }
  if (k === 'data') return anonData(x);
  if (k === 'conn_info' || k === 'chan_info') return anon(x, k);
  if (k === 'tags' && x && typeof x === 'object' && !Array.isArray(x)) {
    const out = {};
    for (const [t, y] of Object.entries(x)) out[t] = t === 'event' && typeof y === 'string' ? y : anon(y, t);
    return out;
  }
  if (k === 'offset' && typeof x === 'number') return 0;
  if (x && typeof x === 'object') return proto(x, k);
  // connect's `name` is the client's ("js"); elsewhere a name is someone's.
  if (typeof x === 'string') return PROTO_KEEP.has(k) || (k === 'name' && parent === 'connect') ? x : anon(x, k);
  // The command's own id is protocol; any other id is someone's.
  return typeof x === 'number' && k !== 'id' ? anon(x, k) : x;
}

// One frame: its lines are Centrifugo commands and replies. A line that is
// not JSON is not written.
export function anonFrame(text) {
  return text
    .split('\n')
    .map((line) => {
      if (!line || line === '{}') return line;
      try {
        const f = JSON.parse(line);
        // A command or reply is an object; anything else is not Centrifugo's.
        if (!f || typeof f !== 'object' || Array.isArray(f)) return '[unparsed]';
        return JSON.stringify(proto(f));
      } catch {
        return '[unparsed]';
      }
    })
    .join('\n');
}

// An HTTP answer's body: JSON, every string replaced but the realtime API's
// own words and the socket's endpoint (its query, if any, dropped).
const HTTP_KEEP = new Set(['provider', 'mode', 'message']);
function anonHttpValue(v, key) {
  if (Array.isArray(v)) return v.map((x) => anonHttpValue(x, key));
  if (v && typeof v === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = anonHttpValue(x, k);
    return out;
  }
  if (typeof v !== 'string') return anon(v, key);
  if (key === 'token') return '[redacted]';
  if (key === 'url') {
    try {
      const u = new URL(v);
      return u.origin + u.pathname;
    } catch {
      return fakeAsset(v);
    }
  }
  return HTTP_KEEP.has(key) ? v : anon(v, key);
}
export function anonHttp(body) {
  try {
    return JSON.stringify(anonHttpValue(JSON.parse(body))) + (body.endsWith('\n') ? '\n' : '');
  } catch {
    return '[unparsed]';
  }
}

export const anonClose = (code, reason) => ({ code, reason: reason ? '[redacted]' : '' });

// ---- Recording --------------------------------------------------------------
// Only when run: importing this file (to try the anonymizer) records nothing.

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const slug = process.argv[2];
  const seconds = Number(process.argv[3] || 30);
  if (!slug) {
    console.error('usage: node tools/record-realtime.mjs <slug> [seconds]');
    process.exit(2);
  }

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
  log('http', { path: `/channels/${fakeId(channelId)}/chat/connection`, status: conn.status, body: anonHttp(conn.body) });
  const tok = await post('/auth/connection', { client_id: clientId });
  const token = JSON.parse(tok.body).data.token;
  log('http', { path: '/auth/connection', status: tok.status, body: anonHttp(tok.body) });

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
  ws.onclose = (e) => log('close', anonClose(e.code, e.reason));

  setTimeout(() => {
    ws.close();
    const stamp = new Date(t0).toISOString().replace(/[-:]/g, '').replace(/\.\d+/, '');
    mkdirSync('fixtures/realtime', { recursive: true });
    const file = `fixtures/realtime/${stamp}-${fakeName(slug)}.jsonl`;
    writeFileSync(file, lines.join('\n') + '\n');
    console.log(`${file}: ${lines.length} frames`);
    process.exit(0);
  }, seconds * 1000);
}
