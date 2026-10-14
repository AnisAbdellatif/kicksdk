// connectChat against a fake Kick, on mock timers: what it does when Kick
// goes quiet, closes for good, refuses or never answers, and when the
// caller's callbacks throw or close it from the inside.

import test from 'node:test';
import assert from 'node:assert/strict';

import { sdk } from './load.js';
import { fakeKick, chatPush, CHATROOM, CHANNEL, CHAT_FEED } from './fake-kick.js';

const flush = () => new Promise((r) => setImmediate(r));

/** Mock timers, a clock that follows them, and a connectChat on a fake Kick. */
function harness(t, { kick = fakeKick(), ...options } = {}) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let clock = 0;
  const log = { events: [], states: [], drops: [], errors: [] };
  const chat = sdk.connectChat({
    chatroomId: CHATROOM,
    channelId: CHANNEL,
    fetch: kick.fetch,
    WebSocket: kick.WebSocket,
    now: () => clock,
    onEvent: (e) => log.events.push(e),
    onState: (s) => log.states.push(s),
    onDrop: (reason, info) => log.drops.push({ reason, ...info }),
    onError: (e) => log.errors.push(e),
    ...options
  });
  /** Lets `ms` pass, in steps, running what falls due and what it awaits. */
  async function advance(ms, step = 50) {
    await flush();
    for (let left = ms; left > 0; left -= step) {
      const d = Math.min(step, left);
      clock += d;
      t.mock.timers.tick(d);
      await flush();
    }
  }
  t.after(() => chat.close());
  return { kick, chat, log, advance };
}

const subscribed = (log) => log.states.some((s) => s.subscribed.includes(CHAT_FEED));

test('connects, subscribes and delivers events', async (t) => {
  const { kick, log, advance } = harness(t);
  await advance(100);
  assert.equal(kick.sockets.length, 1);
  assert.deepEqual(kick.last().options, { headers: { Origin: 'https://kick.com' } });
  assert.ok(subscribed(log));
  kick.last().push(chatPush('hello'));
  await advance(10);
  assert.equal(log.events.at(-1).event.content, 'hello');
  assert.deepEqual(log.drops, []);
});

test('Kick going quiet: the socket is dropped at once and a new one opened', async (t) => {
  const { kick, log, advance } = harness(t);
  await advance(100);
  const first = kick.last();
  // Kick's ping is 25 s, and the session allows 20 s more. The fake socket
  // never reports a close, as a dead TCP connection would not.
  await advance(46_000, 500);
  assert.ok(first.closedByClient);
  assert.equal(first.onmessage, null, 'its handlers are dropped');
  assert.equal(log.drops.length, 1);
  assert.equal(log.drops[0].reconnect, true);
  assert.match(log.drops[0].reason, /quiet/);
  assert.equal(log.states.at(-1).connected, false);
  // The reconnect waits the jittered first backoff: 0.5 to 1 s.
  await advance(1_000);
  assert.equal(kick.sockets.length, 2);
  assert.ok(subscribed(log));
  assert.equal(log.states.at(-1).subscribed.includes(CHAT_FEED), true);
});

test('pings are answered even when onEvent throws', async (t) => {
  const { kick, log, advance } = harness(t, {
    onEvent: () => {
      throw new Error('a bug in the caller');
    }
  });
  await advance(100);
  const s = kick.last();
  s.push(chatPush('one'));
  s.push(chatPush('two'));
  s.push('{}');
  await advance(10);
  assert.equal(log.errors.length, 2);
  assert.equal(s.sent.at(-1), '{}', 'the ping was answered');
  assert.deepEqual(log.drops, []);
  assert.equal(kick.sockets.length, 1);
});

test('a throwing onState or onDrop does not stop the reconnect', async (t) => {
  const kick = fakeKick();
  const { log, advance } = harness(t, {
    kick,
    onState: () => {
      throw new Error('onState');
    },
    onDrop: () => {
      throw new Error('onDrop');
    }
  });
  await advance(100);
  kick.last().serverClose(1006);
  await advance(1_100);
  assert.equal(kick.sockets.length, 2);
  const thrown = new Set(log.errors.map((e) => e.message));
  assert.deepEqual([...thrown].sort(), ['onDrop', 'onState']);
  assert.ok(log.states.length === 0 && log.drops.length === 0);
});

test('without onError, a throwing callback goes to console.error', async (t) => {
  const printed = t.mock.method(console, 'error', () => {});
  const { kick, advance } = harness(t, {
    onError: undefined,
    onEvent: () => {
      throw new Error('a bug in the caller');
    }
  });
  await advance(100);
  kick.last().push(chatPush('x'));
  await advance(10);
  assert.equal(printed.mock.callCount(), 1);
  assert.match(String(printed.mock.calls[0].arguments.at(-1)), /a bug in the caller/);
});

test('a terminal disconnect stops: no reconnect, onDrop and onState say so', async (t) => {
  const { kick, log, advance } = harness(t);
  await advance(100);
  kick.last().push({ push: { disconnect: { code: 3501, reason: 'bad request' } } });
  await advance(10);
  assert.deepEqual(
    log.drops.map(({ code, reconnect }) => ({ code, reconnect })),
    [{ code: 3501, reconnect: false }]
  );
  assert.deepEqual(log.states.at(-1), { connected: false, subscribed: [], stopped: true });
  assert.ok(kick.last().closedByClient);
  const asked = kick.asked.length;
  await advance(120_000, 1_000);
  assert.equal(kick.sockets.length, 1);
  assert.equal(kick.asked.length, asked);
});

test('a socket closed with a terminal code stops; with 1006 it reconnects', async (t) => {
  const { kick, log, advance } = harness(t);
  await advance(100);
  kick.last().serverClose(1006);
  await advance(1_100);
  assert.equal(kick.sockets.length, 2);
  assert.deepEqual(log.drops[0], { reason: 'WebSocket closed (1006)', code: 1006, reconnect: true });
  kick.last().serverClose(4501, 'banned');
  await advance(60_000, 1_000);
  assert.equal(kick.sockets.length, 2);
  assert.deepEqual(log.drops[1], { reason: 'WebSocket closed (4501): banned', code: 4501, reconnect: false });
  assert.equal(log.states.at(-1).stopped, true);
});

test('a refused handshake is a drop, and is retried', async (t) => {
  const kick = fakeKick({ socket: 'refuse' });
  const { log, advance } = harness(t, { kick });
  await advance(100);
  assert.equal(log.drops.length, 1);
  assert.equal(log.drops[0].code, 1006);
  assert.equal(log.drops[0].reconnect, true);
  kick.socketMode = 'open';
  await advance(1_100);
  assert.equal(kick.sockets.length, 2);
  assert.ok(subscribed(log));
});

test('a request that never answers times out, and is retried', async (t) => {
  const kick = fakeKick({ fetch: 'hang' });
  const { log, advance } = harness(t, { kick, timeoutMs: 2_000 });
  await advance(1_900);
  assert.deepEqual(log.drops, []);
  await advance(200);
  assert.equal(log.drops.length, 1);
  assert.match(log.drops[0].reason, /connection: no answer in 2000 ms/);
  assert.equal(kick.aborted, 1, 'the fetch was aborted');
  kick.fetchMode = 'answer';
  await advance(1_100);
  assert.equal(kick.sockets.length, 1);
  assert.ok(subscribed(log));
});

test('a WebSocket that never opens times out, and is retried', async (t) => {
  const kick = fakeKick({ socket: 'hang' });
  const { log, advance } = harness(t, { kick, timeoutMs: 3_000 });
  await advance(3_100);
  assert.equal(log.drops.length, 1);
  assert.match(log.drops[0].reason, /handshake/);
  assert.ok(kick.sockets[0].closedByClient);
  kick.socketMode = 'open';
  await advance(1_100);
  assert.equal(kick.sockets.length, 2);
  assert.ok(subscribed(log));
});

test('close() is idempotent, and nothing fires after it', async (t) => {
  const { kick, chat, log, advance } = harness(t);
  await advance(100);
  const counts = () => [log.events.length, log.states.length, log.drops.length];
  const before = counts();
  chat.close();
  chat.close();
  assert.ok(kick.last().closedByClient);
  kick.last().readyState = 1;
  kick.last().push(chatPush('late'));
  await advance(60_000, 1_000);
  assert.deepEqual(counts(), before);
  assert.equal(kick.sockets.length, 1);
  assert.deepEqual(log.errors, []);
});

test('close() from inside onEvent: the rest of the frame is not delivered', async (t) => {
  let chat;
  const events = [];
  const kick = fakeKick();
  const h = harness(t, {
    kick,
    onEvent: (e) => {
      events.push(e.event.content);
      chat.close();
    }
  });
  chat = h.chat;
  await h.advance(100);
  // Two messages in one frame: one receive(), two actions.
  kick.last().push(`${chatPush('one')}\n${chatPush('two')}`);
  await h.advance(10);
  assert.deepEqual(events, ['one']);
  chat.close();
  assert.deepEqual(h.log.errors, []);
  await h.advance(60_000, 1_000);
  assert.equal(kick.sockets.length, 1);
});

test('close() from inside onDrop and onState does not crash or reconnect', async (t) => {
  for (const where of ['onDrop', 'onState']) {
    let chat;
    let closing = false;
    const kick = fakeKick();
    const h = harness(t, {
      kick,
      [where]: () => {
        if (closing) chat.close();
      }
    });
    chat = h.chat;
    await h.advance(100);
    closing = true;
    kick.last().serverClose(1006);
    await h.advance(60_000, 1_000);
    assert.equal(kick.sockets.length, 1, where);
    assert.deepEqual(h.log.errors, [], where);
    t.mock.timers.reset();
  }
});

test('a request failing after close() is not reported', async (t) => {
  let fail;
  const kick = fakeKick();
  kick.fetch = () => new Promise((_, reject) => (fail = reject)); // ignores the abort signal
  const { chat, log, advance } = harness(t, { kick });
  await advance(10);
  chat.close();
  fail(new TypeError('fetch failed'));
  await advance(10);
  assert.deepEqual(log.drops, []);
  assert.deepEqual(log.errors, []);
});

test('no WebSocket at all: one clear drop, then stopped', async (t) => {
  const saved = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket');
  Object.defineProperty(globalThis, 'WebSocket', { value: undefined, configurable: true, writable: true });
  t.after(() => {
    if (saved) Object.defineProperty(globalThis, 'WebSocket', saved);
    else delete globalThis.WebSocket;
  });
  const { kick, log, advance } = harness(t, { WebSocket: undefined });
  await advance(60_000, 1_000);
  assert.equal(log.drops.length, 1);
  assert.match(log.drops[0].reason, /no WebSocket/);
  assert.equal(log.drops[0].reconnect, false);
  assert.equal(log.states.at(-1).stopped, true);
  assert.equal(kick.asked.length, 0);
});

test('without crypto.randomUUID, the client id still is a v4 UUID', async (t) => {
  const saved = Object.getOwnPropertyDescriptor(globalThis, 'crypto');
  const real = globalThis.crypto;
  Object.defineProperty(globalThis, 'crypto', {
    value: { getRandomValues: (a) => real.getRandomValues(a) },
    configurable: true
  });
  t.after(() => Object.defineProperty(globalThis, 'crypto', saved));
  const { kick, log, advance } = harness(t);
  await advance(100);
  const { client_id } = JSON.parse(kick.asked[1].init.body);
  assert.match(client_id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.ok(subscribed(log));
});

test('the session clock is monotonic: Date.now() is not used', async (t) => {
  t.mock.method(Date, 'now', () => NaN);
  const kick = fakeKick();
  const { log, advance } = harness(t, { kick, now: undefined });
  await advance(100);
  assert.ok(subscribed(log));
  assert.deepEqual(log.errors, []);
});

test('the reconnect delay is jittered with Math.random', async (t) => {
  t.mock.method(Math, 'random', () => 0);
  const { kick, advance } = harness(t);
  await advance(100);
  kick.last().serverClose(1006);
  // Half of the first 1 s backoff.
  await advance(450, 10);
  assert.equal(kick.sockets.length, 1);
  await advance(100, 10);
  assert.equal(kick.sockets.length, 2);
});

test('mistakes in the options throw at once', () => {
  const base = { chatroomId: CHATROOM, onEvent() {} };
  assert.throws(() => sdk.connectChat({ ...base, chatroomId: '1' }), TypeError);
  assert.throws(() => sdk.connectChat({ ...base, channelId: -1 }), RangeError);
  assert.throws(() => sdk.connectChat({ ...base, endpoints: { realtme: 'x' } }), /unknown key/);
  assert.throws(() => sdk.connectChat({ ...base, timeoutMs: 0 }), RangeError);
  assert.throws(() => sdk.connectChat({ chatroomId: 1 }), TypeError);
  // undefined endpoints, as a Partial<> carries them, are fine.
  sdk.connectChat({ ...base, endpoints: { realtime: undefined }, WebSocket: class {} }).close();
});
