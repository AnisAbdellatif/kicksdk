"""Drops claims, for the app that runs a drops campaign."""

from __future__ import annotations

from typing import Any, Mapping, Optional

from . import Request, _call, _request
from .public_api import E


def claims_request(token: str, filter: Optional[Mapping[str, Any]] = None, endpoints: E = None) -> Request:
    """Lists claims, by what ``filter`` gives of ``campaign_id``, ``claim_id``,
    ``user_id``, ``external_status``, ``limit`` (1 to 1000) and ``cursor``
    (a page's ``cursor``); an unknown key is a :class:`kicksdk.KickError`
    (kind ``call``)."""
    return _request("drops.claims_request", token=token, filter=None if filter is None else dict(filter), endpoints=endpoints)


def parse_claims(status: int, body: str) -> dict:
    """``{"claims": [...], "cursor"}``"""
    return _call("drops.parse_claims", status=status, body=body)


def update_claims_request(token: str, updates: list[tuple[str, str]], endpoints: E = None) -> Request:
    """Sets claims' ``external_status``: 1 to 100 ``(claim_id, external_status)``
    pairs. Read the answer with :func:`kicksdk.public_api.parse_done`."""
    return _request(
        "drops.update_claims_request",
        token=token,
        updates=[{"claim_id": c, "external_status": s} for c, s in updates],
        endpoints=endpoints,
    )
