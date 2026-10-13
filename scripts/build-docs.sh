#!/usr/bin/env bash
# Builds the docs site into site/ (or $1): the landing page, the Kick
# reference and architecture pages, and every language's API reference.
#
# Needs what each reference needs: cargo, scripts/build-js.sh's tools plus
# npx (TypeDoc), a Python with kicksdk installed and pdoc + markdown
# ($PYTHON, default python3), dart, and mix.
set -euo pipefail
cd "$(dirname "$0")/.."
site=$(realpath -m "${1:-site}")
python=${PYTHON:-python3}
rm -rf "$site" && mkdir -p "$site"

python3 scripts/docs/api_table.py --check

"$python" scripts/docs/build_site.py "$site"

cargo doc --no-deps -p kicksdk -p kicksdk-ffi --all-features
cp -r target/doc "$site/rust"

[ -f bindings/js/pkg/kicksdk.d.ts ] || scripts/build-js.sh
(cd bindings/js && npx -y -p typedoc@0.28 -p typescript@5 typedoc --tsconfig tsconfig.docs.json \
  --entryPoints index.d.ts --out "$site/js" --name "kicksdk (JavaScript)" --readme README.md)

"$python" -m pdoc kicksdk -o "$site/python"

(cd bindings/dart && dart pub get && dart doc -o "$site/dart")

(cd bindings/elixir && mix deps.get && mix docs -o "$site/elixir")

echo "site: $site"
