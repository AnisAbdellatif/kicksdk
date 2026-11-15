"""User-token modules (oauth, chat, moderation, channels, rewards, ads, drops)
and the newer public_api reads, against Kick's documented examples in
fixtures/docs/."""

import base64
import hashlib
import json
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl, urlsplit

import pytest

from kicksdk import KickError, Request, ads, channels, chat, drops, moderation, oauth, public_api, rewards

DOCS = Path(__file__).resolve().parents[3] / "fixtures" / "docs"


def fixture(name: str) -> dict[str, Any]:
    d = DOCS / "public_api"
    path = d / f"actions-{name}.json" if (d / f"actions-{name}.json").exists() else d / f"{name}.json"
    out: dict[str, Any] = json.loads(path.read_text(encoding="utf-8"))
    return out


def answer(f: dict[str, Any]) -> tuple[int, str]:
    return f["response"]["status"], f["response"]["body"]


def same(r: Request, f: dict[str, Any]) -> None:
    """The request is the documented one: method, URL, query and JSON body."""
    want = f["request"]
    url = urlsplit(r.url)
    assert (r.method, f"{url.scheme}://{url.netloc}{url.path}") == (want["method"], want["url"])
    params = {k: [str(x) for x in (v if isinstance(v, list) else [v])] for k, v in want.get("params", {}).items()}
    got: dict[str, list[str]] = {}
    for k, v in parse_qsl(url.query):
        got.setdefault(k, []).append(v)
    assert got == params, r.url
    assert (json.loads(r.body) if r.body else None) == want.get("json"), r.url
    assert ("authorization", "Bearer t") in r.headers


def test_chat() -> None:
    f = fixture("chat-send")
    same(chat.send_request("t", 123, "Pog", "828f83bb-e391-4c78-9e91-af35a172840e"), f)
    assert chat.parse_send(*answer(f)) == {"is_sent": True, "message_id": "828f83bb-e391-4c78-9e91-af35a172840e"}
    bot = chat.send_request("t", "bot", "hi")
    assert json.loads(bot.body or "") == {"content": "hi", "type": "bot"}
    f = fixture("chat-delete")
    same(chat.delete_request("t", "828f83bb-e391-4c78-9e91-af35a172840e"), f)
    public_api.parse_done(*answer(f))
    with pytest.raises(KickError) as e:
        chat.send_request("t", 123, "")
    assert e.value.kind == "invalid"


def test_moderation() -> None:
    f = fixture("moderation-timeout")
    same(moderation.timeout_request("t", 123, 333, 10, "Not vibing"), f)
    public_api.parse_done(*answer(f))
    same(moderation.unban_request("t", 123, 333), fixture("moderation-unban"))
    ban = moderation.ban_request("t", 1, 2)
    assert (ban.method, json.loads(ban.body or "")) == ("POST", {"broadcaster_user_id": 1, "user_id": 2})
    with pytest.raises(KickError) as e:
        moderation.timeout_request("t", 1, 2, 0)
    assert e.value.kind == "invalid"


def test_channels() -> None:
    f = fixture("channels-update")
    same(channels.update_request("t", 123, ["pog", "chad"], "My first stream"), f)
    public_api.parse_done(*answer(f))
    assert json.loads(channels.update_request("t", custom_tags=[]).body or "") == {"custom_tags": []}
    with pytest.raises(KickError) as e:
        channels.update_request("t")
    assert e.value.kind == "invalid"


def test_rewards() -> None:
    rid = "01HZ8X9K2M4N6P8Q0R2S4T6V8W0Y2Z4"
    f = fixture("rewards-list")
    same(rewards.list_request("t"), f)
    assert [(r["id"], r["cost"]) for r in rewards.parse_rewards(*answer(f))] == [(rid, 100)]
    for name in ("rewards-create", "rewards-update"):
        f = fixture(name)
        fields = f["request"]["json"]
        r = rewards.create_request("t", fields) if name == "rewards-create" else rewards.update_request("t", rid, fields)
        same(r, f)
        reward = rewards.parse_reward(*answer(f))
        assert (reward["id"], reward["title"], reward["is_paused"]) == (rid, "Song Request", False)
    with pytest.raises(KickError) as e:
        rewards.create_request("t", {"titel": "x", "cost": 1})
    assert e.value.kind == "call" and "titel" in e.value.message
    same(rewards.delete_request("t", rid), fixture("rewards-delete"))

    f = fixture("rewards-redemptions")
    same(rewards.redemptions_request("t", status="pending"), f)
    page = rewards.parse_redemptions(*answer(f))
    assert page["rewards"][0]["redemptions"][0]["user_input"] == "sorry mr strimer" and page["next_cursor"]
    by_ids = rewards.redemptions_by_ids_request("t", ["a", "b"], cursor="c")
    assert by_ids.url.endswith("/channels/rewards/redemptions?id=a&id=b&cursor=c")
    with pytest.raises(KickError) as e:
        rewards.redemptions_request("t", status="done")  # type: ignore[arg-type]
    assert e.value.kind == "call"

    ids = ["01KEWZ2RKYZZFGD154K50Y3F8D", "01KEWZ2YC27070XWMHKFJR6C87"]
    for name, r in [("rewards-accept", rewards.accept_request("t", ids)), ("rewards-reject", rewards.reject_request("t", ids))]:
        f = fixture(name)
        same(r, f)
        assert rewards.parse_failures(*answer(f)) == [{"id": "01KERB68MZZ04551D6X5TNRFYV", "reason": "NOT_OWNED"}]


def test_ads() -> None:
    uuid = "123e4567-e89b-12d3-a456-426614174000"
    f = fixture("ads-ad-break")
    same(ads.ad_break_request("t", uuid, 30), f)
    b = ads.parse_ad_break(*answer(f))
    assert (b["ad_break_id"], b["remaining_ad_breaks"]) == ("Xq3LmP9aB2cD", 2)
    with pytest.raises(KickError) as e:
        ads.parse_ad_break(*answer(fixture("ads-ad-break-refused")))
    assert (e.value.kind, e.value.status) == ("status", 400)
    f = fixture("ads-status")
    same(ads.status_request("t"), f)
    s = ads.parse_status(*answer(f))
    assert (s["opted_in"], s["limits"]["max"], s["ad_breaks"][0]["status"]) == (True, 3, "inserted")
    same(ads.enroll_request("t"), fixture("ads-enroll"))


def test_drops() -> None:
    f = fixture("drops-claims")
    same(drops.claims_request("t", f["request"]["params"]), f)
    c = drops.parse_claims(*answer(f))
    assert (len(c["claims"]), c["cursor"]) == (1, "01K0TDDR08Q5ZNWXK92SDH7SDH")
    assert drops.claims_request("t").url == "https://api.kick.com/public/v1/drops/claims"
    with pytest.raises(KickError) as e:
        drops.claims_request("t", {"campaign": "x"})
    assert e.value.kind == "call"
    f = fixture("drops-update")
    pairs = [("01KAAFHJ2PNXS48NG8XXPGWKCZ", "processed"), ("01KAAFHJ2PNXS48NG8XXPGWKCA", "initiated")]
    same(drops.update_claims_request("t", pairs), f)
    public_api.parse_done(*answer(f))
    with pytest.raises(KickError) as e:
        drops.parse_claims(*answer(fixture("drops-unauthorized")))
    assert e.value.status == 401


def test_public_api_reads() -> None:
    f = fixture("v1-users")
    same(public_api.users_request("t", [123]), f)
    assert public_api.parse_users(*answer(f))[0]["user_id"] == 123
    assert public_api.users_request("t").url == "https://api.kick.com/public/v1/users"
    f = fixture("v2-categories")
    page = public_api.parse_categories(*answer(f))
    assert page["data"] and "next_cursor" in page
    r = public_api.categories_request("t", names=["Just Chatting"], limit=5)
    assert r.url.startswith("https://api.kick.com/public/v2/categories?") and "limit=5" in r.url
    f = fixture("v1-livestreams-stats")
    same(public_api.livestream_stats_request("t"), f)
    assert isinstance(public_api.parse_livestream_stats(*answer(f))["total_count"], int)
    f = fixture("v1-kicks-leaderboard")
    same(public_api.kicks_leaderboard_request("t", top=1), f)
    board = public_api.parse_kicks_leaderboard(*answer(f))
    assert board["lifetime"][0] == {"user_id": 123, "username": "john-doe", "rank": 1, "gifted_amount": 100}
    public_api.parse_done(204, "")
    with pytest.raises(KickError) as e:
        public_api.parse_done(403, "")
    assert (e.value.kind, e.value.status) == ("status", 403)


def test_oauth_flow() -> None:
    p = oauth.new_pkce()
    assert len(p["verifier"]) == 43 and oauth.pkce_from_verifier(p["verifier"]) == p
    assert oauth.new_pkce() != p
    # S256: base64url (no padding) of the verifier's SHA-256.
    digest = hashlib.sha256(p["verifier"].encode()).digest()
    assert p["challenge"] == base64.urlsafe_b64encode(digest).decode().rstrip("=")
    assert oauth.pkce(bytes(range(32)))["verifier"] == base64.urlsafe_b64encode(bytes(range(32))).decode().rstrip("=")
    with pytest.raises(KickError) as e:
        oauth.pkce(b"short")
    assert e.value.kind == "invalid"
    state = oauth.new_state()
    assert len(state) == 43 and "=" not in state and state != oauth.new_state()

    url = oauth.authorize_url("c", "http://localhost/cb", ["user:read", "chat:write"], state, p["verifier"])
    q = dict(parse_qsl(urlsplit(url).query))
    assert url.startswith("https://id.kick.com/oauth/authorize?")
    assert (q["scope"], q["state"], q["code_challenge"], q["code_challenge_method"]) == (
        "user:read chat:write",
        state,
        p["challenge"],
        "S256",
    )
    with pytest.raises(KickError):
        oauth.authorize_url("c", "http://localhost/cb", [], state, p["verifier"])

    r = oauth.code_request("c", "s", "http://localhost/cb", "the-code", p["verifier"])
    assert (r.method, r.url) == ("POST", "https://id.kick.com/oauth/token")
    form = dict(parse_qsl(r.body or ""))
    assert (form["grant_type"], form["code"], form["code_verifier"]) == ("authorization_code", "the-code", p["verifier"])
    form = dict(parse_qsl(oauth.refresh_request("c", "s", "r").body or ""))
    assert (form["grant_type"], form["refresh_token"]) == ("refresh_token", "r")
    t = oauth.parse_user_token(200, '{"access_token":"a","refresh_token":"r","expires_in":"7200","scope":"user:read"}')
    assert (t["access_token"], t["expires_in"], t["scopes"]) == ("a", 7200, ["user:read"])
    with pytest.raises(KickError) as e:
        oauth.parse_user_token(400, (DOCS / "oauth" / "error.json").read_text(encoding="utf-8"))
    assert e.value.status == 400

    r = oauth.revoke_request("tok", "refresh_token")
    assert (r.method, r.url) == ("POST", "https://id.kick.com/oauth/revoke?token=tok&token_type_hint=refresh_token")
    assert "tok&" not in repr(r) and "token=[redacted]&token_type_hint=refresh_token" in repr(r)
    oauth.parse_revoke(200, (DOCS / "oauth" / "revoke.txt").read_text(encoding="utf-8"))
    with pytest.raises(KickError) as e:
        oauth.revoke_request("tok", "id_token")
    assert e.value.kind == "call"
    r = oauth.introspect_request("tok")
    assert ("authorization", "Bearer tok") in r.headers
    i = oauth.parse_introspect(200, (DOCS / "oauth" / "introspect.json").read_text(encoding="utf-8"))
    assert (i["active"], i["token_type"], i["scopes"]) == (True, "user", ["user:read", "channel:read"])
    assert len(oauth.scopes()) == 13 and "moderation:chat_message:manage" in oauth.scopes()


def test_request_repr_hides_secrets() -> None:
    r = oauth.code_request("app", "sekrit", "https://app.example/cb", "the-code", "v" * 43)
    shown = repr(r)
    for secret in ("sekrit", "the-code", "v" * 43):
        assert secret not in shown, shown
    assert "client_id=app" in shown and "client_secret=[redacted]" in shown
    assert r.body is not None and "client_secret=sekrit" in r.body
    bearer = public_api.users_request("tok-123")
    assert "tok-123" not in repr(bearer) and "[redacted]" in repr(bearer)
