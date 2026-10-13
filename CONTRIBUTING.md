# Contributing

## When Kick changes

Follow this order, every time:

1. **`docs/KICK.md`** first. Write down what Kick does now, where it was
   seen, and the date. Keep what it used to do if anyone may still meet it,
   the way §6.1a keeps Pusher.
2. **A recording** in `fixtures/`, anonymized (below), that shows it.
3. **The core.** Change `crates/kicksdk` until the new recording passes.
4. **The bindings.** Rebuild them and run their tests, which read the same
   fixtures. A new public function goes in `crates/kicksdk/src/dispatch.rs`
   (which makes it callable from C, Dart and Python) and gets a wrapper in
   `crates/kicksdk-wasm/src/lib.rs`, `crates/kicksdk-nif/src/lib.rs`, and
   each language's module (`bindings/*/`).

Never change a parser to fit what you think Kick sends. Change it to fit a
recording of what Kick did send.

## Recording

`tools/record-realtime.mjs <slug> [seconds]` records a session with the
realtime feed into `fixtures/realtime/`: the connection and token answers,
every frame in and out. Pick a busy live channel so chat is in it.

It anonymizes as it goes:

- tokens are `[redacted]`;
- names become `user0001` (a username and its slug share one pseudonym);
- message text becomes `text-N`, keeping its emote tokens' shape;
- ids become `900000001`…, and UUIDs `00000000-0000-4000-8000-…`;
- URLs become `https://example.invalid/asset/N`.

Kick's protocol, event names, feed names (with the ids replaced), badge kinds,
colours and times are kept.

The fixtures under `public_api/`, `v2/`, `webhook/`, `id/`, `probe/` and
`pusher-legacy/` came from KickTracker's recorder and anonymizer
(`mix record.*`, `mix fixtures.anonymize`).

**Read a recording before committing it.** Look for anything that is a real
person's name or words, a token, a session, or a signed URL (`playback_url`).
This repository is public.

## Checks

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p kicksdk --all-features
cargo test --workspace --all-features
scripts/build-js.sh && (cd bindings/js && npm test)
(cd bindings/python && maturin develop && pytest)
(cd bindings/dart && dart format --line-length 120 . && dart analyze --fatal-infos && dart test)
(cd bindings/elixir && mix format --check-formatted && mix test)
scripts/release-check.sh
```

CI runs all of them.

## Releasing

Bump the version everywhere `scripts/release-check.sh` looks (Cargo.toml,
package.json, mix.exs, pyproject.toml, pubspec.yaml, and `version` in
`bindings/dart/hook/build.dart`), commit, then tag `v<version>` and push the
tag. The release workflow does the rest.
