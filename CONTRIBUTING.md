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

- tokens are `[redacted]`, and so is the reason a socket was closed with;
- names become `user0001` (a username and its slug share one pseudonym);
- message text becomes `text-N`, keeping its emote tokens' shape;
- ids (`id`, `user_id`, `userId`, `user_ids`, …) become `900000001`…, and
  UUIDs `00000000-0000-4000-8000-…`;
- URLs, whatever the scheme, become `https://example.invalid/asset/N`;
- every other string becomes `str-N`, wherever it is: in the events, in the
  history a subscribe or connect answer carries, in join and leave info, in
  a publication's info and tags, and in the HTTP answers;
- a line that is not JSON is written as `[unparsed]`.

Kick's protocol, event names, feed names (with the ids replaced), badge kinds
(a badge's `name` and `text`), colours and times are kept, and the realtime
endpoint the connection answer names (without its query). A field nobody
listed is replaced, not kept: to keep a new one, add it to the recorder's
lists. `import('./tools/record-realtime.mjs')` records nothing, and gives
`anonFrame`, `anonHttp` and `anonClose` to try on made-up input.

Older fixtures under `public_api/`, `v2/`, `webhook/`, `id/`, `probe/` and
`pusher-legacy/` came from KickTracker's recorder and anonymizer
(`mix record.*`, `mix fixtures.anonymize`).

`tools/record-api.mjs` records the official HTTP API with the JS binding.
Set `KICK_CLIENT_ID_FILE` and `KICK_CLIENT_SECRET_FILE` to private credential
files; never put their contents in command arguments. Run:

```sh
node tools/record-api.mjs app
KICK_REDIRECT_URI=http://localhost:4000/auth/kick/callback node tools/record-api.mjs login
node tools/record-api.mjs user-reads
node tools/record-api.mjs actions --target <your-second-account-slug> # plan only
node tools/record-api.mjs actions --target <your-second-account-slug> --yes
node tools/record-api.mjs revoke
node tools/record-api.mjs status
node --test tools/record-api.test.mjs
```

Register the exact redirect URI with your Kick app. Check granted scopes
before actions. Use only accounts you control and explicitly authorize each
live run. Actions send/delete chat, timeout/ban/unban the target, edit/restore
the channel title and create/edit/delete a disabled reward. Cleanup is
attempted even after failure; inspect any `LEFT TO UNDO BY HAND` report.
No ad breaks, enrollment, drops updates or real redemption decisions run.
Raw responses contain secrets and stay in a 0700 directory outside the repo
(`status` prints its location); private files are 0600. Only anonymized
fixtures go under `fixtures/public_api/` and `fixtures/id/`.

**Read a recording before committing it.** Look for anything that is a real
person's name or words, a token, a session, or a signed URL (`playback_url`).
This repository is public.

## Checks

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p kicksdk --all-features
cargo test --workspace --all-features
cargo +1.88 build -p kicksdk -p kicksdk-ffi --all-features   # the MSRV
cargo deny check advisories && cargo audit   # also weekly in CI
scripts/build-js.sh && (cd bindings/js && npm test)
(cd bindings/python && maturin develop && pytest)
(cd bindings/dart && dart format --line-length 120 . && dart analyze --fatal-infos && dart test)
(cd bindings/elixir && mix format --check-formatted && mix test)
scripts/release-check.sh
python3 scripts/docs/api_table.py --check   # every core function bound in every language
```

CI runs all of them. `scripts/build-docs.sh` builds the docs site locally
(into `site/`); the Docs workflow publishes it on every push to main.

## Releasing

Bump the version everywhere `scripts/release-check.sh` looks (Cargo.toml,
package.json, mix.exs, pyproject.toml, pubspec.yaml, and `version` in
`bindings/dart/hook/build.dart`), commit, then tag `v<version>` and push the
tag. The release workflow does the rest; the one-time registry setup it needs
is in [docs/releasing.md](docs/releasing.md).
