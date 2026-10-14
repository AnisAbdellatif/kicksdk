#!/usr/bin/env bash
# The precompiled libraries the Dart package downloads, one release asset name
# per line:  scripts/ffi-targets.sh 0.2.0
#
# The list is read from bindings/dart/hook/build.dart (rustTriple), the code
# that picks the file to download, so a library the hook can ask for is a
# library the release must have.
#
#   scripts/ffi-targets.sh --check   also fails unless the release's FFI build
#                                    matrix (.github/workflows/build.yml) builds
#                                    exactly those targets
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 - "$@" <<'PY'
import re, sys

hook = open("bindings/dart/hook/build.dart").read()
body = re.search(r"String rustTriple\(.*?\n}\n", hook, re.S)
if not body:
    sys.exit("bindings/dart/hook/build.dart: no rustTriple function")
triples = sorted(set(re.findall(r"'([a-z0-9_]+(?:-[a-z0-9_]+){2,3})'", body.group(0))))
if not triples:
    sys.exit("bindings/dart/hook/build.dart: no targets in rustTriple")

def ext(t):
    if "-apple-" in t:
        return "dylib"
    if "-windows-" in t:
        return "dll"
    return "so"

if sys.argv[1:] == ["--check"]:
    wf = open(".github/workflows/build.yml").read()
    job = re.search(r"\n  ffi:\n(.*?)(?=\n  [a-z][a-z0-9-]*:\n)", wf, re.S)
    if not job:
        sys.exit(".github/workflows/build.yml: no ffi job")
    built = sorted(set(re.findall(r"target: ([a-z0-9_-]+)", job.group(1))))
    if built != triples:
        sys.exit(
            "the FFI build matrix and the Dart hook disagree:\n"
            f"  only in the hook:   {sorted(set(triples) - set(built))}\n"
            f"  only in the matrix: {sorted(set(built) - set(triples))}"
        )
    print(f"the FFI build matrix builds the hook's {len(triples)} targets")
elif len(sys.argv) == 2 and not sys.argv[1].startswith("-"):
    for t in triples:
        print(f"kicksdk_ffi-v{sys.argv[1]}-{t}.{ext(t)}")
else:
    sys.exit("usage: scripts/ffi-targets.sh <version> | --check")
PY
