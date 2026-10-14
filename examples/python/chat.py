"""Prints a Kick channel's chat:  python chat.py <channel>

Install the package first:  pip install "../../bindings/python[aio]"
"""

import asyncio
import sys

import httpx

import kicksdk
import kicksdk.aio


async def main(slug: str) -> None:
    async with httpx.AsyncClient(timeout=10, headers={"user-agent": "Mozilla/5.0"}) as http:
        # Refuses a slug that would change the URL (KickError, kind "invalid").
        req = kicksdk.website.channel_request(slug)
        res = await http.request(req.method, req.url, headers=req.headers)
        channel = kicksdk.website.parse_channel(res.status_code, res.text)

        # Reconnects by itself; ends with a ChatError only when going on would
        # not help (a ban, a token refused for good, every attempt refused).
        async with kicksdk.aio.connect_chat(channel["chatroom_id"], channel["id"], http=http) as chat:
            async for e in chat:
                if e["event"]["type"] == "chat_message":
                    print(f'{e["event"]["sender"]["username"]}: {e["event"]["content"]}')


if len(sys.argv) != 2:
    sys.exit("usage: python chat.py <channel>")
try:
    asyncio.run(main(sys.argv[1]))
except kicksdk.aio.ChatError as e:
    sys.exit(str(e))
except kicksdk.KickError as e:
    sys.exit(f"Kick: {e}")
except KeyboardInterrupt:
    pass
