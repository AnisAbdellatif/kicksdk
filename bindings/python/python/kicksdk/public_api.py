"""Kick's official API (``api.kick.com/public/v1``). Batches are 50 ids; one
unknown slug fails a whole ``/channels`` request."""

from __future__ import annotations

from typing import Mapping, Optional

from . import Request, _call, _request, _requests

E = Optional[Mapping[str, str]]


def token_request(client_id: str, client_secret: str, endpoints: E = None) -> Request:
    return _request("public_api.token_request", client_id=client_id, client_secret=client_secret, endpoints=endpoints)


def parse_token(status: int, body: str) -> dict:
    return _call("public_api.parse_token", status=status, body=body)


def livestreams_requests(token: str, user_ids: list[int], endpoints: E = None) -> list[Request]:
    """One request per 50 user ids."""
    return _requests("public_api.livestreams_requests", token=token, user_ids=list(user_ids), endpoints=endpoints)


def top_livestreams_request(
    token: str,
    category_id: Optional[int] = None,
    language: Optional[str] = None,
    sort: str = "viewer_count",
    limit: int = 100,
    endpoints: E = None,
) -> Request:
    """The top live streams, overall or in a category or language: up to 100.
    ``sort`` is ``viewer_count`` or ``started_at``."""
    return _request(
        "public_api.top_livestreams_request",
        token=token,
        category_id=category_id,
        language=language,
        sort=sort,
        limit=limit,
        endpoints=endpoints,
    )


def parse_livestreams(status: int, body: str) -> list[dict]:
    return _call("public_api.parse_livestreams", status=status, body=body)


def channels_by_ids_request(token: str, user_ids: list[int], endpoints: E = None) -> Request:
    return _request("public_api.channels_by_ids_request", token=token, user_ids=list(user_ids), endpoints=endpoints)


def channels_by_slugs_request(token: str, slugs: list[str], endpoints: E = None) -> Request:
    return _request("public_api.channels_by_slugs_request", token=token, slugs=list(slugs), endpoints=endpoints)


def parse_channels(status: int, body: str) -> list[dict]:
    return _call("public_api.parse_channels", status=status, body=body)


def public_key_request(endpoints: E = None) -> Request:
    return _request("public_api.public_key_request", endpoints=endpoints)


def parse_public_key(status: int, body: str) -> str:
    return _call("public_api.parse_public_key", status=status, body=body)


def subscribe_request(token: str, broadcaster_user_id: int, events: list[tuple[str, int]], endpoints: E = None) -> Request:
    return _request(
        "public_api.subscribe_request",
        token=token,
        broadcaster_user_id=broadcaster_user_id,
        events=[{"name": n, "version": v} for n, v in events],
        endpoints=endpoints,
    )


def subscriptions_request(token: str, endpoints: E = None) -> Request:
    return _request("public_api.subscriptions_request", token=token, endpoints=endpoints)


def parse_subscriptions(status: int, body: str) -> list[dict]:
    return _call("public_api.parse_subscriptions", status=status, body=body)


def unsubscribe_requests(token: str, ids: list[str], endpoints: E = None) -> list[Request]:
    return _requests("public_api.unsubscribe_requests", token=token, ids=list(ids), endpoints=endpoints)
