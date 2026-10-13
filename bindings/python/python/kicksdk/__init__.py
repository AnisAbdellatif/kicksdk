"""Kick (kick.com) without the I/O.

kicksdk knows Kick - which requests to make, what the answers look like, how
the realtime chat feed speaks, how webhooks are signed - and never sends a
request or opens a socket itself. Send the :class:`Request` objects with any
HTTP client and hand the status and body back to the matching ``parse_*``.
For a ready chat connection over asyncio, see :mod:`kicksdk.aio`.

Values come back Kick-shaped: dicts with snake_case keys, as Kick writes them.

Modules: :mod:`kicksdk.events`, :mod:`kicksdk.content`,
:mod:`kicksdk.realtime`, :mod:`kicksdk.public_api`, :mod:`kicksdk.website`,
:mod:`kicksdk.webhooks`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping, Optional

from . import _native

__all__ = ["KickError", "Request", "events", "content", "realtime", "public_api", "website", "webhooks"]


class KickError(Exception):
    """Kick answered with an error, or a call could not be made.

    ``kind`` is ``status`` (Kick's HTTP status is in ``status``), ``shape``,
    ``json``, ``invalid``, ``webhook`` or ``call``.
    """

    def __init__(self, kind: str, status: Optional[int], message: str) -> None:
        super().__init__(message)
        self.kind = kind
        self.status = status
        self.message = message

    def __repr__(self) -> str:
        return f"KickError({self.kind!r}, {self.status!r}, {self.message!r})"


@dataclass(frozen=True)
class Request:
    """A request to send with any HTTP client."""

    method: str
    url: str
    headers: list[tuple[str, str]]
    body: Optional[str]

    @classmethod
    def _from(cls, v: Mapping[str, Any]) -> "Request":
        return cls(v["method"], v["url"], [(k, val) for k, val in v["headers"]], v.get("body"))


def _call(_fn: str, /, endpoints: Optional[Mapping[str, str]] = None, **args: Any) -> Any:
    # Positional-only, so a function's own `name` argument (events.parse) is
    # never taken for the function's name.
    if endpoints:
        args["endpoints"] = dict(endpoints)
    try:
        return _native.call(_fn, args)
    except _native.NativeError as e:
        kind, status, message = e.args
        raise KickError(kind, status, message) from None


def _request(_fn: str, /, **kw: Any) -> Request:
    return Request._from(_call(_fn, **kw))


def _requests(_fn: str, /, **kw: Any) -> list[Request]:
    return [Request._from(r) for r in _call(_fn, **kw)]


from . import content, events, public_api, realtime, webhooks, website  # noqa: E402
