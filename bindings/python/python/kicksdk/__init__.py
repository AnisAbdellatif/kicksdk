"""Kick (kick.com) without the I/O.

kicksdk knows Kick - which requests to make, what the answers look like, how
the realtime chat feed speaks, how webhooks are signed - and never sends a
request or opens a socket itself. Send the :class:`Request` objects with any
HTTP client and hand the status and body back to the matching ``parse_*``.
For a ready chat connection over asyncio, see :mod:`kicksdk.aio`.

Values come back Kick-shaped: dicts with snake_case keys, as Kick writes them.

Modules: :mod:`kicksdk.events`, :mod:`kicksdk.content`,
:mod:`kicksdk.realtime`, :mod:`kicksdk.public_api`, :mod:`kicksdk.website`,
:mod:`kicksdk.webhooks`; with a user token (:mod:`kicksdk.oauth`),
:mod:`kicksdk.chat`, :mod:`kicksdk.moderation`, :mod:`kicksdk.channels`,
:mod:`kicksdk.rewards`, :mod:`kicksdk.ads` and :mod:`kicksdk.drops`.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping, Optional

from . import _native

__all__ = [
    "KickError",
    "Request",
    "events",
    "content",
    "realtime",
    "public_api",
    "website",
    "webhooks",
    "oauth",
    "chat",
    "moderation",
    "channels",
    "rewards",
    "ads",
    "drops",
]


class KickError(Exception):
    """Kick answered with an error, or a call could not be made.

    ``kind`` is ``status`` (Kick's HTTP status is in ``status``), ``shape``,
    ``json``, ``invalid``, ``webhook``, ``call`` or ``panic`` (a bug in
    kicksdk; a realtime session it happened in refuses every call until
    :meth:`kicksdk.realtime.Session.closed`).
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

    def __repr__(self) -> str:
        # Safe to log: the same secrets hidden as the core's Debug (bearer
        # tokens, cookies, a form's client_secret/refresh_token/code/
        # code_verifier/password, a `token` in the query). The fields keep
        # the real values, for sending.
        headers = [(k, _REDACTED if k.lower() in _SECRET_HEADERS else v) for k, v in self.headers]
        form = any(
            k.lower() == "content-type" and v.startswith("application/x-www-form-urlencoded")
            for k, v in self.headers
        )
        body = _redact_pairs(self.body, _SECRET_FIELDS) if form and self.body is not None else self.body
        return f"Request(method={self.method!r}, url={_redact_url(self.url)!r}, headers={headers!r}, body={body!r})"


_REDACTED = "[redacted]"
_SECRET_HEADERS = ("authorization", "cookie", "x-csrf-token")
_SECRET_FIELDS = ("client_secret", "refresh_token", "code", "code_verifier", "password")


def _redact_pairs(pairs: str, secret: tuple[str, ...]) -> str:
    out = []
    for pair in pairs.split("&"):
        key, eq, _ = pair.partition("=")
        out.append(f"{key}={_REDACTED}" if eq and key in secret else pair)
    return "&".join(out)


def _redact_url(url: str) -> str:
    url, hash_, fragment = url.partition("#")
    path, q, query = url.partition("?")
    out = f"{path}?{_redact_pairs(query, ('token',))}" if q else path
    return out + hash_ + fragment


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
from . import ads, channels, chat, drops, moderation, oauth, rewards  # noqa: E402
