#!/usr/bin/env bash
# Packs a built NIF the way rustler_precompiled downloads it, reproducibly:
#
#   scripts/package-nif.sh <version> <target> <built library> <out dir>
#
# The tarball's bytes depend on the library's bytes alone (one sorted entry,
# owner and group 0, a fixed mode, the release commit's time, gzip without a
# name or time), so packing the same library again gives the same checksum.
# Needs GNU tar.
set -euo pipefail
version=${1:?version}
target=${2:?target triple}
lib=${3:?built library}
out=${4:?output directory}

case "$target" in
  *windows*) name=kicksdk_nif-v$version-nif-2.15-$target.dll ;;
  *) name=libkicksdk_nif-v$version-nif-2.15-$target.so ;;
esac
# The time of the commit being released, not of the build.
mtime=${SOURCE_DATE_EPOCH:-$(git -C "$(dirname "$0")" log -1 --format=%ct 2>/dev/null || echo 0)}

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp "$lib" "$stage/$name"
mkdir -p "$out"
tar --sort=name --mtime="@$mtime" --owner=0 --group=0 --numeric-owner \
  --mode=0755 --format=gnu -C "$stage" -cf - "$name" | gzip -9 -n > "$out/$name.tar.gz"
echo "$out/$name.tar.gz"
