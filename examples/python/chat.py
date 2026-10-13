"""Prints a Kick channel's chat:  python chat.py <channel>

Install the package first:  pip install "../../bindings/python[aio]"
"""

import asyncio
import sys

import httpx
import kicksdk
import kicksdk.aio


async def main(slug: str) -> None:
    req = kicksdk.website.channel_request(slug)
    res = httpx.get(req.url, headers={"user-agent": "Mozilla/5.0"})
    channel = kicksdk.website.parse_channel(res.status_code, res.text)

    async with kicksdk.aio.connect_chat(channel["chatroom_id"], channel["id"]) as chat:
        async for e in chat:
            if e["event"]["type"] == "chat_message":
                print(f'{e["event"]["sender"]["username"]}: {e["event"]["content"]}')


if len(sys.argv) != 2:
    sys.exit("usage: python chat.py <channel>")
asyncio.run(main(sys.argv[1]))
