# kicksdk (Python)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, from the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core.

```bash
pip install kicksdk-py          # the core
pip install "kicksdk-py[aio]"   # plus connect_chat over asyncio (httpx, websockets)
```

The import is `kicksdk`. (On PyPI the name is `kicksdk-py`: PyPI treats
`kicksdk` as too close to the unrelated `kick-sdk`.)

```python
import asyncio, kicksdk.aio

async def main():
    async with kicksdk.aio.connect_chat(chatroom_id, channel_id) as chat:
        async for e in chat:
            ev = e["event"]
            if ev["type"] == "chat_message":
                print(ev["sender"]["username"], ev["content"])

asyncio.run(main())
```

`connect_chat` reconnects by itself (jittered backoff) after network errors,
5xx answers and dropped sockets; a failed token refresh keeps the connection.
The `async for` raises `kicksdk.aio.ChatClosed` when Kick ends the chat for
good (a ban, a terminal close code) and `kicksdk.aio.ChatRefused` after
`max_refusals` (5) refused attempts in a row (a 403, say), then ends; it also
ends with the `async with` block, which closes the socket normally. Events
the consumer has not read wait in a buffer of `max_queue` (10,000); when it
is full, `overflow="drop_oldest"` (the default) drops the oldest and counts
them in `chat.dropped`, and `overflow="backpressure"` stops reading the
socket until there is room. The `kicksdk.aio` module docs have the details.

Everything else returns `kicksdk.Request` objects for any HTTP client and
parses what comes back (`kicksdk.website.channel_request` /
`parse_channel`, `kicksdk.public_api.livestreams_requests`,
`kicksdk.webhooks.verify`, …). Errors are `kicksdk.KickError` with `kind` and
`status`.

`kicksdk.webhooks.verify(public_key_pem, headers, body)` takes the body as
received (`bytes`, or `str`) and refuses a delivery more than 5 minutes from
now (`now=` in Unix seconds, `tolerance_secs=`), so an old one cannot be
replayed; `skip_freshness=True` re-checks a stored one.

Wheels are built for Linux, macOS and Windows (CPython 3.9+, one abi3 wheel
per platform). Anywhere else pip builds from the source distribution, which
needs a Rust toolchain.

Kick's realtime server only accepts `Origin: https://kick.com`; `connect_chat`
sends it. MIT OR Apache-2.0. Not affiliated with Kick.
