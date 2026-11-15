"""User tokens: OAuth 2.1, authorization code with PKCE, on ``id.kick.com``.

A user token acts as one Kick user, with the scopes they approved. The flow:
make a PKCE pair (:func:`new_pkce`) and a state (:func:`new_state`), send the
user's browser to :func:`authorize_url`, keep the verifier and the state until
Kick redirects back with ``code`` and ``state``, check the state, then send
:func:`code_request` and read the answer with :func:`parse_user_token`.
:func:`refresh_request` before the access token expires; the answer carries a
new refresh token, which replaces the old one.

The requests carry secrets (the client secret, the code, tokens; a
:func:`revoke_request` has the token in its URL): keep them out of logs."""

from __future__ import annotations

import base64
import secrets
from typing import Optional

from . import Request, _call, _request
from .public_api import E


def pkce(random: bytes) -> dict:
    """A PKCE pair, ``{"verifier", "challenge"}``, from 32 to 96 bytes drawn
    from a CSPRNG (the verifier is their base64url). :func:`new_pkce` draws
    them."""
    return _call("oauth.pkce", random=_b64url(bytes(random)))


def new_pkce() -> dict:
    """A PKCE pair from 32 bytes of :func:`secrets.token_bytes`."""
    return pkce(secrets.token_bytes(32))


def new_state() -> str:
    """A ``state`` for :func:`authorize_url`: 32 bytes of
    :func:`secrets.token_bytes`, as base64url without padding."""
    return _b64url(secrets.token_bytes(32))


def pkce_from_verifier(verifier: str) -> dict:
    """The pair for a verifier kept from before: 43 to 128 characters of
    ``A-Z a-z 0-9 - . _ ~``."""
    return _call("oauth.pkce_from_verifier", verifier=verifier)


def authorize_url(
    client_id: str, redirect_uri: str, scopes: list[str], state: str, verifier: str, endpoints: E = None
) -> str:
    """Where to send the user's browser to approve the app, with the S256
    challenge of ``verifier``. ``redirect_uri`` must be exactly the one in the
    app's settings; at least one scope (``user:read``, ``chat:write``, …:
    :func:`scopes`)."""
    return _call(
        "oauth.authorize_url",
        client_id=client_id,
        redirect_uri=redirect_uri,
        scopes=list(scopes),
        state=state,
        verifier=verifier,
        endpoints=endpoints,
    )


def code_request(
    client_id: str, client_secret: str, redirect_uri: str, code: str, code_verifier: str, endpoints: E = None
) -> Request:
    """A user token for the ``code`` on the redirect, with the verifier whose
    challenge was in the :func:`authorize_url`."""
    return _request(
        "oauth.code_request",
        client_id=client_id,
        client_secret=client_secret,
        redirect_uri=redirect_uri,
        code=code,
        code_verifier=code_verifier,
        endpoints=endpoints,
    )


def refresh_request(client_id: str, client_secret: str, refresh_token: str, endpoints: E = None) -> Request:
    return _request(
        "oauth.refresh_request",
        client_id=client_id,
        client_secret=client_secret,
        refresh_token=refresh_token,
        endpoints=endpoints,
    )


def parse_user_token(status: int, body: str) -> dict:
    """``{"access_token", "refresh_token", "expires_in", "refresh_expires_in",
    "token_type", "scopes"}``"""
    return _call("oauth.parse_user_token", status=status, body=body)


def revoke_request(token: str, token_type_hint: Optional[str] = None, endpoints: E = None) -> Request:
    """``token_type_hint`` is ``access_token`` or ``refresh_token``. The token
    is in the URL."""
    return _request("oauth.revoke_request", token=token, token_type_hint=token_type_hint, endpoints=endpoints)


def parse_revoke(status: int, body: str) -> None:
    _call("oauth.parse_revoke", status=status, body=body)


def introspect_request(token: str, endpoints: E = None) -> Request:
    """What Kick says about ``token`` (a user or an app token)."""
    return _request("oauth.introspect_request", token=token, endpoints=endpoints)


def parse_introspect(status: int, body: str) -> dict:
    """``{"active", "client_id", "exp", "scopes", "token_type"}``"""
    return _call("oauth.parse_introspect", status=status, body=body)


def scopes() -> list[str]:
    """Every scope Kick documents."""
    return _call("oauth.scopes")


def _b64url(b: bytes) -> str:
    return base64.urlsafe_b64encode(b).rstrip(b"=").decode("ascii")
