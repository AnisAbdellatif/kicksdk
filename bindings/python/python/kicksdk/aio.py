"""Kick's chat feed over asyncio: ``pip install "kicksdk-py[aio]"``.

::

    async with kicksdk.aio.connect_chat(chatroom_id, channel_id) as chat:
        async for e in chat:
            if e["event"]["type"] == "chat_message":
                print(e["event"]["sender"]["username"], e["event"]["content"])

Each item is an ``event`` action of :class:`kicksdk.realtime.Session`
(``channel``, ``name``, ``data``, ``event``).

**Staying connected.** The feed reconnects, after a jittered backoff (1 s
doubling to 30 s, back to 1 s once the chat feed is subscribed), from
anything that may pass: a network error or timeout, an HTTP 5xx, 408 or
429, a socket dropped with a code :func:`kicksdk.realtime.should_reconnect`
allows, Kick ending the connection with ``reconnect`` true, or the session
finding it silent. A failed token refresh keeps the connection up: Kick
ends it when the old token runs out, and the reconnect gets a new one.

**Stopping.** The ``async for`` raises, after the events received before,
when going on would not help:

- :class:`ChatClosed`: Kick closed the connection for good (a session
  ``close`` with ``reconnect`` false, or a terminal socket close code,
  3500-3999 or 4500-4999), e.g. a ban or a refused token;
- :class:`ChatRefused`: ``max_refusals`` attempts in a row were refused - an
  HTTP 4xx such as a 403 on the WebSocket handshake, or an answer kicksdk
  cannot read - the last error is its ``__cause__``;
- anything else unexpected (a bug, arguments that do not fit) as itself.

Then, and once the ``async with`` block has ended, iteration stops: it never
waits on a feed that is gone. :attr:`ChatFeed.error` keeps the error.
Leaving the block closes the socket normally (1000).

**Slow consumers.** Events wait for the ``async for`` in a buffer of
``max_queue`` (10,000; ``None`` for no limit). When it is full, ``overflow``
decides:

- ``"drop_oldest"`` (the default): the oldest waiting event is dropped,
  counted in :attr:`ChatFeed.dropped` and logged (logger ``kicksdk.aio``).
  The connection keeps up with Kick.
- ``"backpressure"``: reading the socket pauses until there is room. Nothing
  is dropped, but Kick's pings go unanswered meanwhile: a consumer that stays
  behind for long is disconnected (and reconnected, losing what Kick sent in
  between).
"""

from __future__ import annotations

import asyncio
import collections
import contextlib
import logging
import random
import time
import uuid
from typing import Any, Deque, Dict, Mapping, Optional

try:
    import httpx
    from websockets.asyncio.client import connect as _ws_connect
    from websockets.exceptions import (
        ConnectionClosed,
        InvalidHandshake,
        InvalidMessage,
        InvalidStatus,
        InvalidURI,
        WebSocketException,
    )
    from websockets.typing import Origin
except ImportError as e:
    raise ImportError(
        f'kicksdk.aio needs httpx and websockets ({e.name} is missing): pip install "kicksdk-py[aio]"',
        name=e.name,
    ) from e

from . import KickError, Request
from . import realtime as rt

__all__ = ["connect_chat", "ChatFeed", "ChatError", "ChatClosed", "ChatRefused", "Event"]

Event = Dict[str, Any]
"""An ``event`` action: ``{"action": "event", "channel", "name", "data", "event"}``."""

log = logging.getLogger("kicksdk.aio")

OVERFLOW = ("drop_oldest", "backpressure")


class ChatError(Exception):
    """The chat feed stopped for good; see :class:`ChatClosed` and
    :class:`ChatRefused`."""


class ChatClosed(ChatError):
    """Kick closed the connection and a new one would be refused the same way.

    ``code`` is Kick's disconnect or error code, or the socket's close code;
    ``reason`` says why."""

    def __init__(self, code: int, reason: str) -> None:
        super().__init__(f"Kick closed the chat for good: {code} {reason}".rstrip())
        self.code = code
        self.reason = reason


class ChatRefused(ChatError):
    """``attempts`` connection attempts in a row were refused (an HTTP 4xx,
    or an answer kicksdk cannot read). ``status`` is the last HTTP status, if
    there was one; the last error is ``__cause__``."""

    def __init__(self, attempts: int, last: BaseException) -> None:
        super().__init__(f"Kick refused {attempts} connection attempts in a row; the last: {_describe(last)}")
        self.attempts = attempts
        self.status = _status_of(last)


def _now() -> int:
    return int(time.monotonic() * 1000)


async def _send(http: httpx.AsyncClient, req: Request) -> tuple[int, str]:
    res = await http.request(req.method, req.url, headers=req.headers, content=req.body)
    return res.status_code, res.text


def _describe(e: BaseException) -> str:
    return f"{type(e).__name__}: {e}" if str(e) else type(e).__name__


def _status_of(e: BaseException) -> Optional[int]:
    if isinstance(e, KickError):
        return e.status
    if isinstance(e, InvalidStatus):
        return e.response.status_code
    return None


_RETRY = "retry"
_REFUSED = "refused"


def _by_status(status: Optional[int]) -> str:
    if status is None or status >= 500 or status in (408, 429):
        return _RETRY
    return _REFUSED


def _failure(e: BaseException) -> Optional[str]:
    """How a failed attempt counts: ``retry`` (it may pass), ``refused`` (it
    may well not: give up after a few in a row), or None: not a connection
    problem at all, so it is raised."""
    if isinstance(e, KickError):
        if e.kind == "status":
            return _by_status(e.status)
        # Kick answered something kicksdk cannot read; `invalid`, `call` and
        # `webhook` are the caller's arguments or a bug, not the network.
        return _REFUSED if e.kind in ("shape", "json") else None
    if isinstance(e, InvalidStatus):
        return _by_status(e.response.status_code)
    if isinstance(e, InvalidMessage):
        return _RETRY  # the handshake's answer was cut off or garbled
    if isinstance(e, (InvalidHandshake, InvalidURI)):
        return _REFUSED
    if isinstance(e, (WebSocketException, httpx.TransportError, OSError, asyncio.TimeoutError)):
        return _RETRY
    if isinstance(e, httpx.HTTPError):
        return _REFUSED  # too many redirects, an undecodable body
    return None


class ChatFeed:
    """The connection :func:`connect_chat` keeps up; iterate it for events.

    Runs once: from ``async with`` to the end of the block.
    """

    def __init__(
        self,
        chatroom_id: int,
        channel_id: Optional[int] = None,
        channels: Optional[list[str]] = None,
        endpoints: Optional[Mapping[str, str]] = None,
        *,
        max_queue: Optional[int] = 10_000,
        overflow: str = "drop_oldest",
        max_refusals: int = 5,
        http: Optional[httpx.AsyncClient] = None,
    ) -> None:
        if overflow not in OVERFLOW:
            raise ValueError(f"overflow must be one of {OVERFLOW}, not {overflow!r}")
        if max_queue is not None and max_queue < 1:
            raise ValueError("max_queue must be at least 1 (or None, for no limit)")
        if max_refusals < 1:
            raise ValueError("max_refusals must be at least 1")
        self.channels: list[str] = list(channels) if channels else rt.chat_channels(chatroom_id, channel_id)
        self.channel_id = channel_id
        self.endpoints = endpoints
        self.max_queue = max_queue
        self.overflow = overflow
        self.max_refusals = max_refusals
        #: The feeds subscribed right now.
        self.subscribed: set[str] = set()
        #: Events dropped because the buffer was full (``drop_oldest``).
        self.dropped = 0
        self._http = http
        self._buf: Deque[Event] = collections.deque()
        self._task: Optional[asyncio.Task[None]] = None
        self._ready: Optional[asyncio.Event] = None
        self._room: Optional[asyncio.Event] = None
        self._done = False
        self._error: Optional[BaseException] = None
        self._error_raised = False
        self._refusals = 0

    @property
    def error(self) -> Optional[BaseException]:
        """Why the feed stopped on its own (:class:`ChatError`, or an
        unexpected error); None while it runs or if the block ended it."""
        return self._error

    @property
    def done(self) -> bool:
        """The feed has stopped: its error, or the end of the block."""
        return self._done

    async def __aenter__(self) -> ChatFeed:
        if self._task is not None or self._done:
            raise RuntimeError("a ChatFeed runs once: call connect_chat again")
        self._ready = asyncio.Event()
        self._room = asyncio.Event()
        self._task = asyncio.create_task(self._run(), name="kicksdk.aio chat feed")
        return self

    async def __aexit__(self, *exc: object) -> None:
        task = self._task
        if task is not None and not task.done():
            # The task closes its socket normally (1000) on the way out.
            task.cancel()
            await asyncio.wait({task})
        self._buf.clear()
        self._finish()

    def __aiter__(self) -> ChatFeed:
        return self

    async def __anext__(self) -> Event:
        if self._ready is None or self._room is None:
            raise RuntimeError("iterate a ChatFeed inside `async with connect_chat(...)`")
        while True:
            if self._buf:
                event = self._buf.popleft()
                self._room.set()
                return event
            if self._done:
                if self._error is not None and not self._error_raised:
                    self._error_raised = True
                    raise self._error
                raise StopAsyncIteration
            self._ready.clear()
            await self._ready.wait()

    def _finish(self) -> None:
        self._done = True
        if self._ready is not None:
            self._ready.set()
        if self._room is not None:
            self._room.set()

    async def _put(self, event: Event) -> None:
        assert self._ready is not None and self._room is not None
        limit = self.max_queue
        if limit is not None and len(self._buf) >= limit:
            if self.overflow == "backpressure":
                while len(self._buf) >= limit and not self._done:
                    self._room.clear()
                    await self._room.wait()
            else:
                self._buf.popleft()
                self.dropped += 1
                if self.dropped == 1 or self.dropped % 1_000 == 0:
                    log.warning(
                        "chat events dropped: %d so far; the consumer is %d events behind (max_queue)",
                        self.dropped,
                        limit,
                    )
        self._buf.append(event)
        self._ready.set()

    async def _run(self) -> None:
        try:
            if self._http is not None:
                await self._keep_up(self._http)
            else:
                async with httpx.AsyncClient(timeout=10, headers={"user-agent": "Mozilla/5.0"}) as http:
                    await self._keep_up(http)
        except Exception as e:  # noqa: BLE001 - kept for the iterator, which raises it
            self._error = e
            log.warning("chat feed stopped: %s", _describe(e))
        finally:
            self._finish()

    async def _keep_up(self, http: httpx.AsyncClient) -> None:
        session = rt.Session(self.channels)
        backoff = rt.Backoff()
        while True:
            try:
                await self._connect(http, session, backoff)
            except ChatError:
                raise
            except Exception as e:
                failure = _failure(e)
                if failure is None:
                    raise
                if failure == _REFUSED:
                    self._refusals += 1
                    if self._refusals >= self.max_refusals:
                        raise ChatRefused(self._refusals, e) from e
                log.info("chat connection failed (%s); reconnecting", _describe(e))
            finally:
                for a in session.closed():
                    if a["action"] == "unsubscribed":
                        self.subscribed.discard(a["channel"])
            await asyncio.sleep(backoff.next_delay_jittered(random.random()) / 1000)

    async def _connect(self, http: httpx.AsyncClient, session: rt.Session, backoff: rt.Backoff) -> None:
        """One connection, from asking Kick where to its end. Returns when a
        new one is worth trying; raises :class:`ChatClosed` when not."""
        # A guest identity: the token is issued to this id, and so is every
        # refreshed one for this connection.
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

        ws = await _ws_connect(
            found[0]["url"],
            origin=Origin(rt.REQUIRED_ORIGIN),
            ping_interval=None,  # Kick pings; the session answers
            max_size=2**22,
            close_timeout=5,
        )
        refresher: Optional[asyncio.Task[None]] = None
        closing: Optional[Event] = None  # a close the refresh's actions asked for
        close_code = 1000

        async def apply(actions: list[Event]) -> Optional[Event]:
            """Carries out the session's actions; returns a ``close``."""
            nonlocal refresher
            for a in actions:
                kind = a["action"]
                if kind == "send":
                    await ws.send(a["text"])
                elif kind == "close":
                    return a
                elif kind == "refresh_token":
                    # Concurrently: the receive loop keeps answering pings.
                    if refresher is None or refresher.done():
                        refresher = asyncio.create_task(refresh())
                elif kind == "connected":
                    self._refusals = 0
                elif kind == "subscribed":
                    self.subscribed.add(a["channel"])
                    if a["channel"] == self.channels[0]:
                        backoff.reset()
                elif kind == "unsubscribed":
                    self.subscribed.discard(a["channel"])
                elif kind == "subscribe_failed":
                    log.warning("Kick refused feed %s: %s %s", a["channel"], a["code"], a["message"])
                elif kind == "event":
                    await self._put(a)
            return None

        async def refresh() -> None:
            nonlocal closing
            try:
                fresh = rt.parse_token(*await _send(http, rt.token_request(client_id, self.endpoints)))
            except Exception as e:  # noqa: BLE001 - the core's contract: carry on
                log.warning("token refresh failed (%s); the connection stays up until Kick ends it", _describe(e))
                return
            close = await apply(session.refresh(fresh, _now()))
            if close is not None:
                closing = close
                await ws.close()  # ends the receive loop below

        try:
            close = await apply(session.open(token, _now()))
            while close is None:
                deadline = session.deadline
                timeout = None if deadline is None else max(0.0, (deadline - _now()) / 1000)
                try:
                    frame = await asyncio.wait_for(ws.recv(), timeout)
                except asyncio.TimeoutError:
                    close = await apply(session.tick(_now()))
                    continue
                except ConnectionClosed as e:
                    if closing is not None:
                        close = closing
                        break
                    code, reason = (e.rcvd.code, e.rcvd.reason) if e.rcvd is not None else (1006, "")
                    if not rt.should_reconnect(code):
                        raise ChatClosed(code, reason) from e
                    raise
                if isinstance(frame, bytes):
                    frame = frame.decode("utf-8", "replace")
                close = await apply(session.receive(frame, _now()))
            if not close["reconnect"]:
                raise ChatClosed(close["code"], close["reason"])
            log.info("chat connection ended (%s); reconnecting", close["reason"])
        except Exception as e:
            if not isinstance(e, ChatError) and _failure(e) is None:
                close_code = 1011  # a bug of ours, not the connection's end
            raise
        finally:
            if refresher is not None and not refresher.done():
                refresher.cancel()
                await asyncio.wait({refresher})
            with contextlib.suppress(Exception):
                await ws.close(close_code)


def connect_chat(
    chatroom_id: int,
    channel_id: Optional[int] = None,
    channels: Optional[list[str]] = None,
    endpoints: Optional[Mapping[str, str]] = None,
    *,
    max_queue: Optional[int] = 10_000,
    overflow: str = "drop_oldest",
    max_refusals: int = 5,
    http: Optional[httpx.AsyncClient] = None,
) -> ChatFeed:
    """One channel's chat, kept connected for the ``async with`` block.

    ``channel_id`` also finds the server for the chat's region. ``channels``
    replaces the feeds (:func:`kicksdk.realtime.chat_channels`, the chat
    feed first); ``endpoints`` points Kick's URLs elsewhere. ``max_queue``
    and ``overflow`` set what happens to events the consumer is not reading
    fast enough, ``max_refusals`` how many refused attempts in a row end the
    feed (see the module docs). ``http`` is a client to send Kick's HTTP
    requests with (it stays open); by default the feed has its own.
    """
    return ChatFeed(
        chatroom_id,
        channel_id,
        channels,
        endpoints,
        max_queue=max_queue,
        overflow=overflow,
        max_refusals=max_refusals,
        http=http,
    )
