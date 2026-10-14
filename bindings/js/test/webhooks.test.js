// Webhook verification: freshness, the body as text or bytes, the key read
// once, and the realtime helpers that came with the same core release.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { sdk, FIXTURES } from './load.js';

const dir = path.join(FIXTURES, 'signing');
const pem = readFileSync(path.join(dir, 'test-public-key.pem'), 'utf8');
const body = readFileSync(path.join(dir, 'body.json'), 'utf8');
const headers = {
  'kick-event-message-id': '01M38M9K3YRKV8E6Q68C18ZWG4',
  'kick-event-message-timestamp': '2026-09-24T02:35:42Z',
  'kick-event-signature': readFileSync(path.join(dir, 'signature.b64'), 'utf8').trim(),
  'kick-event-type': 'channel.followed',
  'kick-event-version': '1'
};
// The delivery's timestamp, Unix seconds.
const sent = Date.parse(headers['kick-event-message-timestamp']) / 1000;

test('a fresh delivery verifies, as text or bytes, with any headers shape', () => {
  const ok = sdk.verifyWebhook(pem, headers, body, { now: sent + 2 });
  assert.equal(ok.event.type, 'followed');
  assert.equal(ok.headers.message_id, headers['kick-event-message-id']);
  const bytes = new TextEncoder().encode(body);
  assert.equal(sdk.verifyWebhook(pem, headers, bytes, { now: sent }).event.type, 'followed');
  assert.equal(sdk.verifyWebhook(pem, headers, bytes.buffer, { now: sent }).event.type, 'followed');
  assert.equal(sdk.verifyWebhook(pem, Object.entries(headers), body, { now: sent }).event.type, 'followed');
  assert.equal(sdk.verifyWebhook(pem, new Headers(headers), body, { now: sent }).event.type, 'followed');
  assert.equal(sdk.verifyWebhook(pem, new Map(Object.entries(headers)), body, { now: sent }).event.type, 'followed');
  // One byte off fails.
  const changed = Uint8Array.from(bytes);
  changed[changed.length - 2] ^= 1;
  assert.throws(() => sdk.verifyWebhook(pem, headers, changed, { now: sent }));
});

test('freshness: 5 minutes by default, a tolerance, or skipped on purpose', () => {
  assert.ok(sdk.verifyWebhook(pem, headers, body, { now: sent + 300 }));
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: sent + 301 }), /beyond the tolerance/);
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: sent - 301 }));
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: sent + 20, toleranceSecs: 10 }));
  assert.ok(sdk.verifyWebhook(pem, headers, body, { now: sent + 3600, toleranceSecs: 3600 }));
  // This machine's clock is past the fixture's timestamp: refused by default.
  assert.throws(() => sdk.verifyWebhook(pem, headers, body));
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, {}));
  assert.equal(sdk.verifyWebhook(pem, headers, body, { skipFreshness: true }).event.type, 'followed');
  // Mistakes throw rather than pass or fail for the wrong reason.
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: Date.now() }), /milliseconds/);
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: sent, skipFreshness: true }), TypeError);
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { tolerance_secs: 10 }), /unknown key/);
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: -1 }), RangeError);
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: String(sent) }), TypeError);
  assert.throws(() => sdk.verifyWebhook(pem, headers, 42, { skipFreshness: true }), TypeError);
  assert.throws(() => sdk.verifyWebhook(pem, 'nope', body, { skipFreshness: true }), TypeError);
  const numeric = { ...headers, 'kick-event-version': 1 };
  assert.throws(() => sdk.verifyWebhook(pem, numeric, body, { skipFreshness: true }), TypeError);
});

test('a key read once verifies many deliveries', () => {
  const key = new sdk.WebhookPublicKey(pem);
  for (let i = 0; i < 3; i++) assert.equal(key.verify(headers, body, { now: sent }).event.type, 'followed');
  assert.throws(() => key.verify(headers, body, { now: sent + 1000 }));
  key.free();
  assert.throws(() => new sdk.WebhookPublicKey('not a key'));
});

test('parseWebhook takes text or bytes, and refuses another type\'s shape', () => {
  assert.equal(sdk.parseWebhook('channel.followed', body).type, 'followed');
  assert.equal(sdk.parseWebhook('channel.followed', new TextEncoder().encode(body)).type, 'followed');
  assert.throws(() => sdk.parseWebhook('kicks.gifted', body));
});

test('shouldReconnect: terminal codes say no', () => {
  for (const code of [1000, 1006, 3000, 3499, 4000, 4499]) assert.equal(sdk.shouldReconnect(code), true, String(code));
  for (const code of [3500, 3501, 3999, 4500, 4999]) assert.equal(sdk.shouldReconnect(code), false, String(code));
  assert.throws(() => sdk.shouldReconnect(70000), RangeError);
  assert.throws(() => sdk.shouldReconnect(-1), RangeError);
});

test('Backoff.nextDelayJittered spreads the delay over [half, all]', () => {
  const b = new sdk.Backoff();
  assert.equal(b.nextDelayJittered(0), 500);
  assert.equal(b.nextDelayJittered(0.5), 1500);
  assert.equal(b.nextDelayJittered(0.999999), 3999);
  b.nextDelay();
  b.nextDelay();
  for (let i = 0; i < 20; i++) {
    const d = b.nextDelayJittered(Math.random());
    assert.ok(d >= 15000 && d <= 30000, String(d));
  }
  b.free();
});
