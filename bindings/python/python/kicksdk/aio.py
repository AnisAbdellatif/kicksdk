"""Kick's chat feed over asyncio: ``pip install "kicksdk-py[aio]"``.

::

    async with kicksdk.aio.connect_chat(chatroom_id, channel_id) as chat:
        async for e in chat:
            if e["event"]["type"] == "chat_message":
                print(e["event"]["sender"]["username"], e["event"]["content"])

Each item is an ``event`` action of :class:`kicksdk.realtime.Session`
(``channel``, ``name``, ``data``, ``event``).

**Staying connected.** The feed never gives up on its own: it reconnects
until the ``async with`` block ends (KICK.md §6.1). How long it waits first
depends on how the connection, or the attempt at one, ended:

- a network error or timeout, an HTTP error, a socket closed with an
  ordinary code, Kick ending the connection for a reason that may pass, or
  the session finding it silent: a jittered backoff, 1 s doubling to 30 s;
- a **terminal** close, Kick refusing this client (a disconnect or socket
  close code 3500-3999 or 4500-4999, a refused token: a ban, a connection
  limit): at least 5 minutes, then 10, 20 and 30 for more in a row, plus up
  to a quarter at random; the next connection is made as a new client (a
  new client id, so a new guest token);
- ``max_refusals`` (5) attempts refused in a row (an HTTP 4xx such as a 403
  on the WebSocket handshake, or an answer kicksdk cannot read): that
  attempt and every refused one after it count as terminal, until one is
  not refused.

The chat feed subscribing ends a streak of terminal closes, and the waits
start from the beginning again. A failed token refresh keeps
the connection up: Kick ends it when the old token runs out, and the
reconnect gets a new one. :attr:`ChatFeed.last_drop` says why the last
connection ended and how long the feed waits; :attr:`ChatFeed.retrying` is
true while it waits. Terminal closes are logged as warnings, other drops as
info (logger ``kicksdk.aio``).

**Stopping.** Iteration ends with the ``async with`` block, which closes the
socket normally (1000) and returns once the feed has stopped. Only an
unexpected error (a bug, arguments that do not fit) ends the feed before
that: the ``async for`` raises it, after the events received before, then
stops; :attr:`ChatFeed.error` keeps it.

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
import dataclasses
import logging
import random
import time
import uuid
from typing import Any, Deque, Dict, Mapping, Optional, Union

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

__all__ = ["connect_chat", "ChatFeed", "ChatDrop", "Event"]

Event = Dict[str, Any]
"""An ``event`` action: ``{"action": "event", "channel", "name", "data", "event"}``."""

log = logging.getLogger("kicksdk.aio")

OVERFLOW = ("drop_oldest", "backpressure")


@dataclasses.dataclass(frozen=True)
class ChatDrop:
    """Why a connection, or an attempt at one, ended, and how long the feed
    waits before the next."""

    reason: str
    """What happened, readable."""
    code: int
    """Kick's disconnect or error code, the socket's close code (1006: the
    network dropped), or 0 (a failed request or handshake, silence)."""
    terminal: bool
    """Kick refused this client (or ``max_refusals`` attempts in a row were
    refused): the next connection is made with a new client id."""
    retry_after_ms: int
    """The least wait asked for, ms: 0 (the ordinary backoff), or
    :data:`kicksdk.realtime.TERMINAL_RETRY_MS` for a terminal close."""
    wait: float = 0.0
    """How long the feed waits before reconnecting, seconds."""
    status: Optional[int] = None
    """The HTTP status of a refused request or WebSocket handshake."""
    error: Optional[BaseException] = None
    """The exception that ended the attempt, if one did."""


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
    may well not: after ``max_refusals`` in a row, wait as long as for a
    terminal close), or None: not a connection problem at all, so it is
    raised."""
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


def _closed_by_socket(e: ConnectionClosed) -> ChatDrop:
    """The socket closed by itself, seen by a receive or a send: its close
    code decides (:func:`kicksdk.realtime.retry_after_ms`)."""
    code, reason = (e.rcvd.code, e.rcvd.reason) if e.rcvd is not None else (1006, "")
    retry_after = rt.retry_after_ms(code)
    return ChatDrop(reason or f"the socket closed ({code})", code, retry_after > 0, retry_after)


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
        backoff: Optional[rt.Backoff] = None,
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
        #: The reconnect delays (:class:`kicksdk.realtime.Backoff`).
        self.backoff: rt.Backoff = backoff if backoff is not None else rt.Backoff()
        #: The feeds subscribed right now.
        self.subscribed: set[str] = set()
        #: Events dropped because the buffer was full (``drop_oldest``).
        self.dropped = 0
        #: How the last connection, or attempt at one, ended; None before.
        self.last_drop: Optional[ChatDrop] = None
        self._http = http
        self._buf: Deque[Event] = collections.deque()
        self._task: Optional[asyncio.Task[None]] = None
        self._ready: Optional[asyncio.Event] = None
        self._room: Optional[asyncio.Event] = None
        self._done = False
        self._stopping = False  # the block has ended: the task must stop
        self._closing = False  # the task is closing its socket or client: no cancel then
        self._retrying = False
        self._error: Optional[BaseException] = None
        self._error_raised = False
        self._refusals = 0

    @property
    def error(self) -> Optional[BaseException]:
        """The unexpected error that stopped the feed; None while it runs or
        if the block ended it."""
        return self._error

    @property
    def done(self) -> bool:
        """The feed has stopped: an unexpected error, or the end of the
        block."""
        return self._done

    @property
    def retrying(self) -> bool:
        """The feed is waiting to reconnect (:attr:`last_drop` says how
        long, and why)."""
        return self._retrying

    async def __aenter__(self) -> ChatFeed:
        if self._task is not None or self._done:
            raise RuntimeError("a ChatFeed runs once: call connect_chat again")
        self._ready = asyncio.Event()
        self._room = asyncio.Event()
        self._task = asyncio.create_task(self._run(), name="kicksdk.aio chat feed")
        return self

    async def __aexit__(self, *exc: object) -> None:
        self._stopping = True
        if self._room is not None:
            self._room.set()
        task = self._task
        if task is not None:
            try:
                # One cancel is not always enough: before Python 3.12 a
                # wait_for anywhere below (in websockets, say) can swallow it.
                # So cancel until the task ends; it also checks _stopping at
                # each step. Not while it is closing a socket (or its HTTP
                # client): that is how it ends, and a cancel would cut the
                # close (1000) short.
                while not task.done():
                    if not self._closing:
                        task.cancel()
                    await asyncio.wait({task}, timeout=0.1)
            except asyncio.CancelledError:
                task.cancel()
                raise
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
        self._retrying = False
        if self._ready is not None:
            self._ready.set()
        if self._room is not None:
            self._room.set()

    def _check_stop(self) -> None:
        if self._stopping:
            raise asyncio.CancelledError()

    async def _put(self, event: Event) -> None:
        assert self._ready is not None and self._room is not None
        limit = self.max_queue
        if limit is not None and len(self._buf) >= limit:
            if self.overflow == "backpressure":
                while len(self._buf) >= limit and not self._done:
                    self._check_stop()
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
                    try:
                        await self._keep_up(http)
                    finally:
                        self._closing = True  # the client, now: let it close
        except Exception as e:  # noqa: BLE001 - kept for the iterator, which raises it
            self._error = e
            log.warning("chat feed stopped: %s", _describe(e))
        finally:
            self._finish()

    async def _keep_up(self, http: httpx.AsyncClient) -> None:
        session = rt.Session(self.channels)
        backoff = self.backoff
        # A guest identity: the token is issued to this id, and so is every
        # refreshed one. Kept across reconnects; a terminal close (Kick
        # refusing this client) makes a new one.
        client_id = str(uuid.uuid4())
        while True:
            self._check_stop()
            try:
                drop = await self._connect(http, session, backoff, client_id)
                self._refusals = 0
            except Exception as e:
                failure = _failure(e)
                if failure is None:
                    raise
                self._refusals = self._refusals + 1 if failure == _REFUSED else 0
                gave_up = self._refusals >= self.max_refusals
                retry_after = rt.TERMINAL_RETRY_MS if gave_up else 0
                drop = ChatDrop(_describe(e), 0, gave_up, retry_after, status=_status_of(e), error=e)
            finally:
                for a in session.closed():
                    if a["action"] == "unsubscribed":
                        self.subscribed.discard(a["channel"])
            if drop.terminal:
                client_id = str(uuid.uuid4())
            wait_ms = backoff.next_delay_for(drop.retry_after_ms, random.random())
            drop = dataclasses.replace(drop, wait=wait_ms / 1000)
            self.last_drop = drop
            if drop.terminal and drop.error is not None:
                log.warning(
                    "Kick refused %d attempts in a row, the last: %s; trying as a new client in %.0f s",
                    self._refusals,
                    drop.reason,
                    drop.wait,
                )
            elif drop.terminal:
                log.warning(
                    "Kick closed the chat to this client (%d: %s); reconnecting as a new client in %.0f s",
                    drop.code,
                    drop.reason,
                    drop.wait,
                )
            else:
                log.info("chat connection ended (%s); reconnecting in %.1f s", drop.reason, drop.wait)
            self._check_stop()
            self._retrying = True
            try:
                await asyncio.sleep(drop.wait)
            finally:
                self._retrying = False

    async def _connect(
        self, http: httpx.AsyncClient, session: rt.Session, backoff: rt.Backoff, client_id: str
    ) -> ChatDrop:
        """One connection, from asking Kick where to its end; returns how it
        ended. Raises for a failed attempt."""
        where = (
            rt.chat_connection_request(self.channel_id, client_id, self.endpoints)
            if self.channel_id
            else rt.connection_request(client_id, self.endpoints)
        )
        found = [c for c in rt.parse_connection(*await _send(http, where)) if c.get("provider") == "centrifugo"]
        if not found:
            raise KickError("shape", None, "Kick offered no Centrifugo server")
        token = rt.parse_token(*await _send(http, rt.token_request(client_id, self.endpoints)))
        self._check_stop()

        ws = await _ws_connect(
            found[0]["url"],
            origin=Origin(rt.REQUIRED_ORIGIN),
            ping_interval=None,  # Kick pings; the session answers
            max_size=2**22,
            close_timeout=5,
        )
        refresher: Optional[asyncio.Task[None]] = None
        recv: Optional[asyncio.Task[Union[str, bytes]]] = None
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
                    # Kick refuses a guest some of the default feeds (103);
                    # only the chat feed itself is worth a warning.
                    level = logging.WARNING if a["channel"] == self.channels[0] else logging.DEBUG
                    log.log(level, "Kick refused feed %s: %s %s", a["channel"], a["code"], a["message"])
                elif kind == "event":
                    await self._put(a)
            return None

        async def refresh() -> None:
            # Never raises: nothing awaits this task's result.
            try:
                fresh = rt.parse_token(*await _send(http, rt.token_request(client_id, self.endpoints)))
                # Session.refresh only ever sends; its answer comes through
                # receive, in the loop below.
                await apply(session.refresh(fresh, _now()))
            except ConnectionClosed:
                pass  # the receive loop sees the close too, and handles it
            except Exception as e:  # noqa: BLE001 - the core's contract: carry on
                log.warning("token refresh failed (%s); the connection stays up until Kick ends it", _describe(e))

        try:
            try:
                close = await apply(session.open(token, _now()))
                while close is None:
                    self._check_stop()
                    if recv is None:
                        recv = asyncio.create_task(ws.recv())
                    deadline = session.deadline
                    timeout = None if deadline is None else max(0.0, (deadline - _now()) / 1000)
                    # Not wait_for: before Python 3.12 it can swallow a
                    # cancel when the frame arrives at the same moment.
                    # asyncio.wait leaves the receive running on a timeout.
                    done, _ = await asyncio.wait({recv}, timeout=timeout)
                    if not done:
                        close = await apply(session.tick(_now()))
                        continue
                    received, recv = recv, None
                    frame = received.result()
                    if isinstance(frame, bytes):
                        frame = frame.decode("utf-8", "replace")
                    close = await apply(session.receive(frame, _now()))
            except ConnectionClosed as e:
                # A receive or a send found the socket closed by Kick (or by
                # the network): its close code decides, wherever it showed.
                return _closed_by_socket(e)
            return ChatDrop(close["reason"], close["code"], close["terminal"], close["retry_after_ms"])
        except Exception as e:
            if _failure(e) is None:
                close_code = 1011  # a bug of ours, not the connection's end
            raise
        finally:
            self._closing = True
            try:
                for t in (refresher, recv):
                    if t is not None and not t.done():
                        t.cancel()
                        await asyncio.wait({t}, timeout=5)
                if recv is not None and recv.done() and not recv.cancelled():
                    recv.exception()  # retrieved: the socket is closing anyway
                with contextlib.suppress(Exception):
                    await ws.close(close_code)
            finally:
                self._closing = False


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
    backoff: Optional[rt.Backoff] = None,
) -> ChatFeed:
    """One channel's chat, kept connected for the ``async with`` block.

    ``channel_id`` also finds the server for the chat's region. ``channels``
    replaces the feeds (:func:`kicksdk.realtime.chat_channels`, the chat
    feed first); ``endpoints`` points Kick's URLs elsewhere. ``max_queue``
    and ``overflow`` set what happens to events the consumer is not reading
    fast enough, ``max_refusals`` how many refused attempts in a row count
    as a terminal close (see the module docs). ``http`` is a client to send
    Kick's HTTP requests with (it stays open); by default the feed has its
    own. ``backoff`` is the :class:`kicksdk.realtime.Backoff` to wait by
    (default: 1 s doubling to 30 s).
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
        backoff=backoff,
    )
