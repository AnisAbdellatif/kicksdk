"""Kick's chat-feed events, typed: ``{"type": "chat_message", ...}``.

An event kicksdk does not know comes back as ``{"type": "unknown", "name":
name}`` rather than an error.
"""

from typing import Any

from . import _call


def parse(name: str, data: Any) -> dict:
    """Reads one event: its name and its ``data`` as Kick sent it."""
    return _call("events.parse", name=name, data=data)
