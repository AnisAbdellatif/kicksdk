"""The Python package against the repository's shared fixtures."""

import json
from pathlib import Path
from typing import Any

import pytest

import kicksdk
from kicksdk import KickError, content, events, public_api, realtime, webhooks, website

FIXTURES = Path(__file__).resolve().parents[3] / "fixtures"


def files(d: str, part: str = "") -> list[Path]:
    return sorted(p for p in (FIXTURES / d).iterdir() if part in p.name)


def response(p: Path) -> tuple[int, str]:
    r = json.loads(p.read_text(encoding="utf-8"))["response"]
    return r["status"], r["body"]


def test_events_are_typed_and_unknown_ones_kept() -> None:
    host = events.parse("App\\Events\\StreamHostEvent", '{"host_username":"A","number_viewers":3}')
    assert host["type"] == "host" and host["viewers"] == 3
    assert events.parse("New", "{}") == {"type": "unknown", "name": "New"}
    assert content.parse("[emote:1:X] [emote:1:X]", combine=True)[0]["count"] == 2


def test_requests_and_endpoints() -> None:
    r = realtime.chat_connection_request(668, "c1", {"realtime": "http://x/rt"})
    assert (r.method, r.url) == ("POST", "http://x/rt/channels/668/chat/connection")
    assert ("origin", "https://kick.com") in r.headers
    assert len(public_api.livestreams_requests("t", list(range(1, 121)))) == 3


def test_errors_carry_kicks_status() -> None:
    with pytest.raises(KickError) as e:
        website.parse_channel(404, "{}")
    assert (e.value.kind, e.value.status) == ("status", 404)
    with pytest.raises(KickError) as e:
        public_api.channels_by_ids_request("t", [])
    assert e.value.kind == "invalid"


def test_parsers_read_the_recordings() -> None:
    for p in files("v2"):
        assert website.parse_channel(*response(p))["chatroom_id"] > 0
    for p in files("public_api", "livestreams-poll"):
        for s in public_api.parse_livestreams(*response(p)):
            assert s["started_at"].endswith("Z")


def test_webhook_signatures() -> None:
    d = FIXTURES / "signing"
    pem, body = (d / "test-public-key.pem").read_text(encoding="utf-8"), (d / "body.json").read_text(encoding="utf-8")
    headers = {
        "Kick-Event-Message-Id": "01M38M9K3YRKV8E6Q68C18ZWG4",
        "Kick-Event-Message-Timestamp": "2026-09-24T02:35:42Z",
        "Kick-Event-Signature": (d / "signature.b64").read_text(encoding="utf-8").strip(),
        "Kick-Event-Type": "channel.followed",
        "Kick-Event-Version": "1",
    }
    sent = 1_790_217_342  # the timestamp header, in Unix seconds
    out = webhooks.verify(pem, headers, body, now=sent + 2)
    assert out["event"]["type"] == "followed"
    assert out["headers"]["message_id"] == "01M38M9K3YRKV8E6Q68C18ZWG4"
    # Bytes are verified byte for byte (sent as body_base64); a float clock is fine.
    assert webhooks.verify(pem, list(headers.items()), body.encode(), now=sent + 0.9)["event"]["type"] == "followed"
    assert webhooks.verify(pem, headers, bytearray(body.encode()), skip_freshness=True)["event"]["type"] == "followed"

    def refused(**kw: Any) -> str:
        with pytest.raises(KickError) as e:
            webhooks.verify(pem, headers, kw.pop("body", body), **kw)
        return e.value.kind

    assert refused(body=body + " ", now=sent) == "webhook"
    assert refused(body=(body + " ").encode(), now=sent) == "webhook"
    # Stale: an hour late, outside a given tolerance, and by default (the
    # machine's clock, which is past the fixture's day).
    assert refused(now=sent + 3_600) == "webhook"
    assert refused(now=sent + 60, tolerance_secs=10) == "webhook"
    assert refused() == "webhook"
    assert refused(now=-1) == "call"
    # `now` is a finite number of seconds, at least 0, floored.
    for bad in (float("inf"), float("-inf"), float("nan"), -0.5):
        assert refused(now=bad) == "call", bad
    for wrong in ("1790217342", b"1", True, [1]):
        with pytest.raises(TypeError):
            webhooks.verify(pem, headers, body, now=wrong)  # type: ignore[arg-type]
    assert webhooks.verify(pem, headers, body, now=float(sent) + 0.99)["event"]["type"] == "followed"
    with pytest.raises(ValueError):
        webhooks.verify(pem, headers, body, now=sent, skip_freshness=True)
    with pytest.raises(TypeError):
        webhooks.verify(pem, headers, 3)  # type: ignore[arg-type]

    assert webhooks.parse("channel.followed", body.encode())["type"] == "followed"
    with pytest.raises(KickError) as e:
        webhooks.parse("kicks.gifted", body)
    assert e.value.kind == "shape"


def test_a_recorded_session_replays() -> None:
    for p in files("realtime", ".jsonl"):
        records = [json.loads(line) for line in p.read_text(encoding="utf-8").splitlines()]
        sent = [r["frame"] for r in records if r["direction"] == "out"]
        channels = [
            json.loads(line)["subscribe"]["channel"]
            for f in sent
            for line in f.split("\n")
            if "subscribe" in json.loads(line)
        ]
        s = realtime.Session(channels)
        ours = [a["text"] for a in s.open("[redacted]", 0)]
        actions = []
        for r in records:
            if r["direction"] == "in":
                for a in s.receive(r["frame"], r["ms"]):
                    if a["action"] == "send":
                        ours.append(a["text"])
                    actions.append(a)
        assert ours == sent
        assert any(a["action"] == "event" and a["event"]["type"] == "chat_message" for a in actions)
        assert any(a["action"] == "subscribe_failed" and a["code"] == 103 for a in actions)
        assert s.is_subscribed(channels[0])


def test_reconnect_codes_and_backoff() -> None:
    assert realtime.retry_after_ms(3501) == realtime.retry_after_ms(4500) == realtime.TERMINAL_RETRY_MS == 300_000
    assert realtime.retry_after_ms(1006) == realtime.retry_after_ms(3000) == realtime.retry_after_ms(4499) == 0
    with pytest.raises(KickError) as e:
        realtime.retry_after_ms(70_000)
    assert e.value.kind == "call"

    b = realtime.Backoff()
    assert [b.next_delay() for _ in range(7)] == [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]
    b.reset()
    assert (b.next_delay_jittered(0.0), b.next_delay_jittered(0.5), b.next()) == (500, 1_500, 4.0)
    assert (b.initial, b.maximum) == (1.0, 30.0)
    assert realtime.Backoff(0, 0).next_delay() == 1, "never a busy loop"


def test_backoff_has_a_slower_stage_for_terminal_closes() -> None:
    b = realtime.Backoff()
    assert [b.next_delay_for(300_000, 0.0) for _ in range(5)] == [300_000, 600_000, 1_200_000, 1_800_000, 1_800_000]
    assert b.next_delay_for(0, 0.0) == 500, "the ordinary stage is left as it was"
    b.reset()
    assert b.next_delay_for(300_000, 0.5) == 337_500
    assert b.next_delay_for(300_000, float("nan")) == 600_000


def test_backoff_bounds_can_be_set() -> None:
    b = realtime.Backoff()
    b.maximum = 4
    assert [b.next() for _ in range(4)] == [1.0, 2.0, 4.0, 4.0]
    b.initial = 2
    b.reset()
    assert (b.initial, b.maximum, b.next()) == (2.0, 4.0, 2.0)
    b.initial = -1
    b.reset()
    assert b.initial == 0.0 and b.next() == 0.001, "never a busy loop"


def test_every_core_function_is_reachable() -> None:
    assert "website.channel_request" in kicksdk._native.FUNCTIONS
    assert kicksdk._native.NativeError.__module__ == "kicksdk._native"


def test_packaged_licenses_are_the_repositorys() -> None:
    # pyproject's license-files cannot reach above the package directory,
    # so the package keeps copies; they must not drift.
    here, root = Path(__file__).resolve().parents[1], Path(__file__).resolve().parents[3]
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        if not (root / name).exists():
            pytest.skip("not in the repository (an sdist)")
        assert (here / name).read_bytes() == (root / name).read_bytes(), name


def test_next_delay_for_refuses_a_bad_wait() -> None:
    b = kicksdk.realtime.Backoff()
    for bad in (-1, 1.5, "300000", True):
        with pytest.raises((TypeError, ValueError)):
            b.next_delay_for(bad, 0.0)  # type: ignore[arg-type]
