// What crosses into the WebAssembly is checked, never cast: an id that is
// not a whole, safe, non-negative number throws instead of becoming another
// id, and `endpoints` takes what TypeScript's Partial<> lets through.

import test from 'node:test';
import assert from 'node:assert/strict';

import { sdk } from './load.js';

test('ids must be whole, non-negative, safe numbers', () => {
  assert.deepEqual(sdk.chatChannels(668), ['chatrooms.668.v2', 'chatrooms.668', 'chatroom_668']);
  assert.deepEqual(sdk.chatChannels(668, undefined), sdk.chatChannels(668));
  assert.deepEqual(sdk.chatChannels(668, null), sdk.chatChannels(668));
  for (const bad of [-5, 1.5, NaN, Infinity, 2 ** 53, -0.5]) {
    assert.throws(() => sdk.chatChannels(bad), RangeError, String(bad));
    assert.throws(() => sdk.chatChannels(668, bad), RangeError, String(bad));
    assert.throws(() => sdk.historyRequest(bad), RangeError, String(bad));
    assert.throws(() => sdk.livestreamsRequests('t', [1, bad]), RangeError, String(bad));
  }
  // A numeric string is not coerced.
  assert.throws(() => sdk.chatChannels('123'), TypeError);
  assert.throws(() => sdk.chatConnectionRequest('123', 'c'), TypeError);
  assert.throws(() => sdk.deleteMessageRequest('s', '1', 'm'), TypeError);
  assert.throws(() => sdk.chatChannels(undefined), TypeError);
  assert.throws(() => sdk.livestreamsRequests('t', ['1']), /userIds\[0\]/);
  assert.throws(() => sdk.livestreamsRequests('t', '12'), TypeError);
  assert.equal(sdk.chatChannels(2 ** 53 - 1)[0], `chatrooms.${2 ** 53 - 1}.v2`);
});

test('counts and times are checked too', () => {
  assert.throws(() => sdk.timeoutRequest('s', 'somechannel', 'someone', -1), RangeError);
  assert.throws(() => sdk.timeoutRequest('s', 'somechannel', 'someone', 1.5), RangeError);
  assert.throws(() => sdk.timeoutRequest('s', 'somechannel', 'someone', 2 ** 32 + 5), RangeError);
  assert.equal(sdk.timeoutRequest('s', 'somechannel', 'someone', 10).method, 'POST');
  assert.throws(() => sdk.topLivestreamsRequest('t', null, null, null, -1), RangeError);
  assert.throws(() => sdk.topLivestreamsRequest('t', null, null, 'loudest'), RangeError);
  assert.throws(() => sdk.topLivestreamsRequest('t', '5'), TypeError);
  assert.match(sdk.topLivestreamsRequest('t', 5, null, 'started_at', 10).url, /limit=10/);
  assert.throws(() => sdk.subscribeRequest('t', 1, [{ name: 'channel.followed', version: 1.5 }]), RangeError);
  assert.throws(() => sdk.subscribeRequest('t', 1, [{ version: 1 }]), TypeError);
  assert.throws(() => sdk.subscribeRequest('t', -1, [{ name: 'channel.followed' }]), RangeError);
  const body = JSON.parse(sdk.subscribeRequest('t', 1, [{ name: 'channel.followed' }]).body);
  assert.deepEqual(body.events, [{ name: 'channel.followed', version: 1 }]);
  assert.throws(
    () => sdk.sendRequest('s', 1, 'hi', 'r', { message_id: 'm', sender_id: '5', sender_username: 'someone' }),
    TypeError
  );

  const s = new sdk.Session(['chatrooms.1.v2']);
  for (const bad of [-1, 1.5, NaN, 2 ** 53]) assert.throws(() => s.open('t', bad), RangeError, String(bad));
  assert.equal(s.open('t', 0)[0].action, 'send');
  s.free();
  assert.throws(() => new sdk.Backoff(-1), RangeError);
  assert.throws(() => new sdk.Backoff(1000, 1.5), RangeError);
  const b = new sdk.Backoff(10, 40);
  assert.deepEqual([b.nextDelay(), b.nextDelay(), b.nextDelay(), b.nextDelay()], [10, 20, 40, 40]);
  b.free();
});

test('endpoints: undefined or left out keeps Kick, a typo throws', () => {
  const kick = sdk.tokenRequest('c').url;
  assert.equal(sdk.tokenRequest('c', undefined).url, kick);
  assert.equal(sdk.tokenRequest('c', null).url, kick);
  assert.equal(sdk.tokenRequest('c', {}).url, kick);
  // What a Partial<Endpoints> built from optional config looks like.
  assert.equal(sdk.tokenRequest('c', { realtime: undefined, api: undefined }).url, kick);
  const local = sdk.tokenRequest('c', { realtime: 'http://localhost:9/rt' });
  assert.equal(local.url, 'http://localhost:9/rt/auth/connection');
  assert.throws(() => sdk.tokenRequest('c', { realtme: 'http://x' }), /unknown key `realtme`/);
  assert.throws(() => sdk.tokenRequest('c', { realtime: 5 }), TypeError);
  assert.throws(() => sdk.tokenRequest('c', 'http://x'), TypeError);
});
