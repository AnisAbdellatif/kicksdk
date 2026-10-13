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

Everything else returns `kicksdk.Request` objects for any HTTP client and
parses what comes back (`kicksdk.website.channel_request` /
`parse_channel`, `kicksdk.public_api.livestreams_requests`,
`kicksdk.webhooks.verify`, …). Errors are `kicksdk.KickError` with `kind` and
`status`.

Wheels are built for Linux, macOS and Windows (CPython 3.9+, one abi3 wheel
per platform). Anywhere else pip builds from the source distribution, which
needs a Rust toolchain.

Kick's realtime server only accepts `Origin: https://kick.com`; `connect_chat`
sends it. MIT OR Apache-2.0. Not affiliated with Kick.
