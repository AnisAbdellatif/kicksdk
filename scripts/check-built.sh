#!/usr/bin/env bash
# Fails unless a package's checksum file names exactly the files in <dir>,
# each with its bytes. The hex and pub jobs record the checksums of the
# release's own files; this holds those to the files this run built, so a
# same-named file someone else put on the release is never vouched for.
#
#   scripts/check-built.sh <dir> bindings/elixir/checksum-Elixir.Kicksdk.Native.exs
#   scripts/check-built.sh <dir> bindings/dart/hook/checksums.json
set -euo pipefail
dir=${1:?directory with the files this run built}
sums=${2:?checksum file}

python3 - "$dir" "$sums" <<'PY'
import hashlib, json, os, re, sys

d, path = sys.argv[1], sys.argv[2]
text = open(path).read()
if path.endswith(".json"):
    listed = json.loads(text)
else:  # rustler_precompiled's: %{"<file>" => "sha256:<hex>", ...}
    listed = dict(re.findall(r'"([^"]+)"\s*=>\s*"sha256:([0-9a-f]{64})"', text))
built = {}
for n in sorted(os.listdir(d)):
    with open(os.path.join(d, n), "rb") as f:
        built[n] = hashlib.sha256(f.read()).hexdigest()
bad = not listed
if bad:
    print(f"::error::{path} lists no files")
for n in sorted(set(listed) | set(built)):
    if n not in built:
        print(f"::error::{path} names {n}, which this run did not build")
    elif n not in listed:
        print(f"::error::{path} leaves out {n}, which this run built")
    elif listed[n] != built[n]:
        print(f"::error::{n}: the release's file has SHA-256 {listed[n]}; this run built {built[n]}")
    else:
        continue
    bad = True
if bad:
    sys.exit(1)
print(f"{path}: {len(listed)} files, each the bytes this run built")
PY
