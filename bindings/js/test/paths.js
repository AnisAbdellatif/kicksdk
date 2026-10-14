// Where the tests find the fixtures and the built WebAssembly.

import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));

export const FIXTURES = path.join(here, '../../../fixtures');
export const WASM = path.join(here, '../pkg/kicksdk_bg.wasm');
