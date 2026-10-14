#!/usr/bin/env bash
# Builds the Dart package from source as a user of the published package
# would: from the archive pub uploads, not from the checkout, which has files
# pub leaves out (dotfiles, ignored paths).
#
#   (cd bindings/dart && dart pub publish --to-archive=/tmp/kicksdk.tar.gz)
#   scripts/check-dart-archive.sh /tmp/kicksdk.tar.gz
#
# The archive is extracted, and an app that depends on it by path runs with
# build_from_source set, so the hook builds native/ with cargo. Cargo's home
# is an empty directory (nothing from its cache) and, where unprivileged user
# namespaces are allowed, there is no network: native/vendor/ has to hold
# every crate, in a form cargo accepts. The app's dependencies are fetched
# before the network goes.
set -euo pipefail
archive=$(realpath "${1:?archive from dart pub publish --to-archive}")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/kicksdk" "$work/app/bin" "$work/cargo-home"
tar -xzf "$archive" -C "$work/kicksdk"
if [ ! -d "$work/kicksdk/native/vendor" ]; then
  echo "the archive has no native/vendor/: run scripts/vendor-native.sh bindings/dart kicksdk-ffi first" >&2
  exit 1
fi

cat > "$work/app/pubspec.yaml" <<EOF
name: archive_check
publish_to: none
environment:
  sdk: ^3.10.0
dependencies:
  kicksdk:
    path: $work/kicksdk
hooks:
  user_defines:
    kicksdk:
      build_from_source: true
      cargo_home: $work/cargo-home
EOF
cat > "$work/app/bin/main.dart" <<'EOF'
import 'package:kicksdk/kicksdk.dart';

void main() {
  final event = Events.parse(r'App\Events\StreamHostEvent', '{"host_username":"A","number_viewers":3}');
  if (event['type'] != 'host') throw StateError('unexpected $event');
  print('kicksdk built from the archive: $event');
}
EOF
(cd "$work/app" && dart pub get)

offline=()
if unshare -rn true 2>/dev/null; then
  offline=(unshare -rn)
else
  echo "::notice::unshare -rn is not allowed here: building with the network up (still offline for cargo: empty CARGO_HOME, --offline)"
fi
(cd "$work/app" && "${offline[@]}" dart run bin/main.dart)
