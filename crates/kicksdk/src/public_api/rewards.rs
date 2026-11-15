//! Channel point rewards and their redemptions, on the token's own channel:
//! `/public/v1/channels/rewards…`, scopes `channel:rewards:read` (reading)
//! and `channel:rewards:write` (everything).
//!
//! A reward's id and a redemption's id are ULIDs (strings). Only the app
//! that created a reward may change or delete it, or accept and reject its
//! redemptions ([`RedeemedReward::can_manage`]).

use super::common::{at_most, not_blank, query, v1};
use crate::http::{self, Endpoints, Error, Method, Request, segment};
use crate::ids::UserId;
use crate::lenient::{array, boolean, get, id_string, string, uint};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The most rewards a channel has, enabled or not (Kick's spec).
pub const MAX_REWARDS: usize = 50;
/// The most characters in a reward's title (Kick's spec).
pub const MAX_TITLE: usize = 50;
/// The most characters in a reward's description (Kick's spec).
pub const MAX_DESCRIPTION: usize = 200;
/// The most redemptions accepted or rejected in one request (Kick's spec).
pub const MAX_REDEMPTIONS: usize = 25;

/// A reward's settings, for [`create_request`] and [`update_request`]:
/// `None` is "not sent". Start from `RewardFields::default()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RewardFields {
    /// The title, 1 to [`MAX_TITLE`] characters. Needed to create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The cost in channel points, at least 1. Needed to create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<u64>,
    /// Up to [`MAX_DESCRIPTION`] characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `#rrggbb` (Kick's default `#00e701`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    /// Shown to viewers (default `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_enabled: Option<bool>,
    /// Shown but not redeemable for now. Only when updating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_paused: Option<bool>,
    /// The viewer must write something (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_user_input_required: Option<bool>,
    /// Redemptions are accepted at once (default `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub should_redemptions_skip_request_queue: Option<bool>,
}

impl RewardFields {
    fn body(&self) -> Result<Value, Error> {
        if let Some(t) = &self.title {
            not_blank("title", t)?;
            at_most("title", t, MAX_TITLE)?;
        }
        if let Some(d) = &self.description {
            at_most("description", d, MAX_DESCRIPTION)?;
        }
        if self.cost == Some(0) {
            return Err(Error::Invalid("a cost of 0".into()));
        }
        Ok(serde_json::to_value(self)?)
    }
}

/// A reward, as Kick gives it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Reward {
    /// Its id (a ULID).
    pub id: String,
    /// The title.
    pub title: Option<String>,
    /// The description.
    pub description: Option<String>,
    /// The cost in channel points.
    pub cost: Option<u64>,
    /// `#rrggbb`.
    pub background_color: Option<String>,
    /// Shown to viewers.
    pub is_enabled: Option<bool>,
    /// Shown but not redeemable for now.
    pub is_paused: Option<bool>,
    /// The viewer must write something.
    pub is_user_input_required: Option<bool>,
    /// Redemptions are accepted at once.
    pub should_redemptions_skip_request_queue: Option<bool>,
}

impl Reward {
    fn from(v: &Value) -> Option<Self> {
        Some(Reward {
            id: id_string(v, "id")?,
            title: string(v, "title"),
            description: string(v, "description"),
            cost: uint(v, "cost"),
            background_color: string(v, "background_color"),
            is_enabled: boolean(v, "is_enabled"),
            is_paused: boolean(v, "is_paused"),
            is_user_input_required: boolean(v, "is_user_input_required"),
            should_redemptions_skip_request_queue: boolean(v, "should_redemptions_skip_request_queue"),
        })
    }
}

fn rewards(endpoints: &Endpoints, path: &str) -> String {
    v1(endpoints, &format!("/channels/rewards{path}"))
}

/// Every reward of the channel, up to [`MAX_REWARDS`]. Read the answer
/// with [`parse_rewards`].
pub fn list_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Get, rewards(endpoints, "")).bearer(token)
}

/// Reads a [`list_request`] answer. A reward without an id is skipped.
pub fn parse_rewards(status: u16, body: &str) -> Result<Vec<Reward>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list.iter().filter_map(Reward::from).collect())
}

/// Creates a reward; `fields` needs a `title` and a `cost`, and cannot set
/// `is_paused`. Read the answer with [`parse_reward`].
pub fn create_request(endpoints: &Endpoints, token: &str, fields: &RewardFields) -> Result<Request, Error> {
    if fields.title.is_none() || fields.cost.is_none() {
        return Err(Error::Invalid("a new reward needs a title and a cost".into()));
    }
    if fields.is_paused.is_some() {
        return Err(Error::Invalid("a new reward cannot be paused".into()));
    }
    Ok(Request::new(Method::Post, rewards(endpoints, "")).bearer(token)?.json(&fields.body()?))
}

/// Changes what `fields` sets of reward `id`. Refuses `fields` that set
/// nothing. Read the answer with [`parse_reward`].
pub fn update_request(endpoints: &Endpoints, token: &str, id: &str, fields: &RewardFields) -> Result<Request, Error> {
    let body = fields.body()?;
    if body.as_object().is_none_or(|o| o.is_empty()) {
        return Err(Error::Invalid("nothing to change".into()));
    }
    let url = rewards(endpoints, &format!("/{}", segment("reward id", id)?));
    Ok(Request::new(Method::Patch, url).bearer(token)?.json(&body))
}

/// Reads a [`create_request`] or [`update_request`] answer: the reward as
/// it now is.
pub fn parse_reward(status: u16, body: &str) -> Result<Reward, Error> {
    Reward::from(&http::data(status, body)?).ok_or_else(|| Error::shape("no reward `id`"))
}

/// Deletes reward `id`. Kick answers 204: read it with
/// [`parse_done`](super::parse_done).
pub fn delete_request(endpoints: &Endpoints, token: &str, id: &str) -> Result<Request, Error> {
    let url = rewards(endpoints, &format!("/{}", segment("reward id", id)?));
    Request::new(Method::Delete, url).bearer(token)
}

/// Where a redemption stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RedemptionStatus {
    /// Waiting for the broadcaster (or an app) to accept or reject it.
    Pending,
    /// Accepted.
    Accepted,
    /// Rejected (Kick refunds the points).
    Rejected,
}

impl RedemptionStatus {
    /// As Kick writes it: `pending`, `accepted`, `rejected`.
    pub fn as_str(self) -> &'static str {
        match self {
            RedemptionStatus::Pending => "pending",
            RedemptionStatus::Accepted => "accepted",
            RedemptionStatus::Rejected => "rejected",
        }
    }
}

/// Redemptions, grouped by reward: of one reward (`reward_id`) or all, with
/// one `status` (Kick's default: pending), from `cursor` (a page's
/// [`RedemptionPage::next_cursor`]). Read the answer with
/// [`parse_redemptions`].
pub fn redemptions_request(
    endpoints: &Endpoints,
    token: &str,
    reward_id: Option<&str>,
    status: Option<RedemptionStatus>,
    cursor: Option<&str>,
) -> Result<Request, Error> {
    let mut q = Vec::new();
    if let Some(r) = reward_id {
        q.push(("reward_id", r.to_owned()));
    }
    if let Some(s) = status {
        q.push(("status", s.as_str().to_owned()));
    }
    if let Some(c) = cursor {
        q.push(("cursor", c.to_owned()));
    }
    Request::new(Method::Get, rewards(endpoints, &format!("/redemptions{}", query(&q)))).bearer(token)
}

/// Redemptions by their ids, whatever their status. Kick takes no other
/// filter with ids. Read the answer with [`parse_redemptions`].
pub fn redemptions_by_ids_request(
    endpoints: &Endpoints,
    token: &str,
    ids: &[&str],
    cursor: Option<&str>,
) -> Result<Request, Error> {
    if ids.is_empty() {
        return Err(Error::Invalid("no ids".into()));
    }
    let mut q: Vec<(&str, String)> = ids.iter().map(|i| ("id", (*i).to_owned())).collect();
    if let Some(c) = cursor {
        q.push(("cursor", c.to_owned()));
    }
    Request::new(Method::Get, rewards(endpoints, &format!("/redemptions{}", query(&q)))).bearer(token)
}

/// A redemption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Redemption {
    /// Its id (a ULID), for [`accept_request`] and [`reject_request`].
    pub id: String,
    /// `pending`, `accepted` or `rejected`, as Kick wrote it.
    pub status: Option<String>,
    /// When, `YYYY-MM-DDTHH:MM:SSZ`.
    pub redeemed_at: Option<String>,
    /// Who redeemed it.
    pub redeemer: Option<UserId>,
    /// What they wrote, if the reward asks for it.
    pub user_input: Option<String>,
}

/// The reward a group of redemptions is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RedeemedReward {
    /// Its id (a ULID).
    pub id: String,
    /// The title.
    pub title: Option<String>,
    /// The description.
    pub description: Option<String>,
    /// The cost in channel points.
    pub cost: Option<u64>,
    /// This app may accept and reject these redemptions.
    pub can_manage: Option<bool>,
    /// The reward was deleted since (Kick only says so when it was).
    pub is_deleted: bool,
}

/// One reward's redemptions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RewardRedemptions {
    /// The reward.
    pub reward: RedeemedReward,
    /// Its redemptions.
    pub redemptions: Vec<Redemption>,
}

/// One page of redemptions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RedemptionPage {
    /// By reward.
    pub rewards: Vec<RewardRedemptions>,
    /// The next page's cursor; `None` (or Kick's empty string) on the last.
    pub next_cursor: Option<String>,
}

/// Reads a [`redemptions_request`] or [`redemptions_by_ids_request`]
/// answer. A group without a reward id, or a redemption without an id, is
/// skipped.
pub fn parse_redemptions(status: u16, body: &str) -> Result<RedemptionPage, Error> {
    let v = http::json(status, body)?;
    let list = get(&v, "data").and_then(Value::as_array).ok_or_else(|| Error::shape("`data` is not a list"))?;
    let rewards = list
        .iter()
        .filter_map(|g| {
            let r = get(g, "reward")?;
            Some(RewardRedemptions {
                reward: RedeemedReward {
                    id: id_string(r, "id")?,
                    title: string(r, "title"),
                    description: string(r, "description"),
                    cost: uint(r, "cost"),
                    can_manage: boolean(r, "can_manage"),
                    is_deleted: boolean(r, "is_deleted").unwrap_or(false),
                },
                redemptions: array(g, "redemptions")
                    .iter()
                    .filter_map(|x| {
                        Some(Redemption {
                            id: id_string(x, "id")?,
                            status: string(x, "status"),
                            redeemed_at: string(x, "redeemed_at"),
                            redeemer: get(x, "redeemer").and_then(|u| uint(u, "user_id")).map(UserId),
                            user_input: string(x, "user_input"),
                        })
                    })
                    .collect(),
            })
        })
        .collect();
    let next_cursor = get(&v, "pagination").and_then(|p| string(p, "next_cursor")).filter(|c| !c.is_empty());
    Ok(RedemptionPage { rewards, next_cursor })
}

fn decide(endpoints: &Endpoints, token: &str, what: &str, ids: &[&str]) -> Result<Request, Error> {
    match ids.len() {
        0 => return Err(Error::Invalid("no ids".into())),
        n if n > MAX_REDEMPTIONS => {
            return Err(Error::Invalid(format!("{n} ids; Kick takes at most {MAX_REDEMPTIONS}")));
        }
        _ => {}
    }
    if ids.iter().enumerate().any(|(i, id)| ids[..i].contains(id)) {
        return Err(Error::Invalid("an id twice".into()));
    }
    Ok(Request::new(Method::Post, rewards(endpoints, &format!("/redemptions/{what}")))
        .bearer(token)?
        .json(&json!({ "ids": ids })))
}

/// Accepts pending redemptions, 1 to [`MAX_REDEMPTIONS`] distinct ids. Read
/// the answer with [`parse_failures`].
pub fn accept_request(endpoints: &Endpoints, token: &str, ids: &[&str]) -> Result<Request, Error> {
    decide(endpoints, token, "accept", ids)
}

/// Rejects pending redemptions, 1 to [`MAX_REDEMPTIONS`] distinct ids. Read
/// the answer with [`parse_failures`].
pub fn reject_request(endpoints: &Endpoints, token: &str, ids: &[&str]) -> Result<Request, Error> {
    decide(endpoints, token, "reject", ids)
}

/// A redemption Kick did not accept or reject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct FailedRedemption {
    /// Its id.
    pub id: String,
    /// Why: `NOT_PENDING`, `NOT_FOUND`, `NOT_OWNED` (another app's reward) or
    /// `UNKNOWN`, as Kick wrote it.
    pub reason: Option<String>,
}

/// Reads an [`accept_request`] or [`reject_request`] answer: the ids that
/// failed. Empty is all done.
pub fn parse_failures(status: u16, body: &str) -> Result<Vec<FailedRedemption>, Error> {
    let data = http::data(status, body)?;
    if data.is_null() {
        return Ok(vec![]);
    }
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|f| Some(FailedRedemption { id: id_string(f, "id")?, reason: string(f, "reason") }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(r: &Request) -> Value {
        serde_json::from_str(r.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn rewards_are_created_changed_and_deleted() {
        let e = Endpoints::default();
        let base = "https://api.kick.com/public/v1/channels/rewards";
        let r = list_request(&e, "t").unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Get, base));

        let mut f = RewardFields { title: Some("Song Request".into()), cost: Some(100), ..Default::default() };
        let r = create_request(&e, "t", &f).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, base));
        assert_eq!(body(&r), json!({ "title": "Song Request", "cost": 100 }));
        f.is_paused = Some(true);
        assert!(create_request(&e, "t", &f).is_err());
        assert!(create_request(&e, "t", &RewardFields { cost: Some(1), ..Default::default() }).is_err());

        let p = RewardFields { is_paused: Some(false), ..Default::default() };
        let r = update_request(&e, "t", "01HZ8X9K", &p).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Patch, format!("{base}/01HZ8X9K").as_str()));
        assert_eq!(body(&r), json!({ "is_paused": false }));
        assert!(matches!(update_request(&e, "t", "01HZ8X9K", &RewardFields::default()), Err(Error::Invalid(_))));

        let r = delete_request(&e, "t", "01HZ8X9K").unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Delete, format!("{base}/01HZ8X9K").as_str()));
        for bad in ["", ".", ".."] {
            assert!(update_request(&e, "t", bad, &p).is_err());
            assert!(delete_request(&e, "t", bad).is_err());
        }
    }

    #[test]
    fn reward_limits() {
        let e = Endpoints::default();
        let f = |title: &str, description: &str, cost| RewardFields {
            title: Some(title.into()),
            description: Some(description.into()),
            cost: Some(cost),
            ..Default::default()
        };
        assert!(create_request(&e, "t", &f(&"x".repeat(MAX_TITLE), &"y".repeat(MAX_DESCRIPTION), 1)).is_ok());
        assert!(create_request(&e, "t", &f(&"x".repeat(MAX_TITLE + 1), "", 1)).is_err());
        assert!(create_request(&e, "t", &f("x", &"y".repeat(MAX_DESCRIPTION + 1), 1)).is_err());
        assert!(create_request(&e, "t", &f(" ", "", 1)).is_err());
        assert!(create_request(&e, "t", &f("x", "", 0)).is_err());
        let unknown = serde_json::from_value::<RewardFields>(json!({ "titel": "x" }));
        assert!(unknown.is_err(), "a misspelt field is refused, not dropped");
    }

    #[test]
    fn redemptions_are_listed_and_decided() {
        let e = Endpoints::default();
        let base = "https://api.kick.com/public/v1/channels/rewards/redemptions";
        assert_eq!(redemptions_request(&e, "t", None, None, None).unwrap().url, base);
        let r = redemptions_request(&e, "t", Some("01A"), Some(RedemptionStatus::Accepted), Some("c 1")).unwrap();
        assert_eq!(r.url, format!("{base}?reward_id=01A&status=accepted&cursor=c%201"));
        let r = redemptions_by_ids_request(&e, "t", &["a", "b"], None).unwrap();
        assert_eq!(r.url, format!("{base}?id=a&id=b"));
        assert!(redemptions_by_ids_request(&e, "t", &[], None).is_err());

        let r = accept_request(&e, "t", &["a", "b"]).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, format!("{base}/accept").as_str()));
        assert_eq!(body(&r), json!({ "ids": ["a", "b"] }));
        assert_eq!(reject_request(&e, "t", &["a"]).unwrap().url, format!("{base}/reject"));
        let ids: Vec<String> = (0..=MAX_REDEMPTIONS).map(|i| i.to_string()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert!(accept_request(&e, "t", &ids[..MAX_REDEMPTIONS]).is_ok());
        assert!(accept_request(&e, "t", &ids).is_err());
        assert!(reject_request(&e, "t", &[]).is_err());
        assert!(reject_request(&e, "t", &["a", "b", "a"]).is_err());
    }

    #[test]
    fn answers_are_read_leniently() {
        let body = r#"{"data":[{"reward":{"id":"r","cost":"5"},"redemptions":[{"id":"x","redeemer":{"user_id":"7"}},{}]},
            {"redemptions":[]}],"message":"OK","pagination":{"next_cursor":""}}"#;
        let p = parse_redemptions(200, body).unwrap();
        assert_eq!(p.rewards.len(), 1);
        assert_eq!(p.rewards[0].reward.cost, Some(5));
        assert!(!p.rewards[0].reward.is_deleted);
        assert_eq!(p.rewards[0].redemptions.len(), 1);
        assert_eq!(p.rewards[0].redemptions[0].redeemer, Some(UserId(7)));
        assert_eq!(p.next_cursor, None);
        assert!(parse_failures(200, r#"{"data":null,"message":"OK"}"#).unwrap().is_empty());
        assert!(parse_failures(200, r#"{"data":[],"message":"OK"}"#).unwrap().is_empty());
        assert!(matches!(parse_reward(200, r#"{"data":{"title":"x"}}"#), Err(Error::Shape(_))));
        assert_eq!(parse_rewards(401, r#"{"message":"Unauthorized"}"#).unwrap_err().status(), Some(401));
    }
}
