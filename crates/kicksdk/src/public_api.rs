//! Kick's official API, `api.kick.com/public/v1` and `/public/v2`.
//!
//! [`Endpoints::api`] is the host alone (`https://api.kick.com`); every
//! builder adds its path, version included.
//!
//! An app token ([`token_request`]) reads any channel's public data and
//! subscribes to any channel's webhooks; no permission from the channel is
//! needed. It lasts 60 days.
//!
//! What Kick does that callers should know (`docs/KICK.md` §3):
//!
//! - Batches are **50 ids per request** ([`BATCH`]) for `/channels`, **100**
//!   ([`LIVESTREAMS_BATCH`]) for live streams by user id; the builders refuse
//!   more, and the `*_requests` helpers split a longer list.
//! - **An unknown slug fails the whole request** (400): look slugs up one at
//!   a time, batch by id.
//! - `viewer_count` refreshes **about once a minute**; polling faster sees the
//!   same value.
//! - The API **lags** Kick's own state by seconds: a stream can still be
//!   listed after it ended, or not yet after its start webhook.
//! - A stream is identified by `(channel, started_at)`: `started_at` is
//!   string-identical to the status webhook's. The current livestreams
//!   endpoints also give a livestream `id` (Kick's spec; not yet recorded).
//! - `GET /public/v1/livestreams` is deprecated: [`livestreams_request`]
//!   uses `/public/v1/users/livestreams`, [`livestreams_page_request`]
//!   `/public/v2/livestreams`, which has no sort.
//! - No rate-limit headers; back off on 429.
//!
//! Every builder that takes a `token` refuses one with a control character
//! ([`Error::Invalid`]): it goes into a header.

use crate::http::{self, Endpoints, Error, Method, Request, encode};
use crate::ids::{ChannelId, UserId};
use crate::lenient::{array, boolean, get, id_string, int, string, uint};
use serde::Serialize;
use serde_json::{Value, json};

pub mod ads;
pub mod channels;
pub mod chat;
mod common;
pub mod drops;
pub mod moderation;
pub mod rewards;

pub use common::parse_done;

/// The most ids Kick takes in one request where it says nothing else
/// (`/channels`, [`users_request`], removing subscriptions).
pub const BATCH: usize = 50;

/// The most user ids [`livestreams_request`] takes (Kick's spec).
pub const LIVESTREAMS_BATCH: usize = 100;

/// The most categories, and the most languages, [`livestreams_page_request`]
/// filters by (Kick's spec).
pub const LIVESTREAMS_FILTERS: usize = 25;

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
    batch_of(ids, BATCH)
}

/// One to `max` ids.
fn batch_of<T>(ids: &[T], max: usize) -> Result<(), Error> {
    match ids.len() {
        0 => Err(Error::Invalid("no ids".into())),
        n if n > max => Err(Error::Invalid(format!("{n} ids; Kick takes at most {max}"))),
        _ => Ok(()),
    }
}

/// `key=a&key=b`: a list as Kick's "multi" parameters take it.
fn query(key: &str, values: impl IntoIterator<Item = String>) -> String {
    values.into_iter().map(|v| format!("{key}={}", encode(&v))).collect::<Vec<_>>().join("&")
}

/// `key=a,b`: a list as Kick's "csv" parameters take it, or nothing for an
/// empty one. A value with a comma is refused: Kick would split it in two.
fn csv(key: &str, values: impl IntoIterator<Item = String>) -> Result<Option<String>, Error> {
    let values: Vec<String> = values.into_iter().collect();
    if let Some(v) = values.iter().find(|v| v.contains(',')) {
        return Err(Error::Invalid(format!("{key} {v:?} has a comma; Kick splits the list on commas")));
    }
    Ok((!values.is_empty())
        .then(|| format!("{key}={}", values.iter().map(|v| encode(v)).collect::<Vec<_>>().join(","))))
}

/// `limit` within `1..=max`, as `key=n`; nothing for Kick's default.
fn bounded(key: &str, n: Option<u32>, max: u32) -> Result<Option<String>, Error> {
    match n {
        None => Ok(None),
        Some(n) if (1..=max).contains(&n) => Ok(Some(format!("{key}={n}"))),
        Some(n) => Err(Error::Invalid(format!("{key} {n}; Kick takes 1 to {max}"))),
    }
}

/// `{api}{path}`, with the query parts that are there.
fn url(endpoints: &Endpoints, path: &str, parts: impl IntoIterator<Item = Option<String>>) -> String {
    let q: Vec<String> = parts.into_iter().flatten().filter(|p| !p.is_empty()).collect();
    match q.is_empty() {
        true => format!("{}{path}", endpoints.api),
        false => format!("{}{path}?{}", endpoints.api, q.join("&")),
    }
}

/// A list of strings, missing or not as empty.
fn texts(v: &Value, key: &str) -> Vec<String> {
    array(v, key).iter().filter_map(|t| t.as_str().map(str::to_owned)).collect()
}

/// One page of a paginated (`/public/v2`) answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Page<T> {
    /// This page.
    pub data: Vec<T>,
    /// What to pass as `cursor` for the next page; `None` when Kick gives
    /// none (missing or empty).
    pub next_cursor: Option<String>,
}

/// Kick's paginated envelope, `{"data": [...], "pagination": {"next_cursor"}}`.
fn page<T>(status: u16, body: &str, item: impl Fn(&Value) -> Option<T>) -> Result<Page<T>, Error> {
    let v = http::json(status, body)?;
    let list = get(&v, "data").and_then(Value::as_array).ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(Page {
        data: list.iter().filter_map(item).collect(),
        next_cursor: get(&v, "pagination").and_then(|p| string(p, "next_cursor")).filter(|c| !c.is_empty()),
    })
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
    /// Its tags: only [`categories_request`] gives them; empty elsewhere.
    pub tags: Vec<String>,
}

impl Category {
    fn from(v: Option<&Value>) -> Option<Self> {
        let v = v?;
        Some(Category {
            id: uint(v, "id"),
            name: string(v, "name"),
            thumbnail: string(v, "thumbnail"),
            tags: texts(v, "tags"),
        })
    }
}

/// A live stream, from [`livestreams_request`] or [`livestreams_page_request`].
///
/// One type reads both of Kick's shapes: the deprecated
/// `/public/v1/livestreams` (flat, recorded) and the current one
/// (`endpoints.LivestreamV2` in Kick's spec, not yet recorded), which nests
/// the broadcaster and the slug. Fields keep their old names; the comment on
/// each says where the current shape has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Livestream {
    /// The broadcaster's user id (`broadcaster_user.id`).
    pub broadcaster_user_id: UserId,
    /// The livestream's id, a UUID. Only the current shape has it.
    pub id: Option<String>,
    /// The channel id (the one the `channel.<id>` feeds use). Only the old
    /// shape had it.
    pub channel_id: Option<ChannelId>,
    /// The slug (`channel.slug`).
    pub slug: Option<String>,
    /// The broadcaster's username (`broadcaster_user.username`). Only the
    /// current shape has it.
    pub username: Option<String>,
    /// Viewers now (refreshed about once a minute).
    pub viewer_count: Option<i64>,
    /// When it started, `YYYY-MM-DDTHH:MM:SSZ`: with the channel, the stream's
    /// identity.
    pub started_at: Option<String>,
    /// The title (`title`).
    pub stream_title: Option<String>,
    /// The category.
    pub category: Option<Category>,
    /// The language (`language_code`).
    pub language: Option<String>,
    /// The stream's tags (the old shape's `custom_tags`, never seen filled).
    pub tags: Vec<String>,
    /// Marked mature.
    pub has_mature_content: Option<bool>,
    /// The stream's thumbnail.
    pub thumbnail: Option<String>,
    /// The broadcaster's picture (`broadcaster_user.profile_picture`).
    pub profile_picture: Option<String>,
}

impl Livestream {
    fn from(s: &Value) -> Option<Self> {
        let user = get(s, "broadcaster_user");
        let of_user = |key: &str| user.and_then(|u| string(u, key));
        let tags = match texts(s, "tags") {
            t if t.is_empty() => texts(s, "custom_tags"),
            t => t,
        };
        Some(Livestream {
            broadcaster_user_id: UserId(uint(s, "broadcaster_user_id").or_else(|| user.and_then(|u| uint(u, "id")))?),
            id: id_string(s, "id"),
            channel_id: uint(s, "channel_id").map(ChannelId),
            slug: string(s, "slug").or_else(|| get(s, "channel").and_then(|c| string(c, "slug"))),
            username: of_user("username"),
            viewer_count: int(s, "viewer_count"),
            started_at: string(s, "started_at"),
            stream_title: string(s, "stream_title").or_else(|| string(s, "title")),
            category: Category::from(get(s, "category")),
            language: string(s, "language").or_else(|| string(s, "language_code")),
            tags,
            has_mature_content: boolean(s, "has_mature_content"),
            thumbnail: string(s, "thumbnail"),
            profile_picture: string(s, "profile_picture").or_else(|| of_user("profile_picture")),
        })
    }
}

/// Live streams of up to [`LIVESTREAMS_BATCH`] broadcasters, by user id
/// (`GET /public/v1/users/livestreams`). Offline ones are simply missing
/// from the answer.
pub fn livestreams_request(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Request, Error> {
    batch_of(ids, LIVESTREAMS_BATCH)?;
    let q = query("user_id", ids.iter().map(|i| i.to_string()));
    Request::new(Method::Get, url(endpoints, "/public/v1/users/livestreams", [Some(q)])).bearer(token)
}

/// [`livestreams_request`] for any number of ids, [`LIVESTREAMS_BATCH`] at a
/// time. No ids, no requests.
pub fn livestreams_requests(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Vec<Request>, Error> {
    ids.chunks(LIVESTREAMS_BATCH).map(|c| livestreams_request(endpoints, token, c)).collect()
}

/// One page of the streams live now (`GET /public/v2/livestreams`), in up to
/// [`LIVESTREAMS_FILTERS`] categories and languages (none: all of them).
/// `limit` is 1 to 1000 (Kick's default: 100); `cursor` is a previous page's
/// [`Page::next_cursor`] (`None` or empty: the first page).
///
/// There is no sort: Kick lists streams oldest first. For the most watched,
/// read every page and sort by `viewer_count`.
pub fn livestreams_page_request(
    endpoints: &Endpoints,
    token: &str,
    category_ids: &[u64],
    language_codes: &[&str],
    limit: Option<u32>,
    cursor: Option<&str>,
) -> Result<Request, Error> {
    for (what, n) in [("categories", category_ids.len()), ("languages", language_codes.len())] {
        if n > LIVESTREAMS_FILTERS {
            return Err(Error::Invalid(format!("{n} {what}; Kick takes at most {LIVESTREAMS_FILTERS}")));
        }
    }
    let parts = [
        Some(query("category_id", category_ids.iter().map(u64::to_string))),
        Some(query("language_code", language_codes.iter().map(|l| l.to_string()))),
        bounded("limit", limit, 1000)?,
        cursor.filter(|c| !c.is_empty()).map(|c| format!("cursor={}", encode(c))),
    ];
    Request::new(Method::Get, url(endpoints, "/public/v2/livestreams", parts)).bearer(token)
}

/// Reads a [`livestreams_request`] answer, or the streams in a
/// [`livestreams_page_request`] answer (without its cursor: see
/// [`parse_livestreams_page`]). Also reads the deprecated
/// `/public/v1/livestreams`' answers. Offline is `[]`, not an error.
pub fn parse_livestreams(status: u16, body: &str) -> Result<Vec<Livestream>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list.iter().filter_map(Livestream::from).collect())
}

/// Reads a [`livestreams_page_request`] answer: the streams and the next
/// page's cursor.
pub fn parse_livestreams_page(status: u16, body: &str) -> Result<Page<Livestream>, Error> {
    page(status, body, Livestream::from)
}

/// How many streams are live now, from [`livestream_stats_request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct LivestreamStats {
    /// Streams live on Kick.
    pub total_count: Option<i64>,
}

/// How many streams are live on Kick (`GET /public/v1/livestreams/stats`).
/// An app or a user token.
pub fn livestream_stats_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Get, url(endpoints, "/public/v1/livestreams/stats", [])).bearer(token)
}

/// Reads a [`livestream_stats_request`] answer.
pub fn parse_livestream_stats(status: u16, body: &str) -> Result<LivestreamStats, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    Ok(LivestreamStats { total_count: int(&data, "total_count") })
}

/// A user, from [`users_request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct User {
    /// Their id.
    pub user_id: UserId,
    /// Their username.
    pub name: Option<String>,
    /// Their picture.
    pub profile_picture: Option<String>,
    /// Their email: only for a user token with `user:read`, for its own user.
    pub email: Option<String>,
}

/// Users by id, up to [`BATCH`] (`GET /public/v1/users`), with an app or a
/// user token. No ids, with a user token: the token's own user.
pub fn users_request(endpoints: &Endpoints, token: &str, ids: &[UserId]) -> Result<Request, Error> {
    if ids.len() > BATCH {
        return Err(Error::Invalid(format!("{} ids; at most {BATCH}", ids.len())));
    }
    let q = query("id", ids.iter().map(|i| i.to_string()));
    Request::new(Method::Get, url(endpoints, "/public/v1/users", [Some(q)])).bearer(token)
}

/// Reads a [`users_request`] answer.
pub fn parse_users(status: u16, body: &str) -> Result<Vec<User>, Error> {
    let data = http::data(status, body)?;
    let list = data.as_array().ok_or_else(|| Error::shape("`data` is not a list"))?;
    Ok(list
        .iter()
        .filter_map(|u| {
            Some(User {
                user_id: UserId(uint(u, "user_id")?),
                name: string(u, "name"),
                profile_picture: string(u, "profile_picture"),
                email: string(u, "email"),
            })
        })
        .collect())
}

/// One page of categories (`GET /public/v2/categories`): by `names` and
/// `tags`, or by `ids`; none of them, all categories. `limit` is 1 to 1000
/// (Kick's default: 25); `cursor` is a previous page's
/// [`Page::next_cursor`]. Kick takes each list comma-separated, so a name or
/// tag with a comma is refused.
pub fn categories_request(
    endpoints: &Endpoints,
    token: &str,
    names: &[&str],
    tags: &[&str],
    ids: &[u64],
    limit: Option<u32>,
    cursor: Option<&str>,
) -> Result<Request, Error> {
    let parts = [
        csv("name", names.iter().map(|n| n.to_string()))?,
        csv("tag", tags.iter().map(|t| t.to_string()))?,
        csv("id", ids.iter().map(u64::to_string))?,
        bounded("limit", limit, 1000)?,
        cursor.filter(|c| !c.is_empty()).map(|c| format!("cursor={}", encode(c))),
    ];
    Request::new(Method::Get, url(endpoints, "/public/v2/categories", parts)).bearer(token)
}

/// Reads a [`categories_request`] answer.
pub fn parse_categories(status: u16, body: &str) -> Result<Page<Category>, Error> {
    page(status, body, |c| Category::from(Some(c)))
}

/// One place on a Kicks leaderboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct KicksEntry {
    /// The gifter.
    pub user_id: UserId,
    /// Their username.
    pub username: Option<String>,
    /// Their place, from 1.
    pub rank: Option<i64>,
    /// Kicks they gave.
    pub gifted_amount: Option<i64>,
}

/// A channel's Kicks leaderboards, from [`kicks_leaderboard_request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct KicksLeaderboard {
    /// Of all time.
    pub lifetime: Vec<KicksEntry>,
    /// This month.
    pub month: Vec<KicksEntry>,
    /// This week.
    pub week: Vec<KicksEntry>,
}

/// The top `top` Kicks gifters (1 to 100; Kick's default: 10) of the
/// token's own channel (`GET /public/v1/kicks/leaderboard`). Needs a user
/// token with `kicks:read`; an app token has no channel of its own.
pub fn kicks_leaderboard_request(endpoints: &Endpoints, token: &str, top: Option<u32>) -> Result<Request, Error> {
    Request::new(Method::Get, url(endpoints, "/public/v1/kicks/leaderboard", [bounded("top", top, 100)?])).bearer(token)
}

/// Reads a [`kicks_leaderboard_request`] answer.
pub fn parse_kicks_leaderboard(status: u16, body: &str) -> Result<KicksLeaderboard, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    let board = |key: &str| -> Vec<KicksEntry> {
        array(&data, key)
            .iter()
            .filter_map(|e| {
                Some(KicksEntry {
                    user_id: UserId(uint(e, "user_id")?),
                    username: string(e, "username"),
                    rank: int(e, "rank"),
                    gifted_amount: int(e, "gifted_amount"),
                })
            })
            .collect()
    };
    Ok(KicksLeaderboard { lifetime: board("lifetime"), month: board("month"), week: board("week") })
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

/// A channel, from `/public/v1/channels`.
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
    Request::new(Method::Get, format!("{}/public/v1/channels?{q}", endpoints.api)).bearer(token)
}

/// Channels by slug, up to [`BATCH`]. One unknown slug fails the whole
/// request with a 400, so this is for slugs already known to exist; check a
/// new one alone.
pub fn channels_by_slugs_request(endpoints: &Endpoints, token: &str, slugs: &[&str]) -> Result<Request, Error> {
    batch(slugs)?;
    let q = query("slug", slugs.iter().map(|s| s.to_string()));
    Request::new(Method::Get, format!("{}/public/v1/channels?{q}", endpoints.api)).bearer(token)
}

/// Reads a `/public/v1/channels` answer.
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
                    tags: texts(s, "tags"),
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
    Request::new(Method::Get, format!("{}/public/v1/public-key", endpoints.api))
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
    Ok(Request::new(Method::Post, format!("{}/public/v1/events/subscriptions", endpoints.api))
        .bearer(token)?
        .json(&json!({ "broadcaster_user_id": broadcaster.0, "events": events, "method": "webhook" })))
}

/// Every subscription the app has.
pub fn subscriptions_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Get, format!("{}/public/v1/events/subscriptions", endpoints.api)).bearer(token)
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
            Request::new(Method::Delete, format!("{}/public/v1/events/subscriptions?{q}", endpoints.api)).bearer(token)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_are_capped() {
        let e = Endpoints::default();
        let ids: Vec<UserId> = (1..=250).map(UserId).collect();
        assert!(livestreams_request(&e, "t", &ids[..101]).is_err());
        assert!(livestreams_request(&e, "t", &ids[..100]).is_ok());
        assert!(livestreams_request(&e, "t", &[]).is_err());
        let reqs = livestreams_requests(&e, "t", &ids).unwrap();
        assert_eq!(reqs.len(), 3);
        assert!(reqs[0].url.starts_with("https://api.kick.com/public/v1/users/livestreams?user_id=1&user_id=2&"));
        assert!(reqs[0].url.ends_with("user_id=100"));
        assert!(reqs[0].headers.contains(&("authorization".into(), "Bearer t".into())));
        assert_eq!(livestreams_requests(&e, "t", &[]).unwrap(), vec![]);
        assert!(channels_by_ids_request(&e, "t", &ids[..51]).is_err());
        assert!(users_request(&e, "t", &ids[..51]).is_err());
        let ids: Vec<String> = (0..120).map(|i| format!("s{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert_eq!(unsubscribe_requests(&e, "t", &refs).unwrap().len(), 3);
    }

    #[test]
    fn paths_carry_their_version() {
        let e = Endpoints::default().with_api("http://localhost:9");
        let at = |r: Result<Request, Error>| r.unwrap().url;
        assert_eq!(at(Ok(public_key_request(&e))), "http://localhost:9/public/v1/public-key");
        assert_eq!(
            at(channels_by_ids_request(&e, "t", &[UserId(1)])),
            "http://localhost:9/public/v1/channels?broadcaster_user_id=1"
        );
        assert_eq!(at(subscriptions_request(&e, "t")), "http://localhost:9/public/v1/events/subscriptions");
        assert_eq!(at(livestream_stats_request(&e, "t")), "http://localhost:9/public/v1/livestreams/stats");
        assert_eq!(
            at(livestreams_page_request(&e, "t", &[], &[], None, None)),
            "http://localhost:9/public/v2/livestreams"
        );
        assert_eq!(
            at(categories_request(&e, "t", &[], &[], &[], None, None)),
            "http://localhost:9/public/v2/categories"
        );
        assert_eq!(at(users_request(&e, "t", &[])), "http://localhost:9/public/v1/users", "the token's own user");
        assert_eq!(at(users_request(&e, "t", &[UserId(1), UserId(2)])), "http://localhost:9/public/v1/users?id=1&id=2");
        assert_eq!(at(kicks_leaderboard_request(&e, "t", None)), "http://localhost:9/public/v1/kicks/leaderboard");
        assert_eq!(
            at(kicks_leaderboard_request(&e, "t", Some(3))),
            "http://localhost:9/public/v1/kicks/leaderboard?top=3"
        );
    }

    #[test]
    fn livestream_pages_filter_without_sorting() {
        let e = Endpoints::default();
        let r = livestreams_page_request(&e, "t", &[1, 2], &["en", "pt-BR"], Some(1000), Some("a b")).unwrap();
        assert_eq!(
            r.url,
            "https://api.kick.com/public/v2/livestreams?category_id=1&category_id=2&language_code=en&language_code=pt-BR&limit=1000&cursor=a%20b"
        );
        assert!(!livestreams_page_request(&e, "t", &[], &[], None, Some("")).unwrap().url.contains("cursor"));
        assert!(livestreams_page_request(&e, "t", &[], &[], Some(0), None).is_err());
        assert!(livestreams_page_request(&e, "t", &[], &[], Some(1001), None).is_err());
        let many: Vec<u64> = (1..=26).collect();
        assert!(livestreams_page_request(&e, "t", &many, &[], None, None).is_err());
        assert!(livestreams_page_request(&e, "t", &many[..25], &[], None, None).is_ok());
        assert!(livestreams_page_request(&e, "t", &[], &["en"; 26], None, None).is_err());
    }

    #[test]
    fn category_lists_are_comma_separated() {
        let e = Endpoints::default();
        let r =
            categories_request(&e, "t", &["Just Chatting", "Slots & Casino"], &["FPS"], &[7, 8], Some(5), Some("c"))
                .unwrap();
        assert_eq!(
            r.url,
            "https://api.kick.com/public/v2/categories?name=Just%20Chatting,Slots%20%26%20Casino&tag=FPS&id=7,8&limit=5&cursor=c"
        );
        assert!(categories_request(&e, "t", &["a,b"], &[], &[], None, None).is_err());
        assert!(categories_request(&e, "t", &[], &["x,y"], &[], None, None).is_err());
        assert!(categories_request(&e, "t", &[], &[], &[], Some(1001), None).is_err());
        assert!(kicks_leaderboard_request(&e, "t", Some(101)).is_err());
        assert!(kicks_leaderboard_request(&e, "t", Some(0)).is_err());
    }

    #[test]
    fn a_token_cannot_add_headers() {
        let e = Endpoints::default();
        let bad = "t\r\nx-evil: 1";
        assert!(livestreams_requests(&e, bad, &[UserId(1)]).is_err());
        assert!(subscriptions_request(&e, bad).is_err());
        assert!(unsubscribe_requests(&e, bad, &["a"]).is_err());
        assert!(channels_by_slugs_request(&e, bad, &["somechannel"]).is_err());
        assert!(livestreams_page_request(&e, bad, &[], &[], None, None).is_err());
        assert!(subscribe_request(&e, bad, UserId(1), &[("channel.followed", 1)]).is_err());
        assert!(users_request(&e, bad, &[]).is_err());
        assert!(categories_request(&e, bad, &[], &[], &[], None, None).is_err());
        assert!(livestream_stats_request(&e, bad).is_err());
        assert!(kicks_leaderboard_request(&e, bad, None).is_err());
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
        let last =
            parse_livestreams_page(200, r#"{"data":[],"message":"OK","pagination":{"next_cursor":""}}"#).unwrap();
        assert_eq!((last.data, last.next_cursor), (vec![], None));
        assert!(parse_livestreams_page(200, r#"{"message":"OK"}"#).is_err());
    }

    #[test]
    fn objects_are_required_where_kick_sends_one() {
        assert!(parse_livestream_stats(200, r#"{"data":[]}"#).is_err());
        assert!(parse_kicks_leaderboard(200, r#"{"data":null}"#).is_err());
        let board = parse_kicks_leaderboard(200, r#"{"data":{"week":[{"user_id":"5","rank":1},{"rank":2}]}}"#).unwrap();
        assert_eq!((board.week.len(), board.week[0].user_id, board.lifetime.len()), (1, UserId(5), 0));
    }
}
