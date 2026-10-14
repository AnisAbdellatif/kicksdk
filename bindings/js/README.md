# kicksdk-js

kicksdk for JavaScript. (On npm the name is `kicksdk-js`: npm treats
`kicksdk` as too close to the unrelated `kick-sdk`.)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, compiled from the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core to WebAssembly,
plus `connectChat`, which drives Kick's chat feed with `fetch` and `WebSocket`.

```js
import * as kick from 'kicksdk-js';
await kick.init();            // in Node: kick.init(fs.readFileSync(wasmPath))

const chat = kick.connectChat({
  chatroomId,
  channelId,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  },
  onState: ({ connected, subscribed, stopped }) => {},
  onDrop: (reason, { code, reconnect }) => {
    if (!reconnect) console.error(`Kick ended the connection for good: ${reason}`);
  },
});
chat.close();
```

`connectChat` reconnects with a jittered backoff whenever a connection ends
(Kick went quiet, the socket closed, a request or the handshake took longer
than `timeoutMs`, 10 s by default), and stops for good on Kick's terminal
codes: `onDrop` then says `reconnect: false` and `onState` `stopped: true`.
A callback that throws is reported to `onError` (or `console.error`) without
disturbing the connection; none fires after `close()`.

Node 22 and later have a `WebSocket`. On older Node, pass one:
`connectChat({ ..., WebSocket })` with the `ws` package's class. Without
one, `onDrop` says so once and the driver stops.

**Where the feed works.** Kick's realtime server only accepts a WebSocket whose
Origin is `https://kick.com`. Node: works (the driver sets the header). Browser:
only on kick.com - a page there or an extension's content script. Elsewhere,
relay through a server.

Everything else - `channelRequest` / `parseChannel`, `historyRequest`,
`livestreamsRequests`, `verifyWebhook`, `parseEvent`, `parseContent`, … -
returns plain request objects for your `fetch` and parses what comes back.
Parsers throw an `Error` with a `status` when Kick answered with an error.
Ids and counts must be whole numbers (ids at most `Number.MAX_SAFE_INTEGER`);
anything else throws rather than being rounded into another id.

**Webhooks.** Read Kick's key once and check each delivery against the raw
body (text or bytes), its headers (pairs, a fetch `Headers`, or an object),
and the clock: the timestamp must be within 5 minutes of `now` (Unix
seconds; this machine's clock by default).

```js
const key = new kick.WebhookPublicKey(pem);   // from publicKeyRequest / parsePublicKey
const { headers, event } = key.verify(req.headers, rawBody);
// or: kick.verifyWebhook(pem, headers, rawBody, { now, toleranceSecs })
// a stored delivery, checked again later: { skipFreshness: true }
```

A valid signature means Kick sent this body to *some* app (every app shares
the key): deduplicate on `headers.message_id`. See `docs/KICK.md` §4.3.

See the repository's `docs/KICK.md` for everything known about Kick, with
sources. MIT OR Apache-2.0. Not affiliated with Kick.
