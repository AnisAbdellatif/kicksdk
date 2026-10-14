// kicksdk for JavaScript: the WebAssembly build of the Rust core, plus
// connectChat(), which drives the realtime protocol with fetch, WebSocket and
// timers.
//
// Call `await init()` once before anything else (in Node, or anywhere the .wasm
// cannot be fetched beside this file, pass its bytes: `init(bytes)`).
//
// Where it works: Kick's realtime server only takes a socket whose Origin is
// https://kick.com. Node sets that header itself (see `socketFactory` below); a
// browser cannot, so in a browser connectChat only works on kick.com - a page
// there, or an extension's content script. Elsewhere, relay through a server.

import initWasm, {
  initSync,
  Session,
  Backoff,
  chatChannels,
  chatConnectionRequest,
  connectionRequest,
  parseConnection,
  tokenRequest,
  parseToken,
  shouldReconnect
} from './pkg/kicksdk.js';

export * from './pkg/kicksdk.js';

export const REQUIRED_ORIGIN = 'https://kick.com';

let ready = null;

/**
 * Loads the WebAssembly. Safe to call more than once; if loading fails, the
 * next call tries again.
 */
export function init(input) {
  if (!ready) {
    const bytes =
      input instanceof Uint8Array ||
      input instanceof ArrayBuffer ||
      (typeof WebAssembly !== 'undefined' && input instanceof WebAssembly.Module);
    if (bytes) {
      try {
        initSync({ module: input });
      } catch (e) {
        return Promise.reject(e);
      }
      ready = Promise.resolve();
    } else {
      // A failed load (a fetch that did not get through) is not kept: a
      // later init() starts over instead of returning the same rejection.
      const loading = initWasm(input === undefined ? undefined : { module_or_path: input }).then(
        () => undefined,
        (e) => {
          if (ready === loading) ready = null;
          throw e;
        }
      );
      ready = loading;
    }
  }
  return ready;
}

const isNode = typeof process !== 'undefined' && !!(process.versions && process.versions.node);

/** How long one step of connecting (a fetch, the WebSocket handshake) may take. */
const DEFAULT_TIMEOUT_MS = 10_000;

const NO_WEBSOCKET =
  'no WebSocket in this runtime (Node 22 and later have one): pass connectChat a `WebSocket` ' +
  "constructor (the `ws` package's will do) or `createSocket`";

const noop = () => {};

const message = (e) => String((e && e.message) || e);

/** A clock that only goes forward, so a wall-clock jump cannot fake silence. */
function monotonicClock() {
  const p = globalThis.performance;
  return p && typeof p.now === 'function' ? () => p.now() : () => Date.now();
}

/** A guest client id, a v4 UUID. Not a secret: it names the connection. */
function guestId() {
  const c = globalThis.crypto;
  // randomUUID is missing outside secure contexts, and Node 18 has no
  // global crypto at all.
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  const b = new Uint8Array(16);
  if (c && typeof c.getRandomValues === 'function') c.getRandomValues(b);
  else for (let i = 0; i < b.length; i++) b[i] = Math.floor(Math.random() * 256);
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

function socketFactory(o) {
  if (o.createSocket) return (url) => o.createSocket(url);
  const WS = o.WebSocket || globalThis.WebSocket;
  if (typeof WS !== 'function') return null;
  // Node's WebSocket (undici) and the `ws` package take headers; a browser's
  // would read the object as a subprotocol, and sends its page's origin
  // whatever it is given.
  return isNode ? (url) => new WS(url, { headers: { Origin: REQUIRED_ORIGIN } }) : (url) => new WS(url);
}

/**
 * Connects to one channel's chat and keeps the connection up until close(),
 * or until Kick ends it for good: a terminal close code, after which onDrop
 * says `reconnect: false` and onState `stopped: true`.
 *
 * Callbacks are isolated: one that throws is reported to `onError` (or
 * console.error) and the connection carries on. None is called after
 * close(), which may be called from inside one.
 *
 * @param {object} o  ChatOptions, in index.d.ts
 * @returns {{ close(): void }}
 */
export function connectChat(o) {
  if (!o || typeof o.onEvent !== 'function') throw new TypeError('connectChat: onEvent is required');
  const timeoutMs = o.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  if (typeof timeoutMs !== 'number' || !(timeoutMs > 0)) {
    throw new RangeError('connectChat: timeoutMs must be a positive number');
  }
  // Ids and endpoints are checked here, where a mistake throws, rather than
  // failing every reconnect.
  const channels = o.channels ? [...o.channels] : chatChannels(o.chatroomId, o.channelId);
  const hasChannel = o.channelId !== undefined && o.channelId !== null;
  if (hasChannel) chatConnectionRequest(o.channelId, 'x', o.endpoints);
  else connectionRequest('x', o.endpoints);

  const fetchImpl = o.fetch || ((...a) => globalThis.fetch(...a));
  const createSocket = socketFactory(o);
  const clock = o.now || monotonicClock();
  const now = () => Math.max(0, Math.floor(clock()));

  let session = new Session(channels);
  let backoff = new Backoff();
  let closed = false; // close() was called
  let stopped = false; // Kick ended it for good
  let ws = null;
  let attempt = null; // aborts the current connection's fetches
  let clientId = null;
  let timer = null; // session.deadline()
  let handshake = null;
  let retry = null;

  const live = () => !closed && !stopped;

  function report(error) {
    if (o.onError) {
      try {
        o.onError(error);
        return;
      } catch (e) {
        error = e;
      }
    }
    console.error('kicksdk connectChat:', error);
  }

  /** A user callback, by name: never after close(), and never fatal. */
  function callback(name, ...args) {
    const fn = o[name];
    if (closed || typeof fn !== 'function') return;
    try {
      fn.apply(o, args);
    } catch (e) {
      report(e);
    }
  }

  /** An entry point (a socket event, a timer): a failure ends the connection, not the driver. */
  function guard(fn) {
    return (...args) => {
      try {
        return fn(...args);
      } catch (e) {
        report(e);
        if (live()) end(`kicksdk: ${message(e)}`, 0, true);
      }
    };
  }

  /** A fetch that gives up after timeoutMs, or when `signal` aborts. */
  function request(req, what, signal) {
    return new Promise((resolve, reject) => {
      const ctrl = new AbortController();
      const cleanup = () => {
        clearTimeout(timeout);
        signal.removeEventListener('abort', onAbort);
      };
      const fail = (e) => {
        cleanup();
        ctrl.abort(e);
        reject(e);
      };
      const onAbort = () => fail(new Error(`${what}: cancelled`));
      const timeout = setTimeout(() => fail(new Error(`${what}: no answer in ${timeoutMs} ms`)), timeoutMs);
      if (signal.aborted) return onAbort();
      signal.addEventListener('abort', onAbort);
      (async () => {
        // A browser drops the headers it forbids (origin, referer) and sends its own.
        const res = await fetchImpl(req.url, {
          method: req.method,
          headers: Object.fromEntries(req.headers),
          body: req.body ?? undefined,
          credentials: 'omit',
          signal: ctrl.signal
        });
        return { status: res.status, body: await res.text() };
      })().then(
        (r) => {
          cleanup();
          resolve(r);
        },
        (e) => {
          cleanup();
          reject(e);
        }
      );
    });
  }

  function state() {
    if (!live()) return;
    callback('onState', {
      connected: session.isConnected(),
      subscribed: channels.filter((c) => session.isSubscribed(c)),
      stopped: false
    });
  }

  function arm() {
    clearTimeout(timer);
    timer = null;
    if (!live()) return;
    const at = session.deadline();
    if (at === undefined || at === null) return;
    timer = setTimeout(
      guard(() => {
        timer = null;
        if (live()) apply(session.tick(now()));
      }),
      Math.max(0, at - now())
    );
  }

  function apply(actions) {
    let changed = false;
    for (const a of actions) {
      // A callback may have called close().
      if (!live()) return;
      switch (a.action) {
        case 'send':
          if (ws && ws.readyState === 1) ws.send(a.text);
          break;
        case 'close':
          // Kick said goodbye, or went quiet. The socket is finished now: a
          // dead connection may not deliver its close event for minutes.
          end(a.reason, a.code, a.reconnect);
          return;
        case 'refresh_token':
          refresh();
          break;
        case 'subscribed':
          if (a.channel === channels[0]) backoff.reset();
          changed = true;
          break;
        case 'connected':
        case 'unsubscribed':
        case 'subscribe_failed':
          changed = true;
          break;
        case 'event':
          callback('onEvent', { channel: a.channel, name: a.name, data: a.data, event: a.event });
          break;
      }
    }
    if (changed) state();
    arm();
  }

  function refresh() {
    const ctrl = attempt;
    if (!ctrl) return;
    request(tokenRequest(clientId, o.endpoints), 'token refresh', ctrl.signal)
      .then((r) => parseToken(r.status, r.body))
      .then(
        guard((token) => {
          if (live() && attempt === ctrl) apply(session.refresh(token, now()));
        }),
        // Kick ends the connection when the old token runs out; the
        // reconnect fetches a new one.
        noop
      );
  }

  /** Lets go of the socket, without waiting for it, and of what hangs on it. */
  function detach() {
    clearTimeout(timer);
    clearTimeout(handshake);
    timer = handshake = null;
    if (attempt) {
      attempt.abort();
      attempt = null;
    }
    const s = ws;
    ws = null;
    if (s) {
      s.onopen = s.onmessage = s.onclose = null;
      // The `ws` package emits an error on closing a socket still
      // connecting, and throws one that nobody listens to.
      s.onerror = noop;
      try {
        s.close();
      } catch {
        // Closed already.
      }
    }
  }

  function release() {
    if (session) session.free();
    if (backoff) backoff.free();
    session = backoff = null;
  }

  /** A connection, or an attempt at one, ended: try again, or give up. */
  function drop(reason, code, reconnect) {
    if (!live()) return;
    callback('onDrop', reason, { code, reconnect });
    if (!live()) return;
    clearTimeout(retry);
    if (reconnect) {
      retry = setTimeout(start, backoff.nextDelayJittered(Math.random()));
      return;
    }
    stopped = true;
    retry = null;
    detach();
    release();
    callback('onState', { connected: false, subscribed: [], stopped: true });
  }

  /** The socket is finished, however it ended. */
  function end(reason, code, reconnect) {
    detach();
    if (!live()) return;
    session.closed();
    if (reconnect) state();
    drop(reason, code, reconnect);
  }

  function start() {
    retry = null;
    connect().catch((e) => {
      report(e);
      if (live()) end(`kicksdk: ${message(e)}`, 0, true);
    });
  }

  async function connect() {
    if (!live()) return;
    if (!createSocket) {
      // It would fail the same way every time: say so once.
      drop(NO_WEBSOCKET, 0, false);
      return;
    }
    const ctrl = new AbortController();
    attempt = ctrl;
    const mine = () => live() && attempt === ctrl;
    let url;
    let token;
    try {
      // A guest identity: the token is issued to this id, and so is every
      // refreshed one for this connection.
      clientId = guestId();
      const where = hasChannel
        ? chatConnectionRequest(o.channelId, clientId, o.endpoints)
        : connectionRequest(clientId, o.endpoints);
      const c = await request(where, 'connection', ctrl.signal);
      if (!mine()) return;
      const found = parseConnection(c.status, c.body).find((x) => x.provider === 'centrifugo');
      if (!found) throw new Error('Kick offered no Centrifugo server');
      url = found.url;
      const t = await request(tokenRequest(clientId, o.endpoints), 'token', ctrl.signal);
      if (!mine()) return;
      token = parseToken(t.status, t.body);
    } catch (e) {
      // Closed meanwhile: nobody is left to tell.
      if (!mine()) return;
      attempt = null;
      drop(message(e), 0, true);
      return;
    }

    let socket;
    try {
      socket = createSocket(url);
    } catch (e) {
      attempt = null;
      drop(`could not open a WebSocket: ${message(e)}`, 0, true);
      return;
    }
    ws = socket;
    handshake = setTimeout(
      guard(() => {
        handshake = null;
        if (ws === socket) end(`WebSocket handshake: no answer in ${timeoutMs} ms`, 0, true);
      }),
      timeoutMs
    );
    socket.onopen = guard(() => {
      if (ws !== socket) return;
      clearTimeout(handshake);
      handshake = null;
      apply(session.open(token, now()));
    });
    socket.onmessage = guard((e) => {
      if (ws === socket) apply(session.receive(String(e.data), now()));
    });
    socket.onclose = guard((e) => {
      if (ws !== socket) return;
      // A refused handshake ends here too (1006), with no open before it.
      const code = e && Number.isInteger(e.code) && e.code >= 0 && e.code <= 65535 ? e.code : 1006;
      const why = e && e.reason ? `: ${e.reason}` : '';
      end(`WebSocket closed (${code})${why}`, code, shouldReconnect(code));
    });
    // A close event follows an error.
    socket.onerror = noop;
  }

  retry = setTimeout(start, 0);
  return {
    close() {
      if (closed) return;
      closed = true;
      clearTimeout(retry);
      retry = null;
      detach();
      release();
    }
  };
}
