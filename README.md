# kicksdk

Kick (kick.com) without the I/O, from one Rust core, for Rust, JavaScript and
Elixir.

kicksdk knows Kick: which requests to make and what the answers look like,
how the realtime chat feed speaks, how webhooks are signed. It never sends a
request or opens a socket itself. Your code does, with whatever it already
uses (`fetch` and `WebSocket`, Req and Mint, reqwest and tungstenite), and
hands kicksdk what came back. That is what lets one implementation run in a
browser, an extension, a Node service and a BEAM application alike, and when
Kick changes something, it is fixed once.

```
            your HTTP / WebSocket client
                 │            ▲
         request │            │ status + body, frames
                 ▼            │
   ┌─────────────────────────────────────────────────┐
   │ kicksdk (Rust)                                  │
   │  public_api · website · realtime · events ·     │
   │  content · webhooks                             │
   └─────────────────────────────────────────────────┘
     Rust crate      npm (WebAssembly)     hex (NIF)
```

## What it covers

| Module | Kick | |
|---|---|---|
| `public_api` | `api.kick.com/public/v1`, official | App token, `/livestreams` and `/channels` (batched by 50), webhook subscriptions, the signing key |
| `website` | `kick.com/api/v2`, Kick's own frontend | Chatroom id, follower total, live state, chat history and pin, user cards; moderating and sending as a viewer |
| `realtime` | Kick's chat feed (Centrifugo) | Where to connect, the guest token, and a `Session` state machine: connect, subscribe, pings, token refresh, reconnect timing |
| `events` | The feed's events | Chat messages, deletions, bans, pins, subs, gifted subs, Kicks, rewards, hosts, live/offline, polls, typed. Unknown events come out by name, never as errors |
| `content` | Message text | `[emote:id:name]` tokens split out, runs combined |
| `webhooks` | Official webhooks | Headers, RSA signature verification, typed bodies |

Everything rests on [`docs/KICK.md`](docs/KICK.md): every fact about Kick,
with where it was seen. Read it before relying on anything unofficial.

## Two things to know first

**The chat feed only accepts kick.com.** Kick's realtime server refuses a
WebSocket whose `Origin` is not `https://kick.com`, and the endpoints that hand
out its address and tokens answer CORS for kick.com alone. A server sets the
header itself (the JS driver does in Node; set it on your Elixir or Rust
client). A browser cannot: there, the feed works on kick.com (a page, or an
extension's content script), and anywhere else needs a server to relay it.

**Much of Kick is unofficial.** The website API and the realtime feed are what
kick.com's own frontend uses, not a published API. They can change without
notice, and using them is a grey area under Kick's terms. The official API
(`public_api`, `webhooks`) is the stable part. Treat a failure of the rest as
a gap, never a zero.

## Rust

```toml
[dependencies]
kicksdk = "0.1"
```

```rust
use kicksdk::{realtime::{self, Action, Session, SessionOptions}, website, Endpoints};

let e = Endpoints::default();
let req = website::channel_request(&e, "xqc");
// … send it with your client …
let channel = website::parse_channel(status, &body)?;

let mut session = Session::new(SessionOptions::new(
    realtime::chat_channels(channel.chatroom_id, Some(channel.id)),
));
// socket opened (Origin: https://kick.com):
for action in session.open(&token, now_ms) { /* Send { text } → send it */ }
// each text frame:
for action in session.receive(&frame, now_ms) {
    match action {
        Action::Send { text } => { /* send */ }
        Action::Event { event, .. } => println!("{event:?}"),
        Action::Close { .. } => { /* close, then session.closed(), back off, reconnect */ }
        Action::RefreshToken => { /* new token, same client id → session.refresh */ }
        _ => {}
    }
}
```

## JavaScript

`bindings/js` is the npm package: the core built to WebAssembly, plus
`connectChat`, which does the networking around a `Session`.

```js
import * as kick from 'kicksdk';
await kick.init();

const chat = kick.connectChat({
  chatroomId, channelId,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(event.sender.username, event.content);
  },
});
// later: chat.close()
```

`examples/node-chat.mjs` prints a channel's chat from Node.

## Elixir

`bindings/elixir` is the hex package, a NIF over the same core. Values come
back the way JSON decodes: maps with string keys, never atoms made from Kick's
data.

```elixir
session = Kicksdk.Realtime.Session.new(Kicksdk.Realtime.chat_channels(chatroom_id, channel_id))
# socket opened, with Origin: https://kick.com
for %{"action" => "send", "text" => t} <- Kicksdk.Realtime.Session.open(session, token, now), do: send_frame(t)
# each frame
for action <- Kicksdk.Realtime.Session.receive(session, frame, now), do: handle(action)
```

## Layout

```
crates/kicksdk/          the core
crates/kicksdk-wasm/     its WebAssembly binding (built into bindings/js/pkg)
bindings/js/             the npm package: the binding, connectChat, types
bindings/elixir/         the hex package and its NIF crate (native/kicksdk_nif)
fixtures/                Kick's answers and frames, recorded and anonymized
docs/KICK.md             everything known about Kick, with sources
docs/architecture.md     how it is built, and why
tools/                   recorders
examples/
```

## Building and testing

```bash
cargo test --workspace            # the core, against fixtures/
scripts/build-js.sh               # the WebAssembly (needs wasm-bindgen-cli, see the script)
(cd bindings/js && npm test)      # the npm package, against the same fixtures
(cd bindings/elixir && mix test)  # the hex package, against the same fixtures
```

All three test suites read the same `fixtures/`, so a recording of a Kick
change fails every binding until the core handles it. How to record one is in
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT), at your option. Not affiliated with Kick.
