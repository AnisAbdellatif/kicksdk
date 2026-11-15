"""Channel-points rewards and their redemptions (``channel:rewards:read``,
``channel:rewards:write``).

``fields`` is a dict of ``title``, ``cost``, ``description``,
``background_color``, ``is_enabled``, ``is_paused``,
``is_user_input_required`` and ``should_redemptions_skip_request_queue``; an
unknown key is a :class:`kicksdk.KickError` (kind ``call``)."""

from __future__ import annotations

from typing import Any, Literal, Mapping, Optional

from . import Request, _call, _request
from .public_api import E


def list_request(token: str, endpoints: E = None) -> Request:
    return _request("rewards.list_request", token=token, endpoints=endpoints)


def parse_rewards(status: int, body: str) -> list[dict]:
    return _call("rewards.parse_rewards", status=status, body=body)


def create_request(token: str, fields: Mapping[str, Any], endpoints: E = None) -> Request:
    """A new reward: ``title`` and ``cost`` are needed. Read the answer with
    :func:`parse_reward`."""
    return _request("rewards.create_request", token=token, fields=dict(fields), endpoints=endpoints)


def update_request(token: str, id: str, fields: Mapping[str, Any], endpoints: E = None) -> Request:
    """Changes what ``fields`` gives of reward ``id``. Read the answer with
    :func:`parse_reward`."""
    return _request("rewards.update_request", token=token, id=id, fields=dict(fields), endpoints=endpoints)


def parse_reward(status: int, body: str) -> dict:
    return _call("rewards.parse_reward", status=status, body=body)


def delete_request(token: str, id: str, endpoints: E = None) -> Request:
    """Read the answer with :func:`kicksdk.public_api.parse_done`."""
    return _request("rewards.delete_request", token=token, id=id, endpoints=endpoints)


def redemptions_request(
    token: str,
    reward_id: Optional[str] = None,
    status: Optional[Literal["pending", "accepted", "rejected"]] = None,
    cursor: Optional[str] = None,
    endpoints: E = None,
) -> Request:
    """Redemptions grouped by reward: of one reward or all, with one status
    (Kick's default: pending). Read the answer with :func:`parse_redemptions`."""
    return _request(
        "rewards.redemptions_request",
        token=token,
        reward_id=reward_id,
        status=status,
        cursor=cursor,
        endpoints=endpoints,
    )


def redemptions_by_ids_request(token: str, ids: list[str], cursor: Optional[str] = None, endpoints: E = None) -> Request:
    return _request("rewards.redemptions_by_ids_request", token=token, ids=list(ids), cursor=cursor, endpoints=endpoints)


def parse_redemptions(status: int, body: str) -> dict:
    """``{"rewards": [{"reward", "redemptions"}], "next_cursor"}``"""
    return _call("rewards.parse_redemptions", status=status, body=body)


def accept_request(token: str, ids: list[str], endpoints: E = None) -> Request:
    """Accepts 1 to 25 pending redemptions. Read the answer with :func:`parse_failures`."""
    return _request("rewards.accept_request", token=token, ids=list(ids), endpoints=endpoints)


def reject_request(token: str, ids: list[str], endpoints: E = None) -> Request:
    """Rejects 1 to 25 pending redemptions. Read the answer with :func:`parse_failures`."""
    return _request("rewards.reject_request", token=token, ids=list(ids), endpoints=endpoints)


def parse_failures(status: int, body: str) -> list[dict]:
    """The redemptions Kick did not accept or reject: ``[{"id", "reason"}]``."""
    return _call("rewards.parse_failures", status=status, body=body)
