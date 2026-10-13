#!/usr/bin/env bash
# Builds the npm package's WebAssembly: bindings/js/pkg/.
#
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the version
# Cargo.lock pins (cargo install wasm-bindgen-cli --version <it> --locked).
# wasm-opt (binaryen) is used when it is on PATH, and skipped when it is not.
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')
if [ "$want" != "$have" ]; then
  echo "wasm-bindgen-cli $want is needed (found ${have:-none}):" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown -p kicksdk-wasm
rm -rf bindings/js/pkg
wasm-bindgen --target web --out-dir bindings/js/pkg --out-name kicksdk \
  target/wasm32-unknown-unknown/release/kicksdk_wasm.wasm
if command -v wasm-opt >/dev/null; then
  # The features rustc emits for wasm32 by default (bulk memory, sign-ext,
  # reference types, multivalue, non-trapping float-to-int).
  wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-reference-types --enable-multivalue --enable-mutable-globals \
    bindings/js/pkg/kicksdk_bg.wasm -o bindings/js/pkg/kicksdk_bg.wasm
fi
rm -f bindings/js/pkg/.gitignore
ls -l bindings/js/pkg/kicksdk_bg.wasm
