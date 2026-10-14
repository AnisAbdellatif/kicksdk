// Loads the package with its WebAssembly, for every test file.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import * as sdk from '../index.js';

const here = path.dirname(fileURLToPath(import.meta.url));

export const FIXTURES = path.join(here, '../../../fixtures');
export const WASM = path.join(here, '../pkg/kicksdk_bg.wasm');

await sdk.init(readFileSync(WASM));

export { sdk };
