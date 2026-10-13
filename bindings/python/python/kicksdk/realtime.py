"""Kick's realtime chat feed (Centrifugo).

Kick's realtime server refuses a WebSocket whose Origin is not
``https://kick.com``: send :data:`REQUIRED_ORIGIN` on the handshake.
:func:`kicksdk.aio.connect_chat` does all of this.
"""

from __future__ import annotations

from typing import Mapping, Optional

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


class Session:
    """One connection's worth of Kick's Centrifugo protocol.

    Each method returns the actions to carry out, dicts with an ``action``:
    ``send`` (``text``), ``close`` (``reason``), ``refresh_token``,
    ``connected``, ``subscribed``, ``unsubscribed``, ``subscribe_failed``,
    ``event`` (``channel``, ``name``, ``data``, ``event``). Times are
    milliseconds; call :meth:`tick` when :attr:`deadline` comes.
    """

    def __init__(self, channels: list[str]) -> None:
        self._s = _native.NativeSession(list(channels))

    def _do(self, method: str, **args):
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
    """Reconnect delays in seconds: 1 doubling to 30, back to 1 once connected."""

    def __init__(self, initial: float = 1.0, maximum: float = 30.0) -> None:
        self.initial, self.maximum, self._next = initial, maximum, initial

    def next(self) -> float:
        d, self._next = self._next, min(self._next * 2, self.maximum)
        return d

    def reset(self) -> None:
        self._next = self.initial
