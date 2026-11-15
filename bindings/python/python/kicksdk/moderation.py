"""Bans and timeouts with a user token (``moderation:ban``). Read the answers
with :func:`kicksdk.public_api.parse_done`."""

from __future__ import annotations

from typing import Optional

from . import Request, _request
from .public_api import E


def ban_request(
    token: str, broadcaster_user_id: int, user_id: int, reason: Optional[str] = None, endpoints: E = None
) -> Request:
    """Bans ``user_id`` from the chat for good; ``reason`` is at most 100 characters."""
    return _request(
        "moderation.ban_request",
        token=token,
        broadcaster_user_id=broadcaster_user_id,
        user_id=user_id,
        reason=reason,
        endpoints=endpoints,
    )


def timeout_request(
    token: str,
    broadcaster_user_id: int,
    user_id: int,
    minutes: int,
    reason: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """Times ``user_id`` out for 1 to 10 080 minutes (a week)."""
    return _request(
        "moderation.timeout_request",
        token=token,
        broadcaster_user_id=broadcaster_user_id,
        user_id=user_id,
        minutes=minutes,
        reason=reason,
        endpoints=endpoints,
    )


def unban_request(token: str, broadcaster_user_id: int, user_id: int, endpoints: E = None) -> Request:
    """Lifts a ban or a timeout."""
    return _request(
        "moderation.unban_request",
        token=token,
        broadcaster_user_id=broadcaster_user_id,
        user_id=user_id,
        endpoints=endpoints,
    )
