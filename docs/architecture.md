# How kicksdk is built, and why

## No I/O in the core

The core takes bytes in and gives requests, frames and typed values out. It
never connects, sends, sleeps or reads a clock. Every call that depends on
time takes `now` in milliseconds.

- **It is the only design that runs everywhere.** A browser's sockets are the
  browser's, Elixir's belong to supervised processes, and a NIF must not block
  a scheduler. A core that owned I/O would need one networking stack per
  host, which is the duplication the project exists to remove.
- **Some of Kick only works from a particular place.** The realtime feed
  accepts `Origin: https://kick.com` alone. In a browser that means the socket
  has to be opened by a kick.com page or an extension's content script. A
  library that opened its own sockets could not do that.
- **It makes the tests exact.** A recorded session replays frame by frame,
  and the test checks that the session sends back, byte for byte, what Kick's
  server answered to (`realtime_recordings_replay`).

The cost is a driver per host. It is small, and written once per language:
`connectChat` in `bindings/js/index.js` is about 330 lines.

## Requests and parsers

A builder returns a `Request` (method, URL, headers, body). A parser takes
`(status, body)`. Errors keep the status, so a caller can tell "Kick said 429"
from "Kick changed the shape".

Every builder takes an `Endpoints`, so any part of Kick can be pointed
elsewhere: KickTracker's simulator, BetterChat's relay, a proxy.

Builders refuse (`Error::Invalid`) what would make the request a different
one: a path segment that is empty, `.` or `..` (a URL reads them, and
`%2e%2e`, as a move up the path, so encoding does not help) and a token
with a control character (a CR/LF would start a header of its own). The
builders that can refuse return `Result<Request, Error>`.

Secrets never reach a log by accident: `Debug` hides the `authorization`
header and a form's `client_secret` in a `Request`, the token in a
session's connect and refresh frames, and a website channel's signed
`playback_url`. `Serialize` - what a host sends or a binding returns -
keeps them, since they are needed there. The realtime session drops
`playback_url` from event data altogether: nothing read from the feed
needs it.

## Lenient parsing, strict types

Kick sends numbers as strings (`"duration": "1200"`, `followers_count` either
way), leaves fields out, and wraps event data as a JSON string inside the
JSON. Parsing goes through `lenient.rs`, which reads a field or gives `None`.
It never fails the whole value over one odd field. The result is still typed,
so callers never touch raw JSON for anything kicksdk knows.

Unknown chat events come out as `KickEvent::Unknown { name }`, not as errors.
New Kick events must not break clients that do not care about them. Public
enums and output structs are `#[non_exhaustive]` for the same reason in the
other direction: when kicksdk learns a new event or field, adding it is not a
breaking change.

## Units are not invented

Where Kick's unit is not settled - a pin's `duration` is the open case, see
KICK.md §8 - the number is passed through as Kick sent it and the doc comment
says what is known. A guessed conversion would be wrong for somebody.

## The realtime session

`realtime::Session` is a state machine for one connection: `open`, `receive`,
`tick`, `refresh`, `closed` in, a list of `Action`s out (send, close, refresh
the token, connected, subscribed, an event). It keeps:

- command ids, per connection (a new socket starts at 1 again);
- which subscriptions are pending, confirmed or refused, subscribing again
  when Kick asks (unsubscribe codes 2500-2999), and reporting every feed
  that was subscribed or pending as unsubscribed when the socket closes;
- the ping interval Kick announced (25 s), and when silence means the
  connection is dead (that plus a 20 s grace) - never, if the server
  announced no ping;
- when to ask for a new token (2 minutes before the 45-minute lifetime ends,
  or halfway for a short one).

What Kick sends is bounded before use (a ping of 1 s to 10 min, a token
lifetime of 1 s to a day) and time sums saturate, so neither Kick nor a
host's `now` can overflow them.

Reconnecting is the host's. `Action::Close` says whether it makes sense
(`reconnect`, from Centrifugo's code ranges: 3500-3999 and 4500-4999 are
terminal), and `realtime::should_reconnect` answers the same for a close
code the host's socket reports. `Backoff` gives the delays (1 s doubling to
30 s, reset once the chat feed is subscribed); `next_delay_jittered` spreads
them with a random number the host draws, since the core has no randomness
of its own.

## Webhooks

`webhooks::verify` takes a `PublicKey` read once, the headers, the raw body
as bytes (a body is verified byte for byte, UTF-8 or not) and a
`Freshness`: the timestamp must be within 5 minutes of `now` unless the
caller skips the check explicitly, to re-check stored deliveries. A valid
signature only means Kick sent that body to some app, since every app
shares the key; KICK.md §4.3 says what else a receiver should do.
`webhooks::parse` refuses a body that is not the shape of the type header's
type, because that header is not signed.

RSA is the `rsa` crate's, pure Rust so it builds for WebAssembly. Its
advisory RUSTSEC-2023-0071 is a timing side channel in private-key
operations; kicksdk only verifies, with a public key, so it is not
affected (`deny.toml` and `.cargo/audit.toml` record the exception). The
alternatives (ring, aws-lc-rs) bring C and assembly that do not build for
`wasm32-unknown-unknown`. On that target `rsa` reaches `getrandom` 0.2,
which the core's Cargo.toml sets to its `js` backend; verifying never
draws a random number.

## Bindings

Two ways in:

- **Hand-written, typed**: WebAssembly (`crates/kicksdk-wasm`) and the Elixir
  NIF (`crates/kicksdk-nif`) wrap each function. They came first, and a JS or
  BEAM caller gets real function signatures from them.
- **Through `dispatch`**: the core's `dispatch` module (feature `dispatch`)
  exposes every function by name, JSON in and JSON out, and a realtime
  session by method name. The C ABI (`crates/kicksdk-ffi`, six functions) and
  the Python extension (two entry points) sit on it, and their language-side
  modules are thin typed wrappers. A function added to `dispatch` reaches C,
  Dart and Python at once. The cost is a JSON round trip per call: a few
  microseconds, against chat that arrives at most a few hundred times a
  second. Numbers crossing it must be exact integers in range; it refuses
  rather than rounds or clamps. Its JSON keeps each struct's field order,
  which needs serde_json's `preserve_order`: the `dispatch` feature turns
  that on (and the WebAssembly crate does for itself), so a plain Rust
  user of the core does not get it forced on them.

Per language:

- **WebAssembly** (`crates/kicksdk-wasm`, npm `kicksdk-js`): plain JS objects
  with snake_case keys, as Kick writes them. About 370 KB after wasm-opt,
  most of it RSA for webhooks; a browser-only build without the `webhooks`
  feature would be much smaller, if that ever matters.
- **Elixir** (`bindings/elixir`, hex `kicksdk`): a NIF. Chat-sized inputs
  take microseconds; work grows with the input (about 20 µs per KiB of
  JSON), so each NIF that parses or verifies has a `DirtyCpu` twin, which
  the wrappers use above 4 KiB. Builders that can refuse their arguments
  return `{:ok, _} | {:error, _}`. Values come back as
  JSON-shaped terms with **string keys**, not atoms: much of it is Kick's own
  data, and atom keys from it would let Kick grow the atom table.
- **Python** (`bindings/python`, PyPI `kicksdk-py`, imported as `kicksdk`): PyO3 over `dispatch`,
  dicts out. One abi3 wheel per platform covers CPython 3.9 and later.
- **Dart / Flutter** (`bindings/dart`, pub `kicksdk`): `dart:ffi` over the C
  ABI, its library provided by a build hook (`hook/build.dart`), with a
  `NativeFinalizer` freeing sessions.

## Precompiled, and from source

Users should not need Rust, so every native package ships binaries, and every
one can still build without them:

| Package | Precompiled | Checked by | Falls back to |
|---|---|---|---|
| npm | the `.wasm` in the package | the registry | `scripts/build-js.sh` |
| PyPI | abi3 wheels | pip, against the registry | the sdist, which carries the crates |
| pub | `kicksdk_ffi-v<version>-<triple>.<ext>` on the GitHub release, fetched by the build hook | `hook/checksums.json`, written at release | `cargo build` of `native/` |
| hex | rustler_precompiled tarballs on the GitHub release | `checksum-Elixir.Kicksdk.Native.exs`, written at release | Rustler, on `native/` |

`native/` is a copy of the Cargo workspace trimmed to the core and the one
crate the package needs (`scripts/vendor-native.sh`), with the repository's
`Cargo.lock`, so a source build is the build CI tested. Neither `native/` nor
the checksum files are committed. Without them, a checkout of the repository
builds from the workspace itself. A binary whose checksum does not match is
refused outright: falling back to a source build there would hide a
tampered release.

The Dart hook falls back to a source build on any network or TLS failure,
and for targets with no precompiled library; only a checksum mismatch
fails the build.

## Fixtures are the contract

`fixtures/` holds Kick's real answers and frames, anonymized. The Rust, JS,
Python, Dart and Elixir suites all read them. A Kick change goes like this: record it, see
every suite fail, fix the core, and every binding passes again. Recordings
never hold a token, a real name, real text or a signed URL; see
CONTRIBUTING.md.
