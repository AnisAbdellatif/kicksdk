// A fake Kick for connectChat: its fetch answers the connection and token
// requests, and its WebSocket plays Centrifugo (connect, subscribe, pings,
// pushes) as the recordings in fixtures/realtime show it. Timers are the
// test's (node:test mock timers), so silence and backoff run in no time.

const CONNECTION = JSON.stringify({
  data: {
    connections: [
      { credentials: { url: 'wss://realtime.example.invalid/connection/websocket' }, provider: 'centrifugo' }
    ],
    mode: 'websocket'
  },
  message: 'success'
});
const TOKEN = JSON.stringify({ data: { token: '[redacted]' }, message: 'success' });
// Kick's connect reply: a ping every 25 s, a token good for 45 minutes.
const CONNECTED = { client: 'c', version: '6', expires: true, ttl: 2700, ping: 25, pong: true };

export const CHATROOM = 900000002;
export const CHANNEL = 900000001;
export const CHAT_FEED = `chatrooms.${CHATROOM}.v2`;

/** A chat message push on the chat feed. */
export function chatPush(content) {
  const data = JSON.stringify({
    id: '00000000-0000-4000-8000-000000000001',
    chatroom_id: CHATROOM,
    content,
    type: 'message',
    created_at: '2026-10-13T04:03:25+00:00',
    sender: { id: 900000003, username: 'user0001', slug: 'user0001', identity: { color: '#FF55B3', badges: [] } }
  });
  return JSON.stringify({
    push: { channel: CHAT_FEED, pub: { data: { event: 'App\\Events\\ChatMessageEvent', data } } }
  });
}

/**
 * @param {object} [opts]
 * @param {'open' | 'hang' | 'refuse'} [opts.socket]  what a new socket does
 * @param {'answer' | 'hang' | 'fail'} [opts.fetch]   what a request does
 */
export function fakeKick(opts = {}) {
  const kick = {
    socketMode: opts.socket || 'open',
    fetchMode: opts.fetch || 'answer',
    /** Every request: {url, init}. */
    asked: [],
    /** Every socket, in order. */
    sockets: [],
    /** How many hung requests were aborted. */
    aborted: 0
  };

  kick.fetch = (url, init) => {
    kick.asked.push({ url, init });
    if (kick.fetchMode === 'fail') return Promise.reject(new TypeError('fetch failed'));
    if (kick.fetchMode === 'hang') {
      return new Promise((_, reject) => {
        init.signal.addEventListener('abort', () => {
          kick.aborted++;
          reject(init.signal.reason);
        });
      });
    }
    const body = url.endsWith('/auth/connection') ? TOKEN : CONNECTION;
    return Promise.resolve({ status: 200, text: async () => body });
  };

  class FakeSocket {
    constructor(url, options) {
      this.url = url;
      this.options = options;
      this.readyState = 0;
      this.sent = [];
      this.closedByClient = false;
      this.onopen = this.onmessage = this.onclose = this.onerror = null;
      kick.sockets.push(this);
      const mode = kick.socketMode;
      if (mode === 'open') {
        setTimeout(() => {
          if (this.readyState !== 0) return;
          this.readyState = 1;
          this.onopen?.({});
        }, 0);
      } else if (mode === 'refuse') {
        // A refused handshake (Kick's answer to a wrong Origin): an error,
        // then a close, and never an open.
        setTimeout(() => {
          this.readyState = 3;
          this.onerror?.({});
          this.onclose?.({ code: 1006, reason: '' });
        }, 0);
      }
    }

    send(text) {
      if (this.readyState !== 1) throw new Error('not open');
      this.sent.push(text);
      for (const line of text.split('\n')) {
        const f = JSON.parse(line);
        if (f.connect) {
          this.push({ id: f.id, connect: CONNECTED });
        } else if (f.subscribe) {
          this.push({ id: f.id, subscribe: {} });
        }
      }
    }

    /** A frame from Kick, delivered on the next timer turn. */
    push(frame) {
      const text = typeof frame === 'string' ? frame : JSON.stringify(frame);
      setTimeout(() => {
        if (this.readyState === 1) this.onmessage?.({ data: text });
      }, 0);
    }

    /** Kick (or the network) closes the socket. */
    serverClose(code, reason = '') {
      this.readyState = 3;
      this.onclose?.({ code, reason });
    }

    // A dead connection's close never completes: no close event comes back.
    close() {
      this.closedByClient = true;
      this.readyState = 2;
    }
  }

  kick.WebSocket = FakeSocket;
  kick.last = () => kick.sockets.at(-1);
  return kick;
}
