"""The Python package against the repository's shared fixtures."""

import json
from pathlib import Path

import pytest

import kicksdk
from kicksdk import KickError, content, events, public_api, realtime, webhooks, website

FIXTURES = Path(__file__).resolve().parents[3] / "fixtures"


def files(d, part=""):
    return sorted(p for p in (FIXTURES / d).iterdir() if part in p.name)


def response(p):
    r = json.loads(p.read_text())["response"]
    return r["status"], r["body"]


def test_events_are_typed_and_unknown_ones_kept():
    host = events.parse("App\\Events\\StreamHostEvent", '{"host_username":"A","number_viewers":3}')
    assert host["type"] == "host" and host["viewers"] == 3
    assert events.parse("New", "{}") == {"type": "unknown", "name": "New"}
    assert content.parse("[emote:1:X] [emote:1:X]", combine=True)[0]["count"] == 2


def test_requests_and_endpoints():
    r = realtime.chat_connection_request(668, "c1", {"realtime": "http://x/rt"})
    assert (r.method, r.url) == ("POST", "http://x/rt/channels/668/chat/connection")
    assert ("origin", "https://kick.com") in r.headers
    assert len(public_api.livestreams_requests("t", list(range(1, 121)))) == 3


def test_errors_carry_kicks_status():
    with pytest.raises(KickError) as e:
        website.parse_channel(404, "{}")
    assert (e.value.kind, e.value.status) == ("status", 404)
    with pytest.raises(KickError) as e:
        public_api.channels_by_ids_request("t", [])
    assert e.value.kind == "invalid"


def test_parsers_read_the_recordings():
    for p in files("v2"):
        assert website.parse_channel(*response(p))["chatroom_id"] > 0
    for p in files("public_api", "livestreams-poll"):
        for s in public_api.parse_livestreams(*response(p)):
            assert s["started_at"].endswith("Z")


def test_webhook_signatures():
    d = FIXTURES / "signing"
    pem, body = (d / "test-public-key.pem").read_text(), (d / "body.json").read_text()
    headers = {
        "Kick-Event-Message-Id": "01M38M9K3YRKV8E6Q68C18ZWG4",
        "Kick-Event-Message-Timestamp": "2026-09-24T02:35:42Z",
        "Kick-Event-Signature": (d / "signature.b64").read_text().strip(),
        "Kick-Event-Type": "channel.followed",
        "Kick-Event-Version": "1",
    }
    assert webhooks.verify(pem, headers, body)["event"]["type"] == "followed"
    with pytest.raises(KickError) as e:
        webhooks.verify(pem, headers, body + " ")
    assert e.value.kind == "webhook"


def test_a_recorded_session_replays():
    for p in files("realtime", ".jsonl"):
        records = [json.loads(line) for line in p.read_text().splitlines()]
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


def test_reconnect_codes_and_backoff():
    assert not realtime.should_reconnect(3501)
    assert not realtime.should_reconnect(4500)
    assert realtime.should_reconnect(1006) and realtime.should_reconnect(3000)
    with pytest.raises(KickError) as e:
        realtime.should_reconnect(70_000)
    assert e.value.kind == "call"

    b = realtime.Backoff()
    assert [b.next_delay() for _ in range(7)] == [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]
    b.reset()
    assert (b.next_delay_jittered(0.0), b.next_delay_jittered(0.5), b.next()) == (500, 1_500, 4.0)
    assert (b.initial, b.maximum) == (1.0, 30.0)
    assert realtime.Backoff(0, 0).next_delay() == 1, "never a busy loop"


def test_every_core_function_is_reachable():
    assert "website.channel_request" in kicksdk._native.FUNCTIONS
