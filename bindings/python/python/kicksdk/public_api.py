"""Kick's official API (``api.kick.com/public/v1`` and ``/public/v2``).
Batches are 50 ids (100 for live streams by user id); one unknown slug fails a
whole ``/channels`` request."""

from __future__ import annotations

from typing import Mapping, Optional

from . import Request, _call, _request, _requests

E = Optional[Mapping[str, str]]


def token_request(client_id: str, client_secret: str, endpoints: E = None) -> Request:
    return _request("public_api.token_request", client_id=client_id, client_secret=client_secret, endpoints=endpoints)


def parse_token(status: int, body: str) -> dict:
    return _call("public_api.parse_token", status=status, body=body)


def livestreams_requests(token: str, user_ids: list[int], endpoints: E = None) -> list[Request]:
    """``/public/v1/users/livestreams``: one request per 100 user ids."""
    return _requests("public_api.livestreams_requests", token=token, user_ids=list(user_ids), endpoints=endpoints)


def livestreams_page_request(
    token: str,
    category_ids: Optional[list[int]] = None,
    language_codes: Optional[list[str]] = None,
    limit: Optional[int] = None,
    cursor: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """One page of the streams live now (``/public/v2/livestreams``), in up to
    25 categories and 25 languages (none: all). ``limit`` is 1 to 1000;
    ``cursor`` is the previous page's ``next_cursor``. Kick has no sort:
    streams come oldest first."""
    return _request(
        "public_api.livestreams_page_request",
        token=token,
        category_ids=None if category_ids is None else list(category_ids),
        language_codes=None if language_codes is None else list(language_codes),
        limit=limit,
        cursor=cursor,
        endpoints=endpoints,
    )


def parse_livestreams(status: int, body: str) -> list[dict]:
    return _call("public_api.parse_livestreams", status=status, body=body)


def parse_livestreams_page(status: int, body: str) -> dict:
    """A ``livestreams_page_request`` answer: ``{"data": [...], "next_cursor": ...}``."""
    return _call("public_api.parse_livestreams_page", status=status, body=body)


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


def users_request(token: str, user_ids: Optional[list[int]] = None, endpoints: E = None) -> Request:
    """Up to 50 users by id; none, with a user token: the token's own user."""
    return _request(
        "public_api.users_request",
        token=token,
        user_ids=None if user_ids is None else list(user_ids),
        endpoints=endpoints,
    )


def parse_users(status: int, body: str) -> list[dict]:
    """``[{"user_id", "name", "profile_picture", "email"}]``"""
    return _call("public_api.parse_users", status=status, body=body)


def categories_request(
    token: str,
    names: Optional[list[str]] = None,
    tags: Optional[list[str]] = None,
    ids: Optional[list[int]] = None,
    limit: Optional[int] = None,
    cursor: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """One page of categories (``/public/v2/categories``), by names and tags
    or by ids (none: all). ``limit`` is 1 to 1000."""
    return _request(
        "public_api.categories_request",
        token=token,
        names=None if names is None else list(names),
        tags=None if tags is None else list(tags),
        ids=None if ids is None else list(ids),
        limit=limit,
        cursor=cursor,
        endpoints=endpoints,
    )


def parse_categories(status: int, body: str) -> dict:
    """``{"data": [...], "next_cursor": ...}``"""
    return _call("public_api.parse_categories", status=status, body=body)


def livestream_stats_request(token: str, endpoints: E = None) -> Request:
    """How many streams are live on Kick."""
    return _request("public_api.livestream_stats_request", token=token, endpoints=endpoints)


def parse_livestream_stats(status: int, body: str) -> dict:
    """``{"total_count"}``"""
    return _call("public_api.parse_livestream_stats", status=status, body=body)


def kicks_leaderboard_request(token: str, top: Optional[int] = None, endpoints: E = None) -> Request:
    """The top 1 to 100 Kicks gifters of the token's own channel (``kicks:read``)."""
    return _request("public_api.kicks_leaderboard_request", token=token, top=top, endpoints=endpoints)


def parse_kicks_leaderboard(status: int, body: str) -> dict:
    """``{"lifetime", "month", "week"}``, each ``[{"user_id", "username", "rank", "gifted_amount"}]``"""
    return _call("public_api.parse_kicks_leaderboard", status=status, body=body)


def parse_done(status: int, body: str) -> None:
    """Reads an answer that carries nothing (a delete, a ban, an update):
    raises :class:`kicksdk.KickError` unless Kick says it is done."""
    _call("public_api.parse_done", status=status, body=body)
