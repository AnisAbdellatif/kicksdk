//! The user-token (and drops) builders and parsers against Kick's documented
//! examples, `fixtures/docs/public_api/actions-*.json`: each builder makes the
//! documented request, and each parser reads the documented answer. These are
//! transcribed from Kick's docs, not recorded (`fixtures/docs/README.md`).

use kicksdk::public_api::{self, ads, channels, chat, drops, moderation, rewards};
use kicksdk::{Endpoints, Error, Request, UserId};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

struct Fixture {
    request: Value,
    status: u16,
    body: String,
}

fn fixture(name: &str) -> Fixture {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/docs/public_api/actions-{name}.json"));
    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    Fixture {
        request: v["request"].clone(),
        status: v["response"]["status"].as_u64().unwrap() as u16,
        body: v["response"]["body"].as_str().unwrap().to_owned(),
    }
}

/// The request is the documented one: method, URL, query and JSON body.
fn same(r: &Request, f: &Fixture) {
    assert_eq!(serde_json::to_value(r.method).unwrap(), f.request["method"]);
    let (url, query) = r.url.split_once('?').unwrap_or((&r.url, ""));
    assert_eq!(url, f.request["url"]);
    let params: Map<String, Value> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
        .collect();
    let wanted: Map<String, Value> = f.request["params"]
        .as_object()
        .map(|m| {
            m.iter().map(|(k, v)| (k.clone(), Value::String(v.to_string().trim_matches('"').to_owned()))).collect()
        })
        .unwrap_or_default();
    assert_eq!(params, wanted, "{}", r.url);
    let body: Value = r.body.as_deref().map(|b| serde_json::from_str(b).unwrap()).unwrap_or(Value::Null);
    assert_eq!(body, f.request["json"], "{}", r.url);
    assert!(r.headers.contains(&("authorization".into(), "Bearer t".into())));
}

#[test]
fn chat() {
    let e = Endpoints::default();
    let f = fixture("chat-send");
    let reply = f.request["json"]["reply_to_message_id"].as_str();
    same(&chat::send_request(&e, "t", chat::Sender::User { broadcaster: UserId(123) }, "Pog", reply).unwrap(), &f);
    let sent = chat::parse_send(f.status, &f.body).unwrap();
    assert!(sent.is_sent);
    assert_eq!(sent.message_id.as_deref(), Some("828f83bb-e391-4c78-9e91-af35a172840e"));

    let f = fixture("chat-delete");
    same(&chat::delete_request(&e, "t", "828f83bb-e391-4c78-9e91-af35a172840e").unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();
}

#[test]
fn moderation() {
    let e = Endpoints::default();
    let f = fixture("moderation-timeout");
    same(&moderation::timeout_request(&e, "t", UserId(123), UserId(333), 10, Some("Not vibing")).unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();
    let f = fixture("moderation-unban");
    same(&moderation::unban_request(&e, "t", UserId(123), UserId(333)).unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();
}

#[test]
fn channel_update() {
    let f = fixture("channels-update");
    let r = channels::update_request(
        &Endpoints::default(),
        "t",
        Some(123),
        Some(&["pog", "chad"]),
        Some("My first stream"),
    );
    same(&r.unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();
}

#[test]
fn rewards() {
    let e = Endpoints::default();
    let id = "01HZ8X9K2M4N6P8Q0R2S4T6V8W0Y2Z4";

    let f = fixture("rewards-list");
    same(&rewards::list_request(&e, "t").unwrap(), &f);
    let list = rewards::parse_rewards(f.status, &f.body).unwrap();
    assert_eq!((list.len(), list[0].id.as_str(), list[0].cost), (1, id, Some(100)));

    for (name, update) in [("rewards-create", false), ("rewards-update", true)] {
        let f = fixture(name);
        let fields: rewards::RewardFields = serde_json::from_value(f.request["json"].clone()).unwrap();
        let r = match update {
            false => rewards::create_request(&e, "t", &fields),
            true => rewards::update_request(&e, "t", id, &fields),
        };
        same(&r.unwrap(), &f);
        let reward = rewards::parse_reward(f.status, &f.body).unwrap();
        assert_eq!(
            (reward.id.as_str(), reward.title.as_deref(), reward.is_paused),
            (id, Some("Song Request"), Some(false))
        );
    }

    let f = fixture("rewards-delete");
    same(&rewards::delete_request(&e, "t", id).unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();

    let f = fixture("rewards-redemptions");
    same(&rewards::redemptions_request(&e, "t", None, Some(rewards::RedemptionStatus::Pending), None).unwrap(), &f);
    let page = rewards::parse_redemptions(f.status, &f.body).unwrap();
    let group = &page.rewards[0];
    assert_eq!((group.reward.title.as_deref(), group.reward.can_manage), (Some("Unban Request"), Some(true)));
    let x = &group.redemptions[0];
    assert_eq!(
        (x.status.as_deref(), x.redeemer, x.user_input.as_deref()),
        (Some("pending"), Some(UserId(123)), Some("sorry mr strimer"))
    );
    assert!(page.next_cursor.is_some());

    let ids = ["01KEWZ2RKYZZFGD154K50Y3F8D", "01KEWZ2YC27070XWMHKFJR6C87"];
    for (name, r) in [
        ("rewards-accept", rewards::accept_request(&e, "t", &ids)),
        ("rewards-reject", rewards::reject_request(&e, "t", &ids)),
    ] {
        let f = fixture(name);
        same(&r.unwrap(), &f);
        let failed = rewards::parse_failures(f.status, &f.body).unwrap();
        assert_eq!(
            (failed[0].id.as_str(), failed[0].reason.as_deref()),
            ("01KERB68MZZ04551D6X5TNRFYV", Some("NOT_OWNED"))
        );
    }
}

#[test]
fn ads() {
    let e = Endpoints::default();
    let uuid = "123e4567-e89b-12d3-a456-426614174000";
    let f = fixture("ads-ad-break");
    same(&ads::ad_break_request(&e, "t", uuid, 30).unwrap(), &f);
    let b = ads::parse_ad_break(f.status, &f.body).unwrap();
    assert_eq!((b.ad_break_id.as_deref(), b.remaining_ad_breaks), (Some("Xq3LmP9aB2cD"), Some(2)));

    let f = fixture("ads-ad-break-refused");
    same(&ads::ad_break_request(&e, "t", uuid, 30).unwrap(), &f);
    match ads::parse_ad_break(f.status, &f.body).unwrap_err() {
        Error::Status { status: 400, excerpt } => assert!(excerpt.contains("CHANNEL_IS_NOT_LIVE")),
        other => panic!("{other:?}"),
    }

    let f = fixture("ads-status");
    same(&ads::status_request(&e, "t").unwrap(), &f);
    let s = ads::parse_status(f.status, &f.body).unwrap();
    assert_eq!((s.opted_in, s.remaining_ad_breaks, s.ad_breaks.len()), (Some(true), Some(2), 1));
    let limits = s.limits.unwrap();
    assert_eq!((limits.max, limits.period_seconds), (Some(3), Some(3600)));
    assert_eq!(s.ad_breaks[0].status.as_deref(), Some("inserted"));

    let f = fixture("ads-enroll");
    same(&ads::enroll_request(&e, "t").unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();
}

#[test]
fn drops() {
    let e = Endpoints::default();
    let f = fixture("drops-claims");
    let filter: drops::ClaimFilter = serde_json::from_value(f.request["params"].clone()).unwrap();
    same(&drops::claims_request(&e, "t", &filter).unwrap(), &f);
    let c = drops::parse_claims(f.status, &f.body).unwrap();
    // The guide's claim is a type sketch: its `"user_id": "integer"` is read as no id.
    assert_eq!(
        (c.claims.len(), c.claims[0].user_id, c.cursor.as_deref()),
        (1, None, Some("01K0TDDR08Q5ZNWXK92SDH7SDH"))
    );

    let f = fixture("drops-update");
    let pairs = [("01KAAFHJ2PNXS48NG8XXPGWKCZ", "processed"), ("01KAAFHJ2PNXS48NG8XXPGWKCA", "initiated")];
    same(&drops::update_claims_request(&e, "t", &pairs).unwrap(), &f);
    public_api::parse_done(f.status, &f.body).unwrap();

    let f = fixture("drops-unauthorized");
    assert_eq!(drops::parse_claims(f.status, &f.body).unwrap_err().status(), Some(401));
}
