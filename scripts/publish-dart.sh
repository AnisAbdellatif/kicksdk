#!/usr/bin/env bash
# Publishes the Dart package by hand, the way the release workflow does:
#
#   git checkout v0.2.0 && scripts/publish-dart.sh 0.2.0
#
# For the first version, before pub.dev's automated publishing can be switched
# on (it needs the package to exist), and for any version whose workflow run
# was not a tag push (pub.dev takes automated publishes from tag pushes only).
# Run it from a clean checkout of the version's tag, after the release has all
# the libraries: what is published is the tag's code, with the checksums of
# the files the release serves.
set -euo pipefail
cd "$(dirname "$0")/.."
version=${1:?version}
tag=v$version
repo=AnisAbdellatif/kicksdk

if ! head=$(git rev-parse --verify -q HEAD) || [ "$head" != "$(git rev-parse --verify -q "$tag^{commit}" || true)" ]; then
  echo "HEAD is not $tag: git checkout $tag first" >&2
  exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "the checkout has changes; publish from a clean checkout of $tag:" >&2
  git status --short >&2
  exit 1
fi
scripts/release-check.sh "$version"

tmp=$(mktemp -d)
# The release-time files never outlive the publish, whichever way it ends.
cleanup() {
  rm -rf "$tmp" bindings/dart/native bindings/dart/hook/checksums.json \
    bindings/dart/LICENSE bindings/dart/CHANGELOG.md
}
trap cleanup EXIT

gh release download "$tag" --repo "$repo" --pattern 'kicksdk_ffi-*' --dir "$tmp"
scripts/dart-checksums.sh "$version" "$tmp"
scripts/vendor-native.sh bindings/dart kicksdk-ffi
scripts/package-files.sh bindings/dart
# The source fallback, from the archive pub would upload (pub leaves out
# files the checkout has), before anything is published.
(cd bindings/dart && dart pub get && dart pub publish --to-archive="$tmp/kicksdk.tar.gz")
scripts/check-dart-archive.sh "$tmp/kicksdk.tar.gz"
(cd bindings/dart && dart pub publish)
