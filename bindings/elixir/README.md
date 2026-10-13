# Kicksdk (Elixir)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, as a NIF over the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core.

```elixir
def deps, do: [{:kicksdk, "~> 0.1"}]
```

Kicksdk never sends a request or opens a socket: it builds requests for your
HTTP client (Req, Finch, Mint…), parses what comes back, and runs Kick's
realtime protocol as a state machine (`Kicksdk.Realtime.Session`) that your
socket process drives. Send `Origin: https://kick.com` on the WebSocket
handshake: Kick's realtime server refuses anything else.

Values come back the way JSON decodes: maps with string keys.

Building needs a Rust toolchain for now; precompiled NIFs (rustler_precompiled)
are planned. MIT OR Apache-2.0. Not affiliated with Kick.
