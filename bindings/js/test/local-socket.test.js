// connectChat over real sockets: Node's own fetch and WebSocket against a
// Kick on 127.0.0.1 (local-kick.js). Kick's waits are minutes long, so
// connectChat's clock and its own timers - and only those - run SPEED times
// faster than real time: a 5-minute wait takes 1.2 s, and Kick's silence
// limit (45 s) 180 ms.

import test from 'node:test';
import assert from 'node:assert/strict';

import { sdk } from './load.js';
import { localKick } from './local-kick.js';

const SPEED = 250;
const CHATROOM = 900000002;
const CHANNEL = 900000001;
const CHAT_FEED = `chatrooms.${CHATROOM}.v2`;
const INDEX = new URL('../index.js', import.meta.url).pathname;

/** A chat message push on the chat feed. */
function chatPush(content) {
  const data = JSON.stringify({
    id: '00000000-0000-4000-8000-000000000001',
    chatroom_id: CHATROOM,
    content,
    type: 'message',
    created_at: '2026-10-13T04:03:25+00:00',
    sender: { id: 900000003, username: 'user0001', slug: 'user0001', identity: { color: '#FF55B3', badges: [] } }
  });
  const pub = { data: { event: 'App\\Events\\ChatMessageEvent', data } };
  return JSON.stringify({ push: { channel: CHAT_FEED, pub } });
}

/** Waits, in real time, until `cond()` holds. */
async function until(cond, what, ms = 5_000) {
  const end = performance.now() + ms;
  while (!cond()) {
    if (performance.now() > end) assert.fail(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 2));
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * A local Kick and a connectChat on it, with connectChat's timers (those set
 * from index.js itself) and clock sped up. `scheduled` lists the delays it
 * asked for, unscaled.
 */
async function harness(t, options = {}) {
  const kick = await localKick({ speed: SPEED });
  const real = globalThis.setTimeout;
  const scheduled = [];
  globalThis.setTimeout = function (fn, ms, ...args) {
    // The frame below this one is the caller's.
    const caller = new Error().stack.split('\n')[2] || '';
    if (caller.includes(INDEX)) {
      scheduled.push(ms);
      ms = (ms || 0) / SPEED;
    }
    return real(fn, ms, ...args);
  };
  const t0 = performance.now();
  const log = { events: [], states: [], drops: [], errors: [] };
  const chat = sdk.connectChat({
    chatroomId: CHATROOM,
    channelId: CHANNEL,
    endpoints: kick.endpoints,
    now: () => (performance.now() - t0) * SPEED,
    // 1 s of real time for each request and the handshake.
    timeoutMs: 1_000 * SPEED,
    onEvent: (e) => log.events.push(e),
    onState: (s) => log.states.push(s),
    onDrop: (reason, info) => log.drops.push({ reason, ...info }),
    onError: (e) => log.errors.push(e),
    ...options
  });
  t.after(async () => {
    chat.close();
    globalThis.setTimeout = real;
    await kick.stop();
  });
  return { kick, chat, log, scheduled };
}

const subscribed = (log) => log.states.at(-1)?.subscribed.includes(CHAT_FEED);

test('real sockets: Origin https://kick.com, subscribed, events delivered', async (t) => {
  const { kick, log } = await harness(t);
  await until(() => subscribed(log), 'the chat feed');
  assert.equal(kick.refused, 0);
  assert.equal(kick.conns.length, 1);
  kick.last().send(chatPush('hello'));
  await until(() => log.events.length === 1, 'the event');
  assert.equal(log.events[0].event.content, 'hello');
  assert.deepEqual(log.drops, []);
  assert.deepEqual(log.errors, []);
});

test('real sockets: a handshake without Origin https://kick.com is refused, and retried', async (t) => {
  const { kick, log } = await harness(t, { createSocket: (url) => new WebSocket(url) });
  await until(() => log.drops.length >= 2, 'two refusals');
  assert.ok(kick.refused >= 2);
  assert.equal(kick.conns.length, 0);
  assert.deepEqual(
    { code: log.drops[0].code, terminal: log.drops[0].terminal },
    { code: 1006, terminal: false },
    'a refused handshake is an ordinary drop'
  );
});

test('real sockets: a 3501 close waits the 5-minute stage, then comes back as a new client', async (t) => {
  t.mock.method(Math, 'random', () => 0);
  const { kick, log, scheduled } = await harness(t);
  await until(() => subscribed(log), 'the chat feed');
  kick.last().close(3501, 'bad request');
  await until(() => log.drops.length === 1, 'the drop');
  assert.deepEqual(log.drops[0], {
    reason: 'WebSocket closed (3501): bad request',
    code: 3501,
    terminal: true,
    waitMs: 300_000
  });
  assert.ok(scheduled.includes(300_000), 'the reconnect is scheduled 300000 ms out');
  // 200 s on connectChat's clock: still waiting.
  await sleep(200_000 / SPEED);
  assert.equal(kick.conns.length, 1);
  await until(() => kick.conns.length === 2 && subscribed(log), 'the new connection');
  const ids = kick.clientIds();
  assert.equal(ids.length, 2);
  assert.notEqual(ids[0], ids[1], 'a new client id');
  assert.deepEqual(log.errors, []);
});

test('real sockets: a quiet connection is let go at once, without waiting for its close', async (t) => {
  const { kick, log } = await harness(t);
  await until(() => subscribed(log), 'the chat feed');
  const first = kick.last();
  // A dead connection: no pings, and the client's close frame is never
  // answered, so its socket would not report a close for a long time.
  first.quiet = true;
  await until(() => log.drops.length === 1, 'the drop');
  assert.match(log.drops[0].reason, /quiet/);
  assert.equal(log.drops[0].terminal, false);
  await until(() => kick.conns.length === 2 && subscribed(log), 'the new connection');
  assert.equal(first.ended, false, 'the old TCP connection is still open');
  await until(() => first.closeFrame, 'the close frame of the old socket');
  assert.equal(log.drops.length, 1);
  assert.deepEqual(log.errors, []);
});

test('real sockets: close() inside onEvent stops the frame, the socket and every reconnect', async (t) => {
  let chat;
  const events = [];
  const h = await harness(t, {
    onEvent: (e) => {
      events.push(e.event.content);
      chat.close();
    }
  });
  chat = h.chat;
  await until(() => subscribed(h.log), 'the chat feed');
  h.kick.last().send(`${chatPush('one')}\n${chatPush('two')}`);
  await until(() => h.kick.last().ended, 'the socket closed');
  assert.deepEqual(events, ['one']);
  // 25 s on connectChat's clock, where a reconnect would take 1 s at most.
  await sleep(25_000 / SPEED);
  assert.equal(h.kick.conns.length, 1);
  assert.deepEqual(h.log.drops, []);
  assert.deepEqual(h.log.errors, []);
});

test('real sockets: close() inside onDrop or onState, on a server close, does not reconnect', async (t) => {
  for (const where of ['onDrop', 'onState']) {
    await t.test(where, async (t) => {
      let chat;
      let closing = false;
      const h = await harness(t, {
        [where]: (...args) => {
          if (where === 'onState') h.log.states.push(args[0]);
          if (closing) chat.close();
        }
      });
      chat = h.chat;
      await until(() => subscribed(h.log), 'the chat feed');
      closing = true;
      h.kick.last().close(1001, 'going away');
      await sleep(25_000 / SPEED);
      assert.equal(h.kick.conns.length, 1);
      assert.deepEqual(h.log.errors, []);
    });
  }
});
