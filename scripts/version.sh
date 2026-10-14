#!/usr/bin/env bash
# Prints the workspace version (Cargo.toml), after checking it is one a
# release tag can carry: x.y.z or x.y.z-pre.
set -euo pipefail
cd "$(dirname "$0")/.."
v=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if ! [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "Cargo.toml: '$v' is not a release version" >&2
  exit 1
fi
echo "$v"
