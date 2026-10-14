#!/usr/bin/env bash
# Installs what scripts/build-js.sh needs, on Linux x86_64 (CI):
#   * wasm-bindgen-cli at the version Cargo.lock pins (cargo install --locked);
#   * binaryen's wasm-opt, a pinned release checked against its SHA-256
#     (apt's is too old for wasm-bindgen's reference-type tables).
# binaryen goes to $1 (default: $RUNNER_TEMP or /tmp), and onto $GITHUB_PATH
# when there is one.
set -euo pipefail
cd "$(dirname "$0")/.."

binaryen=version_120
binaryen_sha256=ddb097af51d1bdb17300d986b0de7d97422f1933dedb4c9eda3510e0bf4076cc
dest=${1:-${RUNNER_TEMP:-/tmp}}

v=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
cargo install wasm-bindgen-cli --version "$v" --locked

tarball="$dest/binaryen-$binaryen-x86_64-linux.tar.gz"
mkdir -p "$dest"
curl -sSfL -o "$tarball" \
  "https://github.com/WebAssembly/binaryen/releases/download/$binaryen/binaryen-$binaryen-x86_64-linux.tar.gz"
echo "$binaryen_sha256  $tarball" | sha256sum -c -
tar xzf "$tarball" -C "$dest"
bin="$dest/binaryen-$binaryen/bin"
"$bin/wasm-opt" --version
if [ -n "${GITHUB_PATH:-}" ]; then echo "$bin" >> "$GITHUB_PATH"; fi
