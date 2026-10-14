"""Kick's realtime chat feed (Centrifugo).

Kick's realtime server refuses a WebSocket whose Origin is not
``https://kick.com``: send :data:`REQUIRED_ORIGIN` on the handshake.
:func:`kicksdk.aio.connect_chat` does all of this.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

from . import KickError, Request, _call, _native, _request

REQUIRED_ORIGIN = "https://kick.com"


def chat_channels(chatroom_id: int, channel_id: Optional[int] = None) -> list[str]:
    """The feeds for one chat, the chat feed (``chatrooms.<id>.v2``) first."""
    return _call("realtime.chat_channels", chatroom_id=chatroom_id, channel_id=channel_id)


def chat_connection_request(
    channel_id: int, client_id: str, endpoints: Optional[Mapping[str, str]] = None
) -> Request:
    """Where a channel's chat is served (each channel's chat has a region)."""
    return _request("realtime.chat_connection_request", channel_id=channel_id, client_id=client_id, endpoints=endpoints)


def connection_request(client_id: str, endpoints: Optional[Mapping[str, str]] = None) -> Request:
    return _request("realtime.connection_request", client_id=client_id, endpoints=endpoints)


def parse_connection(status: int, body: str) -> list[dict]:
    """``[{"provider": "centrifugo", "url": ...}]``"""
    return _call("realtime.parse_connection", status=status, body=body)


def token_request(client_id: str, endpoints: Optional[Mapping[str, str]] = None) -> Request:
    """A guest token (45 minutes) for ``client_id``; refresh it for the same id."""
    return _request("realtime.token_request", client_id=client_id, endpoints=endpoints)


def parse_token(status: int, body: str) -> str:
    return _call("realtime.parse_token", status=status, body=body)


def retry_after_ms(code: int) -> int:
    """The least to wait, in ms, before reconnecting after the socket closed
    with this WebSocket close code: 300000 (5 minutes) for Centrifugo's
    terminal codes (3500-3999 and 4500-4999: a bad token, a ban, a refused
    request; reconnect after the wait with a new client id), 0 (the ordinary
    backoff) otherwise (a network drop is 1006). For the code the socket
    reports; a session's ``close`` action already carries it in
    ``retry_after_ms``."""
    return _call("realtime.retry_after_ms", code=code)


#: The least wait, in ms, after a terminal close (a ban, a refused token, a
#: connection limit): 300000, 5 minutes. :meth:`Backoff.next_delay_for`
#: lengthens it for terminal closes in a row.
TERMINAL_RETRY_MS: int = retry_after_ms(3500)


class Session:
    """One connection's worth of Kick's Centrifugo protocol.

    Each method returns the actions to carry out, dicts with an ``action``:

    - ``send`` (``text``): send this frame;
    - ``close`` (``code``, ``reason``, ``terminal``, ``retry_after_ms``):
      close the socket, call :meth:`closed`, wait
      ``backoff.next_delay_for(retry_after_ms, random.random())`` ms and open
      a new connection with a new token. ``terminal`` (Kick refused this
      client: a ban, a bad token, a limit) asks for a **new client id** too,
      and comes with ``retry_after_ms`` :data:`TERMINAL_RETRY_MS`; it is
      never a reason to stop for good (KICK.md §6.1). ``code`` is Kick's
      disconnect code, Kick's error code for a refused connect or refresh,
      or 0 when the session gave up (silence);
    - ``refresh_token``: fetch a token for the same client id and pass it to
      :meth:`refresh`. If that fails, carry on: Kick ends the connection when
      the old token runs out, and the reconnect gets a new one;
    - ``connected`` (``client``), ``subscribed`` (``channel``),
      ``unsubscribed`` (``channel``, ``code``, ``reason``),
      ``subscribe_failed`` (``channel``, ``code``, ``message``);
    - ``event`` (``channel``, ``name``, ``data``, ``event``).

    Times are milliseconds; call :meth:`tick` when :attr:`deadline` comes.
    """

    def __init__(self, channels: list[str]) -> None:
        self._s = _native.NativeSession(list(channels))

    def _do(self, method: str, **args: Any) -> Any:
        try:
            return self._s.call(method, args)
        except _native.NativeError as e:
            raise KickError(*e.args) from None

    def open(self, token: str, now: int) -> list[dict]:
        return self._do("open", token=token, now=now)

    def receive(self, text: str, now: int) -> list[dict]:
        return self._do("receive", text=text, now=now)

    def refresh(self, token: str, now: int) -> list[dict]:
        return self._do("refresh", token=token, now=now)

    def tick(self, now: int) -> list[dict]:
        return self._do("tick", now=now)

    def closed(self) -> list[dict]:
        return self._do("closed")

    @property
    def deadline(self) -> Optional[int]:
        return self._do("deadline")

    def is_subscribed(self, channel: str) -> bool:
        return self._do("is_subscribed", channel=channel)

    @property
    def is_connected(self) -> bool:
        return self._do("is_connected")


class Backoff:
    """Reconnect delays: 1 s doubling to 30 s, back to 1 s once a connection
    has worked (call :meth:`reset` then). The core's arithmetic: a first
    wait under 1 ms counts as 1 ms, and a maximum below it is raised to it.

    Many clients dropped at once (a server restart) would all come back in
    step; :meth:`next_delay_jittered` spreads them::

        await asyncio.sleep(backoff.next_delay_jittered(random.random()) / 1000)

    After a close, :meth:`next_delay_for` takes the close's
    ``retry_after_ms`` and keeps a second, slower stage for terminal closes
    (5, 10, 20, then 30 minutes).
    """

    def __init__(self, initial: float = 1.0, maximum: float = 30.0) -> None:
        """``initial`` and ``maximum`` in seconds."""
        self._b = _native.NativeBackoff(_ms(initial), _ms(maximum))

    @property
    def initial(self) -> float:
        """The first wait, seconds. Setting it takes effect at once as the
        shortest wait, and as the first one after :meth:`reset`."""
        return self._b.initial_ms / 1000

    @initial.setter
    def initial(self, seconds: float) -> None:
        self._b.initial_ms = _ms(seconds)

    @property
    def maximum(self) -> float:
        """The longest wait, seconds (of the ordinary stage). Setting it
        takes effect at once."""
        return self._b.max_ms / 1000

    @maximum.setter
    def maximum(self, seconds: float) -> None:
        self._b.max_ms = _ms(seconds)

    def next(self) -> float:
        """The next wait in seconds, without jitter; the one after is longer."""
        return self._b.next_delay() / 1000

    def next_delay(self) -> int:
        """The next wait in milliseconds, without jitter."""
        return self._b.next_delay()

    def next_delay_jittered(self, random: float) -> int:
        """The next wait in milliseconds, spread over [half, all] of it by
        ``random``, a number drawn uniformly from [0, 1) (``random.random()``).
        Out of range is clamped. Never less than 1 ms."""
        return self._b.next_delay_jittered(float(random))

    def next_delay_for(self, retry_after_ms: int, random: float) -> int:
        """The wait in milliseconds after a close that asks for at least
        ``retry_after_ms`` (a session ``close`` action's, or
        :func:`retry_after_ms` of the socket's close code), spread by
        ``random`` from [0, 1).

        0 (an ordinary drop) is :meth:`next_delay_jittered`. More (a
        terminal close) is ``retry_after_ms`` doubled for each terminal close
        before it since the last :meth:`reset`, at most six times it, plus
        ``random`` times a quarter of that: 300000, 600000, 1200000, 1800000,
        1800000 … ms from Kick's 5 minutes and ``random`` 0. The ordinary
        stage is left as it was."""
        if isinstance(retry_after_ms, bool) or not isinstance(retry_after_ms, int):
            raise TypeError("retry_after_ms must be an int")
        if retry_after_ms < 0:
            raise ValueError(f"retry_after_ms must be 0 or more, not {retry_after_ms}")
        return self._b.next_delay_for(retry_after_ms, float(random))

    def reset(self) -> None:
        """The chat feed is subscribed: the next failure starts from the
        beginning, and so does the next terminal close."""
        self._b.reset()


def _ms(seconds: float) -> int:
    # Negative or NaN is 0, which the core raises to 1 ms; too long is the
    # longest the core holds.
    ms = seconds * 1000
    if not ms > 0:
        return 0
    return min(int(ms), 2**64 - 1) if ms < float("inf") else 2**64 - 1
