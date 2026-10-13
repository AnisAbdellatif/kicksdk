"""Kick's chat feed over asyncio: ``pip install kicksdk[aio]``.

::

    async with kicksdk.aio.connect_chat(chatroom_id, channel_id) as chat:
        async for e in chat:
            if e["event"]["type"] == "chat_message":
                print(e["event"]["sender"]["username"], e["event"]["content"])

Reconnects with backoff until the ``async with`` block ends; each item is an
``event`` action (``channel``, ``name``, ``data``, ``event``).
"""

from __future__ import annotations

import asyncio
import contextlib
import time
import uuid
from typing import AsyncIterator, Mapping, Optional

from . import KickError, Request
from . import realtime as rt


def _now() -> int:
    return int(time.monotonic() * 1000)


async def _send(http, req: Request) -> tuple[int, str]:
    res = await http.request(req.method, req.url, headers=req.headers, content=req.body)
    return res.status_code, res.text


class ChatFeed:
    """The connection :func:`connect_chat` keeps up; iterate it for events."""

    def __init__(
        self,
        chatroom_id: int,
        channel_id: Optional[int] = None,
        channels: Optional[list[str]] = None,
        endpoints: Optional[Mapping[str, str]] = None,
    ) -> None:
        self.channels = channels or rt.chat_channels(chatroom_id, channel_id)
        self.channel_id = channel_id
        self.endpoints = endpoints
        self.subscribed: set[str] = set()
        self._queue: asyncio.Queue = asyncio.Queue(maxsize=10_000)
        self._task: Optional[asyncio.Task] = None

    async def __aenter__(self) -> "ChatFeed":
        self._task = asyncio.create_task(self._run())
        return self

    async def __aexit__(self, *exc) -> None:
        if self._task:
            self._task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._task

    def __aiter__(self) -> AsyncIterator[dict]:
        return self

    async def __anext__(self) -> dict:
        return await self._queue.get()

    async def _run(self) -> None:
        import httpx

        backoff = rt.Backoff()
        session = rt.Session(self.channels)
        async with httpx.AsyncClient(timeout=10, headers={"user-agent": "Mozilla/5.0"}) as http:
            while True:
                try:
                    await self._connect(http, session, backoff)
                except (KickError, OSError, asyncio.TimeoutError, httpx.HTTPError):
                    pass
                except Exception:  # noqa: BLE001 - a websockets error; reconnect
                    pass
                for a in session.closed():
                    self.subscribed.discard(a.get("channel"))
                await asyncio.sleep(backoff.next())

    async def _connect(self, http, session: rt.Session, backoff: rt.Backoff) -> None:
        from websockets.asyncio.client import connect

        client_id = str(uuid.uuid4())
        where = (
            rt.chat_connection_request(self.channel_id, client_id, self.endpoints)
            if self.channel_id
            else rt.connection_request(client_id, self.endpoints)
        )
        found = [c for c in rt.parse_connection(*await _send(http, where)) if c.get("provider") == "centrifugo"]
        if not found:
            raise KickError("shape", None, "Kick offered no Centrifugo server")
        token = rt.parse_token(*await _send(http, rt.token_request(client_id, self.endpoints)))

        async with connect(found[0]["url"], origin=rt.REQUIRED_ORIGIN, ping_interval=None, max_size=2**22) as ws:

            async def apply(actions: list[dict]) -> bool:
                for a in actions:
                    kind = a["action"]
                    if kind == "send":
                        await ws.send(a["text"])
                    elif kind == "close":
                        return False
                    elif kind == "refresh_token":
                        fresh = rt.parse_token(*await _send(http, rt.token_request(client_id, self.endpoints)))
                        if not await apply(session.refresh(fresh, _now())):
                            return False
                    elif kind == "subscribed":
                        self.subscribed.add(a["channel"])
                        if a["channel"] == self.channels[0]:
                            backoff.reset()
                    elif kind == "unsubscribed":
                        self.subscribed.discard(a["channel"])
                    elif kind == "event":
                        with contextlib.suppress(asyncio.QueueFull):
                            self._queue.put_nowait(a)
                return True

            if not await apply(session.open(token, _now())):
                return
            while True:
                deadline = session.deadline
                timeout = None if deadline is None else max(0.0, (deadline - _now()) / 1000)
                try:
                    frame = await asyncio.wait_for(ws.recv(), timeout)
                except asyncio.TimeoutError:
                    if not await apply(session.tick(_now())):
                        return
                    continue
                if isinstance(frame, bytes):
                    frame = frame.decode("utf-8", "replace")
                if not await apply(session.receive(frame, _now())):
                    return


def connect_chat(
    chatroom_id: int,
    channel_id: Optional[int] = None,
    channels: Optional[list[str]] = None,
    endpoints: Optional[Mapping[str, str]] = None,
) -> ChatFeed:
    """One channel's chat, kept connected for the ``async with`` block."""
    return ChatFeed(chatroom_id, channel_id, channels, endpoints)
