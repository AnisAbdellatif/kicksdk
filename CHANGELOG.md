# Changelog

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
