#!/usr/bin/env bash
# Runs a command with no Rust toolchain on PATH, so that a package which would
# fall back to building from source cannot, and has to use its precompiled
# library:  scripts/without-rust.sh dart test
set -euo pipefail
path=
IFS=: read -ra dirs <<< "$PATH"
for d in "${dirs[@]}"; do
  if [ -e "$d/cargo" ] || [ -e "$d/cargo.exe" ] || [ -e "$d/rustc" ] || [ -e "$d/rustup" ]; then
    continue
  fi
  path=${path:+$path:}$d
done
export PATH=$path
if command -v cargo >/dev/null; then
  echo "cargo is still on PATH: $(command -v cargo)" >&2
  exit 1
fi
exec "$@"
