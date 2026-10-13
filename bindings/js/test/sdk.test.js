// The npm package against the shared fixtures, and connectChat against a fake
// Kick: the same recordings the Rust tests replay.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import * as sdk from '../index.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(here, '../../../fixtures');

await sdk.init(readFileSync(path.join(here, '../pkg/kicksdk_bg.wasm')));

const realtimeRecordings = () =>
  readdirSync(path.join(FIXTURES, 'realtime'))
    .filter((f) => f.endsWith('.jsonl'))
    .map((f) => readFileSync(path.join(FIXTURES, 'realtime', f), 'utf8').trim().split('\n').map((l) => JSON.parse(l)));

test('events are typed, unknown ones kept by name', () => {
  const data = JSON.stringify({ chatroom_id: 1, host_username: 'Someone', number_viewers: 16, optional_message: '' });
  assert.deepEqual(sdk.parseEvent('App\\Events\\StreamHostEvent', data), {
    type: 'host',
    host_username: 'Someone',
    viewers: 16,
    message: null
  });
  assert.deepEqual(sdk.parseEvent('NewThing', '{}'), { type: 'unknown', name: 'NewThing' });
});

test('emotes are split out and combined', () => {
  assert.deepEqual(sdk.parseContent('a [emote:1:X][emote:1:X]', true), [
    { type: 'text', text: 'a ' },
    { type: 'emote', id: '1', name: 'X', count: 2 }
  ]);
  assert.equal(sdk.emoteUrl('37226'), 'https://files.kick.com/emotes/37226/fullsize');
});

test('requests are plain objects for fetch, and endpoints can be overridden', () => {
  const r = sdk.chatConnectionRequest(668, 'c1', { realtime: 'http://localhost:9/rt' });
  assert.equal(r.method, 'POST');
  assert.equal(r.url, 'http://localhost:9/rt/channels/668/chat/connection');
  assert.deepEqual(r.headers.find(([k]) => k === 'origin'), ['origin', 'https://kick.com']);
  assert.equal(sdk.livestreamsRequests('t', Array.from({ length: 120 }, (_, i) => i + 1)).length, 3);
});

test('parsers throw with the status', () => {
  assert.throws(() => sdk.parseChannel(404, '{}'), (e) => e.status === 404);
});

test('a recorded session replays through Session', () => {
  for (const records of realtimeRecordings()) {
    const sent = records.filter((r) => r.direction === 'out').map((r) => r.frame);
    const channels = sent
      .flatMap((f) => f.split('\n'))
      .map((l) => JSON.parse(l))
      .filter((f) => f.subscribe)
      .map((f) => f.subscribe.channel);
    const s = new sdk.Session(channels);
    const ours = s.open('[redacted]', 0).map((a) => a.text);
    let chat = 0;
    for (const r of records.filter((r) => r.direction === 'in')) {
      for (const a of s.receive(r.frame, r.ms)) {
        if (a.action === 'send') ours.push(a.text);
        if (a.action === 'event' && a.event.type === 'chat_message') chat++;
      }
    }
    assert.deepEqual(ours, sent);
    assert.ok(chat > 0);
    assert.ok(s.isSubscribed(channels[0]));
    s.free();
  }
});

test('connectChat: fetches the server and a token, connects, subscribes, delivers', async () => {
  const records = realtimeRecordings()[0];
  const http = records.filter((r) => r.direction === 'http').map((r) => r.frame);
  const incoming = records.filter((r) => r.direction === 'in').map((r) => r.frame);
  const asked = [];
  const fakeFetch = async (url, init) => {
    asked.push([init.method, url]);
    const answer = url.endsWith('/auth/connection') ? http[1] : http[0];
    return { status: answer.status, text: async () => answer.body };
  };
  let socket;
  class FakeSocket {
    constructor(url) {
      this.url = url;
      this.readyState = 0;
      this.sent = [];
      socket = this;
      setTimeout(() => {
        this.readyState = 1;
        this.onopen();
      });
    }
    send(text) {
      this.sent.push(text);
      // Kick answers each frame the recorder sent with the frames that came next.
      const reply = this.sent.length === 1 ? incoming.slice(0, 1) : this.sent.length === 2 ? incoming.slice(1) : [];
      for (const f of reply) setTimeout(() => this.onmessage({ data: f }));
    }
    close() {
      this.readyState = 3;
      if (this.onclose) this.onclose({});
    }
  }

  const events = [];
  const states = [];
  const chat = sdk.connectChat({
    chatroomId: 900000002,
    channelId: 900000001,
    channels: JSON.parse(
      `[${records
        .filter((r) => r.direction === 'out')[1]
        .frame.split('\n')
        .map((l) => JSON.stringify(JSON.parse(l).subscribe.channel))
        .join(',')}]`
    ),
    fetch: fakeFetch,
    createSocket: (url) => new FakeSocket(url),
    onEvent: (e) => events.push(e),
    onState: (s) => states.push(s)
  });
  await new Promise((r) => setTimeout(r, 50));
  chat.close();

  assert.deepEqual(asked.map(([m, u]) => [m, u.replace(/channels\/\d+/, 'channels/N')]), [
    ['POST', 'https://web.kick.com/api/v1/realtime/channels/N/chat/connection'],
    ['POST', 'https://web.kick.com/api/v1/realtime/auth/connection']
  ]);
  assert.match(socket.url, /^wss:\/\/realtime\..*\/connection\/websocket$/);
  assert.ok(states.at(-1).subscribed.includes('chatrooms.900000002.v2'));
  assert.ok(events.some((e) => e.event.type === 'chat_message' && e.channel === 'chatrooms.900000002.v2'));
});
