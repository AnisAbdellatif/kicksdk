"""kicksdk.aio against a fake Kick: its realtime HTTP endpoints through
httpx's MockTransport, its Centrifugo server as a local websockets server."""

from __future__ import annotations

import asyncio
import functools
import gc
import importlib
import json
import logging
import sys
from typing import Any, Awaitable, Callable, Optional

import pytest

pytest.importorskip("httpx")
pytest.importorskip("websockets")

import httpx  # noqa: E402
from websockets.asyncio.server import ServerConnection, serve  # noqa: E402
from websockets.exceptions import ConnectionClosed  # noqa: E402

import kicksdk.aio as aio  # noqa: E402
from kicksdk import realtime  # noqa: E402

CHATROOM, CHANNEL = 1, 2
CHAT_FEED = realtime.chat_channels(CHATROOM, CHANNEL)[0]
ENDPOINTS = {"realtime": "http://kick.test/rt"}


def run(test: Callable[..., Awaitable[None]]) -> Callable[..., None]:
    """Runs an async test in its own loop, bounded: nothing here may hang.
    Anything the loop reports (a task's exception never retrieved, say)
    fails the test."""

    @functools.wraps(test)
    def wrapper(*args: Any, **kwargs: Any) -> None:
        reported: list[dict[str, Any]] = []

        async def main() -> None:
            asyncio.get_running_loop().set_exception_handler(lambda loop, context: reported.append(context))
            await asyncio.wait_for(test(*args, **kwargs), 15)
            gc.collect()  # a task dropped with its exception reports it now

        asyncio.run(main())
        assert reported == []

    return wrapper


class QuickBackoff(realtime.Backoff):
    """The feed's backoff, quicker: ordinary waits from 10 ms, terminal ones
    10,000 times shorter (300 000 ms is 30 ms). Records what it was asked
    and what the core answered."""

    def __init__(self) -> None:
        super().__init__(0.01, 0.05)
        self.asked: list[int] = []
        self.waits: list[int] = []

    def next_delay_for(self, retry_after_ms: int, random: float) -> int:
        wait = super().next_delay_for(retry_after_ms, random)
        self.asked.append(retry_after_ms)
        self.waits.append(wait)
        return wait if retry_after_ms == 0 else wait // 10_000


def chat_message(n: int) -> str:
    data = {
        "id": f"00000000-0000-4000-8000-{n:012d}",
        "chatroom_id": CHATROOM,
        "content": f"text-{n}",
        "type": "message",
        "created_at": "2026-10-13T04:03:25+00:00",
        "sender": {"id": 3, "username": "user0001", "slug": "user0001", "identity": {"color": "#FF55B3", "badges": []}},
    }
    envelope = {"event": "App\\Events\\ChatMessageEvent", "data": json.dumps(data)}
    return json.dumps({"push": {"channel": CHAT_FEED, "pub": {"data": envelope}}})


Script = Callable[[ServerConnection, "Kick"], Awaitable[None]]


class Kick:
    """One fake Kick. ``scripts`` play the WebSocket connections in turn (the
    last one again for any after it)."""

    def __init__(self, *scripts: Script) -> None:
        self.scripts = list(scripts)
        self.connections = 0
        self.origins: list[Optional[str]] = []
        self.close_codes: list[Optional[int]] = []
        self.connection_statuses: list[int] = []  # answers to the connection request, then 200
        self.handshake_status: Optional[int] = None  # refuse the WebSocket handshake
        self.token_requests = 0
        self.token_answer: Callable[[int], Awaitable[httpx.Response]] = self.token
        # The client id of each connection request and each token request.
        self.connection_clients: list[str] = []
        self.token_clients: list[str] = []

    async def __aenter__(self) -> Kick:
        self.server = await serve(self.handle, "127.0.0.1", 0, process_request=self.process)
        port = next(iter(self.server.sockets)).getsockname()[1]
        self.url = f"ws://127.0.0.1:{port}/connection/websocket"
        self.client = httpx.AsyncClient(transport=httpx.MockTransport(self.answer))
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.client.aclose()
        self.server.close()
        await self.server.wait_closed()

    def feed(self, **kw: Any) -> aio.ChatFeed:
        kw.setdefault("backoff", QuickBackoff())
        return aio.connect_chat(CHATROOM, CHANNEL, endpoints=ENDPOINTS, http=self.client, **kw)

    async def answer(self, request: httpx.Request) -> httpx.Response:
        assert request.headers["origin"] == "https://kick.com"
        if request.url.path.endswith("/chat/connection"):
            self.connection_clients.append(json.loads(request.content)["client"]["id"])
            status = self.connection_statuses.pop(0) if self.connection_statuses else 200
            server = {"provider": "centrifugo", "credentials": {"url": self.url}}
            body: dict[str, Any] = {"data": {"connections": [server]}}
            return httpx.Response(status, json=body if status == 200 else {"message": "no"})
        assert request.url.path.endswith("/auth/connection")
        self.token_clients.append(json.loads(request.content)["client_id"])
        self.token_requests += 1
        return await self.token_answer(self.token_requests)

    async def token(self, n: int) -> httpx.Response:
        return httpx.Response(200, json={"data": {"token": f"token-{n}"}})

    def process(self, connection: ServerConnection, request: Any) -> Any:
        if self.handshake_status is not None:
            return connection.respond(self.handshake_status, "refused\n")
        return None

    async def handle(self, ws: ServerConnection) -> None:
        script = self.scripts[min(self.connections, len(self.scripts) - 1)]
        self.connections += 1
        self.origins.append(ws.request.headers.get("Origin") if ws.request else None)
        try:
            await script(ws, self)
            await ws.wait_closed()
        except ConnectionClosed:
            pass
        # The code the client sent (`close_code` is websockets 14+).
        rcvd = ws.protocol.close_rcvd
        self.close_codes.append(rcvd.code if rcvd is not None else None)


async def accept(ws: ServerConnection, **reply: Any) -> dict[str, Any]:
    """Answers the connect command and the subscribes; returns the connect."""
    connect = json.loads(await ws.recv())
    await ws.send(json.dumps({"id": connect["id"], "connect": {"client": "c1", "ping": 25, "pong": True, **reply}}))
    subscribes = [json.loads(line) for line in str(await ws.recv()).split("\n")]
    await ws.send("\n".join(json.dumps({"id": s["id"], "subscribe": {}}) for s in subscribes))
    return dict(connect)


async def recv_json(ws: ServerConnection, timeout: float = 2) -> Any:
    return json.loads(await asyncio.wait_for(ws.recv(), timeout))


async def stay(ws: ServerConnection, kick: Kick) -> None:
    await accept(ws)


async def next_event(feed: aio.ChatFeed, timeout: float = 3) -> aio.Event:
    return await asyncio.wait_for(feed.__anext__(), timeout)


async def until(check: Callable[[], bool], timeout: float = 3) -> None:
    async def poll() -> None:
        while not check():
            await asyncio.sleep(0.01)

    await asyncio.wait_for(poll(), timeout)


@run
async def test_connects_subscribes_and_delivers_events() -> None:
    async def script(ws: ServerConnection, kick: Kick) -> None:
        connect = await accept(ws)
        assert connect["connect"]["token"] == "token-1"
        for n in range(3):
            await ws.send(chat_message(n))

    async with Kick(script) as kick, kick.feed() as chat:
        got = [await next_event(chat) for _ in range(3)]
        assert [e["event"]["content"] for e in got] == ["text-0", "text-1", "text-2"]
        assert {e["action"] for e in got} == {"event"} and got[0]["channel"] == CHAT_FEED
        await until(lambda: set(chat.channels) <= chat.subscribed)
        assert kick.origins == ["https://kick.com"]


@run
async def test_answers_kicks_pings() -> None:
    pongs: list[Any] = []

    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        for _ in range(2):
            await ws.send("{}")
            pongs.append(await recv_json(ws))

    async with Kick(script) as kick, kick.feed():
        await until(lambda: len(pongs) == 2)
    assert pongs == [{}, {}]


@run
async def test_a_failed_refresh_keeps_the_connection_and_a_slow_one_does_not_block_pings() -> None:
    # A token that lives 1 s: the session asks for a new one after 0.5 s.
    refresh_started = asyncio.Event()
    pong: list[Any] = []

    async def token(n: int) -> httpx.Response:
        if n == 1:
            return httpx.Response(200, json={"data": {"token": "token-1"}})
        refresh_started.set()
        await asyncio.sleep(0.5)  # slow, and then a failure
        return httpx.Response(500, json={"message": "down"})

    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws, expires=True, ttl=1)
        await asyncio.wait_for(refresh_started.wait(), 3)
        await ws.send("{}")  # answered while the refresh is still out
        pong.append(await recv_json(ws, timeout=0.3))
        await asyncio.sleep(0.5)  # the refresh has failed by now
        await ws.send(chat_message(7))

    async with Kick(script) as kick:
        kick.token_answer = token
        async with kick.feed() as chat:
            assert (await next_event(chat))["event"]["content"] == "text-7"
            assert pong == [{}]
            assert kick.token_requests == 2
            assert kick.connections == 1 and not chat.done


@run
async def test_a_refreshed_token_is_sent_on_the_same_connection() -> None:
    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws, expires=True, ttl=1)
        refresh = await recv_json(ws, timeout=3)
        assert refresh["refresh"] == {"token": "token-2"}
        await ws.send(json.dumps({"id": refresh["id"], "refresh": {"expires": True, "ttl": 60}}))
        await ws.send(chat_message(1))

    async with Kick(script) as kick, kick.feed() as chat:
        assert (await next_event(chat))["event"]["content"] == "text-1"
        assert kick.connections == 1


def backoff_of(chat: aio.ChatFeed) -> QuickBackoff:
    assert isinstance(chat.backoff, QuickBackoff)
    return chat.backoff


@run
async def test_kick_closing_the_chat_to_this_client_reconnects_later_as_a_new_one() -> None:
    async def banned(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.send(chat_message(1))
        await ws.send(json.dumps({"push": {"disconnect": {"code": 3501, "reason": "banned"}}}))

    async def second(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.send(chat_message(2))

    async with Kick(banned, second) as kick, kick.feed() as chat:
        assert (await next_event(chat))["event"]["content"] == "text-1"
        assert (await next_event(chat))["event"]["content"] == "text-2", "the feed goes on"
        drop = chat.last_drop
        assert drop is not None
        assert (drop.code, drop.reason, drop.terminal) == (3501, "Kick disconnected: 3501 banned", True)
        assert drop.retry_after_ms == realtime.TERMINAL_RETRY_MS
        assert backoff_of(chat).asked == [realtime.TERMINAL_RETRY_MS]
        assert 300_000 <= backoff_of(chat).waits[0] <= 375_000
        assert drop.wait == backoff_of(chat).waits[0] // 10_000 / 1000
        # A new client: a new id for the connection request and the token.
        assert len(set(kick.connection_clients)) == 2 and kick.token_clients == kick.connection_clients
        assert kick.close_codes == [1000], "the client closed the first socket normally"
        assert not chat.done and chat.error is None


async def close_4501(ws: ServerConnection, kick: Kick) -> None:
    await accept(ws)
    await ws.close(4501, "go away")


async def ping_then_close_3501(ws: ServerConnection, kick: Kick) -> None:
    await accept(ws)
    await ws.send("{}")  # the pong may find the socket closed
    await ws.close(3501, "go away")


async def connect_then_close_4501(ws: ServerConnection, kick: Kick) -> None:
    connect = json.loads(await ws.recv())
    await ws.send(json.dumps({"id": connect["id"], "connect": {"client": "c1", "ping": 25, "pong": True}}))
    await ws.close(4501, "go away")  # the subscribes may find the socket closed


async def close_4999_at_once(ws: ServerConnection, kick: Kick) -> None:
    await ws.close(4999)


@pytest.mark.parametrize(
    ("first", "code"),
    [(close_4501, 4501), (ping_then_close_3501, 3501), (connect_then_close_4501, 4501), (close_4999_at_once, 4999)],
)
@run
async def test_a_terminal_socket_close_code_reconnects_later_as_a_new_client(first: Script, code: int) -> None:
    # However the close shows (a receive, or a send finding the socket
    # closed), the close code decides.
    async def second(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.send(chat_message(2))

    async with Kick(first, second) as kick, kick.feed() as chat:
        assert (await next_event(chat))["event"]["content"] == "text-2"
        drop = chat.last_drop
        assert drop is not None and (drop.code, drop.terminal) == (code, True)
        assert drop.retry_after_ms == realtime.TERMINAL_RETRY_MS
        assert backoff_of(chat).asked == [realtime.TERMINAL_RETRY_MS]
        assert len(set(kick.connection_clients)) == 2 and kick.token_clients == kick.connection_clients


@run
async def test_terminal_closes_in_a_row_wait_longer_until_the_chat_feed_subscribes() -> None:
    async def refused(ws: ServerConnection, kick: Kick) -> None:
        await ws.close(3500)

    async def works_then_refused(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.close(3500)

    async def last(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.send(chat_message(9))

    async with Kick(refused, refused, works_then_refused, last) as kick, kick.feed() as chat:
        assert (await next_event(chat))["event"]["content"] == "text-9"
        # 5, 10, then (the chat feed subscribed in between) 5 minutes again,
        # each plus up to a quarter.
        waits = backoff_of(chat).waits
        assert len(waits) == 3
        assert all(least <= w <= least * 5 // 4 for w, least in zip(waits, [300_000, 600_000, 300_000])), waits
        assert len(set(kick.connection_clients)) == 4, "a new client after each terminal close"


@run
async def test_the_feed_says_while_it_waits_and_stops_waiting_with_the_block() -> None:
    class SlowBackoff(QuickBackoff):
        def next_delay_for(self, retry_after_ms: int, random: float) -> int:
            super().next_delay_for(retry_after_ms, random)
            return 60_000  # a minute: longer than the test

    async def banned(ws: ServerConnection, kick: Kick) -> None:
        await ws.close(4501, "go away")

    async with Kick(banned) as kick:
        async with kick.feed(backoff=SlowBackoff()) as chat:
            assert not chat.retrying and chat.last_drop is None
            await until(lambda: chat.retrying)
            assert chat.last_drop is not None and chat.last_drop.wait == 60.0
            assert chat.last_drop.reason == "go away" and chat.last_drop.terminal
        assert chat.done and not chat.retrying and chat.error is None
        assert kick.connections == 1


@run
async def test_a_dropped_connection_or_a_5xx_is_retried() -> None:
    async def dropped(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        await ws.close(1012, "restarting")  # reconnectable

    async def second(ws: ServerConnection, kick: Kick) -> None:
        connect = await accept(ws)
        assert connect["connect"]["token"] != "token-1", "a new token for a new connection"
        await ws.send(chat_message(2))

    async with Kick(dropped, second) as kick:
        kick.connection_statuses = [503]
        async with kick.feed() as chat:
            assert (await next_event(chat))["event"]["content"] == "text-2"
            assert kick.connections == 2
            assert backoff_of(chat).asked == [0, 0], "the ordinary backoff"
            assert chat.last_drop is not None and (chat.last_drop.code, chat.last_drop.terminal) == (1012, False)
            assert len(set(kick.connection_clients)) == 1, "the same client, for an ordinary drop"


@run
async def test_refusals_in_a_row_wait_as_long_as_terminal_closes() -> None:
    async with Kick(stay) as kick:
        kick.handshake_status = 403
        async with kick.feed(max_refusals=3) as chat:
            await until(lambda: len(backoff_of(chat).asked) >= 5, timeout=5)
            T = realtime.TERMINAL_RETRY_MS
            assert backoff_of(chat).asked[:5] == [0, 0, T, T, T]
            drop = chat.last_drop
            assert drop is not None and drop.terminal and (drop.code, drop.status) == (0, 403)
            assert isinstance(drop.error, Exception)
            assert not chat.done and chat.error is None, "the feed goes on"
        assert kick.connections == 0
        # The same client until the third refusal, a new one after each.
        ids = kick.connection_clients
        assert ids[0] == ids[1] == ids[2] and len(set(ids[2:])) == len(ids) - 2


@run
async def test_refusals_count_only_in_a_row() -> None:
    async with Kick(stay) as kick:
        kick.connection_statuses = [403, 403, 503, 403, 403, 200]
        async with kick.feed(max_refusals=3) as chat:
            await until(lambda: CHAT_FEED in chat.subscribed)
            assert backoff_of(chat).asked == [0] * 5, "a 503 between refusals starts the count again"


@run
async def test_an_unexpected_error_is_raised_not_retried() -> None:
    async with Kick(stay) as kick:

        async def broken(n: int) -> httpx.Response:
            raise RuntimeError("a bug")

        kick.token_answer = broken
        async with kick.feed() as chat:
            with pytest.raises(RuntimeError, match="a bug"):
                await next_event(chat)
        assert kick.token_requests == 1


@run
async def test_the_iterator_ends_with_the_block() -> None:
    async with Kick(stay) as kick:
        ended = asyncio.Event()
        async with kick.feed() as chat:

            async def consume() -> None:
                async for _ in chat:
                    pass
                ended.set()

            consumer = asyncio.create_task(consume())
            await until(lambda: CHAT_FEED in chat.subscribed)
        await asyncio.wait_for(ended.wait(), 1)
        await consumer
        with pytest.raises(StopAsyncIteration):
            await next_event(chat, timeout=1)
        assert chat.done and chat.error is None


@run
async def test_leaving_the_block_closes_normally() -> None:
    async with Kick(stay) as kick:
        async with kick.feed() as chat:
            await until(lambda: CHAT_FEED in chat.subscribed)
        await until(lambda: len(kick.close_codes) == 1)
        assert kick.close_codes == [1000]


@run
async def test_a_full_buffer_drops_the_oldest_and_says_so(caplog: pytest.LogCaptureFixture) -> None:
    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        for n in range(5):
            await ws.send(chat_message(n))

    caplog.set_level(logging.WARNING, logger="kicksdk.aio")
    async with Kick(script) as kick, kick.feed(max_queue=2) as chat:
        await until(lambda: chat.dropped == 3)
        assert [(await next_event(chat))["event"]["content"] for _ in range(2)] == ["text-3", "text-4"]
    assert any("dropped" in r.message for r in caplog.records)


@run
async def test_backpressure_pauses_reading_and_drops_nothing() -> None:
    pongs: list[Any] = []

    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws)
        for n in range(5):
            await ws.send(chat_message(n))
        await ws.send("{}")
        pongs.append(await recv_json(ws, timeout=5))

    async with Kick(script) as kick, kick.feed(max_queue=2, overflow="backpressure") as chat:
        await asyncio.sleep(0.3)
        assert pongs == [], "the socket is not read while the buffer is full"
        got = [(await next_event(chat))["event"]["content"] for _ in range(5)]
        assert got == [f"text-{n}" for n in range(5)] and chat.dropped == 0
        await until(lambda: pongs == [{}])


@run
async def test_a_refresh_that_fails_after_the_fetch_is_logged_not_lost(
    monkeypatch: pytest.MonkeyPatch, caplog: pytest.LogCaptureFixture
) -> None:
    # The refresh runs as its own task; whatever goes wrong in it is logged
    # (`run` fails on "Task exception was never retrieved").
    def broken(self: realtime.Session, token: str, now: int) -> list[dict[str, Any]]:
        raise RuntimeError("refresh broke")

    monkeypatch.setattr(realtime.Session, "refresh", broken)

    async def script(ws: ServerConnection, kick: Kick) -> None:
        await accept(ws, expires=True, ttl=1)
        await asyncio.sleep(0.8)
        await ws.send(chat_message(1))

    caplog.set_level(logging.WARNING, logger="kicksdk.aio")
    async with Kick(script) as kick, kick.feed() as chat:
        assert (await next_event(chat))["event"]["content"] == "text-1"
        assert kick.token_requests == 2 and kick.connections == 1
    assert any("refresh broke" in r.getMessage() for r in caplog.records)


def test_bad_options_are_refused() -> None:
    with pytest.raises(ValueError):
        aio.connect_chat(CHATROOM, overflow="block")
    with pytest.raises(ValueError):
        aio.connect_chat(CHATROOM, max_queue=0)
    with pytest.raises(ValueError):
        aio.connect_chat(CHATROOM, max_refusals=0)


@run
async def test_iterating_outside_the_block_is_an_error() -> None:
    with pytest.raises(RuntimeError):
        await aio.connect_chat(CHATROOM).__anext__()


def test_without_the_aio_extra_the_import_says_what_to_install(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delitem(sys.modules, "kicksdk.aio")
    monkeypatch.setitem(sys.modules, "httpx", None)  # as if not installed
    with pytest.raises(ImportError, match=r'pip install "kicksdk-py\[aio\]"') as e:
        importlib.import_module("kicksdk.aio")
    assert e.value.name == "httpx"
