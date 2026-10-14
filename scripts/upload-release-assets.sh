#!/usr/bin/env bash
# Uploads files to the GitHub release <tag>, never replacing one it has:
#
#   scripts/upload-release-assets.sh v0.2.0 dist/*
#
# A release asset's checksum goes into the hex and pub packages, and neither
# registry takes a version twice: replacing the file under a published
# checksum breaks every install of that version. So an asset already there is
# kept as it is. When the new file differs (a re-run rebuilt it), that is
# reported as a warning and the release's file still wins; the packages take
# their checksums from the release, never from the build.
set -euo pipefail
tag=${1:?tag}
shift
[ "$#" -gt 0 ] || { echo "usage: $0 <tag> <file>..." >&2; exit 2; }

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | awk '{print $1}'
}

# name -> "sha256:<hex>" (GitHub records a digest for each upload).
existing=$(gh release view "$tag" --json assets --jq '.assets[] | "\(.name) \(.digest // "")"')
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for f in "$@"; do
  name=$(basename "$f")
  ours=$(sha256 "$f")
  line=$(printf '%s\n' "$existing" | awk -v n="$name" '$1 == n' || true)
  if [ -z "$line" ]; then
    gh release upload "$tag" "$f"
    echo "uploaded $name ($ours)"
    continue
  fi
  theirs=$(printf '%s\n' "$line" | awk '{print $2}' | sed 's/^sha256://')
  if [ -z "$theirs" ]; then
    gh release download "$tag" --pattern "$name" --dir "$tmp" --clobber
    theirs=$(sha256 "$tmp/$name")
  fi
  if [ "$theirs" = "$ours" ]; then
    echo "kept $name: the release has these bytes already"
  else
    echo "::warning title=Release asset kept::$name is already on $tag with SHA-256 $theirs; this build's is $ours. The release's file stays (published checksums may name it); delete it by hand first only if no registry has published this version."
  fi
done
