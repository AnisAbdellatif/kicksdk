// init() in a fresh module (node --test runs each file in its own process):
// a failed load is not cached, so a later init() can succeed.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import * as sdk from '../index.js';
import { WASM } from './paths.js';

test('a failed init is retried by the next call', async () => {
  const offline = Promise.reject(new TypeError('fetch failed'));
  await assert.rejects(sdk.init(offline), /fetch failed/);
  const wasm = new Response(readFileSync(WASM), { headers: { 'content-type': 'application/wasm' } });
  assert.equal(await sdk.init(wasm), undefined);
  // Loaded once; later calls are the same promise.
  assert.equal(sdk.init(), sdk.init());
  assert.deepEqual(sdk.chatChannels(1)[0], 'chatrooms.1.v2');
});
