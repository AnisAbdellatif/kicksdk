# kicksdk (JavaScript)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, compiled from the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core to WebAssembly,
plus `connectChat`, which drives Kick's chat feed with `fetch` and `WebSocket`.

```js
import * as kick from 'kicksdk';
await kick.init();            // in Node: kick.init(fs.readFileSync(wasmPath))

const chat = kick.connectChat({
  chatroomId,
  channelId,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  },
  onState: ({ connected, subscribed }) => {},
});
chat.close();
```

**Where the feed works.** Kick's realtime server only accepts a WebSocket whose
Origin is `https://kick.com`. Node: works (the driver sets the header). Browser:
only on kick.com - a page there or an extension's content script. Elsewhere,
relay through a server.

Everything else - `channelRequest` / `parseChannel`, `historyRequest`,
`livestreamsRequests`, `verifyWebhook`, `parseEvent`, `parseContent`, … -
returns plain request objects for your `fetch` and parses what comes back.
Parsers throw an `Error` with a `status` when Kick answered with an error.

See the repository's `docs/KICK.md` for everything known about Kick, with
sources. MIT OR Apache-2.0. Not affiliated with Kick.
