"""Kick's official webhooks: RSA signature verification and typed bodies."""

from __future__ import annotations

from typing import Iterable, Mapping, Union

from . import _call

Headers = Union[Mapping[str, str], Iterable[tuple[str, str]]]


def verify(public_key_pem: str, headers: Headers, body: str) -> dict:
    """Verifies and reads a delivery: ``{"headers": ..., "event": ...}``.
    Raises :class:`kicksdk.KickError` (kind ``webhook``) if it does not verify."""
    pairs = list(headers.items()) if isinstance(headers, Mapping) else list(headers)
    return _call("webhooks.verify", public_key_pem=public_key_pem, headers=[[k, v] for k, v in pairs], body=body)


def parse(event_type: str, body: str) -> dict:
    """Reads a body by its type header, without checking anything."""
    return _call("webhooks.parse", event_type=event_type, body=body)
