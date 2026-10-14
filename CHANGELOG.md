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
- WebAssembly and the NIF got compile fixes only: their `verifyWebhook`
  checks freshness against the machine clock (5 minutes) until they expose
  `now`/`tolerance`/skip; the NIF raises `{:invalid, message}` from the
  builders that became fallible (its own `channel_request`,
  `channel_user_request`, `livestreams_requests`, `subscriptions_request`,
  `unsubscribe_requests`) and `ArgumentError` from `emote_url`; JS throws.
  Still to expose: `PublicKey` reuse, `Freshness`, `body` as bytes,
  `Backoff.nextDelayJittered`, `should_reconnect`, and `Close`'s new fields
  in the language-side drivers (stop reconnecting when `reconnect` is
  false). The WebAssembly crate's own `getrandom` dependency is now
  redundant with the core's.

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
