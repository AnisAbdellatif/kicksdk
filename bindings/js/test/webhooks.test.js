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
  const ms = /milliseconds \(Math\.floor\(Date\.now\(\) \/ 1000\)\)/;
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: Date.now() }), ms);
  // Date.now() / 1000 is fractional; the fix the message suggests is taken
  // (and only refused because the fixture is old).
  assert.throws(() => sdk.verifyWebhook(pem, headers, body, { now: Date.now() / 1000 }), /whole number/);
  assert.throws(
    () => sdk.verifyWebhook(pem, headers, body, { now: Math.floor(Date.now() / 1000) }),
    /beyond the tolerance/
  );
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

test('retryAfterMs: terminal codes wait 5 minutes, the rest the ordinary backoff', () => {
  for (const code of [1000, 1006, 3000, 3499, 4000, 4499, 5000]) assert.equal(sdk.retryAfterMs(code), 0, String(code));
  for (const code of [3500, 3501, 3999, 4500, 4999]) assert.equal(sdk.retryAfterMs(code), 300_000, String(code));
  assert.throws(() => sdk.retryAfterMs(70000), RangeError);
  assert.throws(() => sdk.retryAfterMs(-1), RangeError);
});

test('Backoff.nextDelayFor: 5, 10, 20, 30, 30 minutes for terminal closes in a row', () => {
  const b = new sdk.Backoff();
  const waits = () => Array.from({ length: 5 }, () => b.nextDelayFor(300_000, 0));
  assert.deepEqual(waits(), [300_000, 600_000, 1_200_000, 1_800_000, 1_800_000]);
  b.reset();
  assert.equal(b.nextDelayFor(300_000, 0), 300_000);
  b.reset();
  assert.equal(b.nextDelayFor(300_000, 0.5), 337_500, 'plus up to a quarter');
  b.free();
  // 0 is the ordinary stage: nextDelayJittered.
  const c = new sdk.Backoff();
  assert.equal(c.nextDelayFor(0, 0), 500);
  assert.equal(c.nextDelayFor(0, 0), 1000);
  // The ordinary stage is not touched by a terminal close.
  assert.equal(c.nextDelayFor(300_000, 0), 300_000);
  assert.equal(c.nextDelayFor(0, 0), 2000);
  assert.throws(() => c.nextDelayFor(-1, 0), RangeError);
  assert.throws(() => c.nextDelayFor(1.5, 0), RangeError);
  assert.throws(() => c.nextDelayFor(2 ** 50, 0), RangeError, 'its wait would not be exact');
  c.free();
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
