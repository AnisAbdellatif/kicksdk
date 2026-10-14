"""Kick's official webhooks: RSA signature verification and typed bodies.

A valid signature means Kick sent this body to *some* app: every app shares
Kick's key. :func:`verify` also refuses a delivery whose timestamp is more
than ``tolerance_secs`` (5 minutes) from now, so an old one cannot be
replayed; deduplicate on ``headers["message_id"]`` as well (Kick retries).
"""

from __future__ import annotations

import base64
import time
from typing import Any, Iterable, Mapping, Optional, Union

from . import _call

Headers = Union[Mapping[str, str], Iterable[tuple[str, str]]]
Body = Union[str, bytes, bytearray, memoryview]


def verify(
    public_key_pem: str,
    headers: Headers,
    body: Body,
    *,
    now: Optional[float] = None,
    tolerance_secs: Optional[int] = None,
    skip_freshness: bool = False,
) -> dict[str, Any]:
    """Verifies and reads a delivery: ``{"headers": ..., "event": ...}``.

    ``body`` is the raw body: pass the bytes as received (``bytes`` are
    verified byte for byte, UTF-8 or not) or the text. The delivery's
    timestamp must be within ``tolerance_secs`` (default 300) of ``now``,
    Unix seconds, which defaults to this machine's clock (``time.time()``).
    ``skip_freshness=True`` checks the signature alone, to re-check a stored
    delivery on purpose; it cannot be combined with ``now``.

    Raises :class:`kicksdk.KickError`: kind ``webhook`` if it does not
    verify or is stale, ``call`` for arguments that do not fit.
    """
    pairs = list(headers.items()) if isinstance(headers, Mapping) else list(headers)
    args: dict[str, Any] = {"public_key_pem": public_key_pem, "headers": [[k, v] for k, v in pairs]}
    args.update(_body(body))
    if skip_freshness:
        if now is not None or tolerance_secs is not None:
            raise ValueError("skip_freshness=True checks no time: leave out now and tolerance_secs")
        args["skip_freshness"] = True
    else:
        # The core takes whole seconds; a clock reading's fraction is noise.
        args["now"] = int(time.time() if now is None else now)
        if tolerance_secs is not None:
            args["tolerance_secs"] = tolerance_secs
    out: dict[str, Any] = _call("webhooks.verify", **args)
    return out


def parse(event_type: str, body: Body) -> dict[str, Any]:
    """Reads a body by its type header, without checking its signature.
    Raises :class:`kicksdk.KickError` (kind ``shape``) for a body that is not
    that type's shape: the type header is not signed."""
    out: dict[str, Any] = _call("webhooks.parse", event_type=event_type, **_body(body))
    return out


def _body(body: Body) -> dict[str, Any]:
    if isinstance(body, str):
        return {"body": body}
    if isinstance(body, (bytes, bytearray, memoryview)):
        return {"body_base64": base64.b64encode(bytes(body)).decode("ascii")}
    raise TypeError(f"body must be str or bytes, not {type(body).__name__}")
