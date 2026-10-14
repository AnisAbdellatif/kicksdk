//! Kick's official API, `api.kick.com/public/v1`.
//!
//! An app token ([`token_request`]) reads any channel's public data and
//! subscribes to any channel's webhooks; no permission from the channel is
//! needed. It lasts 60 days.
//!
//! What Kick does that callers should know (`docs/KICK.md` §3):
//!
//! - Batches are **50 ids per request**; the builders refuse more, and the
//!   `*_requests` helpers split a longer list.
//! - **An unknown slug fails the whole request** (400): look slugs up one at
//!   a time, batch by id.
//! - `viewer_count` refreshes **about once a minute**; polling faster sees the
//!   same value.
//! - The API **lags** Kick's own state by seconds: a stream can still be
//!   listed after it ended, or not yet after its start webhook.
//! - There is **no livestream id**: a stream is `(channel, started_at)`, and
//!   `started_at` is string-identical to the status webhook's.
//! - No rate-limit headers; back off on 429.
//!
//! Every builder that takes a `token` refuses one with a control character
//! ([`Error::Invalid`]): it goes into a header.

use crate::http::{self, Endpoints, Error, Method, Request, encode};
use crate::ids::{ChannelId, UserId};
use crate::lenient::{array, boolean, get, int, string, uint};
use serde::Serialize;
use serde_json::{Value, json};

/// The most ids Kick takes in one request.
pub const BATCH: usize = 50;

/// An app access token.
///
/// Its `Debug` hides [`access_token`](Self::access_token); `Serialize`, what
/// a binding hands on, does not.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct AppToken {
    /// The token.
    pub access_token: String,
    /// Seconds until it expires (5 184 000: 60 days).
    pub expires_in: Option<i64>,
    /// `Bearer`.
    pub token_type: Option<String>,
}

impl std::fmt::Debug for AppToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { access_token: _, expires_in, token_type } = self;
        f.debug_struct("AppToken")
            .field("access_token", &"[redacted]")
            .field("expires_in", expires_in)
            .field("token_type", token_type)
            .finish()
    }
}

/// An app token, by the OAuth client-credentials grant.
pub fn token_request(endpoints: &Endpoints, client_id: &str, client_secret: &str) -> Request {
    Request::new(Method::Post, format!("{}/oauth/token", endpoints.id)).form(&[
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ])
}

/// Reads a [`token_request`] answer. OAuth's answer is not in Kick's usual
/// `data` envelope.
pub fn parse_token(status: u16, body: &str) -> Result<AppToken, Error> {
    let v = http::json(status, body)?;
    Ok(AppToken {
        access_token: string(&v, "access_token").ok_or_else(|| Error::shape("no `access_token`"))?,
        expires_in: int(&v, "expires_in"),
        token_type: string(&v, "token_type"),
    })
}

fn batch<T>(ids: &[T]) -> Result<(), Error> {
    match ids.len() {
        0 => Err(Error::Invalid("no ids".into())),
        n if n > BATCH => Err(Error::Invalid(format!("{n} ids; Kick takes at most {BATCH}"))),
        _ => Ok(()),
    }
}

fn query(key: &str, values: impl IntoIterator<Item = String>) -> String {
    values.into_iter().map(|v| format!("{key}={}", encode(&v))).collect::<Vec<_>>().join("&")
}

/// A category, as the API gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Category {
    /// Kick's id.
    pub id: Option<u64>,
    /// Its name.
    pub name: Option<String>,
    /// Its picture.
    pub thumbnail: Option<String>,
}

impl Category {
    fn from(v: Option<&Value>) -> Option<Self> {
        let v = v?;
        Some(Category { id: uint(v, "id"), name: string(v, "name"), thumbnail: string(v, "thumbnail") })
    }
}

/// A live stream, from `/livestreams`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Livestream {
    /// The broadcaster's user id.
    pub broadcaster_user_id: UserId,
    /// The channel id (the one the `channel.<id>` feeds use).
    pub channel_id: Option<ChannelId>,
    /// The slug.
    pub slug: Option<String>,
    /// Viewers now (refreshed about once a minute).
    pub viewer_count: Option<i64>,
    /// When it started, `YYYY-MM-DDTHH:MM:SSZ`: with the channel, the stream's
    /// identity.
    pub started_at: Option<String>,
    /// The title.
    pub stream_title: Option<String>,
    /// The category.
    pub category: Option<Category>,
    /// The language.
    pub language: Option<String>,
    /// Marked mature.
    pub has_mature_content: Option<bool>,
    /// The stream's thumbnail.
    pub thumbnail: Option<String>,
    /// The channel's picture.
    pub profile_picture: Option<String>,
}

/// Live streams of up to [`BATCH`] broadcasters. Offline ones are simply
/// missing from the answer.
pub fn livestreams_request(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Request, Error> {
    batch(ids)?;
    let q = query("broadcaster_user_id", ids.iter().map(|i| i.to_string()));
    Request::new(Method::Get, format!("{}/livestreams?{q}", endpoints.api)).bearer(token)
}

/// [`livestreams_request`] for any number of ids, [`BATCH`] at a time. No
/// ids, no requests.
pub fn livestreams_requests(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Vec<Request>, Error> {
    ids.chunks(BATCH).map(|c| livestreams_request(endpoints, token, c)).collect()
}

/// How to sort a top-streams query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sort {
    /// Most viewers first.
    ViewerCount,
    /// Newest first.
    StartedAt,
}

/// The top live streams overall, or in a category or language: up to 100.
pub fn top_livestreams_request(
    endpoints: &Endpoints,
    token: &str,
    category_id: Option<u64>,
    language: Option<&str>,
    sort: Sort,
    limit: u32,
) -> Result<Request, Error> {
    if !(1..=100).contains(&limit) {
        return Err(Error::Invalid(format!("limit {limit}; Kick takes 1 to 100")));
    }
    let mut q = vec![
        format!("limit={limit}"),
        format!(
            "sort={}",
            match sort {
                Sort::ViewerCount => "viewer_count",
                Sort::StartedAt => "started_at",
            }
        ),
    ];
    if let Some(c) = category_id {
        q.push(format!("category_id={c}"));
    }
    if let Some(l) = language {
        q.push(format!("language={}", encode(l)));
    }
    Request::new(Method::Get, format!("{}/livestreams?{}", endpoints.api, q.join("&"))).bearer(token)
}

/// Reads a `/livestreams` answer. Offline is `[]`, not an error.
pub fn parse_livestreams(status: u16, body: &str) -> Result<Vec<Livestream>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|s| {
            Some(Livestream {
                broadcaster_user_id: UserId(uint(s, "broadcaster_user_id")?),
                channel_id: uint(s, "channel_id").map(ChannelId),
                slug: string(s, "slug"),
                viewer_count: int(s, "viewer_count"),
                started_at: string(s, "started_at"),
                stream_title: string(s, "stream_title"),
                category: Category::from(get(s, "category")),
                language: string(s, "language"),
                has_mature_content: boolean(s, "has_mature_content"),
                thumbnail: string(s, "thumbnail"),
                profile_picture: string(s, "profile_picture"),
            })
        })
        .collect())
}

/// A channel's live state, inside [`Channel`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct StreamState {
    /// Live now.
    pub is_live: bool,
    /// Viewers now.
    pub viewer_count: Option<i64>,
    /// When it started.
    pub start_time: Option<String>,
    /// The language.
    pub language: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
}

/// A channel, from `/channels`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Channel {
    /// The broadcaster's user id.
    pub broadcaster_user_id: UserId,
    /// The current slug: this is where renames show.
    pub slug: Option<String>,
    /// The stream title.
    pub stream_title: Option<String>,
    /// The category.
    pub category: Option<Category>,
    /// The description.
    pub channel_description: Option<String>,
    /// The banner.
    pub banner_picture: Option<String>,
    /// The stream's state.
    pub stream: Option<StreamState>,
    /// Active subscribers (filled with an app token, for any channel).
    pub active_subscribers_count: Option<i64>,
    /// Active gifted subscribers.
    pub active_gifted_subscribers_count: Option<i64>,
    /// Cancelled subscribers.
    pub canceled_subscribers_count: Option<i64>,
}

/// Channels by user id, up to [`BATCH`]. A renamed or banned channel can be
/// missing from the answer.
pub fn channels_by_ids_request(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Request, Error> {
    batch(ids)?;
    let q = query("broadcaster_user_id", ids.iter().map(|i| i.to_string()));
    Request::new(Method::Get, format!("{}/channels?{q}", endpoints.api)).bearer(token)
}

/// Channels by slug, up to [`BATCH`]. One unknown slug fails the whole
/// request with a 400, so this is for slugs already known to exist; check a
/// new one alone.
pub fn channels_by_slugs_request(endpoints: &Endpoints, token: &str, slugs: &[&str]) -> Result<Request, Error> {
    batch(slugs)?;
    let q = query("slug", slugs.iter().map(|s| s.to_string()));
    Request::new(Method::Get, format!("{}/channels?{q}", endpoints.api)).bearer(token)
}

/// Reads a `/channels` answer.
pub fn parse_channels(status: u16, body: &str) -> Result<Vec<Channel>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|c| {
            Some(Channel {
                broadcaster_user_id: UserId(uint(c, "broadcaster_user_id")?),
                slug: string(c, "slug"),
                stream_title: string(c, "stream_title"),
                category: Category::from(get(c, "category")),
                channel_description: string(c, "channel_description"),
                banner_picture: string(c, "banner_picture"),
                stream: get(c, "stream").map(|s| StreamState {
                    is_live: boolean(s, "is_live").unwrap_or(false),
                    viewer_count: int(s, "viewer_count"),
                    start_time: string(s, "start_time"),
                    language: string(s, "language"),
                    tags: array(s, "tags").iter().filter_map(|t| t.as_str().map(str::to_owned)).collect(),
                }),
                active_subscribers_count: int(c, "active_subscribers_count"),
                active_gifted_subscribers_count: int(c, "active_gifted_subscribers_count"),
                canceled_subscribers_count: int(c, "canceled_subscribers_count"),
            })
        })
        .collect())
}

/// The PEM key webhooks are signed with. Needs no token.
pub fn public_key_request(endpoints: &Endpoints) -> Request {
    Request::new(Method::Get, format!("{}/public-key", endpoints.api))
}

/// The PEM from a [`public_key_request`] answer.
pub fn parse_public_key(status: u16, body: &str) -> Result<String, Error> {
    let data = http::data(status, body)?;
    string(&data, "public_key").ok_or_else(|| Error::shape("no `public_key`"))
}

/// A webhook subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Subscription {
    /// Its id, for deleting it.
    pub id: String,
    /// Whose events.
    pub broadcaster_user_id: Option<UserId>,
    /// Which event type.
    pub event: Option<String>,
    /// Its version.
    pub version: Option<i64>,
}

/// Subscribes the app to `events` (name, version) for one channel. The
/// delivery URL is a setting of the Kick app, not part of the request.
pub fn subscribe_request(
    endpoints: &Endpoints,
    token: &str,
    broadcaster: UserId,
    events: &[(&str, u32)],
) -> Result<Request, Error> {
    if events.is_empty() {
        return Err(Error::Invalid("no events".into()));
    }
    let events: Vec<Value> = events.iter().map(|(name, version)| json!({ "name": name, "version": version })).collect();
    Ok(Request::new(Method::Post, format!("{}/events/subscriptions", endpoints.api))
        .bearer(token)?
        .json(&json!({ "broadcaster_user_id": broadcaster.0, "events": events, "method": "webhook" })))
}

/// Every subscription the app has.
pub fn subscriptions_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Get, format!("{}/events/subscriptions", endpoints.api)).bearer(token)
}

/// Reads [`subscriptions_request`] and [`subscribe_request`] answers.
pub fn parse_subscriptions(status: u16, body: &str) -> Result<Vec<Subscription>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|s| {
            Some(Subscription {
                id: crate::lenient::id_string(s, "id").or_else(|| crate::lenient::id_string(s, "subscription_id"))?,
                broadcaster_user_id: uint(s, "broadcaster_user_id").map(UserId),
                event: string(s, "event").or_else(|| string(s, "name")),
                version: int(s, "version"),
            })
        })
        .collect())
}

/// Removes subscriptions, [`BATCH`] per request: a request removing ~330
/// failed every time, 50 works (the real limit is undocumented).
pub fn unsubscribe_requests(endpoints: &Endpoints, token: &str, ids: &[&str]) -> Result<Vec<Request>, Error> {
    ids.chunks(BATCH)
        .map(|c| {
            let q = query("id", c.iter().map(|s| s.to_string()));
            Request::new(Method::Delete, format!("{}/events/subscriptions?{q}", endpoints.api)).bearer(token)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_are_capped_at_fifty() {
        let e = Endpoints::default();
        let ids: Vec<UserId> = (1..=120).map(UserId).collect();
        assert!(livestreams_request(&e, "t", &ids).is_err());
        assert!(livestreams_request(&e, "t", &[]).is_err());
        let reqs = livestreams_requests(&e, "t", &ids).unwrap();
        assert_eq!(reqs.len(), 3);
        assert!(reqs[0].url.ends_with("broadcaster_user_id=50"));
        assert!(reqs[0].headers.contains(&("authorization".into(), "Bearer t".into())));
        let ids: Vec<String> = (0..120).map(|i| format!("s{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert_eq!(unsubscribe_requests(&e, "t", &refs).unwrap().len(), 3);
        assert_eq!(livestreams_requests(&e, "t", &[]).unwrap(), vec![]);
    }

    #[test]
    fn a_token_cannot_add_headers() {
        let e = Endpoints::default();
        let bad = "t\r\nx-evil: 1";
        assert!(livestreams_requests(&e, bad, &[UserId(1)]).is_err());
        assert!(subscriptions_request(&e, bad).is_err());
        assert!(unsubscribe_requests(&e, bad, &["a"]).is_err());
        assert!(channels_by_slugs_request(&e, bad, &["somechannel"]).is_err());
        assert!(top_livestreams_request(&e, bad, None, None, Sort::ViewerCount, 10).is_err());
        assert!(subscribe_request(&e, bad, UserId(1), &[("channel.followed", 1)]).is_err());
    }

    #[test]
    fn debug_hides_the_app_token() {
        let t = parse_token(200, r#"{"access_token":"sekrit","expires_in":5184000,"token_type":"Bearer"}"#).unwrap();
        let shown = format!("{t:?}");
        assert!(!shown.contains("sekrit") && shown.contains("[redacted]") && shown.contains("5184000"), "{shown}");
        assert_eq!(t.access_token, "sekrit");
        assert!(serde_json::to_string(&t).unwrap().contains("sekrit"), "what a binding hands on is untouched");
    }

    #[test]
    fn token_is_a_form() {
        let r = token_request(&Endpoints::default(), "id", "s&cret");
        assert_eq!(r.url, "https://id.kick.com/oauth/token");
        assert_eq!(r.body.as_deref(), Some("grant_type=client_credentials&client_id=id&client_secret=s%26cret"));
    }

    #[test]
    fn offline_is_an_empty_list() {
        assert_eq!(parse_livestreams(200, r#"{"data":[],"message":"OK"}"#).unwrap(), vec![]);
        assert_eq!(parse_livestreams(400, r#"{"message":"Invalid request"}"#).unwrap_err().status(), Some(400));
    }
}
