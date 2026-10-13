"""Chat message text, with Kick's ``[emote:id:name]`` tokens."""

from . import _call


def parse(text: str, combine: bool = False) -> list[dict]:
    """Splits text into ``text`` and ``emote`` parts; ``combine`` counts runs."""
    return _call("content.parse", text=text, combine=combine)


def emote_url(id: str) -> str:
    """An emote's image."""
    return _call("content.emote_url", id=id)
