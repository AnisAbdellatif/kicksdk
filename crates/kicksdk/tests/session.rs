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
    assert!(matches!(out.as_slice(), [Action::Close { reason }] if reason.contains("3001")));
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
    assert!(matches!(out.as_slice(), [Action::Close { reason }] if reason.contains("101")));
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
