#!/usr/bin/env bash
# Builds the npm package's WebAssembly: bindings/js/pkg/.
#
# Needs the wasm32-unknown-unknown target, wasm-bindgen-cli at the version
# Cargo.lock pins and binaryen's wasm-opt 116 or newer
# (scripts/install-wasm-tools.sh installs both). Without wasm-opt the build
# fails, unless KICKSDK_NO_WASM_OPT=1 says an unoptimized module is wanted
# (never for a release: it is larger and slower).
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$want" != "$have" ]; then
  echo "wasm-bindgen-cli $want is needed (found ${have:-none}):" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

# Older wasm-opt (Ubuntu's apt package among them) mangles the reference-type
# table wasm-bindgen sets up, and the module then fails to load ("failed to
# grow table").
optimize=1
opt_version=$(wasm-opt --version 2>/dev/null | sed -n 's/.*version \([0-9]*\).*/\1/p' || true)
if [ "${opt_version:-0}" -lt 116 ]; then
  if [ "${KICKSDK_NO_WASM_OPT:-}" = 1 ]; then
    echo "KICKSDK_NO_WASM_OPT=1: not optimizing" >&2
    optimize=0
  else
    echo "wasm-opt 116+ is needed (found ${opt_version:-none}): scripts/install-wasm-tools.sh," >&2
    echo "or KICKSDK_NO_WASM_OPT=1 for an unoptimized build" >&2
    exit 1
  fi
fi

cargo build --release --locked --target wasm32-unknown-unknown -p kicksdk-wasm
rm -rf bindings/js/pkg
wasm-bindgen --target web --out-dir bindings/js/pkg --out-name kicksdk \
  target/wasm32-unknown-unknown/release/kicksdk_wasm.wasm
if [ "$optimize" = 1 ]; then
  # The features rustc emits for wasm32 by default (bulk memory, sign-ext,
  # reference types, multivalue, non-trapping float-to-int).
  wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-reference-types --enable-multivalue --enable-mutable-globals \
    bindings/js/pkg/kicksdk_bg.wasm -o bindings/js/pkg/kicksdk_bg.wasm
fi
rm -f bindings/js/pkg/.gitignore
ls -l bindings/js/pkg/kicksdk_bg.wasm
