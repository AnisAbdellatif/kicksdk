"""kick.com's own endpoints (``kick.com/api/v2``): undocumented, a grey area
under Kick's terms. From a server, ``User-Agent: Mozilla/5.0`` passes where a
full browser one is refused."""

from __future__ import annotations

from typing import Any, Mapping, Optional

from . import Request, _call, _request

E = Optional[Mapping[str, str]]


def channel_request(slug: str, endpoints: E = None) -> Request:
    return _request("website.channel_request", slug=slug, endpoints=endpoints)


def parse_channel(status: int, body: str) -> dict:
    """Chatroom id, follower total, live state."""
    return _call("website.parse_channel", status=status, body=body)


def history_request(channel_id: int, endpoints: E = None) -> Request:
    return _request("website.history_request", channel_id=channel_id, endpoints=endpoints)


def parse_history(status: int, body: str) -> dict:
    """``{"messages": [...], "pinned": event | None}``"""
    return _call("website.parse_history", status=status, body=body)


def channel_user_request(channel_slug: str, username: str, endpoints: E = None) -> Request:
    return _request("website.channel_user_request", channel_slug=channel_slug, username=username, endpoints=endpoints)


def parse_channel_user(status: int, body: str) -> dict:
    return _call("website.parse_channel_user", status=status, body=body)


def me_request(session: str, channel_slug: str, endpoints: E = None) -> Request:
    return _request("website.me_request", session=session, channel_slug=channel_slug, endpoints=endpoints)


def parse_me(status: int, body: str) -> dict:
    return _call("website.parse_me", status=status, body=body)


def ban_request(session: str, channel_slug: str, username: str, endpoints: E = None) -> Request:
    return _request("website.ban_request", session=session, channel_slug=channel_slug, username=username, endpoints=endpoints)


def timeout_request(session: str, channel_slug: str, username: str, minutes: int, endpoints: E = None) -> Request:
    return _request(
        "website.timeout_request",
        session=session,
        channel_slug=channel_slug,
        username=username,
        minutes=minutes,
        endpoints=endpoints,
    )


def unban_request(session: str, channel_slug: str, username: str, endpoints: E = None) -> Request:
    return _request("website.unban_request", session=session, channel_slug=channel_slug, username=username, endpoints=endpoints)


def delete_message_request(session: str, chatroom_id: int, message_id: str, endpoints: E = None) -> Request:
    return _request(
        "website.delete_message_request", session=session, chatroom_id=chatroom_id, message_id=message_id, endpoints=endpoints
    )


def pin_request(session: str, channel_slug: str, message: Any, minutes: int, endpoints: E = None) -> Request:
    return _request(
        "website.pin_request", session=session, channel_slug=channel_slug, message=message, minutes=minutes, endpoints=endpoints
    )


def unpin_request(session: str, channel_slug: str, endpoints: E = None) -> Request:
    return _request("website.unpin_request", session=session, channel_slug=channel_slug, endpoints=endpoints)


def send_request(
    session: str,
    chatroom_id: int,
    content: str,
    message_ref: str,
    reply_to: Optional[Mapping[str, Any]] = None,
    endpoints: E = None,
) -> Request:
    """A message, or a reply when ``reply_to`` is
    ``{"message_id", "content", "sender_id", "sender_username"}``."""
    return _request(
        "website.send_request",
        session=session,
        chatroom_id=chatroom_id,
        content=content,
        message_ref=message_ref,
        reply_to=dict(reply_to) if reply_to else None,
        endpoints=endpoints,
    )


def parse_action(status: int, body: str) -> None:
    _call("website.parse_action", status=status, body=body)
