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
  onState: ({ connected, subscribed }) => {},
  onDrop: (reason, { code, terminal, waitMs }) => {
    if (terminal) console.warn(`Kick refused this client (${reason}); back in ${waitMs / 60_000} min`);
  },
});
chat.close();
```

`connectChat` keeps the chat up until `close()`. Whenever a connection ends
(Kick went quiet, the socket closed, a request or the handshake took longer
than `timeoutMs`, 10 s by default), `onDrop` says why and how long it waits
before connecting again (`waitMs`): a jittered backoff from 0.5 s to 30 s.
After one of Kick's terminal closes (a refused token, a ban, a connection
limit: `terminal: true`) it never gives up, but waits long - 5, 10, 20, then
30 minutes for terminal closes in a row, each plus up to a quarter - and
comes back as a new guest client (a new client id, so a new token). The chat
feed subscribing again ends the streak. `docs/KICK.md` §6.1 has the rules.
A callback that throws is reported to `onError` (or `console.error`) without
disturbing the connection; none fires after `close()`.

Node 22 and later have a `WebSocket`. On older Node, pass one:
`connectChat({ ..., WebSocket })` with the `ws` package's class. Without
one, `connectChat` throws.

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
