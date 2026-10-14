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

Values come back the way JSON decodes: maps with string keys. Parsers, and
builders that can refuse their arguments, return `{:ok, value}` or
`{:error, reason}`; see the `Kicksdk` module docs. `examples/elixir/chat.exs`
in the repository is a complete chat client: ticks, token refresh,
reconnecting with backoff.

The NIF comes precompiled for Linux (glibc and musl; x86_64, aarch64, arm,
riscv64), macOS and Windows, downloaded from the GitHub release and checked
against the checksums published with the package. On any other target, or
with `KICKSDK_BUILD=1`, it is built from the Rust sources shipped in the
package, which needs a Rust toolchain and adding `{:rustler, "~> 0.38",
optional: true}` to your deps. rustler_precompiled's own switch works too:
`config :rustler_precompiled, :force_build, kicksdk: true`.
`KICKSDK_BASE_URL` points the download at a mirror of the release assets
(a directory URL, such as `http://localhost:8080` or
`https://mirror.example/kicksdk/v0.1.1/`; the checksums still apply).
Nerves' `TARGET_ARCH`, `TARGET_VENDOR`, `TARGET_OS` and `TARGET_ABI` choose
the target.

These variables are read when the package compiles. Mix does not recompile a
dependency when they change: run `mix deps.compile kicksdk --force`. (In the
package's own checkout a change recompiles it, after a cross build too.)

On Alpine and other musl systems the precompiled NIF links against
`libgcc_s` for unwinding, which a slim image may lack: `apk add libgcc` (in
the image that runs the release, not only the build one).

MIT OR Apache-2.0. Not affiliated with Kick.
