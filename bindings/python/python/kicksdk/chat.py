"""Chat with a user token (``chat:write``, ``moderation:chat_message:manage``)."""

from __future__ import annotations

from typing import Literal, Optional, Union

from . import Request, _call, _request
from .public_api import E


def send_request(
    token: str,
    sender: Union[int, Literal["bot"]],
    content: str,
    reply_to_message_id: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """Sends ``content`` (at most 500 characters) as the token's user into
    the chat of ``sender``, a broadcaster's user id, or with ``sender="bot"``
    as the app's bot into the token's own channel."""
    return _request(
        "chat.send_request",
        token=token,
        sender=sender if sender == "bot" else {"user": sender},
        content=content,
        reply_to_message_id=reply_to_message_id,
        endpoints=endpoints,
    )


def parse_send(status: int, body: str) -> dict:
    """``{"is_sent", "message_id"}``"""
    return _call("chat.parse_send", status=status, body=body)


def delete_request(token: str, message_id: str, endpoints: E = None) -> Request:
    """Read the answer with :func:`kicksdk.public_api.parse_done`."""
    return _request("chat.delete_request", token=token, message_id=message_id, endpoints=endpoints)
