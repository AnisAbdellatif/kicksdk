#!/usr/bin/env bash
# Writes bindings/dart/hook/checksums.json, the SHA-256 of every precompiled
# library the Dart package may download, from a directory holding them:
#
#   scripts/dart-checksums.sh <version> <dir>
#
# Every library scripts/ffi-targets.sh names must be there: a package missing
# one would build from source on that target, or, worse, be published with a
# list that the release does not match. Used by the release workflow and
# scripts/publish-dart.sh, with the files the release itself serves.
set -euo pipefail
cd "$(dirname "$0")/.."
version=${1:?version}
dir=${2:?directory with the kicksdk_ffi-* libraries}

scripts/ffi-targets.sh "$version" | python3 -c '
import hashlib, json, os, sys
d = sys.argv[1]
want = sys.stdin.read().split()
missing = [n for n in want if not os.path.isfile(os.path.join(d, n))]
if missing:
    sys.exit("missing precompiled libraries:\n  " + "\n  ".join(missing))
extra = sorted(n for n in os.listdir(d) if n.startswith("kicksdk_ffi-") and n not in want)
for n in extra:
    print(f"warning: {n} is not a target the Dart hook knows; left out", file=sys.stderr)
sums = {}
for n in want:
    with open(os.path.join(d, n), "rb") as f:
        sums[n] = hashlib.sha256(f.read()).hexdigest()
with open("bindings/dart/hook/checksums.json", "w") as f:
    json.dump(sums, f, indent=2, sort_keys=True)
    f.write("\n")
print(f"bindings/dart/hook/checksums.json: {len(sums)} libraries")
' "$dir"
