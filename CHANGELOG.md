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
  (was `serde_json::Error`). It refuses (`Error::Shape`) a body that lacks
  the claimed type's own field (`is_live`, `metadata` with a
  title/language/category, `follower`, `subscriber`, `giftees`, `gift`):
  the type header is unsigned, so a signed follow can no longer be passed
  off as, say, a Kicks gift. No known type's body has another's own field,
  so every re-typing between known types is refused (tested pairwise and
  against the recordings). Other fields, including ones Kick adds later,
  are ignored. Unknown types still come out as `Other`, unchecked; `.new`
  and `.renewal` subscriptions share one shape.
- Docs (`webhooks` module, KICK.md §4.3): a valid signature means "Kick
  sent this to some app"; deduplicate on the message id and check freshness.
  A Kick retry (none seen yet) would likely carry the original timestamp
  and come back `Stale`; a receiver that prefers late retries to losing
  them can widen the tolerance and must remember message ids as long.
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
  `client_secret`; `WebsiteChannel`'s `Debug` hides `playback_url`;
  `public_api::AppToken`'s `Debug` hides `access_token`. `Serialize`
  output is unchanged.
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

- **Breaking**: `Action::Close { code: i64, reason: String, terminal: bool, retry_after_ms: u64 }`
  (was `{ reason }`; the variant is `#[non_exhaustive]`). Serialized, in
  this key order:
  `{"action":"close","code":3501,"reason":"Kick disconnected: 3501 banned","terminal":true,"retry_after_ms":300000}`.
  `code` is Kick's disconnect code, Kick's error code for a refused
  connect/refresh, or 0 when the session gave up (silence). `terminal` is
  true for a disconnect code in 3500-3999 or 4500-4999, a disconnect
  outside 3000-4999 whose push says `"reconnect": false`, and a refused
  connect or refresh other than 109, `temporary` or a transport error
  (< 100); `retry_after_ms` is then `realtime::TERMINAL_RETRY_MS`
  (300 000), else 0.
- **Product decision: a terminal close is never the end.** A refused guest
  token, a connection limit or a ban on an IP usually passes for a chat
  reader, so drivers no longer stop on one: they wait at least
  `retry_after_ms` (5 minutes, escalating, see `Backoff`) and reconnect
  with a **new client id** (so a new guest token). They stop only when the
  host closes them. KICK.md §6.1 has the table every driver follows.
- New `realtime::retry_after_ms(code: u16) -> u64` and
  `realtime::TERMINAL_RETRY_MS: u64 = 300_000`: for the close code a
  host's socket reports, 300 000 for 3500-3999 and 4500-4999 (terminal),
  0 otherwise. (An earlier unreleased `should_reconnect(code) -> bool` is
  gone.)
- New `Backoff::next_delay_for(&mut self, retry_after_ms: u64, random: f64) -> u64`:
  with 0, exactly `next_delay_jittered(random)`; with more, a terminal
  stage: `wait = min(retry_after_ms * 2^min(n, 3), retry_after_ms * 6)`
  where `n` is the number of terminal closes since the last `reset()`,
  returned as `wait + floor(floor(wait / 4) * r)` (`r` = `random` clamped
  to [0, 1], NaN as 0; saturating). With 300 000 and `r` = 0: 300 000,
  600 000, 1 200 000, 1 800 000, 1 800 000 … The ordinary stage is left as
  it was. `reset()` now also clears the terminal count; call it when the
  chat feed subscribes. `Backoff`'s serde form gains `terminal_closes`
  (`#[serde(default)]`, so older state still reads).
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
  depth, e.g. from `ChatMoveToSupportedChannelEvent`). Only data that had
  one is re-encoded; any other `data` string (a chat message that merely
  mentions `playback_url`, say) is handed on byte for byte as Kick sent it.
  `Action`'s `Debug` hides the token in connect/refresh frames.
- `Backoff::new` clamps a first wait of 0 to 1 ms and a max below the first
  up to it. New `Backoff::next_delay_jittered(random: f64) -> u64`: the
  delay spread over [half, all] by a host-drawn number in [0, 1):
  `half + floor((delay - half) * r)`, at least 1, where `half = floor(delay / 2)`.

### Build

- `kicksdk` builds for `wasm32-unknown-unknown` with default features
  (getrandom's `js` backend, for rsa's dependencies, on that target only).
- Every feature combination builds and tests; `dispatch::FUNCTIONS` lists
  `webhooks.*` only with the `webhooks` feature.
- serde_json's `preserve_order` is no longer forced on every user through
  the workspace: the `dispatch` feature and the WebAssembly crate enable
  it, so binding output keeps its key order.

### JavaScript (kicksdk-js)

- New: `retryAfterMs(code)` (replaces the unreleased `shouldReconnect`),
  `Backoff.nextDelayJittered(random)`,
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

- New: `kicksdk.realtime.retry_after_ms(code)` (replaces the unreleased
  `should_reconnect`); `Backoff` wraps the core's,
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
- New: `Realtime.retryAfterMs(code)` (replaces the unreleased
  `shouldReconnect`), `Backoff.nextDelayJittered`.
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

### Elixir (kicksdk)

- **Breaking**: builders that can refuse their arguments return
  `{:ok, request} | {:error, reason}`: `Website.channel_request`,
  `channel_user_request` and the acting builders, `PublicApi`'s
  `livestreams_requests`, `subscriptions_request`, `unsubscribe_requests`,
  `top_livestreams_request`, and `Content.emote_url`. `Kicksdk.call`
  returns request headers as `{k, v}` tuples, like the wrappers.
- **Breaking**: `Webhooks.verify/4` takes `:now` (default
  `System.os_time(:second)`), `:tolerance_secs` and `:skip_freshness`, and
  verifies the raw bytes. A delivery that cannot be trusted (header, key,
  signature, timestamp) is `{:error, {:webhook, message}}`; a signed one
  whose body is not the claimed type's shape is `{:error, {:shape, what}}`
  (`{:json, _}` if not JSON), from `verify/4` and `Kicksdk.call/2` alike.
  New `Webhooks.public_key/1` reads Kick's key once.
- Non-UTF-8 input returns `{:error, _}` instead of raising (webhook bodies
  verify byte for byte).
- `Session` takes any integer `now`, `System.monotonic_time(:millisecond)`
  included; `deadline/1` answers on the caller's clock. A poisoned session
  raises a clear `RuntimeError`; `closed/1` recovers it. A call on a session
  that another process is calling at the same moment raises `RuntimeError`
  ("busy") instead of blocking a scheduler until the other call ends.
- New: `Realtime.retry_after_ms/1` (replaces the unreleased
  `should_reconnect?/1`), `Kicksdk.Realtime.Backoff` (with
  `next_delay_jittered/2` and `next_delay_for/3`, whose struct carries the
  terminal-close count in `terminal_closes`; `reset/1` clears it).
- **Breaking**: the session's `"close"` action has `"code"`, `"reason"`,
  `"terminal"` and `"retry_after_ms"` instead of `"reconnect"`; a terminal
  close means "wait longer and come back as a new client", never "stop".
- **Breaking**: endpoints with an unknown key (even with a `nil` value), a
  non-string value, or a struct (`%URI{}`) instead of a map raise
  `ArgumentError`; HTTP statuses outside 0..65535 are `{:error, {:invalid, _}}`.
  The `Kicksdk.endpoints` type says what is accepted: atom or string keys,
  `nil` values for Kick's own.
- NIFs whose work grows with the input run on a dirty scheduler above 4 KiB.
- `config :rustler_precompiled, :force_build, kicksdk: true` works; changing
  `KICKSDK_BUILD`, `KICKSDK_BASE_URL` or `TARGET_*` recompiles the module,
  also after a cross build (whose NIF does not load on the build machine;
  the check now lives in `Kicksdk.Native.Build`, which always loads). As a
  dependency, Mix does not look: `mix deps.compile kicksdk --force`. An
  unrecognised Unix system architecture or a `TARGET_*` target without a
  precompiled NIF falls back to a source build instead of failing; Windows
  (architecture `win32`) gets the precompiled NIF as before.
- `KICKSDK_BASE_URL` without a path (`http://localhost:8080`) works: a `/`
  is added (rustler_precompiled asked for `http://localhost:8080kicksdk_nif-…`).
- `examples/elixir/chat.exs` ticks, refreshes its token, and reconnects or
  stops as the session says.
- Integers beyond 64 bits in Kick's JSON still arrive as floats (the core
  reads JSON without arbitrary precision); documented.

### Dispatch

- **Breaking**: `endpoints` must be an object whose keys are the
  `Endpoints` fields; an unknown key is a `call` error instead of being
  ignored (a misspelt key would have sent that request to kick.com). A
  `null` value keeps Kick's URL.
- **Breaking**: optional string and boolean arguments are checked like
  numbers: a non-string `sort`, `language`, `reply_to.content`, `body` or
  `body_base64`, or a non-boolean `combine` or `skip_freshness` (`"true"`,
  `1`), is a `call` error instead of being taken for "left out" (which
  silently sorted by viewers, dropped the language filter, or checked
  freshness the host meant to skip). Left out or `null` still means the
  default.

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

- **Terminal closes reconnect** (Realtime above; KICK.md §6.1). What each
  driver does, exactly:
  - On a session `close` action
    `{"action":"close","code":i64,"reason":str,"terminal":bool,"retry_after_ms":u64}`:
    close the socket, call `session.closed()`, report it (`terminal` is
    worth a warning), then, unless the host closed the driver, wait
    `backoff.next_delay_for(retry_after_ms, random())` ms and reconnect. If
    `terminal`, generate a **new client id** before the connection request
    and the token request. Never stop because of `terminal`.
  - On the socket closing by itself with a close code (no `close` action
    first): `session.closed()`, then
    `wait = backoff.next_delay_for(retry_after_ms(code), random())`;
    `retry_after_ms(code) > 0` means terminal: new client id. The
    language-level function is `realtime.retry_after_ms {code}` (dispatch)
    -> integer ms; a code outside 0-65535 is a `call` error.
  - On a failure without a close code (handshake refused, network error,
    a token or connection request failing): `backoff.next_delay_for(0, random())`,
    i.e. `next_delay_jittered`.
  - On `{"action":"subscribed","channel":C}` where `C` is the chat feed
    (`chat_channels(...)[0]`): `backoff.reset()` (both stages).
  - Drivers' public surface: replace `reconnect` in drop/close callbacks
    with `terminal` and `retry_after_ms` (or the actual wait); remove
    "stopped" / `ChatClosed`-style terminal outcomes, or keep them only for
    a host-requested close. Tests that expect a stop on 3501/4501 must
    expect a reconnect after the long wait (inject the backoff or the
    clock: the wait is 300 000 ms or more).
  - `Backoff` in each binding gains `next_delay_for(retry_after_ms, random)`
    (JS `nextDelayFor`, Dart `nextDelayFor` returning a `Duration`, Elixir
    `next_delay_for/3` -> `{delay, backoff}`) and `reset` clearing the
    terminal count. Where `Backoff` wraps the core (wasm `Backoff`,
    Python's `NativeBackoff` in `bindings/python/src/lib.rs`), add the
    method there; Dart's `Backoff` is pure Dart and must implement the
    formula above exactly (test vectors: `next_delay_for(300000, 0.0)` x5
    -> 300000, 600000, 1200000, 1800000, 1800000; after `reset`, 300000;
    `next_delay_for(300000, 0.5)` first -> 337500;
    `next_delay_for(0, 0.0)` on a fresh default -> 500). The Elixir NIF
    keeps a `Backoff` as `{initial_ms, max_ms, next_ms}`: add the fourth
    integer, `terminal_closes` (the serde field name), to the tuple and to
    `Kicksdk.Realtime.Backoff`, or the count is lost between calls.
  - Done here (thin layers only): the wasm export `retryAfterMs`, the NIF
    `retry_after_ms/1` (with its `Kicksdk.Native` stub), Python
    `kicksdk.realtime.retry_after_ms`, Dart `Realtime.retryAfterMs`,
    Elixir `Kicksdk.Realtime.retry_after_ms/1`; `shouldReconnect` /
    `should_reconnect` / `should_reconnect?` were removed from those layers.
    Still to update, and broken until then: `bindings/js/index.js` (imports
    `shouldReconnect`), `index.d.ts`, its tests and README;
    `bindings/python/python/kicksdk/aio.py` (`rt.should_reconnect`,
    `close["reconnect"]`, `ChatClosed`), its tests; `bindings/dart/lib/io.dart`
    (`Realtime.shouldReconnect`, `a['reconnect']`), its tests and README;
    `bindings/elixir` session docs, tests (`should_reconnect?`,
    `"reconnect"`) and `examples/elixir/chat.exs`; `examples/javascript`
    and `examples/dart`; the Session doc comments in each language layer
    that still describe `reconnect`. `examples/rust` is updated.
- `dispatch`:
  - New function `realtime.retry_after_ms {code}` -> integer ms (0 or
    300000). The unreleased `realtime.should_reconnect` is removed; calling
    it is a `call` error ("no function").
  - `webhooks.verify {public_key_pem, headers, body | body_base64, now,
    tolerance_secs?}` or `{…, skip_freshness: true}` instead of `now`.
    `now` is Unix **seconds**. `body_base64` carries a body that is not
    UTF-8. Missing `now` is a `call` error.
  - `webhooks.parse {event_type, body | body_base64}`; a body that is not
    the type's shape fails with kind `shape` (was always `json`).
  - `webhooks.verify`: a delivery whose signature verifies but whose body
    is not the claimed type's shape now fails with kind `shape` (`json` if
    the body is not JSON), no longer `webhook`. `webhook` is kept for what
    makes a delivery untrustworthy (missing/malformed header, bad key,
    signature mismatch, stale timestamp). Bindings that map kinds (Elixir's
    `{:error, {:webhook, msg}}`, Python's and Dart's exceptions) should
    surface `shape` distinctly.
  - Numbers must be exact integers in their range (`1.0` is accepted;
    `1.5`, `-1`, `1e30`, 2^32+ for `limit`/`version`/`minutes` are `call`
    errors instead of being rounded or truncated).
  - Optional strings and booleans must be of their type or null/absent
    (see Dispatch): pass `None`/`null` for "default", never `""`-for-bool
    or a number-for-string.
  - Builders listed above under Requests can now fail with kind `invalid`;
    `content.emote_url` too.
  - Session actions: `close` is
    `{"action","code","reason","terminal","retry_after_ms"}`, in that key
    order; see Realtime.
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
