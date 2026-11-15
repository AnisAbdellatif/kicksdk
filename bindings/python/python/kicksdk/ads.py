"""Ads on the token's own channel (``ads:read``, ``ads:write``)."""

from __future__ import annotations

from . import Request, _call, _request
from .public_api import E


def ad_break_request(token: str, id: str, seconds: int, endpoints: E = None) -> Request:
    """Starts an ad break of 7 to 90 seconds; ``id`` is the caller's, to tell
    a retry from a new break."""
    return _request("ads.ad_break_request", token=token, id=id, seconds=seconds, endpoints=endpoints)


def parse_ad_break(status: int, body: str) -> dict:
    """``{"id", "ad_break_id", "remaining_ad_breaks"}``"""
    return _call("ads.parse_ad_break", status=status, body=body)


def status_request(token: str, endpoints: E = None) -> Request:
    return _request("ads.status_request", token=token, endpoints=endpoints)


def parse_status(status: int, body: str) -> dict:
    return _call("ads.parse_status", status=status, body=body)


def enroll_request(token: str, endpoints: E = None) -> Request:
    """Read the answer with :func:`kicksdk.public_api.parse_done`."""
    return _request("ads.enroll_request", token=token, endpoints=endpoints)
