#!/usr/bin/env bash
# Uploads files to the GitHub release <tag>, never replacing one it has:
#
#   scripts/upload-release-assets.sh v0.2.0 dist/*
#
# A release asset's checksum goes into the hex and pub packages, and neither
# registry takes a version twice: replacing the file under a published
# checksum breaks every install of that version. So an asset already there is
# kept as it is. When it differs from this build's file (a re-run rebuilt it,
# or someone else uploaded a file of that name), the script fails, after
# going through every file: the hex and pub jobs, which need the upload jobs,
# then do not run, so no package vouches for bytes this run did not build.
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
fail=0

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
    echo "::error title=Release asset differs::$name is already on $tag with SHA-256 $theirs; this build's is $ours. The release's file is kept (published checksums may name it). If neither hex nor pub has published this version, delete $name from the release by hand (gh release delete-asset $tag $name) and re-run; once either has, release a new version."
    fail=1
  fi
done
exit "$fail"
