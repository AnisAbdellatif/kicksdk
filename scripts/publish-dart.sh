#!/usr/bin/env bash
# Publishes the Dart package by hand, the way the release workflow does: for
# the first version, before pub.dev's automated publishing can be switched on
# (it needs the package to exist). Run after the release workflow has uploaded
# the libraries:  scripts/publish-dart.sh 0.1.0
set -euo pipefail
cd "$(dirname "$0")/.."
version=${1:?version}
scripts/release-check.sh "$version"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
gh release download "v$version" --pattern 'kicksdk_ffi-*' --dir "$tmp"
(cd "$tmp" && sha256sum kicksdk_ffi-*) | python3 -c '
import json, sys
sums = {}
for line in sys.stdin:
    digest, name = line.split()
    sums[name.lstrip("*")] = digest
json.dump(sums, open("bindings/dart/hook/checksums.json", "w"), indent=2, sort_keys=True)
'
echo "$(python3 -c 'import json;print(len(json.load(open("bindings/dart/hook/checksums.json"))))') libraries checksummed"
scripts/vendor-native.sh bindings/dart kicksdk-ffi
(cd bindings/dart && dart pub get && dart pub publish)
rm -rf bindings/dart/native bindings/dart/hook/checksums.json
