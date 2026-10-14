#!/usr/bin/env bash
# Fails when a Linux (glibc) library asks for a newer glibc than the release
# promises:  scripts/check-glibc.sh 2.17 target/x86_64-unknown-linux-gnu/release/libkicksdk_ffi.so
#
# The newest GLIBC_x.y version a library needs is the oldest glibc it loads
# on. readelf reads the version needs of any architecture's ELF, so this
# works on the cross-built ones too.
set -euo pipefail
floor=${1:?glibc floor, e.g. 2.17}
shift
[ "$#" -gt 0 ] || { echo "usage: $0 <floor> <library>..." >&2; exit 2; }

fail=0
for lib in "$@"; do
  needs=$(readelf --version-info --wide "$lib" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV)
  newest=$(printf '%s\n' "$needs" | tail -1)
  if [ -z "$newest" ]; then
    echo "$lib: no glibc version needs (not a glibc library?)" >&2
    fail=1
  elif [ "$(printf '%s\n%s\n' "$floor" "$newest" | sort -V | tail -1)" != "$floor" ]; then
    echo "$lib: needs glibc $newest, newer than the $floor floor; symbols:" >&2
    for v in $(printf '%s\n' "$needs" | sort -V); do
      if [ "$(printf '%s\n%s\n' "$floor" "$v" | sort -V | tail -1)" != "$floor" ]; then
        readelf --dyn-syms --wide "$lib" | grep -o "[^ ]*@GLIBC_$v\( \|\$\)" | sed 's/^/  /' >&2
      fi
    done
    fail=1
  else
    echo "$lib: needs glibc $newest (floor $floor)"
  fi
done
exit "$fail"
