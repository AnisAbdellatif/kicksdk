// Loads the package with its WebAssembly, for the test files that need it
// ready.

import { readFileSync } from 'node:fs';

import * as sdk from '../index.js';
import { FIXTURES, WASM } from './paths.js';

await sdk.init(readFileSync(WASM));

export { sdk, FIXTURES, WASM };
