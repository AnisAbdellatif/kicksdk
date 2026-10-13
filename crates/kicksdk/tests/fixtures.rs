//! The parsers against what Kick actually sent: every recorded answer in
//! `fixtures/` is read, and the realtime recordings are replayed through a
//! [`Session`] frame by frame.

use kicksdk::events::KickEvent;
use kicksdk::realtime::{self, Action, Session, SessionOptions};
use kicksdk::{public_api, webhooks, website};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(dir);
    let mut files: Vec<PathBuf> =
        fs::read_dir(&root).unwrap_or_else(|e| panic!("{}: {e}", root.display())).map(|e| e.unwrap().path()).collect();
    files.sort();
    files
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn response(path: &Path) -> (u16, String) {
    let v = read(path);
    let r = &v["response"];
    (r["status"].as_u64().unwrap() as u16, r["body"].as_str().unwrap().to_owned())
}

fn named(dir: &str, part: &str) -> Vec<PathBuf> {
    fixtures(dir).into_iter().filter(|p| p.to_string_lossy().contains(part)).collect()
}

#[test]
fn livestreams_polls_are_read() {
    let mut live = 0;
    for path in named("public_api", "livestreams-poll") {
        let (status, body) = response(&path);
        let streams =
            public_api::parse_livestreams(status, &body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for s in &streams {
            assert!(s.slug.is_some() && s.viewer_count.is_some(), "{}", path.display());
            // `YYYY-MM-DDTHH:MM:SSZ`: the stream's identity, compared as a string.
            let at = s.started_at.as_deref().unwrap();
            assert_eq!(at.len(), 20, "{at}");
            assert!(at.ends_with('Z'));
        }
        live += streams.len();
    }
    assert!(live > 0, "no live stream in any recording");
}

#[test]
fn a_missing_token_is_an_error_status() {
    for path in named("public_api", "livestreams-no-token") {
        let (status, body) = response(&path);
        let err = public_api::parse_livestreams(status, &body).unwrap_err();
        assert_eq!(err.status(), Some(status));
    }
}

#[test]
fn channels_carry_subscriber_counts() {
    for path in named("public_api", "-channels") {
        let (status, body) = response(&path);
        if status == 400 {
            // One unknown slug in the batch fails all of it.
            assert_eq!(public_api::parse_channels(status, &body).unwrap_err().status(), Some(400));
            continue;
        }
        let channels = public_api::parse_channels(status, &body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!channels.is_empty(), "{}", path.display());
        assert!(channels.iter().all(|c| c.active_subscribers_count.is_some()));
    }
}

#[test]
fn the_public_key_is_a_pem() {
    for path in named("public_api", "public-key") {
        let (status, body) = response(&path);
        assert!(public_api::parse_public_key(status, &body).unwrap().starts_with("-----BEGIN PUBLIC KEY-----"));
    }
}

#[test]
fn the_app_token_answer_is_read() {
    for path in named("id", "token") {
        let (status, body) = response(&path);
        if status == 200 {
            let token = public_api::parse_token(status, &body).unwrap();
            assert_eq!(token.expires_in, Some(5_184_000), "60 days");
        }
    }
}

#[test]
fn website_channels_give_the_chatroom() {
    let files = fixtures("v2");
    assert!(!files.is_empty());
    for path in files {
        let (status, body) = response(&path);
        let c = website::parse_channel(status, &body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(c.chatroom_id.0 > 0);
        assert!(c.followers_count.is_some(), "{}", path.display());
        assert!(c.username.is_some());
        // Redacted in the recordings: the real one is signed.
        assert_eq!(c.playback_url.as_deref(), Some("[redacted]"));
    }
}

#[test]
fn webhook_bodies_are_read_by_type() {
    for path in fixtures("webhook") {
        let v = read(&path);
        let headers: Vec<(String, String)> = v["request"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| (h[0].as_str().unwrap().to_owned(), h[1].as_str().unwrap().to_owned()))
            .collect();
        let h = webhooks::Headers::from_pairs(headers.iter().map(|(a, b)| (a.as_str(), b.as_str()))).unwrap();
        let event = webhooks::parse(&h.event_type, v["request"]["body"].as_str().unwrap()).unwrap();
        match (h.event_type.as_str(), &event) {
            ("channel.followed", webhooks::WebhookEvent::Followed { follower, .. }) => {
                assert!(follower.as_ref().unwrap().user_id.is_some())
            }
            ("livestream.status.updated", webhooks::WebhookEvent::LivestreamStatus { started_at, .. }) => {
                assert!(started_at.is_some())
            }
            ("livestream.metadata.updated", webhooks::WebhookEvent::LivestreamMetadata { title, .. }) => {
                assert!(title.is_some())
            }
            (t, e) => panic!("{t} read as {e:?}"),
        }
    }
}

#[test]
fn webhook_signatures_verify() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing");
    let pem = fs::read_to_string(dir.join("test-public-key.pem")).unwrap();
    let body = fs::read(dir.join("body.json")).unwrap();
    let signature = fs::read_to_string(dir.join("signature.b64")).unwrap();
    let headers = webhooks::Headers::from_pairs([
        ("kick-event-message-id", "01M38M9K3YRKV8E6Q68C18ZWG4"),
        ("kick-event-message-timestamp", "2026-09-24T02:35:42Z"),
        ("kick-event-signature", signature.trim()),
        ("kick-event-type", "channel.followed"),
        ("kick-event-version", "1"),
    ])
    .unwrap();
    assert_eq!(webhooks::verify(&pem, &headers, &body), Ok(()));

    let mut tampered = body.clone();
    tampered[10] ^= 1;
    assert_eq!(webhooks::verify(&pem, &headers, &tampered), Err(webhooks::WebhookError::Mismatch));
    let mut later = headers.clone();
    later.timestamp = "2026-09-24T02:35:43Z".into();
    assert_eq!(webhooks::verify(&pem, &later, &body), Err(webhooks::WebhookError::Mismatch));
    assert_eq!(webhooks::verify("nope", &headers, &body), Err(webhooks::WebhookError::BadKey));
}

/// Chat payloads recorded off Pusher: the data is the same on Centrifugo.
#[test]
fn legacy_chat_payloads_parse() {
    let mut messages = 0;
    let mut hosts = 0;
    for path in fixtures("pusher-legacy") {
        for line in fs::read_to_string(&path).unwrap().lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            let Some(frame) = record["frame"].as_str() else { continue };
            let frame: Value = serde_json::from_str(frame).unwrap();
            let name = frame["event"].as_str().unwrap();
            if name.starts_with("pusher") {
                continue;
            }
            match kicksdk::events::parse(name, &frame["data"]) {
                KickEvent::ChatMessage(m) => {
                    assert!(m.sender.id.is_some() && !m.id.is_empty());
                    assert!(m.created_at.as_deref().unwrap().ends_with("+00:00"));
                    messages += 1;
                }
                KickEvent::Host { host_username, viewers, .. } => {
                    assert!(host_username.is_some() && viewers.is_some());
                    hosts += 1;
                }
                KickEvent::HostingOut { hosted } => assert!(hosted.slug.is_some()),
                other => panic!("{name} read as {other:?}"),
            }
        }
    }
    assert_eq!(messages, 498);
    assert_eq!(hosts, 3);
}

/// Each Centrifugo recording, replayed: the session must send what Kick's
/// server answered to, and read everything it sent.
#[test]
fn realtime_recordings_replay() {
    let files = fixtures("realtime");
    assert!(!files.is_empty(), "record one with tools/record-realtime.mjs");
    for path in files {
        let records: Vec<Value> =
            fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();

        let http: Vec<&Value> = records.iter().filter(|r| r["direction"] == "http").collect();
        let (s, b) = (http[0]["frame"]["status"].as_u64().unwrap() as u16, http[0]["frame"]["body"].as_str().unwrap());
        let url = realtime::parse_connection(s, b).unwrap()[0].centrifugo_url().unwrap().to_owned();
        assert!(url.starts_with("wss://realtime.") && url.ends_with("/connection/websocket"));
        let (s, b) = (http[1]["frame"]["status"].as_u64().unwrap() as u16, http[1]["frame"]["body"].as_str().unwrap());
        assert_eq!(realtime::parse_token(s, b).unwrap(), "[redacted]");

        // The feeds the recorder asked for, in its order.
        let sent: Vec<&str> =
            records.iter().filter(|r| r["direction"] == "out").map(|r| r["frame"].as_str().unwrap()).collect();
        let channels: Vec<String> = sent
            .iter()
            .flat_map(|f| f.lines())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v["subscribe"]["channel"].as_str().map(str::to_owned))
            .collect();
        assert!(channels.len() >= 5);

        let mut session = Session::new(SessionOptions::new(channels.clone()));
        let mut ours: Vec<String> = Vec::new();
        let mut actions: Vec<Action> = Vec::new();
        for a in session.open("[redacted]", 0) {
            if let Action::Send { text } = &a {
                ours.push(text.clone());
            }
        }
        for r in records.iter().filter(|r| r["direction"] == "in") {
            let ms = r["ms"].as_u64().unwrap();
            for a in session.receive(r["frame"].as_str().unwrap(), ms) {
                if let Action::Send { text } = &a {
                    ours.push(text.clone());
                }
                actions.push(a);
            }
        }

        // What we sent is what the recorder sent, frame for frame.
        assert_eq!(ours, sent, "{}", path.display());

        assert!(matches!(actions[0], Action::Connected { .. }));
        let subscribed: Vec<&str> = actions
            .iter()
            .filter_map(|a| if let Action::Subscribed { channel } = a { Some(channel.as_str()) } else { None })
            .collect();
        for c in channels.iter().filter(|c| !c.starts_with("private-")) {
            assert!(subscribed.contains(&c.as_str()), "{c} not subscribed");
        }
        for c in channels.iter().filter(|c| c.starts_with("private-")) {
            // A guest is refused private feeds: 103 permission denied.
            assert!(
                actions.iter().any(|a| matches!(a, Action::SubscribeFailed { channel, code: 103, .. } if channel == c))
            );
        }
        let chat = actions
            .iter()
            .filter(|a| matches!(a, Action::Event { event: KickEvent::ChatMessage(_), channel, .. } if channel.ends_with(".v2")))
            .count();
        assert!(chat > 0, "{}: no chat", path.display());
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Event { event: KickEvent::Unknown { .. }, .. })),
            "an event in the recording is not read: {:?}",
            actions.iter().find(|a| matches!(a, Action::Event { event: KickEvent::Unknown { .. }, .. }))
        );
    }
}
