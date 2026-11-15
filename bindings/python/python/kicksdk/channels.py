"""The token's own channel (``channel:write``)."""

from __future__ import annotations

from typing import Optional

from . import Request, _request
from .public_api import E


def update_request(
    token: str,
    category_id: Optional[int] = None,
    custom_tags: Optional[list[str]] = None,
    stream_title: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """Sets what is given of the stream's category, custom tags (``[]`` clears
    them; at most 10) and title; what is ``None`` stays. Read the answer with
    :func:`kicksdk.public_api.parse_done`."""
    return _request(
        "channels.update_request",
        token=token,
        category_id=category_id,
        custom_tags=None if custom_tags is None else list(custom_tags),
        stream_title=stream_title,
        endpoints=endpoints,
    )
