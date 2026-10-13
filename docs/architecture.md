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
`connectChat` in `bindings/js/index.js` is about 150 lines.

## Requests and parsers

A builder returns a `Request` (method, URL, headers, body). A parser takes
`(status, body)`. Errors keep the status, so a caller can tell "Kick said 429"
from "Kick changed the shape".

Every builder takes an `Endpoints`, so any part of Kick can be pointed
elsewhere: KickTracker's simulator, BetterChat's relay, a proxy.

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
- which subscriptions are pending, confirmed or refused;
- the ping interval Kick announced (25 s), and when silence means the
  connection is dead (that plus a 20 s grace);
- when to ask for a new token (2 minutes before the 45-minute lifetime ends,
  or halfway for a short one).

Reconnecting is the host's: `Backoff` gives the delays (1 s doubling to
30 s, reset once the chat feed is subscribed).

## Bindings

- **WebAssembly** (`crates/kicksdk-wasm`, npm `kicksdk`): plain JS objects
  with snake_case keys, as Kick writes them. About 600 KB before wasm-opt.
  Most of that is RSA for webhooks; a browser-only build without the
  `webhooks` feature would be much smaller, if that ever matters.
- **Elixir** (`bindings/elixir`, hex `kicksdk`): a NIF. Every call returns in
  microseconds, so none needs a dirty scheduler. Values come back as
  JSON-shaped terms with **string keys**, not atoms: much of it is Kick's own
  data, and atom keys from it would let Kick grow the atom table.
- **Dart / Flutter** (for Viewmux) and **Python** are the next bindings:
  `flutter_rust_bridge` and PyO3 over the same core.

## Fixtures are the contract

`fixtures/` holds Kick's real answers and frames, anonymized. The Rust, JS and
Elixir suites all read them. A Kick change goes like this: record it, see
every suite fail, fix the core, and every binding passes again. Recordings
never hold a token, a real name, real text or a signed URL; see
CONTRIBUTING.md.
