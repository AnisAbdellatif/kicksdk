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
  # Each crate's .cargo-checksum.json lists its files, and cargo refuses a
  # crate with a listed file missing. Dropped here, with their entries:
  # - the crates' own tests, examples and benches, never built here; some
  #   carry test private keys that pub refuses as leaks;
  # - every dotfile and dot-directory (.github/, .travis.yml, ...): pub
  #   never publishes them. The checksum file itself is one, which
  #   bindings/dart/.pubignore lets through.
  python3 - "$dest/vendor" <<'PY'
import json, pathlib, shutil, sys

def dropped(rel):
    parts = rel.split("/")
    return parts[0] in ("tests", "examples", "benches") or any(p.startswith(".") for p in parts)

for crate in pathlib.Path(sys.argv[1]).iterdir():
    sums = crate / ".cargo-checksum.json"
    meta = json.loads(sums.read_text())
    # Deepest first, so a directory's contents go before it.
    for path in sorted(crate.rglob("*"), reverse=True):
        if path == sums or not dropped(path.relative_to(crate).as_posix()):
            continue
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
    meta["files"] = {f: h for f, h in meta["files"].items() if not dropped(f)}
    sums.write_text(json.dumps(meta))
PY
fi
echo "$dest: kicksdk + $crate"
