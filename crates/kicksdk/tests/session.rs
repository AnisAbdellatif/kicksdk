//! The session's timing: pings, silence, token refresh, a server disconnect,
//! and starting over on a new socket.

use kicksdk::realtime::{Action, Session, SessionOptions};

fn connected(ttl: u64) -> Session {
    let mut s = Session::new(SessionOptions::new(vec!["chatrooms.1.v2".into()]));
    s.open("t1", 0);
    let out = s.receive(
        &format!(r#"{{"id":1,"connect":{{"client":"c","expires":true,"ttl":{ttl},"ping":25,"pong":true}}}}"#),
        0,
    );
    assert!(matches!(&out[1], Action::Send { text } if text == r#"{"id":2,"subscribe":{"channel":"chatrooms.1.v2"}}"#));
    s.receive(r#"{"id":2,"subscribe":{}}"#, 10);
    assert!(s.is_subscribed("chatrooms.1.v2"));
    s
}

#[test]
fn pings_are_answered_and_keep_it_alive() {
    let mut s = connected(2700);
    assert_eq!(s.receive("{}", 25_000), vec![Action::Send { text: "{}".into() }]);
    // 25 s ping + 20 s grace after the last traffic.
    assert_eq!(s.tick(69_999), vec![]);
    assert!(matches!(s.tick(70_000).as_slice(), [Action::Close { .. }]));
    // Once closing, nothing more.
    assert_eq!(s.tick(80_000), vec![]);
    assert_eq!(s.receive("{}", 80_000), vec![]);
}

#[test]
fn the_token_is_refreshed_before_it_runs_out() {
    let mut s = connected(2700);
    // 2700 s less the 120 s lead.
    let at = 2_580_000;
    s.receive("{}", at - 1_000); // stay alive
    assert_eq!(s.deadline(), Some(at));
    assert_eq!(s.tick(at), vec![Action::RefreshToken]);
    assert_eq!(s.tick(at + 1), vec![], "asked once");
    let out = s.refresh("t2", at + 100);
    assert_eq!(out, vec![Action::Send { text: r#"{"id":3,"refresh":{"token":"t2"}}"#.into() }]);
    s.receive(r#"{"id":3,"refresh":{"expires":true,"ttl":2700}}"#, at + 200);
    assert_eq!(s.deadline(), Some(at + 200 + 45_000), "silence comes first again");
}

#[test]
fn a_short_token_is_refreshed_halfway() {
    let s = connected(60);
    assert_eq!(s.deadline(), Some(30_000));
}

#[test]
fn a_refused_refresh_closes() {
    let mut s = connected(2700);
    s.refresh("bad", 1_000);
    let out = s.receive(r#"{"id":3,"error":{"code":109,"message":"token expired"}}"#, 1_100);
    assert!(matches!(out.as_slice(), [Action::Close { .. }]));
}

#[test]
fn a_disconnect_push_closes_and_closed_reports_every_feed() {
    let mut s = connected(2700);
    let out = s.receive(r#"{"push":{"disconnect":{"code":3001,"reason":"shutdown","reconnect":true}}}"#, 5_000);
    assert!(matches!(
        out.as_slice(),
        [Action::Close { reason, terminal: false, retry_after_ms: 0, .. }] if reason.contains("3001")
    ));
    let out = s.closed();
    assert_eq!(
        out,
        vec![Action::Unsubscribed { channel: "chatrooms.1.v2".into(), code: 0, reason: "socket closed".into() }]
    );
    assert!(!s.is_connected());
    assert_eq!(s.deadline(), None);
    // Ready for a new socket.
    let out = s.open("t3", 6_000);
    assert_eq!(out, vec![Action::Send { text: r#"{"id":1,"connect":{"token":"t3","name":"js"}}"#.into() }]);
}

#[test]
fn a_refused_connect_closes() {
    let mut s = Session::new(SessionOptions::new(vec![]));
    s.open("bad", 0);
    let out = s.receive(r#"{"id":1,"error":{"code":101,"message":"unauthorized"}}"#, 10);
    assert!(matches!(
        out.as_slice(),
        [Action::Close { reason, terminal: true, retry_after_ms: 300_000, .. }] if reason.contains("101")
    ));
}

#[test]
fn an_unsub_push_is_reported() {
    let mut s = connected(2700);
    let out =
        s.receive(r#"{"push":{"channel":"chatrooms.1.v2","unsub":{"code":2000,"reason":"server unsubscribe"}}}"#, 1);
    assert_eq!(
        out,
        vec![Action::Unsubscribed {
            channel: "chatrooms.1.v2".into(),
            code: 2000,
            reason: "server unsubscribe".into()
        }]
    );
    assert!(!s.is_subscribed("chatrooms.1.v2"));
}

#[test]
fn several_replies_in_one_frame() {
    let mut s = Session::new(SessionOptions::new(vec!["a".into(), "b".into()]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 0);
    let out = s.receive("{\"id\":2,\"subscribe\":{}}\n{\"id\":3,\"subscribe\":{}}\n{}", 1);
    assert_eq!(out.len(), 3);
    assert!(s.is_subscribed("a") && s.is_subscribed("b"));
}

/// (code, terminal), checking that `retry_after_ms` goes with `terminal`.
fn close_of(out: &[Action]) -> (i64, bool) {
    match out {
        [Action::Close { code, terminal, retry_after_ms, .. }] => {
            assert_eq!(*retry_after_ms, if *terminal { 300_000 } else { 0 });
            (*code, *terminal)
        }
        other => panic!("not a close: {other:?}"),
    }
}

#[test]
fn close_says_how_long_to_wait() {
    let push = |code: i64| format!(r#"{{"push":{{"disconnect":{{"code":{code},"reason":"x"}}}}}}"#);
    for (code, terminal) in [(3001, false), (3499, false), (3501, true), (4004, false), (4501, true), (4999, true)] {
        let mut s = connected(2700);
        assert_eq!(close_of(&s.receive(&push(code), 1)), (code, terminal), "{code}");
    }
    // Outside Centrifugo's ranges, the push's own flag: `reconnect: false`
    // is terminal, true or absent is not.
    for (flag, terminal) in [(",\"reconnect\":false", true), (",\"reconnect\":true", false), ("", false)] {
        let mut s = connected(2700);
        let frame = format!(r#"{{"push":{{"disconnect":{{"code":1,"reason":"x"{flag}}}}}}}"#);
        assert_eq!(close_of(&s.receive(&frame, 1)), (1, terminal), "{frame}");
    }
    // A refused connect: an expired token or a temporary error is worth
    // another try at once, being unauthorized is terminal.
    for (error, expected) in [
        (r#"{"code":101,"message":"unauthorized"}"#, (101, true)),
        (r#"{"code":109,"message":"token expired"}"#, (109, false)),
        (r#"{"code":111,"message":"too many requests","temporary":true}"#, (111, false)),
        (r#"{"code":3,"message":"transport"}"#, (3, false)),
    ] {
        let mut s = Session::new(SessionOptions::new(vec![]));
        s.open("t", 0);
        assert_eq!(close_of(&s.receive(&format!(r#"{{"id":1,"error":{error}}}"#), 1)), expected);
    }
    // The same for a refused refresh.
    let mut s = connected(2700);
    s.refresh("t2", 1);
    assert_eq!(close_of(&s.receive(r#"{"id":3,"error":{"code":101,"message":"unauthorized"}}"#, 2)), (101, true));
    // Silence: the session gave up itself, code 0, the ordinary backoff.
    let mut s = connected(2700);
    assert_eq!(close_of(&s.tick(1_000_000)), (0, false));
}

#[test]
fn a_close_serializes_with_the_wait() {
    let mut s = connected(2700);
    let out = s.receive(r#"{"push":{"disconnect":{"code":3501,"reason":"banned"}}}"#, 1);
    assert_eq!(
        serde_json::to_value(&out).unwrap(),
        serde_json::json!([{"action": "close", "code": 3501, "reason": "Kick disconnected: 3501 banned",
            "terminal": true, "retry_after_ms": 300_000}])
    );
}

/// A host's loop, as every driver runs it: terminal closes wait 5, 10, 20,
/// 30 minutes, and the chat feed subscribing starts over.
#[test]
fn terminal_closes_escalate_until_the_chat_feed_subscribes() {
    use kicksdk::realtime::{Backoff, retry_after_ms};
    let mut backoff = Backoff::default();
    let mut waits = vec![];
    let mut s = Session::new(SessionOptions::new(vec!["chatrooms.1.v2".into()]));
    for _ in 0..2 {
        // Refused at connect: Action::Close.
        s.open("t", 0);
        let out = s.receive(r#"{"id":1,"error":{"code":101,"message":"unauthorized"}}"#, 1);
        let [Action::Close { retry_after_ms: wait, .. }] = out.as_slice() else { panic!("{out:?}") };
        waits.push(backoff.next_delay_for(*wait, 0.0));
        s.closed();
        // Connected, then the socket closed with a terminal code.
        s.open("t", 0);
        s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 1);
        s.closed();
        waits.push(backoff.next_delay_for(retry_after_ms(4501), 0.0));
    }
    assert_eq!(waits, [300_000, 600_000, 1_200_000, 1_800_000]);
    // A connection that got as far as `connected` did not reset it; the
    // chat feed subscribing does.
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 1);
    let out = s.receive(r#"{"id":2,"subscribe":{}}"#, 2);
    for a in &out {
        if matches!(a, Action::Subscribed { channel } if channel == "chatrooms.1.v2") {
            backoff.reset();
        }
    }
    assert_eq!(backoff.next_delay_for(retry_after_ms(3501), 0.0), 300_000);
    // An ordinary drop still waits the ordinary backoff.
    assert_eq!(backoff.next_delay_for(retry_after_ms(1006), 0.0), 500);
}

#[test]
fn unsub_codes_2500_to_2999_resubscribe() {
    let mut s = connected(2700);
    let out =
        s.receive(r#"{"push":{"channel":"chatrooms.1.v2","unsub":{"code":2500,"reason":"insufficient state"}}}"#, 1);
    assert_eq!(
        out,
        vec![
            Action::Unsubscribed { channel: "chatrooms.1.v2".into(), code: 2500, reason: "insufficient state".into() },
            Action::Send { text: r#"{"id":3,"subscribe":{"channel":"chatrooms.1.v2"}}"#.into() },
        ]
    );
    assert!(!s.is_subscribed("chatrooms.1.v2"));
    // Closing before the answer does not report it a second time.
    assert_eq!(s.clone().closed(), vec![]);
    assert_eq!(
        s.receive(r#"{"id":3,"subscribe":{}}"#, 2),
        vec![Action::Subscribed { channel: "chatrooms.1.v2".into() }]
    );
    assert!(s.is_subscribed("chatrooms.1.v2"));
}

#[test]
fn no_ping_from_the_server_means_no_silence_timeout() {
    let mut s = Session::new(SessionOptions::new(vec!["a".into()]));
    s.open("t", 0);
    // Waiting for the connect reply still times out.
    assert_eq!(s.deadline(), Some(45_000));
    s.receive(r#"{"id":1,"connect":{"client":"c"}}"#, 10);
    assert_eq!(s.deadline(), None);
    assert_eq!(s.tick(10_000_000), vec![]);
    assert!(s.is_connected());
    // With a token lifetime, only the refresh is scheduled.
    let mut s = Session::new(SessionOptions::new(vec![]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"expires":true,"ttl":60,"ping":0}}"#, 0);
    assert_eq!(s.deadline(), Some(30_000));
}

#[test]
fn numbers_from_kick_and_the_host_cannot_overflow() {
    let mut s = Session::new(SessionOptions::new(vec![]));
    s.open("t", u64::MAX - 5);
    s.receive(
        r#"{"id":1,"connect":{"expires":true,"ttl":18446744073709551615,"ping":18446744073709551615}}"#,
        u64::MAX - 5,
    );
    assert_eq!(s.deadline(), Some(u64::MAX));
    assert_eq!(s.tick(u64::MAX - 1), vec![]);
    // Bounded: a ping of 10 minutes at most, a token of a day at most.
    let mut s = Session::new(SessionOptions::new(vec![]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"expires":true,"ttl":99999999,"ping":99999}}"#, 0);
    assert_eq!(s.deadline(), Some(600_000 + 20_000));
    let mut s = Session::new(SessionOptions::new(vec![]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"expires":true,"ttl":99999999}}"#, 0);
    assert_eq!(s.deadline(), Some(86_280_000), "a day less the lead");
}

#[test]
fn a_feed_listed_twice_is_subscribed_once() {
    let mut s = Session::new(SessionOptions::new(vec!["a".into(), "b".into(), "a".into()]));
    s.open("t", 0);
    let out = s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 0);
    assert_eq!(
        out[1],
        Action::Send {
            text: "{\"id\":2,\"subscribe\":{\"channel\":\"a\"}}\n{\"id\":3,\"subscribe\":{\"channel\":\"b\"}}".into()
        }
    );
}

#[test]
fn feeds_still_waiting_are_reported_on_close_and_reopen() {
    let unsub = |c: &str| Action::Unsubscribed { channel: c.into(), code: 0, reason: "socket closed".into() };
    let mut s = Session::new(SessionOptions::new(vec!["a".into(), "b".into()]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 0);
    s.receive(r#"{"id":2,"subscribe":{}}"#, 1);
    // "b" never answered: it is reported too.
    assert_eq!(s.clone().closed(), vec![unsub("a"), unsub("b")]);
    // open() on a session that was not closed reports the same, then connects.
    let out = s.open("t2", 5);
    assert_eq!(out[..2], [unsub("a"), unsub("b")]);
    assert!(matches!(&out[2], Action::Send { text } if text.starts_with(r#"{"id":1,"connect""#)));
    assert_eq!(s.closed(), vec![]);
}

#[test]
fn signed_urls_never_reach_the_host() {
    let mut s = Session::new(SessionOptions::new(vec!["channel.2".into()]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 0);
    let data = serde_json::json!({
        "slug": "otherchannel",
        "hosted": {"id": 3, "slug": "otherchannel", "username": "otherchannel", "viewers_count": 5, "is_live": true},
        "channel": {"id": 2, "slug": "somechannel", "playback_url": "https://x/master.m3u8?sig=sekrit"}
    })
    .to_string();
    let frame = serde_json::json!({"push": {"channel": "channel.2", "pub": {"data": {
        "event": "App\\Events\\ChatMoveToSupportedChannelEvent", "data": data}}}});
    let out = s.receive(&frame.to_string(), 1);
    let [Action::Event { data, event, .. }] = out.as_slice() else { panic!("{out:?}") };
    let text = data.as_str().unwrap();
    assert!(!text.contains("playback_url") && !text.contains("sekrit"), "{text}");
    assert!(
        matches!(event, kicksdk::KickEvent::HostingOut { hosted } if hosted.slug.as_deref() == Some("otherchannel"))
    );
    assert!(!format!("{out:?}").contains("sekrit"));
}

#[test]
fn a_chat_message_that_mentions_playback_url_is_passed_on_untouched() {
    let mut s = Session::new(SessionOptions::new(vec!["chatrooms.1.v2".into()]));
    s.open("t", 0);
    s.receive(r#"{"id":1,"connect":{"ping":25}}"#, 0);
    // As Kick encodes it: `\/` and `é` escapes, its own key order.
    let data = r##"{"id":"m1","chatroom_id":1,"content":"where is the playback_url? café \/ ok","type":"message","created_at":"2026-10-01T00:00:00+00:00","sender":{"id":2,"username":"somechannel","slug":"somechannel","identity":{"color":"#fff","badges":[]}}}"##;
    let frame = serde_json::json!({"push": {"channel": "chatrooms.1.v2", "pub": {"data": {
        "event": "App\\Events\\ChatMessageEvent", "data": data}}}});
    let out = s.receive(&frame.to_string(), 1);
    let [Action::Event { data: got, event, .. }] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(got.as_str(), Some(data), "byte for byte");
    assert!(matches!(event, kicksdk::KickEvent::ChatMessage(m) if m.content == "where is the playback_url? café / ok"));
}
