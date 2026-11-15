#!/usr/bin/env bash
# Checks that every package carries the same version, and that it is the tag
# being released: scripts/release-check.sh 0.2.0
set -euo pipefail
cd "$(dirname "$0")/.."

want=${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}
fail=0
check() {
  if [ "$2" != "$want" ]; then
    echo "$1: ${2:-nothing found} (expected $want)" >&2
    fail=1
  fi
}
check Cargo.toml "$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
# The kicksdk the published kicksdk-ffi asks crates.io for: the one released
# with it.
check "Cargo.toml [workspace.dependencies] kicksdk" \
  "$(sed -n 's/^kicksdk = {.*version = "=\{0,1\}\([^"]*\)".*$/\1/p' Cargo.toml)"
check bindings/js/package.json "$(node -p 'require("./bindings/js/package.json").version')"
check bindings/elixir/mix.exs "$(sed -n 's/^  @version "\(.*\)"$/\1/p' bindings/elixir/mix.exs)"
check bindings/python/pyproject.toml "$(sed -n 's/^version = "\(.*\)"$/\1/p' bindings/python/pyproject.toml)"
check bindings/dart/pubspec.yaml "$(sed -n 's/^version: \(.*\)$/\1/p' bindings/dart/pubspec.yaml)"
check bindings/dart/hook/build.dart "$(sed -n "s/^const version = '\(.*\)';$/\1/p" bindings/dart/hook/build.dart)"
# The hex and pub packages ship CHANGELOG.md (scripts/package-files.sh); it
# has to say what this version is.
if ! grep -qxF "## $want" CHANGELOG.md; then
  echo "CHANGELOG.md: no '## $want' heading" >&2
  fail=1
fi
# The packages that ship the two licence files as committed copies (cargo
# and PEP 639 take nothing from outside the package): the same text as the
# repository's.
for dir in crates/kicksdk crates/kicksdk-ffi bindings/python; do
  for f in LICENSE-MIT LICENSE-APACHE; do
    if ! cmp -s "$f" "$dir/$f"; then
      echo "$dir/$f: missing, or not the same as $f (cp $f $dir/)" >&2
      fail=1
    fi
  done
done
[ "$fail" = 0 ] && echo "every package is at $want"
exit "$fail"
