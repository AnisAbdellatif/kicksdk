// A Kick on 127.0.0.1, for connectChat over real sockets: an HTTP server
// answering the connection and token requests, and a bare RFC 6455
// WebSocket server playing Centrifugo (connect, subscribe, pings, pushes,
// close codes). Like Kick's, it refuses a handshake whose Origin is not
// https://kick.com.

import http from 'node:http';
import { createHash } from 'node:crypto';

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
// Kick's connect reply: a ping every 25 s, a token good for 45 minutes.
const CONNECTED = { client: 'c', version: '6', expires: true, ttl: 2700, ping: 25, pong: true };

/** One server-side WebSocket connection. */
class Conn {
  constructor(socket, kick) {
    this.socket = socket;
    this.kick = kick;
    this.received = [];
    /** The client sent a close frame. */
    this.closeFrame = false;
    /** The TCP connection ended. */
    this.ended = false;
    /** No pings, and no answer to the client's close: a dead connection. */
    this.quiet = false;
    this.buffer = Buffer.alloc(0);
    socket.on('data', (chunk) => this.data(chunk));
    socket.on('close', () => {
      this.ended = true;
      clearInterval(this.pinger);
    });
    socket.on('error', () => {});
    // Kick pings every 25 s; connectChat's clock runs `kick.speed` times
    // faster than this one, so this pings well within that.
    this.pinger = setInterval(() => {
      if (!this.quiet) this.send('{}');
    }, kick.pingEveryMs);
  }

  data(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const b = this.buffer;
      if (b.length < 2) return;
      const opcode = b[0] & 0x0f;
      let len = b[1] & 0x7f;
      let at = 2;
      if (len === 126) {
        if (b.length < 4) return;
        len = b.readUInt16BE(2);
        at = 4;
      } else if (len === 127) {
        if (b.length < 10) return;
        len = Number(b.readBigUInt64BE(2));
        at = 10;
      }
      // A client's frames are always masked.
      if (b.length < at + 4 + len) return;
      const mask = b.subarray(at, at + 4);
      const payload = Buffer.from(b.subarray(at + 4, at + 4 + len));
      for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
      this.buffer = b.subarray(at + 4 + len);
      if (opcode === 1) this.text(payload.toString('utf8'));
      else if (opcode === 8) {
        this.closeFrame = true;
        if (!this.quiet) this.close(1000);
      } else if (opcode === 9) this.frame(10, payload);
    }
  }

  text(text) {
    this.received.push(text);
    for (const line of text.split('\n')) {
      const f = JSON.parse(line);
      if (f.connect) {
        this.send(JSON.stringify({ id: f.id, connect: CONNECTED }));
      } else if (f.subscribe) {
        this.send(JSON.stringify({ id: f.id, subscribe: {} }));
      }
    }
  }

  frame(opcode, payload) {
    if (this.socket.destroyed) return;
    const head =
      payload.length < 126
        ? Buffer.from([0x80 | opcode, payload.length])
        : Buffer.from([0x80 | opcode, 126, payload.length >> 8, payload.length & 0xff]);
    this.socket.write(Buffer.concat([head, payload]));
  }

  send(text) {
    this.frame(1, Buffer.from(text, 'utf8'));
  }

  /** Closes with a code, as Kick would: a close frame, then the TCP connection. */
  close(code, reason = '') {
    const payload = Buffer.alloc(2 + Buffer.byteLength(reason));
    payload.writeUInt16BE(code, 0);
    payload.write(reason, 2);
    this.frame(8, payload);
    this.socket.end();
    clearInterval(this.pinger);
  }
}

/**
 * Starts a local Kick. `speed`: how many times faster than real time the
 * client's clock runs (pings are scaled to it).
 */
export async function localKick({ speed = 1 } = {}) {
  const kick = {
    speed,
    pingEveryMs: Math.max(1, Math.floor(10_000 / speed)),
    /** Every HTTP request: {url, body}. */
    requests: [],
    /** Every WebSocket connection, in order. */
    conns: [],
    /** Handshakes refused for their Origin. */
    refused: 0
  };
  const server = http.createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      kick.requests.push({ url: req.url, body });
      const answer = req.url.endsWith('/auth/connection')
        ? { data: { token: '[redacted]' }, message: 'success' }
        : {
            data: {
              connections: [{ provider: 'centrifugo', credentials: { url: `${kick.ws}/connection/websocket` } }],
              mode: 'websocket'
            },
            message: 'success'
          };
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify(answer));
    });
  });
  server.on('upgrade', (req, socket) => {
    if (req.headers.origin !== 'https://kick.com') {
      kick.refused++;
      socket.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n');
      return;
    }
    const accept = createHash('sha1')
      .update(req.headers['sec-websocket-key'] + GUID)
      .digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n' +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`
    );
    kick.conns.push(new Conn(socket, kick));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  kick.http = `http://127.0.0.1:${port}`;
  kick.ws = `ws://127.0.0.1:${port}`;
  kick.endpoints = { realtime: `${kick.http}/realtime` };
  kick.last = () => kick.conns.at(-1);
  /** The client id of each connection request, in order. */
  kick.clientIds = () =>
    kick.requests.filter((r) => r.url.endsWith('/chat/connection')).map((r) => JSON.parse(r.body).client.id);
  kick.stop = () =>
    new Promise((resolve) => {
      for (const c of kick.conns) {
        clearInterval(c.pinger);
        c.socket.destroy();
      }
      server.closeAllConnections();
      server.close(resolve);
    });
  return kick;
}
