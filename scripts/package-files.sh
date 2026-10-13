#!/usr/bin/env bash
# Copies the repository's LICENSE (both licenses, in one file, as registries
# that want a single LICENSE expect) and CHANGELOG.md into a package
# directory before it is published:  scripts/package-files.sh bindings/dart
set -euo pipefail
cd "$(dirname "$0")/.."
pkg=${1:?package directory}
{
  echo "kicksdk is licensed under either of the Apache License, Version 2.0 or the"
  echo "MIT license, at your option. Both texts follow."
  echo
  echo "=============================================================================="
  echo
  cat LICENSE-MIT
  echo
  echo "=============================================================================="
  echo
  cat LICENSE-APACHE
} > "$pkg/LICENSE"
cp CHANGELOG.md "$pkg/CHANGELOG.md"
