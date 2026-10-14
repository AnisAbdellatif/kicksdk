#!/usr/bin/env bash
# Puts a self-contained Cargo workspace in <package>/native/, for packages
# whose fallback is to build from source: the hex package (crate kicksdk-nif)
# and the pub package (crate kicksdk-ffi). Run by the release workflow just
# before publishing; native/ is never committed.
#
#   scripts/vendor-native.sh bindings/elixir kicksdk-nif
#   scripts/vendor-native.sh bindings/dart kicksdk-ffi
#
# The workspace is the repository's own Cargo.toml, Cargo.lock and the two
# crates it needs, so a source build there is the build CI tested, down to
# the dependency versions (--locked).
set -euo pipefail
cd "$(dirname "$0")/.."

pkg=${1:?package directory}
crate=${2:?binding crate under crates/}
dest="$pkg/native"

rm -rf "$dest"
mkdir -p "$dest/crates"
for c in kicksdk "$crate"; do
  mkdir -p "$dest/crates/$c"
  cp -r "crates/$c/src" "crates/$c/Cargo.toml" "$dest/crates/$c/"
  [ -d "crates/$c/include" ] && cp -r "crates/$c/include" "$dest/crates/$c/"
done
cp Cargo.lock LICENSE-MIT LICENSE-APACHE "$dest/"
python3 - "$dest/Cargo.toml" "$crate" <<'PY'
import re, sys
dest, crate = sys.argv[1], sys.argv[2]
s = open("Cargo.toml").read()
members = f'["crates/kicksdk", "crates/{crate}"]'
s = re.sub(r"members = \[[^\]]*\]", f"members = {members}", s, count=1)
s = re.sub(r"default-members = \[[^\]]*\]", f"default-members = {members}", s, count=1)
open(dest, "w").write(s)
PY
# The crate sources' readme points at the repository's README.
cp README.md "$dest/README.md"
# Drops the other members' packages from the copied lock file, keeping every
# version as it was, so `cargo build --locked` holds there.
cargo fetch --locked
(cd "$dest" && cargo metadata --offline --format-version 1 >/dev/null)
# pub's package also carries the dependencies' sources (about 6 MB
# compressed), so the Dart hook's source build works offline: it builds
# from native/vendor/ when that is there. hex's package size limit leaves no
# room for them, so a NIF source build fetches its crates.
if [ "$crate" = kicksdk-ffi ]; then
  (cd "$dest" && cargo vendor --locked --versioned-dirs --quiet vendor >/dev/null)
  # The crates' own tests, examples and benches are never built here, and
  # some carry test private keys that pub refuses as leaks: drop them, and
  # their entries in each crate's checksum list so cargo still accepts the
  # rest.
  python3 - "$dest/vendor" <<'PY'
import json, pathlib, shutil, sys
for crate in pathlib.Path(sys.argv[1]).iterdir():
    sums = crate / ".cargo-checksum.json"
    meta = json.loads(sums.read_text())
    for d in ("tests", "examples", "benches"):
        shutil.rmtree(crate / d, ignore_errors=True)
        meta["files"] = {f: h for f, h in meta["files"].items() if not f.startswith(d + "/")}
    sums.write_text(json.dumps(meta))
PY
fi
echo "$dest: kicksdk + $crate"
