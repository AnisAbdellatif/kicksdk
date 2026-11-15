#!/usr/bin/env node
// Records Kick's public API (api.kick.com) and OAuth (id.kick.com) answers
// into fixtures/public_api/ and fixtures/id/, anonymized, for the parsers to
// be checked against. Every request is built by the SDK (bindings/js), so a
// recording also exercises the builders; after each answer the matching SDK
// parser is run on the real body and one line says whether it read it.
//
//   KICK_CLIENT_ID_FILE=… KICK_CLIENT_SECRET_FILE=… node tools/record-api.mjs app
//   KICK_CLIENT_ID_FILE=… KICK_CLIENT_SECRET_FILE=… KICK_REDIRECT_URI=http://localhost:8765/callback \
//     node tools/record-api.mjs login
//   node tools/record-api.mjs user-reads        (and refresh: needs the two _FILE vars)
//   node tools/record-api.mjs actions --target <slug> [--yes]
//   node tools/record-api.mjs revoke
//   node tools/record-api.mjs status
//
// Secrets are read from the files the _FILE variables name, never from
// arguments or variables' values. The real exchanges (and the user token) go
// only to a private directory outside the repository:
// ${XDG_RUNTIME_DIR or the OS temp dir}/kick-record-<uid>, mode 0700, files
// 0600. Nothing secret is printed.
//
// The anonymizer is default-deny: a string is kept only when its key says it
// is Kick's own word (a status, a type, a scope, a language, an error
// message), a time, or catalogue data (a category's id, name and tags).
// Tokens and codes become [redacted], names user0001, ids 900000001… (UUIDs
// and ULIDs keep their shape), emails user0001@example.invalid, free text
// text-N, URLs https://example.invalid/asset/N, cursors cursor-N, anything
// else str-N; then every real value seen is searched for in what is written,
// and replaced if it survived anywhere.
//
// Importing the file records nothing; createAnonymizer is exported to try
// the anonymizer on made-up input. Needs Node 22+.

import { chmodSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync,
  writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';
import { tmpdir, userInfo } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const REPO = fileURLToPath(new URL('..', import.meta.url));

// ---- Anonymizer -------------------------------------------------------------

// Strings under these keys are secrets, wherever they are.
const SECRET_KEYS = new Set(['access_token', 'refresh_token', 'id_token', 'token', 'client_id', 'client_secret', 'code',
  'code_verifier', 'code_challenge', 'verifier', 'challenge', 'state', 'secret', 'password', 'key', 'stream_key',
  'authorization', 'cookie', 'session', 'signature', 'api_key']);
// id, ids, user_id, broadcaster_user_ids, messageId, …
const ID_KEY = /^ids?$|_ids?$|[a-z]Ids?$/;
const CATEGORY_ID_KEY = /^category_ids?$/;
const NAME_KEY = /(^|_)(username|slug|login|nickname|display_name)$/;
const CURSOR_KEYS = new Set(['cursor', 'next_cursor', 'prev_cursor', 'previous_cursor']);
const FREE_TEXT = new Set(['content', 'title', 'stream_title', 'description', 'channel_description', 'bio', 'prompt',
  'user_input', 'reason', 'text', 'tags', 'tag', 'custom_tags', 'message', 'error', 'error_description']);
const ENUM_KEYS = new Set(['status', 'type', 'token_type', 'grant_type', 'token_type_hint', 'source', 'response_type',
  'code_challenge_method', 'external_status']);
const LANG_KEYS = new Set(['language', 'language_code', 'language_codes', 'locale']);
const SCOPE_KEYS = new Set(['scope', 'scopes']);
const COLOR_KEYS = new Set(['background_color', 'color']);
// The answer's own words, at its top level only.
const ROOT_KEEP = new Set(['message', 'error', 'error_description']);
// A category's (public catalogue data, kept as the existing fixtures keep it).
const CATALOG_KEEP = new Set(['name', 'tags', 'tag']);
// Path words of Kick's endpoints; any other segment is replaced.
const PATH_WORDS = new Set(['public', 'v1', 'v2', 'chat', 'channels', 'rewards', 'redemptions', 'accept', 'reject',
  'users', 'livestreams', 'stats', 'categories', 'kicks', 'leaderboard', 'public-key', 'ads', 'ad-break',
  'ad-break-status', 'enroll', 'oauth', 'token', 'introspect', 'revoke', 'authorize', 'drops', 'claims', 'moderation',
  'bans', 'events', 'subscriptions']);
const KICK_HOSTS = new Set(['api.kick.com', 'id.kick.com', 'kick.com']);
const KEEP_HEADER = /^(content-type|retry-after)$|rate-?limit/i;

const TS_RE = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?$/;
const URL_RE = /^[a-z][a-z0-9+.-]*:\/\//i;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const ULID_RE = /^[0-9A-HJKMNP-TV-Z]{26}$/i;
const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;
const LANG_RE = /^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;
const ENUM_RE = /^[A-Za-z0-9_.:-]{1,40}$/;
const SCREAMING_RE = /^[A-Z][A-Z0-9_]{2,40}$/;
const SCOPE_RE = /^[a-z_]+(:[a-z_]+)*( [a-z_]+(:[a-z_]+)*)*$/;
const HEX_RE = /^#[0-9a-f]{3,8}$/i;
const CATALOG_RE = /^[\p{L}\p{N} &'.:!+-]{1,60}$/u;
// A body that is not JSON is kept only when it is one of these.
const PLAIN_RE = /^(OK|Unauthorized|Forbidden|Not Found|Bad Request|Invalid request|Too Many Requests|Internal Server Error|Bad Gateway|Service Unavailable|Gateway Timeout)\.?$/i;

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');

/**
 * A fresh anonymizer: one per recording session, so pseudonyms are
 * consistent across its files. `secret(v)` adds a value (a token, the client
 * secret) to what the leak guard looks for.
 */
export function createAnonymizer() {
  const maps = { id: new Map(), name: new Map(), uuid: new Map(), ulid: new Map(), sid: new Map(), asset: new Map(),
    cursor: new Map(), email: new Map(), text: new Map(), str: new Map(), seg: new Map() };
  const secrets = new Set(); // exact substrings, removed anywhere
  const reals = new Map(); // lowercased real string -> its replacement
  const realIds = new Map(); // real numeric id (string) -> fake
  const kept = new Set([...PATH_WORDS, ...KICK_HOSTS]); // lowercased strings kept on purpose
  let guardRe = null;

  const remember = (real, fake, min = 4) => {
    if (typeof real === 'string' && real.length >= min && !reals.has(real.toLowerCase())) {
      reals.set(real.toLowerCase(), String(fake));
      guardRe = null;
    }
    return fake;
  };
  const counter = (map, k, make) => map.get(k) ?? (map.set(k, make(map.size + 1)), map.get(k));

  const fakeId = (n) => {
    const fake = counter(maps.id, String(n), (i) => 900000000 + i);
    if (String(n).length >= 5) {
      realIds.set(String(n), fake);
      guardRe = null;
    }
    return fake;
  };
  const fakeName = (s) => {
    // A slug is the username lowercased, `_` as `-`: one pseudonym for both.
    const fake = counter(maps.name, s.toLowerCase().replace(/_/g, '-'), (i) => `user${String(i).padStart(4, '0')}`);
    remember(s, fake, 3);
    remember(s.replace(/_/g, '-'), fake, 3);
    return fake;
  };
  // An email takes its owner's pseudonym (the name beside it), or one of its own.
  const fakeEmail = (s, owner) => remember(s, counter(maps.email, s.toLowerCase(),
    () => `${owner ? fakeName(owner) : counter(maps.name, `email:${s.toLowerCase()}`, (i) => `user${String(i).padStart(4, '0')}`)}@example.invalid`));
  const fakeUuid = (s) => remember(s, counter(maps.uuid, s.toLowerCase(),
    (i) => `00000000-0000-4000-8000-${String(i).padStart(12, '0')}`));
  const fakeUlid = (s) => remember(s, counter(maps.ulid, s.toUpperCase(), (i) => String(i).padStart(26, '0')));
  const fakeAsset = (s) => remember(s, counter(maps.asset, s, (i) => `https://example.invalid/asset/${i}`));
  const fakeCursor = (s) => remember(s, counter(maps.cursor, s, (i) => `cursor-${i}`));
  const fakeText = (s) => remember(s, counter(maps.text, s, (i) => `text-${i}`));
  const fakeStr = (s) => remember(s, counter(maps.str, s, (i) => `str-${i}`));
  const fakeIdString = (s) => {
    if (/^\d+$/.test(s)) return String(fakeId(s.length < 16 ? Number(s) : s));
    if (UUID_RE.test(s)) return fakeUuid(s);
    if (ULID_RE.test(s)) return fakeUlid(s);
    return remember(s, counter(maps.sid, s, (i) => `id-${i}`));
  };
  const keep = (s) => {
    if (s) kept.add(s.toLowerCase());
    return s;
  };
  const redact = (s) => {
    if (!s) return s;
    secret(s);
    return '[redacted]';
  };
  function secret(v) {
    if (typeof v === 'string' && v.length >= 6) {
      secrets.add(v);
      guardRe = null;
    }
  }

  function str(v, key, ctx) {
    if (key && SECRET_KEYS.has(key)) return redact(v);
    if (v === '') return v;
    if (key && CURSOR_KEYS.has(key)) return fakeCursor(v);
    if (key && (CATEGORY_ID_KEY.test(key) || (ctx.catalog && ID_KEY.test(key))) && /^\d+$/.test(v)) return keep(v);
    if (key && ID_KEY.test(key)) return fakeIdString(v);
    if (URL_RE.test(v)) return fakeAsset(v);
    if (key === 'email' || EMAIL_RE.test(v)) return fakeEmail(v, ctx.owner);
    if (key && ctx.catalog && CATALOG_KEEP.has(key) && CATALOG_RE.test(v)) return keep(v);
    if (key && (NAME_KEY.test(key) || key === 'name')) return fakeName(v);
    if (ctx.depth === 1 && ROOT_KEEP.has(key) && v.length <= 200) return keep(v);
    if (ctx.depth === 1 && key === 'data' && SCREAMING_RE.test(v)) return keep(v);
    if (key === 'reason' && SCREAMING_RE.test(v)) return keep(v);
    if (key && FREE_TEXT.has(key)) return fakeText(v);
    if (TS_RE.test(v)) return keep(v);
    if (key && ENUM_KEYS.has(key) && ENUM_RE.test(v)) return keep(v);
    if (key && LANG_KEYS.has(key) && LANG_RE.test(v)) return keep(v);
    if (key && SCOPE_KEYS.has(key) && SCOPE_RE.test(v)) return keep(v);
    if (key && COLOR_KEYS.has(key) && HEX_RE.test(v)) return keep(v);
    if (key === 'public_key' && v.startsWith('-----BEGIN PUBLIC KEY-----')) return keep(v);
    return fakeStr(v);
  }

  function value(v, key, ctx) {
    if (Array.isArray(v)) return v.map((x) => value(x, key, ctx));
    if (v && typeof v === 'object') {
      const catalog = ctx.catalog || key === 'category' || key === 'categories';
      const owner = catalog ? null : [v.username, v.slug, v.name].find((x) => typeof x === 'string' && x && !URL_RE.test(x));
      const inner = { depth: ctx.depth + 1, catalog, owner };
      const out = {};
      for (const [k, x] of Object.entries(v)) out[k] = value(x, k, inner);
      return out;
    }
    if (typeof v === 'number') {
      if (!key || !ID_KEY.test(key) || CATEGORY_ID_KEY.test(key) || ctx.catalog) return v;
      return fakeId(v);
    }
    if (typeof v === 'string') return str(v, key, ctx);
    return v; // booleans, null
  }

  /** A JSON value (a body, params, a form), anonymized. `catalog`: it is category data. */
  const anonValue = (v, catalog = false) => value(v, null, { depth: 0, catalog });

  /** A response or request body (text). Not JSON: kept only if a few plain words (`OK`). */
  function anonBody(text, catalog = false) {
    if (!text) return text ?? '';
    try {
      return JSON.stringify(anonValue(JSON.parse(text), catalog));
    } catch {
      return PLAIN_RE.test(text.trim()) ? keep(text) : '[unparsed]';
    }
  }

  function anonSegment(seg) {
    if (seg === '' || PATH_WORDS.has(seg)) return seg;
    if (/^\d+$/.test(seg) || UUID_RE.test(seg) || ULID_RE.test(seg)) return String(fakeIdString(seg));
    return remember(seg, counter(maps.seg, seg, (i) => `seg-${i}`));
  }

  /** An SDK request ({method, url, headers, body}) in the fixtures' shape; its headers are not kept. */
  function anonRequest(req, catalog = false) {
    const u = new URL(req.url);
    const host = KICK_HOSTS.has(u.host) ? u.host : 'example.invalid';
    const params = {};
    for (const [k, v] of u.searchParams) {
      const x = /^\d{1,15}$/.test(v) ? Number(v) : v;
      params[k] = k in params ? [].concat(params[k], x) : x;
    }
    const type = (req.headers ?? []).find(([h]) => h.toLowerCase() === 'content-type')?.[1] ?? '';
    const out = { json: null, method: req.method, params: anonValue(params, catalog),
      url: `${u.protocol}//${host}${u.pathname.split('/').map(anonSegment).join('/')}` };
    if (req.body) {
      if (type.includes('json')) {
        try {
          out.json = anonValue(JSON.parse(req.body), catalog);
        } catch {
          out.json = '[unparsed]';
        }
      } else if (type.includes('x-www-form-urlencoded')) {
        out.form = anonValue(Object.fromEntries(new URLSearchParams(req.body)), catalog);
      } else out.body = '[unparsed]';
    }
    return Object.fromEntries(Object.entries(out).sort(([a], [b]) => a.localeCompare(b)));
  }

  const anonHeaders = (headers) =>
    [...headers].map(([k, v]) => [k.toLowerCase(), v]).filter(([k]) => KEEP_HEADER.test(k)).sort(([a], [b]) => a.localeCompare(b));

  // The leak guard: every real value replaced so far, searched for in what is
  // about to be written. Secrets anywhere; the rest (names, ids, texts) as
  // whole words, except where the same string was kept on purpose.
  function buildGuard() {
    const alts = [];
    for (const s of secrets) for (const f of new Set([s, JSON.stringify(s).slice(1, -1), encodeURIComponent(s)])) alts.push(escapeRe(f));
    const words = [];
    for (const r of reals.keys()) {
      if (kept.has(r)) continue;
      for (const f of new Set([r, JSON.stringify(r).slice(1, -1)])) words.push(escapeRe(f));
    }
    const parts = [];
    if (alts.length) parts.push(`(${alts.sort((a, b) => b.length - a.length).join('|')})`);
    else parts.push('((?!))');
    if (words.length) parts.push(`(?<![\\p{L}\\p{N}_])(${words.sort((a, b) => b.length - a.length).join('|')})(?![\\p{L}\\p{N}_])`);
    else parts.push('((?!))');
    const ids = [...realIds.keys()];
    parts.push(ids.length ? `(?<![0-9])(${ids.sort((a, b) => b.length - a.length).join('|')})(?![0-9])` : '((?!))');
    return new RegExp(parts.join('|'), 'giu');
  }

  /** Replaces any real value left in `v` (deep). Returns `{value, hits}`. */
  function guard(v) {
    guardRe ??= buildGuard();
    let hits = 0;
    const walk = (x) => {
      if (Array.isArray(x)) return x.map(walk);
      if (x && typeof x === 'object') return Object.fromEntries(Object.entries(x).map(([k, y]) => [k, walk(y)]));
      if (typeof x === 'number' && realIds.has(String(x))) {
        hits++;
        return realIds.get(String(x));
      }
      if (typeof x !== 'string') return x;
      return x.replace(guardRe, (m, secretHit, word, id) => {
        hits++;
        if (id) return String(realIds.get(id));
        return '[redacted]';
      });
    };
    return { value: walk(v), hits };
  }

  /**
   * One exchange as a fixture: `{elapsed_ms, kind, recorded_at, request,
   * response}`, the existing recordings' envelope, and how many leaks the
   * guard caught (should be 0: anything else is an anonymizer gap).
   */
  function anonExchange({ req, status, headers = [], body = '', elapsedMs = 0, recordedAt = new Date(), catalog = false }) {
    const fixture = {
      elapsed_ms: elapsedMs,
      kind: 'http',
      recorded_at: recordedAt.toISOString(),
      request: anonRequest(req, catalog),
      response: { body: anonBody(body, catalog), headers: anonHeaders(headers), status }
    };
    const { value: out, hits } = guard(fixture);
    return { fixture: out, hits };
  }

  /** Any message about to be printed: secrets out. */
  const scrub = (s) => {
    let out = String(s);
    for (const x of [...secrets].sort((a, b) => b.length - a.length)) out = out.split(x).join('[redacted]');
    return out;
  };

  return { secret, anonValue, anonBody, anonRequest, anonHeaders, anonExchange, guard, scrub, fakeName };
}

// ---- Private directory ------------------------------------------------------

const uid = () => process.getuid?.() ?? userInfo().uid;
export const privateDir = () => join(process.env.XDG_RUNTIME_DIR || tmpdir(), `kick-record-${uid()}`);

function ensurePrivateDir(dir) {
  if (!existsSync(dir)) mkdirSync(dir, { mode: 0o700 });
  const st = lstatSync(dir);
  if (st.isSymbolicLink() || !st.isDirectory()) die(`${dir} is not a directory: refusing to write there`);
  if (st.uid !== uid()) die(`${dir} belongs to another user: refusing to write there`);
  if (st.mode & 0o077) chmodSync(dir, 0o700);
  return dir;
}

function writePrivate(file, text) {
  const tmp = `${file}.${process.pid}.tmp`;
  writeFileSync(tmp, text, { mode: 0o600, flag: 'wx' });
  chmodSync(tmp, 0o600);
  renameSync(tmp, file);
}

function readJson(file) {
  try {
    return JSON.parse(readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

// ---- Recording --------------------------------------------------------------

let interrupted = false;
let undoing = false;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));

function die(msg) {
  console.error(`record-api: ${msg}`);
  process.exit(2);
}

function readSecretFile(env, anon) {
  const path = process.env[env];
  if (!path) die(`${env} must name a file holding the value (the value itself never goes in a variable)`);
  let st;
  try {
    st = statSync(path);
  } catch {
    die(`${env}: cannot read the file it names`);
  }
  if (st.mode & 0o077) console.error(`record-api: warning: the file ${env} names is readable by others; chmod 600 it`);
  const v = readFileSync(path, 'utf8').trim();
  if (!v) die(`${env}: the file is empty`);
  anon.secret(v);
  return v;
}

// A parser's error, without the excerpt of Kick's answer it ends with (which
// can carry a token or a name), and without secrets.
function errorText(e, anon) {
  let m = String(e?.message ?? e);
  if (e?.excerpt && m.endsWith(e.excerpt)) m = m.slice(0, -e.excerpt.length).replace(/[:\s]+$/, '');
  return anon.scrub(m).slice(0, 200);
}

function summary(x) {
  if (x === undefined || x === null) return '';
  if (Array.isArray(x)) return ` (${x.length} items)`;
  if (typeof x === 'object' && Array.isArray(x.data)) return ` (${x.data.length} items, next_cursor ${x.next_cursor ? 'yes' : 'no'})`;
  if (typeof x === 'object' && Array.isArray(x.rewards)) return ` (${x.rewards.length} rewards, next_cursor ${x.next_cursor ? 'yes' : 'no'})`;
  return '';
}

class Recorder {
  constructor(sdk, session, fixturesRoot) {
    this.sdk = sdk;
    this.session = session;
    this.anon = createAnonymizer();
    this.fixturesRoot = fixturesRoot;
    this.started = new Date();
    this.stamp = this.started.toISOString().replace(/[-:]/g, '').replace(/\.\d+/, '');
    this.priv = ensurePrivateDir(privateDir());
    this.rawDir = join(this.priv, 'raw', `${this.stamp}-${session}`);
    this.counters = { id: 0, public_api: 0 };
    this.files = [];
    this.failed = 0;
    this.leaks = 0;
  }

  /** Sends `req`, records it both ways, reads it with `parse`. Never throws for Kick's answer. */
  async call(label, req, parse, { catalog = false } = {}) {
    if (interrupted && !undoing) throw new Error('interrupted');
    await pause(400);
    const dir = new URL(req.url).host === 'id.kick.com' ? 'id' : 'public_api';
    const nnnn = String(this.counters[dir]++).padStart(4, '0');
    const at = new Date();
    const t0 = performance.now();
    let status = 0;
    let headers = [];
    let body = '';
    let network = null;
    try {
      const res = await fetch(req.url, { method: req.method, headers: req.headers, body: req.body ?? undefined,
        redirect: 'manual', signal: AbortSignal.timeout(20000) });
      status = res.status;
      headers = [...res.headers];
      body = await res.text();
    } catch (e) {
      network = String(e?.cause?.code ?? e?.name ?? 'error');
    }
    const elapsedMs = Math.round(performance.now() - t0);
    ensurePrivateDir(join(this.priv, 'raw'));
    ensurePrivateDir(this.rawDir);
    // The private copy: Kick's answer as it came. The request's credentials
    // (the bearer token, the client secret, a token in the URL) are left out:
    // the answer is what the parsers need.
    const u = new URL(req.url);
    if (u.searchParams.has('token')) u.searchParams.set('token', '[redacted]');
    const rawReq = { method: req.method, url: u.href,
      headers: req.headers.map(([k, v]) => [k, k.toLowerCase() === 'authorization' ? '[redacted]' : v]),
      body: req.body ? req.body.replace(/(client_secret=)[^&]*/, '$1[redacted]') : null };
    writePrivate(join(this.rawDir, `${dir}__${nnnn}-${label}.json`), JSON.stringify(
      { recorded_at: at.toISOString(), elapsed_ms: elapsedMs, request: rawReq, network_error: network,
        response: network ? null : { status, headers, body } }, null, 2));
    if (network) {
      this.failed++;
      console.log(`${dir}/${nnnn} ${label}  network error: ${network}`);
      return { ok: false, status: 0, body: '', json: null, error: new Error(`network error: ${network}`) };
    }
    const { fixture, hits } = this.anon.anonExchange({ req, status, headers, body, elapsedMs, recordedAt: at, catalog });
    const file = join(this.fixturesRoot, dir, `${this.stamp}-${this.session}__${nnnn}-${label}.json`);
    mkdirSync(join(this.fixturesRoot, dir), { recursive: true });
    writeFileSync(file, JSON.stringify(fixture, null, 2));
    this.files.push(relative(REPO, file));
    this.leaks += hits;
    let parsed;
    let error = null;
    try {
      parsed = parse(status, body);
    } catch (e) {
      error = e;
      this.failed++;
    }
    const verdict = error
      ? `parse error: ${errorText(error, this.anon)}${error.status !== undefined ? ` (Kick error ${error.status})` : ''}`
      : `parse ok${summary(parsed)}`;
    console.log(`${dir}/${nnnn} ${label}  ${status}  ${verdict}${hits ? `  [guard replaced ${hits} leftover value(s): read the file]` : ''}`);
    let json = null;
    try {
      json = JSON.parse(body);
    } catch {}
    return { ok: !error, status, body, json, parsed, error };
  }

  finish(phase, extra = {}) {
    const stateFile = join(this.priv, 'state.json');
    const state = readJson(stateFile) ?? { phases: {} };
    state.phases[phase] = { at: this.started.toISOString(), session: `${this.stamp}-${this.session}`, raw: this.rawDir,
      fixtures: this.files, failed: this.failed, guard_hits: this.leaks, ...extra };
    writePrivate(stateFile, JSON.stringify(state, null, 2));
    console.log(`\n${this.files.length} fixtures (${this.failed} failed); raw exchanges in ${this.rawDir}`);
    if (this.leaks) console.log(`The leak guard replaced ${this.leaks} value(s) the anonymizer missed: read those files before committing.`);
    console.log('Read every fixture before committing it (CONTRIBUTING.md).');
  }
}

function must(r, what, anon) {
  if (!r.ok) throw new Error(`${what}: ${errorText(r.error, anon)}`);
  return r.parsed;
}

async function loadSdk() {
  const sdk = await import(new URL('../bindings/js/index.js', import.meta.url).href);
  const wasm = new URL('../bindings/js/pkg/kicksdk_bg.wasm', import.meta.url);
  if (!existsSync(wasm)) die('bindings/js/pkg is not built: run scripts/build-js.sh');
  await sdk.init(readFileSync(wasm));
  return sdk;
}

const tokenFile = () => join(privateDir(), 'user-token.json');

function loadUserToken(anon) {
  const t = readJson(tokenFile());
  if (!t?.access_token) die('no stored user token: run `login` first');
  anon.secret(t.access_token);
  anon.secret(t.refresh_token);
  return t;
}

function storeUserToken(token) {
  ensurePrivateDir(privateDir());
  writePrivate(tokenFile(), JSON.stringify({ ...token, obtained_at: new Date().toISOString() }, null, 2));
}

// ---- app --------------------------------------------------------------------

async function cmdApp(sdk, opts) {
  const rec = new Recorder(sdk, 'app', opts.fixtures);
  const a = rec.anon;
  const cid = readSecretFile('KICK_CLIENT_ID_FILE', a);
  const csec = readSecretFile('KICK_CLIENT_SECRET_FILE', a);
  try {
    const t = await rec.call('app-token', sdk.appTokenRequest(cid, csec), sdk.parseAppToken);
    const tok = must(t, 'app token', a).access_token;
    a.secret(tok);

    await rec.call('public-key', sdk.publicKeyRequest(), sdk.parsePublicKey);
    await rec.call('livestreams-stats', sdk.livestreamStatsRequest(tok), sdk.parseLivestreamStats);

    const p1 = await rec.call('v2-livestreams', sdk.livestreamsPageRequest(tok, null, null, 25, null), sdk.parseLivestreamsPage);
    const streams = Array.isArray(p1.json?.data) ? p1.json.data : [];
    const cursor = p1.json?.pagination?.next_cursor ?? p1.json?.next_cursor ?? p1.parsed?.next_cursor;
    if (cursor) await rec.call('v2-livestreams-page2', sdk.livestreamsPageRequest(tok, null, null, 25, cursor), sdk.parseLivestreamsPage);
    const first = streams[0];
    const catId = first?.category?.id;
    if (Number.isSafeInteger(catId)) {
      await rec.call('v2-livestreams-category', sdk.livestreamsPageRequest(tok, [catId], null, 10, null), sdk.parseLivestreamsPage);
    }
    const lang = typeof first?.language_code === 'string' && LANG_RE.test(first.language_code) ? first.language_code : 'en';
    await rec.call('v2-livestreams-language', sdk.livestreamsPageRequest(tok, null, [lang], 10, null), sdk.parseLivestreamsPage);
    await rec.call('v2-livestreams-bad-cursor', sdk.livestreamsPageRequest(tok, null, null, 5, 'not-a-real-cursor'), sdk.parseLivestreamsPage);

    const ids = streams.map((s) => s?.broadcaster_user?.id).filter(Number.isSafeInteger).slice(0, 3);
    const offline = opts['offline-id'] ? Number(opts['offline-id']) : readJson(join(privateDir(), 'state.json'))?.own_user_id;
    const withOffline = Number.isSafeInteger(offline) && !ids.includes(offline) ? [...ids, offline] : ids;
    if (withOffline.length) {
      for (const req of sdk.livestreamsRequests(tok, withOffline)) await rec.call('users-livestreams', req, sdk.parseLivestreams);
      await rec.call('users', sdk.usersRequest(tok, withOffline), sdk.parseUsers);
    } else console.log('(no live stream on the first page: users and users-livestreams skipped)');
    await rec.call('users-unknown-id', sdk.usersRequest(tok, [2147483646]), sdk.parseUsers);
    await rec.call('users-livestreams-unknown-id', sdk.livestreamsRequests(tok, [2147483646])[0], sdk.parseLivestreams);

    const catName = opts['category-name'] ?? (typeof first?.category?.name === 'string' && first.category.name.length >= 3
      && !first.category.name.includes(',') ? first.category.name : 'Just Chatting');
    const cat = { catalog: true };
    await rec.call('categories-by-name', sdk.categoriesRequest(tok, [catName], null, null, 10, null), sdk.parseCategories, cat);
    if (Number.isSafeInteger(catId)) await rec.call('categories-by-id', sdk.categoriesRequest(tok, null, null, [catId], null, null), sdk.parseCategories, cat);
    const c1 = await rec.call('categories-page', sdk.categoriesRequest(tok, null, null, null, 5, null), sdk.parseCategories, cat);
    const cc = c1.json?.pagination?.next_cursor ?? c1.parsed?.next_cursor;
    if (cc) await rec.call('categories-page2', sdk.categoriesRequest(tok, null, null, null, 5, cc), sdk.parseCategories, cat);
  } finally {
    rec.finish('app');
  }
}

// ---- login ------------------------------------------------------------------

function redirectTarget() {
  const raw = process.env.KICK_REDIRECT_URI;
  if (!raw) die('KICK_REDIRECT_URI must be set: the redirect URL registered in the Kick app, e.g. http://localhost:8765/callback');
  let u;
  try {
    u = new URL(raw);
  } catch {
    die('KICK_REDIRECT_URI is not a URL');
  }
  if (u.protocol !== 'http:' || !['localhost', '127.0.0.1', '[::1]'].includes(u.hostname)) {
    die('KICK_REDIRECT_URI must be http://localhost:<port>/<path> or http://127.0.0.1:<port>/<path>: the recorder listens there');
  }
  return { raw, url: u, port: Number(u.port || 80) };
}

// The docs' workaround for a 127.0.0.1 redirect: Kick's frontend (NextJS)
// rewrites the URL's first `127.0.0.1` to `localhost`, so a throwaway
// parameter holding it goes before redirect_uri. The SDK does not do this.
export function with127Workaround(authorizeUrl) {
  const out = authorizeUrl.replace('&redirect_uri=', '&redirect=127.0.0.1&redirect_uri=');
  if (out === authorizeUrl) throw new Error('no redirect_uri in the authorize URL');
  return out;
}

function waitForCallback(target, state) {
  const hosts = target.url.hostname === 'localhost' ? ['127.0.0.1', '::1'] : [target.url.hostname.replace(/^\[|\]$/g, '')];
  return new Promise((resolve, reject) => {
    const servers = [];
    const done = (err, val) => {
      clearTimeout(timer);
      for (const s of servers) s.close();
      err ? reject(err) : resolve(val);
    };
    const timer = setTimeout(() => done(new Error('no callback within 10 minutes')), 600000);
    const handler = (req, res) => {
      const u = new URL(req.url, target.url);
      if (req.method !== 'GET' || u.pathname !== target.url.pathname) {
        res.writeHead(404).end();
        return;
      }
      res.writeHead(200, { 'content-type': 'text/plain; charset=utf-8' }).end('kicksdk recorder: received. You can close this tab.\n');
      const got = Buffer.from(u.searchParams.get('state') ?? '');
      const want = Buffer.from(state);
      if (got.length !== want.length || !timingSafeEqual(got, want)) return done(new Error('the callback\'s state is not ours: ignored, nothing exchanged'));
      const error = u.searchParams.get('error');
      if (error) return done(new Error(`Kick refused the authorization: ${/^[\w.-]{1,40}$/.test(error) ? error : '[unreadable]'}`));
      const code = u.searchParams.get('code');
      if (!code) return done(new Error('the callback has no code'));
      done(null, code);
    };
    for (const h of hosts) {
      const s = createServer(handler);
      servers.push(s);
      s.on('error', (e) => {
        // ::1 may not exist on this machine; one listener is enough.
        if (hosts.length > 1 && h === '::1') return;
        done(new Error(`cannot listen on ${h}:${target.port}: ${e.code}`));
      });
      s.listen(target.port, h);
    }
  });
}

async function cmdLogin(sdk, opts) {
  const rec = new Recorder(sdk, 'login', opts.fixtures);
  const a = rec.anon;
  const target = redirectTarget();
  const cid = readSecretFile('KICK_CLIENT_ID_FILE', a);
  const csec = readSecretFile('KICK_CLIENT_SECRET_FILE', a);
  const pkce = sdk.oauthPkce(new Uint8Array(randomBytes(32)));
  const state = sdk.newState();
  a.secret(pkce.verifier);
  a.secret(state);
  let url = sdk.oauthAuthorizeUrl(cid, target.raw, sdk.oauthScopes(), state, pkce.verifier);
  if (target.url.hostname === '127.0.0.1') url = with127Workaround(url);
  const callback = waitForCallback(target, state);
  console.log('Open this URL, signed in to Kick as the account to record with:\n');
  console.log(url);
  console.log('');
  try {
    const code = await callback;
    a.secret(code);
    const r = await rec.call('oauth-code', sdk.oauthCodeRequest(cid, csec, target.raw, code, pkce.verifier), sdk.oauthParseUserToken);
    if (r.ok) {
      a.secret(r.parsed.access_token);
      a.secret(r.parsed.refresh_token);
      storeUserToken(r.parsed);
      console.log(`user token stored in ${tokenFile()} (${r.parsed.scopes.length} scopes)`);
    } else console.log('no user token stored');
  } catch (e) {
    console.log(`login failed: ${a.scrub(e.message)}`);
    process.exitCode = 1;
  } finally {
    rec.finish('login');
  }
}

// ---- user-reads -------------------------------------------------------------

async function cmdUserReads(sdk, opts) {
  const rec = new Recorder(sdk, 'user-reads', opts.fixtures);
  const a = rec.anon;
  const t = loadUserToken(a);
  const tok = t.access_token;
  let me;
  try {
    await rec.call('introspect', sdk.oauthIntrospectRequest(tok), sdk.oauthParseIntrospect);
    const u = await rec.call('users-me', sdk.usersRequest(tok), sdk.parseUsers);
    me = u.ok ? u.parsed[0]?.user_id : undefined;
    await rec.call('kicks-leaderboard', sdk.kicksLeaderboardRequest(tok, 10), sdk.parseKicksLeaderboard);
    await rec.call('rewards-list', sdk.rewardsListRequest(tok), sdk.rewardsParseRewards);
    for (const s of ['pending', 'accepted', 'rejected']) {
      await rec.call(`redemptions-${s}`, sdk.rewardsRedemptionsRequest(tok, null, s, null), sdk.rewardsParseRedemptions);
    }
    await rec.call('ads-status', sdk.adsStatusRequest(tok), sdk.adsParseStatus);
    if (Number.isSafeInteger(me)) await rec.call('channel-by-user-id', sdk.channelsByIdsRequest(tok, [me]), sdk.parseChannels);
    if (t.refresh_token) {
      const cid = readSecretFile('KICK_CLIENT_ID_FILE', a);
      const csec = readSecretFile('KICK_CLIENT_SECRET_FILE', a);
      const r = await rec.call('oauth-refresh', sdk.oauthRefreshRequest(cid, csec, t.refresh_token), sdk.oauthParseUserToken);
      if (r.ok) {
        a.secret(r.parsed.access_token);
        a.secret(r.parsed.refresh_token);
        storeUserToken(r.parsed);
        console.log('refreshed: the new token is stored');
      }
    } else console.log('(no refresh token stored: refresh not recorded)');
  } finally {
    rec.finish('user-reads');
    if (Number.isSafeInteger(me)) {
      const f = join(privateDir(), 'state.json');
      const st = readJson(f);
      st.own_user_id = me;
      writePrivate(f, JSON.stringify(st, null, 2));
    }
  }
}

// ---- actions ----------------------------------------------------------------

const TEST_TEXT = 'kicksdk test, please ignore';
const PLAN = `Plan (every step recorded; every change undone in a finally, even when a step fails):
  1. chat: send "${TEST_TEXT}" as the token's user in their own channel -> delete it
  2. chat: send a message, send a reply to it (reply_to_message_id) -> delete both
  3. chat: send as the app's bot (recorded whatever Kick answers) -> delete it if sent
  4. moderation: find <target>'s user id (channels by slug); timeout 1 minute with a reason -> unban;
     ban -> unban. A timeout outside 1..10080 minutes is refused by the SDK itself: nothing sent
  5. channel: read title/category/tags; PATCH a test title -> PATCH the original title/category/tags back;
     read again
  6. rewards: create "kicksdk test" (cost 1, disabled) -> rename it -> list -> delete it
  7. errors: delete a message id that does not exist
Not recorded (changes that cannot be undone): ads ad-break, ads enroll, drops claims and updates,
redemptions accept/reject.`;

async function cmdActions(sdk, opts) {
  const slug = opts.target;
  if (!slug || !/^[A-Za-z0-9_-]{1,64}$/.test(slug)) die('actions needs --target <slug>: the user to time out and ban (then unban) in your own channel');
  const rec = new Recorder(sdk, 'actions', opts.fixtures);
  const a = rec.anon;
  const tok = loadUserToken(a).access_token;
  const failures = [];
  const step = async (name, fn) => {
    if (interrupted) return;
    console.log(`-- ${name}`);
    const undo = [];
    try {
      await fn(undo);
    } catch (e) {
      console.log(`   step failed: ${a.scrub(e.message)}`);
    } finally {
      undoing = true;
      while (undo.length) {
        const u = undo.pop();
        let ok = false;
        try {
          ok = (await u.run()).ok;
        } catch (e) {
          console.log(`   ${a.scrub(e.message)}`);
        }
        if (!ok) {
          failures.push(u.what);
          console.log(`   UNDO FAILED: ${u.what}`);
        }
      }
      undoing = false;
    }
  };
  try {
    const u = await rec.call('users-me', sdk.usersRequest(tok), sdk.parseUsers);
    const me = must(u, 'users-me', a)[0]?.user_id;
    if (!Number.isSafeInteger(me)) throw new Error('users-me gave no user id');
    const send = async (label, content, replyTo, sender = { user: me }) => {
      const r = await rec.call(label, sdk.chatSendRequest(tok, sender, content, replyTo ?? null), sdk.chatParseSend);
      return r.ok && r.parsed.is_sent && r.parsed.message_id ? r.parsed.message_id : null;
    };
    const deleting = (undo, label, id, what) =>
      undo.push({ what: `${what} (message ${id}) is still in chat: delete it`, run: () => rec.call(label, sdk.chatDeleteRequest(tok, id), sdk.parseDone) });

    await step('1. chat send -> delete', async (undo) => {
      const id = await send('chat-send', TEST_TEXT);
      if (!id) throw new Error('not sent');
      deleting(undo, 'chat-delete', id, 'the test message');
    });

    await step('2. chat reply -> delete both', async (undo) => {
      const parent = await send('chat-send-reply-parent', TEST_TEXT);
      if (!parent) throw new Error('not sent');
      deleting(undo, 'chat-delete-reply-parent', parent, 'the test message');
      const reply = await send('chat-send-reply', `${TEST_TEXT} (reply)`, parent);
      if (!reply) throw new Error('reply not sent');
      deleting(undo, 'chat-delete-reply', reply, 'the test reply');
    });

    await step('3. chat send as bot', async (undo) => {
      const id = await send('chat-send-bot', TEST_TEXT, null, 'bot');
      if (id) deleting(undo, 'chat-delete-bot', id, 'the bot message');
    });

    await step('4. moderation', async (undo) => {
      const c = await rec.call('channel-by-slug', sdk.channelsBySlugsRequest(tok, [slug]), sdk.parseChannels);
      const target = must(c, 'channel-by-slug', a)[0]?.broadcaster_user_id;
      if (!Number.isSafeInteger(target)) throw new Error('--target not found');
      if (target === me) throw new Error('--target is the token\'s own user: pick someone else');
      try {
        sdk.moderationTimeoutRequest(tok, me, target, 10081, 'kicksdk test');
        console.log('   the SDK built a 10081-minute timeout: it should refuse it');
      } catch (e) {
        console.log(`   10081-minute timeout refused by the SDK, nothing sent: ${a.scrub(e.message)}`);
      }
      const unban = (label, what) => ({ what, run: () => rec.call(label, sdk.moderationUnbanRequest(tok, me, target), sdk.parseDone) });
      const t = await rec.call('moderation-timeout', sdk.moderationTimeoutRequest(tok, me, target, 1, 'kicksdk test'), sdk.parseDone);
      if (t.ok) {
        const un = unban('moderation-unban-timeout', `--target is still timed out in your channel: lift it`);
        const r = await un.run();
        if (!r.ok) undo.push(unban('moderation-unban-timeout-retry', un.what));
      }
      const b = await rec.call('moderation-ban', sdk.moderationBanRequest(tok, me, target, 'kicksdk test'), sdk.parseDone);
      // Unbanned even when Kick's answer did not read: it may have banned anyway.
      if (b.status) undo.push(unban('moderation-unban', '--target is still banned in your channel: unban them'));
    });

    await step('5. channel title', async (undo) => {
      const read = async (label) => {
        const r = await rec.call(label, sdk.channelsByIdsRequest(tok, [me]), sdk.parseChannels);
        const ch = must(r, label, a)[0];
        const raw = r.json?.data?.[0] ?? {};
        const tags = [raw.stream?.custom_tags, raw.custom_tags, raw.stream?.tags].find(Array.isArray) ?? null;
        return { title: ch?.stream_title ?? null, category: ch?.category?.id ?? null, tags };
      };
      const before = await read('channel-before');
      if (!before.title) throw new Error('the channel has no title to restore: step skipped');
      undo.push({
        what: 'the channel title/category/tags may not be restored: check them',
        run: async () => {
          const r = await rec.call('channel-update-restore',
            sdk.channelsUpdateRequest(tok, before.category, before.tags, before.title), sdk.parseDone);
          if (r.ok) {
            const after = await read('channel-after');
            if (after.title !== before.title || after.category !== before.category) return { ok: false };
          }
          return r;
        }
      });
      if (!before.tags) console.log('   (the answer carries no custom tags: they are not sent back, and the test does not change them)');
      await rec.call('channel-update-title', sdk.channelsUpdateRequest(tok, null, null, 'kicksdk test title'), sdk.parseDone);
    });

    await step('6. rewards', async (undo) => {
      const c = await rec.call('rewards-create',
        sdk.rewardsCreateRequest(tok, { title: 'kicksdk test', cost: 1, is_enabled: false }), sdk.rewardsParseReward);
      const id = must(c, 'rewards-create', a).id;
      undo.push({ what: `the test reward ${id} still exists: delete it`, run: () => rec.call('rewards-delete', sdk.rewardsDeleteRequest(tok, id), sdk.parseDone) });
      await rec.call('rewards-update', sdk.rewardsUpdateRequest(tok, id, { title: 'kicksdk test 2' }), sdk.rewardsParseReward);
      await rec.call('rewards-list', sdk.rewardsListRequest(tok), sdk.rewardsParseRewards);
    });

    await step('7. errors', async () => {
      await rec.call('chat-delete-unknown', sdk.chatDeleteRequest(tok, randomUUID()), sdk.parseDone);
    });
  } catch (e) {
    console.log(`actions stopped: ${a.scrub(e.message)}`);
    process.exitCode = 1;
  } finally {
    console.log('\nNot recorded: ads ad-break, ads enroll, drops claims/update, redemptions accept/reject.');
    if (failures.length) {
      console.log('\nLEFT TO UNDO BY HAND:');
      for (const f of failures) console.log(`  - ${f}`);
      process.exitCode = 1;
    }
    rec.finish('actions', { undo_failed: failures });
  }
}

// ---- revoke -----------------------------------------------------------------

async function cmdRevoke(sdk, opts) {
  const rec = new Recorder(sdk, 'revoke', opts.fixtures);
  const a = rec.anon;
  const t = loadUserToken(a);
  let ok = true;
  try {
    const r1 = await rec.call('revoke-access', sdk.oauthRevokeRequest(t.access_token, 'access_token'), sdk.oauthParseRevoke);
    ok &&= r1.ok;
    if (t.refresh_token) {
      const r2 = await rec.call('revoke-refresh', sdk.oauthRevokeRequest(t.refresh_token, 'refresh_token'), sdk.oauthParseRevoke);
      ok &&= r2.ok;
    }
    await rec.call('introspect-revoked', sdk.oauthIntrospectRequest(t.access_token), sdk.oauthParseIntrospect);
  } finally {
    if (ok) {
      rmSync(tokenFile(), { force: true });
      console.log('stored user token deleted');
    } else console.log(`a revoke failed: the token is kept in ${tokenFile()} to try again`);
    rec.finish('revoke');
  }
}

// ---- status -----------------------------------------------------------------

function cmdStatus(opts) {
  const priv = privateDir();
  console.log(`private dir: ${priv}${existsSync(priv) ? '' : ' (not created yet)'}  (raw exchanges, user token: never commit)`);
  console.log(`fixtures:    ${join(opts.fixtures, 'public_api')}, ${join(opts.fixtures, 'id')}`);
  const t = readJson(tokenFile());
  console.log(`user token:  ${t?.access_token ? `stored, obtained ${t.obtained_at ?? '?'}, ${t.scopes?.length ?? 0} scopes` : 'none'}`);
  const state = readJson(join(priv, 'state.json'));
  for (const p of ['app', 'login', 'user-reads', 'actions', 'revoke']) {
    const s = state?.phases?.[p];
    if (!s) {
      console.log(`  ${p.padEnd(10)} not run`);
      continue;
    }
    const present = s.fixtures.filter((f) => existsSync(join(REPO, f))).length;
    console.log(`  ${p.padEnd(10)} ${s.at}  ${s.fixtures.length} fixtures (${present} still there), ${s.failed} failed` +
      `${s.guard_hits ? `, guard hits ${s.guard_hits}` : ''}${s.undo_failed?.length ? `, UNDO FAILED ${s.undo_failed.length}` : ''}`);
    console.log(`  ${''.padEnd(10)} session ${s.session}; raw ${s.raw}${existsSync(s.raw) ? ` (${readdirSync(s.raw).length} files)` : ' (gone)'}`);
  }
}

// ---- CLI --------------------------------------------------------------------

const USAGE = `usage: node tools/record-api.mjs <app|login|user-reads|actions|revoke|status> [options]
  env (paths to files, never the values): KICK_CLIENT_ID_FILE, KICK_CLIENT_SECRET_FILE
  env: KICK_REDIRECT_URI (login; the app's registered redirect, http://localhost:<port>/<path>)
  app        [--offline-id <user id>] [--category-name <name>]
  actions    --target <slug> [--yes]   (without --yes: print the plan and exit)
  any        [--fixtures <dir>]        (default: the repository's fixtures/)`;

async function main(argv) {
  let parsed;
  try {
    parsed = parseArgs({ args: argv, allowPositionals: true, options: {
      target: { type: 'string' }, yes: { type: 'boolean' }, 'offline-id': { type: 'string' },
      'category-name': { type: 'string' }, fixtures: { type: 'string' }, help: { type: 'boolean', short: 'h' } } });
  } catch (e) {
    die(`${e.message}\n${USAGE}`);
  }
  const { values: opts, positionals } = parsed;
  const cmd = positionals[0];
  opts.fixtures = resolve(opts.fixtures ?? join(REPO, 'fixtures'));
  if (opts.fixtures.startsWith(privateDir())) die('--fixtures may not be in the private dir');
  if (opts.help || !cmd) {
    console.log(USAGE);
    return;
  }
  if (cmd === 'status') return cmdStatus(opts);
  if (cmd === 'actions' && !opts.yes) {
    console.log(PLAN.replaceAll('<target>', opts.target ?? '<target>'));
    console.log('\nNothing sent. Run again with --yes to do it, on the account whose token is stored.');
    return;
  }
  const commands = { app: cmdApp, login: cmdLogin, 'user-reads': cmdUserReads, actions: cmdActions, revoke: cmdRevoke };
  if (!commands[cmd]) die(`unknown command ${cmd}\n${USAGE}`);
  process.on('SIGINT', () => {
    if (interrupted) process.exit(130);
    interrupted = true;
    console.log('\ninterrupted: undoing what was changed (Ctrl-C again to quit now)');
  });
  await commands[cmd](await loadSdk(), opts);
}

// Only when run: importing this file (to try the anonymizer) records nothing.
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv.slice(2)).then(
    () => process.exit(process.exitCode ?? 0),
    (e) => {
      // A message here may come from anywhere: print its kind and where, not its text.
      const at = String(e?.stack ?? '').split('\n').find((l) => l.includes('record-api.mjs'))?.trim() ?? '';
      console.error(`record-api: failed (${e?.name ?? 'error'}) ${at}; the message is not printed, as it may hold a secret`);
      process.exit(1);
    }
  );
}
