# Contributing

## When Kick changes

Follow this order, every time:

1. **`docs/KICK.md`** first. Write down what Kick does now, where it was
   seen, and the date. Keep what it used to do if anyone may still meet it,
   the way §6.1a keeps Pusher.
2. **A recording** in `fixtures/`, anonymized (below), that shows it.
3. **The core.** Change `crates/kicksdk` until the new recording passes.
4. **The bindings.** Rebuild them and run their tests, which read the same
   fixtures. A new public function needs a wrapper in
   `crates/kicksdk-wasm/src/lib.rs`,
   `bindings/elixir/native/kicksdk_nif/src/lib.rs` and the matching Elixir
   module.

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
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p kicksdk
cargo test --workspace
scripts/build-js.sh && (cd bindings/js && npm test)
(cd bindings/elixir && mix format --check-formatted && mix test)
```

CI runs all of them.
