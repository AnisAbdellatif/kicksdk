// connectChat against a fake Kick, on mock timers: what it does when Kick
// goes quiet, closes terminally, refuses or never answers, and when the
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
  assert.equal(log.drops[0].terminal, false);
  assert.ok(log.drops[0].waitMs >= 500 && log.drops[0].waitMs <= 1_000, String(log.drops[0].waitMs));
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

test('a terminal disconnect: 5 minutes, then a new client', async (t) => {
  t.mock.method(Math, 'random', () => 0);
  const { kick, log, advance } = harness(t);
  await advance(100);
  const [firstId] = kick.clientIds();
  kick.last().push({ push: { disconnect: { code: 3501, reason: 'bad request' } } });
  await advance(10);
  assert.deepEqual(log.drops, [
    { reason: 'Kick disconnected: 3501 bad request', code: 3501, terminal: true, waitMs: 300_000 }
  ]);
  assert.deepEqual(log.states.at(-1), { connected: false, subscribed: [] });
  assert.ok(kick.last().closedByClient, 'the socket is let go at once');
  const asked = kick.asked.length;
  // Nothing for 5 minutes...
  await advance(299_000, 5_000);
  assert.equal(kick.sockets.length, 1);
  assert.equal(kick.asked.length, asked);
  // ...then a new guest client: a new id for the connection and the token.
  await advance(1_000);
  assert.equal(kick.sockets.length, 2);
  assert.ok(subscribed(log));
  const ids = kick.clientIds().slice(asked);
  assert.equal(ids.length, 2);
  assert.equal(ids[0], ids[1]);
  assert.notEqual(ids[0], firstId);
  assert.deepEqual(log.errors, []);
});

test('a socket closed with a terminal code waits long; with 1006, the ordinary backoff', async (t) => {
  t.mock.method(Math, 'random', () => 0);
  const { kick, log, advance } = harness(t);
  await advance(100);
  kick.last().serverClose(1006);
  await advance(1_100);
  assert.equal(kick.sockets.length, 2);
  assert.deepEqual(log.drops[0], { reason: 'WebSocket closed (1006)', code: 1006, terminal: false, waitMs: 500 });
  // The chat feed subscribed again in between: the backoff starts over.
  kick.last().serverClose(4501, 'banned');
  await advance(10);
  assert.deepEqual(log.drops[1], {
    reason: 'WebSocket closed (4501): banned',
    code: 4501,
    terminal: true,
    waitMs: 300_000
  });
  await advance(290_000, 5_000);
  assert.equal(kick.sockets.length, 2);
  await advance(11_000);
  assert.equal(kick.sockets.length, 3);
  assert.ok(log.states.at(-1).subscribed.includes(CHAT_FEED));
});

test('terminal closes in a row wait 5, 10, 20, 30, 30 minutes; the chat feed resets it', async (t) => {
  t.mock.method(Math, 'random', () => 0);
  // Every connect refused: a terminal close each time, before any subscribe.
  const kick = fakeKick({ connectError: { code: 101, message: 'unauthorized' } });
  const { log, advance } = harness(t, { kick });
  const waits = [300_000, 600_000, 1_200_000, 1_800_000, 1_800_000];
  await advance(100);
  for (const [i, wait] of waits.entries()) {
    assert.equal(log.drops.length, i + 1);
    assert.deepEqual(
      { code: log.drops[i].code, terminal: log.drops[i].terminal, waitMs: log.drops[i].waitMs },
      { code: 101, terminal: true, waitMs: wait }
    );
    await advance(wait - 5_000, 5_000);
    assert.equal(kick.sockets.length, i + 1, `no reconnect before ${wait} ms`);
    // The last time, Kick takes the client: the chat feed subscribes, and
    // the streak is over.
    if (i === waits.length - 1) kick.connectError = null;
    await advance(6_000);
  }
  assert.equal(log.drops.length, waits.length);
  assert.ok(subscribed(log));
  const drops = log.drops.length;
  kick.last().serverClose(3501);
  await advance(10);
  assert.equal(log.drops.length, drops + 1);
  assert.equal(log.drops.at(-1).waitMs, 300_000);
});

test('a refused handshake is a drop, and is retried', async (t) => {
  const kick = fakeKick({ socket: 'refuse' });
  const { log, advance } = harness(t, { kick });
  await advance(100);
  assert.equal(log.drops.length, 1);
  assert.equal(log.drops[0].code, 1006);
  assert.equal(log.drops[0].terminal, false);
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

test('no WebSocket at all: connectChat throws at once', (t) => {
  const saved = Object.getOwnPropertyDescriptor(globalThis, 'WebSocket');
  Object.defineProperty(globalThis, 'WebSocket', { value: undefined, configurable: true, writable: true });
  t.after(() => {
    if (saved) Object.defineProperty(globalThis, 'WebSocket', saved);
    else delete globalThis.WebSocket;
  });
  const kick = fakeKick();
  assert.throws(
    () => sdk.connectChat({ chatroomId: CHATROOM, onEvent() {}, fetch: kick.fetch }),
    (e) => e instanceof TypeError && /no WebSocket/.test(e.message)
  );
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
