#!/usr/bin/env bash
# Builds the NIF from source as a user of the hex package would: from the
# package hex uploads (mix hex.build --unpack), not from the checkout.
#
#   scripts/vendor-native.sh bindings/elixir kicksdk-nif
#   scripts/package-files.sh bindings/elixir
#   (cd bindings/elixir && mix deps.get)   # and the checksum file
#   scripts/check-hex-package.sh
#
# An app that depends on the unpacked package by path compiles it with
# KICKSDK_BUILD=1, so rustler builds the package's native/ (the crates come
# from crates.io: the hex package does not vendor them), then calls the NIF.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
(cd bindings/elixir && mix hex.build --unpack --output "$work/kicksdk")
if [ ! -d "$work/kicksdk/native/crates/kicksdk-nif" ]; then
  echo "the package has no native/: run scripts/vendor-native.sh bindings/elixir kicksdk-nif first" >&2
  exit 1
fi

mkdir -p "$work/app"
cat > "$work/app/mix.exs" <<EOF
defmodule ArchiveCheck.MixProject do
  use Mix.Project

  def project do
    [
      app: :archive_check,
      version: "0.0.0",
      deps: [{:kicksdk, path: "$work/kicksdk"}, {:rustler, "~> 0.38.0", runtime: false}]
    ]
  end
end
EOF
cd "$work/app"
mix deps.get
KICKSDK_BUILD=1 mix run -e '
  event = Kicksdk.Events.parse(~S(App\Events\StreamHostEvent), ~s({"host_username":"A","number_viewers":3}))
  %{"type" => "host"} = event
  IO.puts("kicksdk built from the hex package: #{inspect(event)}")
'
