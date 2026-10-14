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


def should_reconnect(code: int) -> bool:
    """Whether a connection that closed with this WebSocket close code is
    worth opening again: false for Centrifugo's terminal codes (3500-3999 and
    4500-4999: a bad token, a ban, a refused request), true otherwise (a
    network drop is 1006). For the code the socket reports; a session's
    ``close`` action already carries the answer in ``reconnect``."""
    return _call("realtime.should_reconnect", code=code)


class Session:
    """One connection's worth of Kick's Centrifugo protocol.

    Each method returns the actions to carry out, dicts with an ``action``:

    - ``send`` (``text``): send this frame;
    - ``close`` (``code``, ``reason``, ``reconnect``): close the socket and
      call :meth:`closed`; open a new connection (with a new token, after a
      :class:`Backoff` delay) only if ``reconnect`` is true. ``code`` is
      Kick's disconnect code, Kick's error code for a refused connect or
      refresh, or 0 when the session gave up (silence);
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
    """

    def __init__(self, initial: float = 1.0, maximum: float = 30.0) -> None:
        """``initial`` and ``maximum`` in seconds."""
        self._b = _native.NativeBackoff(_ms(initial), _ms(maximum))

    @property
    def initial(self) -> float:
        """The first wait, seconds."""
        return self._b.initial_ms / 1000

    @property
    def maximum(self) -> float:
        """The longest wait, seconds."""
        return self._b.max_ms / 1000

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

    def reset(self) -> None:
        """A connection worked: the next failure starts from the beginning."""
        self._b.reset()


def _ms(seconds: float) -> int:
    # Negative or NaN is 0, which the core raises to 1 ms; too long is the
    # longest the core holds.
    ms = seconds * 1000
    if not ms > 0:
        return 0
    return min(int(ms), 2**64 - 1) if ms < float("inf") else 2**64 - 1
