# Changelog

## Unreleased

Fixes from a security and robustness audit of the core. **Breaking** (0.x):
Rust signatures and some `dispatch` arguments and `Action` shapes change;
every binding needs the updates listed under "For binding authors". The
version stays 0.1.1 until release.

### Webhooks

- **Breaking**: `webhooks::verify` checks freshness and takes a parsed key
  and raw bytes:
  `verify(key: &PublicKey, headers: &Headers, body: impl AsRef<[u8]>, freshness: Freshness) -> Result<(), WebhookError>`.
  - `PublicKey::from_pem(pem: &str) -> Result<PublicKey, WebhookError>`
    (also `FromStr`): read Kick's key once, not per delivery.
  - `Freshness::at(now_unix_secs: u64)` (tolerance `DEFAULT_TOLERANCE_SECS`
    = 300), `Freshness::Within { now, tolerance_secs }`, or
    `Freshness::Skip` to re-check stored deliveries on purpose.
  - New `verify_pem(pem: &str, headers, body, freshness)`: the old
    PEM-per-call form, with freshness.
  - New errors: `WebhookError::Stale { offset_secs: i64 }` (timestamp minus
    now), `WebhookError::MalformedHeader(&'static str)`.
- **Breaking**: `Headers::from_pairs` (and `verify`, for hand-built headers)
  require the message id to be a ULID and the timestamp RFC 3339, so no
  header can move the `id.timestamp.body` boundary of the signed text.
- **Breaking**: `webhooks::parse(event_type: &str, body: impl AsRef<[u8]>) -> Result<WebhookEvent, kicksdk::Error>`
  (was `serde_json::Error`). It refuses (`Error::Shape`) a body that is not
  the shape of the claimed type: the type header is unsigned, so a signed
  follow can no longer be passed off as, say, a Kicks gift. Unknown types
  still come out as `Other`, unchecked; `.new` and `.renewal` subscriptions
  share one shape.
- Docs (`webhooks` module, KICK.md §4.3): a valid signature means "Kick
  sent this to some app"; deduplicate on the message id and check freshness.
- `rsa` stays (verify only); RUSTSEC-2023-0071 does not apply and is
  recorded in `deny.toml` and `.cargo/audit.toml`.

### Requests

- **Breaking**: builders that put user input in a URL path refuse an empty
  segment, `.` and `..` (`Error::Invalid`), and builders that send a token
  refuse one with a control character (CR/LF header injection). Now
  returning `Result<Request, Error>`: `website::channel_request`,
  `channel_user_request`, `me_request`, `ban_request`, `unban_request`,
  `delete_message_request`, `unpin_request`;
  `public_api::subscriptions_request`. Now `Result<Vec<Request>, Error>`:
  `public_api::livestreams_requests` (no longer drops a failing batch
  silently) and `public_api::unsubscribe_requests`.
- **Breaking**: `content::emote_url(id) -> Result<String, Error>`: digits
  only.
- **Breaking**: `#[non_exhaustive]` on `Endpoints` (build with
  `Endpoints::default().with_api(..)`, `.with_id`, `.with_website`,
  `.with_realtime`, or assign fields), `Request` (`Request::new(method,
  url)` is now public), `Method`, `public_api::Sort`, `content::Part`,
  `realtime::SessionOptions` (use `SessionOptions::new`, then set fields).
- `Request`'s `Debug` hides the `authorization` header and a form's
  `client_secret`; `WebsiteChannel`'s `Debug` hides `playback_url`.
  `Serialize` output is unchanged.
- **Breaking** (Rust only; JSON unchanged, the ids are transparent): typed
  ids where outputs had a bare `u64`: `ChatMessage::chatroom_id`
  (`ChatroomId`), `Sender::id`, `ReplyTo::sender_id`, `UserRef::id`,
  `website::ChannelUser::id`, `webhooks::Person::user_id` (`UserId`),
  `Hosted::channel_id`, `public_api::Livestream::channel_id` (`ChannelId`),
  and `website::ReplyTarget::sender_id` (`UserId`) on the way in.
- Ids above `i64::MAX` are read exactly instead of being clamped to
  `i64::MAX`; fractional or inexact float ids are `None`, never truncated.
- `content::parse` is linear on any input (it was quadratic on
  `[emote:1:` repeated without `]`). An emote name ends at the first `[`
  or `]`.

### Realtime

- **Breaking**: `Action::Close { code: i64, reason: String, reconnect: bool }`
  (was `{ reason }`; the variant is `#[non_exhaustive]`). Serialized:
  `{"action":"close","code":3501,"reason":"…","reconnect":false}`.
  `code` is Kick's disconnect code, Kick's error code for a refused
  connect/refresh, or 0 when the session gave up (silence). Do not
  reconnect when `reconnect` is false.
- New core function `realtime::should_reconnect(code: u16) -> bool`, for the
  close code a host's socket reports: false for 3500-3999 and 4500-4999.
- Unsubscribe pushes with codes 2500-2999 are resubscribed by the session:
  it emits `Unsubscribed` then a `Send` with the subscribe; `Subscribed` or
  `SubscribeFailed` follows.
- A connect reply without `ping` means the server does not ping: no silence
  timeout then (`deadline()` can be `None` while connected). Kick's ping is
  bounded to 1-600 s and ttl to 1 s - 1 day; time sums saturate.
- `closed()` also reports feeds whose subscribe had no answer yet, and
  `open()` on a session that was not `closed()` returns those
  `Unsubscribed` actions before the connect `Send`.
- A feed listed twice in `SessionOptions::channels` is subscribed once.
- `Action::Event`'s `data` never contains a `playback_url` (removed at any
  depth, e.g. from `ChatMoveToSupportedChannelEvent`). `Action`'s `Debug`
  hides the token in connect/refresh frames.
- `Backoff::new` clamps a first wait of 0 to 1 ms and a max below the first
  up to it. New `Backoff::next_delay_jittered(random: f64) -> u64`: the
  delay spread over [half, all] by a host-drawn number in [0, 1).

### Build

- `kicksdk` builds for `wasm32-unknown-unknown` with default features
  (getrandom's `js` backend, for rsa's dependencies, on that target only).
- Every feature combination builds and tests; `dispatch::FUNCTIONS` lists
  `webhooks.*` only with the `webhooks` feature.
- serde_json's `preserve_order` is no longer forced on every user through
  the workspace: the `dispatch` feature and the WebAssembly crate enable
  it, so binding output keeps its key order.

### JavaScript (kicksdk-js)

- New: `shouldReconnect(code)`, `Backoff.nextDelayJittered(random)`,
  `new WebhookPublicKey(pem).verify(...)`. **Breaking**: `verifyWebhook(pem,
  headers, body, {now, toleranceSecs} | {skipFreshness: true})` checks
  freshness (default: this machine's clock, 5 minutes; `now` in seconds);
  bodies may be a string, `Uint8Array` or `ArrayBuffer`; `parseWebhook`
  takes bytes too.
- **Breaking**: ids, `now`, Backoff bounds, `minutes`, `limit` and event
  versions must be whole, non-negative safe-integer numbers (`TypeError` /
  `RangeError` otherwise); an unknown endpoint key or `sort`, or a malformed
  `replyTo`, throws instead of being replaced or dropped.
- `connectChat`:
  - reconnects as soon as the session gives up on a quiet socket, without
    waiting for the old socket to close (in Node it never did);
  - stops on a terminal close (`reconnect: false`, or a socket close code
    `shouldReconnect` refuses): `onDrop(reason, {code, reconnect: false})`,
    then `onState({..., stopped: true})`. **Breaking**: `onDrop` gets a
    second argument, `onState` a `stopped` field;
  - new options: `onError(error)` (default `console.error`) for exceptions
    thrown by your callbacks, which no longer break the driver;
    `timeoutMs` (default 10 s) for each request and the handshake;
    `WebSocket` for runtimes without a global one (Node 20, or the `ws`
    package);
  - `onDrop` also fires when the socket closes or the handshake is refused,
    and nothing fires after `close()`, which may be called twice or from
    inside a callback;
  - reconnect waits are jittered; the session clock is `performance.now()`.
- `init()` retries after a failed load instead of returning the same
  rejection forever; it resolves to `undefined`.
- Types: `endpoints` optional everywhere, user ids `readonly number[]`,
  `ChatState`, `ChatDrop`, `ChatSocket` exported; checked by a strict
  TypeScript consumer in the tests.
- The npm package includes `LICENSE-MIT` and `LICENSE-APACHE`.

### Python (kicksdk-py)

- New: `kicksdk.realtime.should_reconnect(code)`; `Backoff` wraps the core's,
  with `next_delay()` and `next_delay_jittered(random)` (ms). `Backoff(0)`
  now waits 1 ms.
- **Breaking**: `webhooks.verify(pem, headers, body, *, now=None,
  tolerance_secs=None, skip_freshness=False)` checks freshness by default
  (`now` defaults to `time.time()`); `bytes` bodies are verified as bytes.
  `webhooks.parse` takes bytes too.
- **Breaking**: `kicksdk.aio`:
  - `import kicksdk.aio` without the `[aio]` extra raises an `ImportError`
    that says to install it (it used to hang the iterator);
  - the iterator raises `ChatClosed` (a terminal close), `ChatRefused`
    (`max_refusals` refusals in a row, default 5, e.g. a 403 handshake) or
    an unexpected error, instead of retrying forever, and ends when the
    `async with` block exits; `chat.error`, `chat.done`;
  - a failed token refresh keeps the connection, and the refresh no longer
    blocks ping replies; a normal exit closes with 1000, not 1011;
  - `max_queue` and `overflow` (`"drop_oldest"`, counted in `chat.dropped`,
    or `"backpressure"`) replace the silent drop at 10,000 events;
  - `http=` takes your own `httpx.AsyncClient`; reconnect waits are
    jittered.
- Wheels and the sdist include `LICENSE-MIT` and `LICENSE-APACHE`.

### Dart (kicksdk) and the C ABI (kicksdk-ffi)

- C: `#define KICKSDK_ABI_VERSION 1u` in `kicksdk.h`; the header documents
  that `kicksdk_session_new` may return NULL and one call at a time per
  session. A panic inside a call no longer prints to stderr, and its
  envelope carries the panic message.
- New: `Realtime.shouldReconnect(code)`, `Backoff.nextDelayJittered`.
  **Breaking**: `Webhooks.verify(pem, headers, Object body, {DateTime? now,
  Duration? tolerance, bool skipFreshness = false})` checks freshness
  (default: the machine clock); a `List<int>` body is verified as bytes.
- `connectChat`: new `timeout` (default 10 s, each request and the
  handshake), `onError`, `backoff`, and `ChatConnection.done`; stops on a
  terminal close; nothing is called back after `close()`; exceptions in
  your callbacks no longer lose the rest of a batch; a failed token refresh
  is retried on the same socket; reconnect waits are jittered.
- Build hook: the download has a connect and an overall timeout; any
  network or TLS failure, or a `binary_base_url` that is not http(s), falls
  back to a source build, while a checksum mismatch still fails the build.
  Targets without a precompiled library (Linux arm, riscv64 and musl,
  Windows ia32, Android ia32/riscv64) build from source instead of failing.
  New user defines `cargo_path`, `cargo_home`, `rustup_home` for toolchains
  outside the default location; Android source builds link for the app's
  minimum NDK API.

### Release

- The Linux libraries are built with `cross` and checked against a glibc
  floor (2.17, 2.18 for the arm64 C library, 2.27 for riscv64); 0.1.1's
  x86_64 NIF and x86_64/arm64 C libraries needed glibc 2.34.
- Release assets are never replaced once uploaded; NIF tarballs are
  reproducible; hex and pub checksum the release's own files, and pub
  requires all 12 libraries.
- Libraries are stripped, macOS ones get `@rpath` install names, and the
  Windows MSVC builds link the C runtime statically.
- Re-runs run from the tag; a dry run builds and checks everything on pull
  requests that touch the release, and weekly. See `docs/releasing.md`.

### For binding authors

- `dispatch`:
  - New function `realtime.should_reconnect {code}` -> bool. **Every binding
    must add it** (`scripts/docs/api_table.py --check` fails until they do).
  - `webhooks.verify {public_key_pem, headers, body | body_base64, now,
    tolerance_secs?}` or `{…, skip_freshness: true}` instead of `now`.
    `now` is Unix **seconds**. `body_base64` carries a body that is not
    UTF-8. Missing `now` is a `call` error.
  - `webhooks.parse {event_type, body | body_base64}`; a body that is not
    the type's shape fails with kind `shape` (was always `json`).
  - Numbers must be exact integers in their range (`1.0` is accepted;
    `1.5`, `-1`, `1e30`, 2^32+ for `limit`/`version`/`minutes` are `call`
    errors instead of being rounded or truncated).
  - Builders listed above under Requests can now fail with kind `invalid`;
    `content.emote_url` too.
  - Session actions: `close` has `code` and `reconnect`; see Realtime.
- Every binding now exposes all of the above; see its section.


## 0.1.1

The first published release. 0.1.0 reached PyPI only (as `kicksdk-py`); its
other packages never shipped:

- npm: published as `kicksdk-js` (npm refuses `kicksdk` as too close to `kick-sdk`).
- Elixir NIFs and the Android libraries now build in the release workflow.

## 0.1.0

First version, from what KickTracker, BetterChat, KickPlus and Viewmux had
each learnt about Kick (docs/KICK.md).

- Core (`kicksdk` crate): official API, website API, realtime (Centrifugo)
  endpoints and session, chat-feed events, message content, webhooks.
- JavaScript (`kicksdk-js` on npm): the core in WebAssembly, and `connectChat`.
- Elixir (`kicksdk` on hex): the core as a NIF, precompiled (rustler_precompiled).
- Python (`kicksdk-py` on PyPI, imported as `kicksdk`): PyO3, abi3 wheels, and `kicksdk.aio.connect_chat`.
- Dart / Flutter (`kicksdk` on pub.dev): `dart:ffi` over a C ABI
  (`kicksdk-ffi`), precompiled through a build hook, and `connectChat`.
- `kicksdk::dispatch`: every function by name, JSON in and out.
- Every native package builds from vendored sources when no precompiled
  binary fits, and refuses one whose checksum does not match.
