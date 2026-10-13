// kicksdk for JavaScript: the WebAssembly build of the Rust core, plus
// connectChat(), which drives the realtime protocol with fetch, WebSocket and
// timers.
//
// Call `await init()` once before anything else (in Node, or anywhere the .wasm
// cannot be fetched beside this file, pass its bytes: `init(bytes)`).
//
// Where it works: Kick's realtime server only takes a socket whose Origin is
// https://kick.com. Node sets that header itself (see `createSocket` below); a
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
  parseToken
} from './pkg/kicksdk.js';

export * from './pkg/kicksdk.js';

export const REQUIRED_ORIGIN = 'https://kick.com';

let ready = null;

/** Loads the WebAssembly. Safe to call more than once. */
export function init(input) {
  if (!ready) {
    if (input instanceof Uint8Array || input instanceof ArrayBuffer) {
      initSync({ module: input });
      ready = Promise.resolve();
    } else {
      ready = initWasm(input === undefined ? undefined : { module_or_path: input });
    }
  }
  return ready;
}

const isNode = typeof process !== 'undefined' && !!(process.versions && process.versions.node);

function defaultSocket(url) {
  // Node's WebSocket (undici) takes headers; a browser's ignores the argument
  // and sends its own page's origin.
  return isNode ? new WebSocket(url, { headers: { Origin: REQUIRED_ORIGIN } }) : new WebSocket(url);
}

async function send(fetchImpl, req) {
  // A browser drops the headers it forbids (origin, referer) and sends its own.
  const res = await fetchImpl(req.url, {
    method: req.method,
    headers: Object.fromEntries(req.headers),
    body: req.body ?? undefined,
    credentials: 'omit'
  });
  return { status: res.status, body: await res.text() };
}

/**
 * Connects to one channel's chat and keeps the connection up until close().
 *
 * @param {object} o
 * @param {number} o.chatroomId          the chatroom (website API `chatroom.id`)
 * @param {number} [o.channelId]         the channel; also locates the chat's region
 * @param {string[]} [o.channels]        feeds to subscribe to; chatChannels() by default
 * @param {(e: {channel, name, data, event}) => void} o.onEvent
 * @param {(s: {connected: boolean, subscribed: string[]}) => void} [o.onState]
 * @param {(reason: string) => void} [o.onDrop]   a connection ended (a new one follows)
 * @param {typeof fetch} [o.fetch]
 * @param {(url: string) => WebSocket} [o.createSocket]
 * @param {object} [o.endpoints]         overrides of Kick's URLs
 * @param {() => number} [o.now]
 * @returns {{ close(): void }}
 */
export function connectChat(o) {
  const fetchImpl = o.fetch || ((...a) => fetch(...a));
  const createSocket = o.createSocket || defaultSocket;
  const now = o.now || (() => Date.now());
  const channels = o.channels || chatChannels(o.chatroomId, o.channelId);
  const backoff = new Backoff();
  let closed = false;
  let ws = null;
  let session = null;
  let timer = null;
  let retry = null;
  let clientId = null;

  const state = () => {
    if (o.onState && session) {
      o.onState({ connected: session.isConnected(), subscribed: channels.filter((c) => session.isSubscribed(c)) });
    }
  };

  function arm() {
    clearTimeout(timer);
    const at = session && session.deadline();
    if (at !== undefined && at !== null) timer = setTimeout(() => apply(session.tick(now())), Math.max(0, at - now()));
  }

  function apply(actions) {
    // Anything still in flight once close() has run - a timer, a token
    // arriving - finds nothing left to act on.
    if (closed || !session) return;
    let changed = false;
    for (const a of actions) {
      switch (a.action) {
        case 'send':
          if (ws && ws.readyState === 1) ws.send(a.text);
          break;
        case 'close':
          if (o.onDrop) o.onDrop(a.reason);
          if (ws) ws.close();
          break;
        case 'refresh_token':
          send(fetchImpl, tokenRequest(clientId, o.endpoints))
            .then((r) => parseToken(r.status, r.body))
            .then((token) => {
              if (!closed && session) apply(session.refresh(token, now()));
            })
            .catch(() => {
              // Kick ends the connection when the old token runs out; the
              // reconnect fetches a new one.
            });
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
          o.onEvent({ channel: a.channel, name: a.name, data: a.data, event: a.event });
          break;
      }
    }
    if (changed) state();
    arm();
  }

  function again() {
    if (closed) return;
    clearTimeout(retry);
    retry = setTimeout(connect, backoff.nextDelay());
  }

  async function connect() {
    if (closed) return;
    // A guest identity: the token is issued to this id, and so is every
    // refreshed one for this connection.
    clientId = crypto.randomUUID();
    let url;
    let token;
    try {
      const where = o.channelId
        ? chatConnectionRequest(o.channelId, clientId, o.endpoints)
        : connectionRequest(clientId, o.endpoints);
      const c = await send(fetchImpl, where);
      const found = parseConnection(c.status, c.body).find((x) => x.provider === 'centrifugo');
      if (!found) throw new Error('Kick offered no Centrifugo server');
      url = found.url;
      const t = await send(fetchImpl, tokenRequest(clientId, o.endpoints));
      token = parseToken(t.status, t.body);
    } catch (e) {
      if (o.onDrop) o.onDrop(String((e && e.message) || e));
      again();
      return;
    }
    if (closed) return;

    if (!session) session = new Session(channels);
    let socket;
    try {
      socket = createSocket(url);
    } catch (e) {
      again();
      return;
    }
    ws = socket;
    socket.onopen = () => apply(session.open(token, now()));
    socket.onmessage = (e) => {
      if (ws === socket) apply(session.receive(String(e.data), now()));
    };
    socket.onclose = () => {
      if (ws !== socket) return;
      ws = null;
      clearTimeout(timer);
      apply(session.closed());
      state();
      again();
    };
    socket.onerror = () => {};
  }

  connect();
  return {
    close() {
      closed = true;
      clearTimeout(timer);
      clearTimeout(retry);
      if (ws) {
        const s = ws;
        ws = null;
        s.onclose = null;
        s.close();
      }
      if (session) {
        session.free();
        session = null;
      }
      backoff.free();
    }
  };
}
