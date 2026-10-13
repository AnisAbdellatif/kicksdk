# Changelog

## 0.1.0 (unreleased)

First version, from what KickTracker, BetterChat, KickPlus and Viewmux had
each learnt about Kick (docs/KICK.md).

- Core (`kicksdk` crate): official API, website API, realtime (Centrifugo)
  endpoints and session, chat-feed events, message content, webhooks.
- JavaScript (`kicksdk` on npm): the core in WebAssembly, and `connectChat`.
- Elixir (`kicksdk` on hex): the core as a NIF.
