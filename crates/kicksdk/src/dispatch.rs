//! Every function by name, JSON in and JSON out.
//!
//! The bindings that cross a C ABI or a dynamic language (Dart, Python) call
//! the core through here rather than through one hand-written wrapper per
//! function each: [`call`] takes a function's name and its arguments as a JSON
//! object and returns its result as JSON, and [`SessionHandle::call`] does the
//! same for a realtime session's methods. A function added here is available
//! in every such binding at once.
//!
//! Names are the module path: `website.channel_request`,
//! `realtime.parse_token`, `events.parse`. Arguments are named, `snake_case`,
//! as in the Rust signatures; optional ones may be left out. `endpoints`, on
//! every request builder, is a partial [`Endpoints`] override.
//!
//! `webhooks.verify` takes `public_key_pem`, `headers` (`[[name, value]]` or
//! an object), the body as `body` (text) or `body_base64` (the raw bytes,
//! for a body that is not UTF-8), and `now` (Unix seconds) with an optional
//! `tolerance_secs` (default 300) - or `skip_freshness: true` instead, to
//! re-check a stored delivery. `webhooks.parse` takes `event_type` and the
//! body the same two ways.
//!
//! `oauth.pkce` takes `random`, the CSPRNG bytes, as base64url without
//! padding; `oauth.authorize_url` takes the PKCE `verifier` and makes the
//! challenge from it. `chat.send_request` takes `sender` as
//! `{"user": broadcaster_user_id}` or `"bot"`. `rewards.*` take `fields`
//! and `drops.claims_request` takes `filter` as objects with the Rust
//! struct's fields (`RewardFields`, `ClaimFilter`); an unknown key is a
//! `call` error. A function with nothing to answer (`public_api.parse_done`,
//! `oauth.parse_revoke`) returns null.
//!
//! Numbers must be integers in range: `1.0` is 1, but `1.5`, `-1`, `1e30`
//! or a value too big for its argument is a `call` error, never rounded or
//! clamped. Every argument must be of its type: an optional one may be
//! left out or null, but a number for `cursor` or `"true"` for `combine` is a
//! `call` error, never taken for "left out".
//!
//! Failures come back as a [`DispatchError`]: `kind` is `status` (Kick
//! answered an error status, in `status`), `shape`, `json`, `invalid`
//! (refused before sending), `webhook` (a delivery that does not verify: a
//! header missing or malformed, a bad key, a signature that does not match,
//! a stale timestamp), `call` (no such function, or arguments that do
//! not fit it), or `panic` (a [`SessionHandle`] an earlier call panicked
//! in; a binding that catches panics reports the panic itself the same
//! way). `webhooks.verify` on a delivery that verifies but whose body
//! is not the claimed type's shape fails with `shape` (`json` if it is not
//! JSON): the type header is not signed, so that is a mismatched header or
//! a body Kick changed, not a forgery.

use crate::http::{Endpoints, Error};
use crate::ids::{ChannelId, ChatroomId, UserId};
use crate::public_api::{ads, channels, chat, drops, moderation, rewards};
use crate::{content, events, public_api, realtime, website};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DispatchError {
    /// `status`, `shape`, `json`, `invalid`, `webhook`, `call` or `panic`.
    pub kind: &'static str,
    /// Kick's HTTP status, for `status`.
    pub status: Option<u16>,
    /// What went wrong.
    pub message: String,
}

impl DispatchError {
    fn call(message: impl Into<String>) -> Self {
        Self { kind: "call", status: None, message: message.into() }
    }
}

impl From<Error> for DispatchError {
    fn from(e: Error) -> Self {
        let kind = match &e {
            Error::Status { .. } => "status",
            Error::Json(_) => "json",
            Error::Invalid(_) => "invalid",
            _ => "shape",
        };
        // A status error's message carries the start of Kick's answer, which
        // often says why (`CHANNEL_IS_NOT_LIVE`): the bindings over this have
        // no other way to see it. The Elixir NIF passes it on as its own field.
        let message = match &e {
            Error::Status { excerpt, .. } if !excerpt.is_empty() => format!("{e}: {excerpt}"),
            _ => e.to_string(),
        };
        Self { kind, status: e.status(), message }
    }
}

type Out = Result<Value, DispatchError>;

struct Args<'a>(&'a Map<String, Value>);

/// A non-negative integer: a JSON integer, or a float that is a whole number
/// an `f64` holds exactly (`1.0` from a language whose numbers are all
/// floats). `1.5`, `-1` and `1e30` are not: never rounded or clamped.
fn whole(v: &Value) -> Option<u64> {
    const EXACT: f64 = 9_007_199_254_740_991.0;
    v.as_u64().or_else(|| v.as_f64().filter(|f| (0.0..=EXACT).contains(f) && f.fract() == 0.0).map(|f| f as u64))
}

impl Args<'_> {
    fn value(&self, key: &str) -> Result<&Value, DispatchError> {
        self.0.get(key).filter(|v| !v.is_null()).ok_or_else(|| DispatchError::call(format!("missing `{key}`")))
    }
    fn str(&self, key: &str) -> Result<&str, DispatchError> {
        self.value(key)?.as_str().ok_or_else(|| DispatchError::call(format!("`{key}` must be a string")))
    }
    /// Left out or null: `None`. Anything but a string is a `call` error,
    /// never taken for "left out".
    fn opt_str(&self, key: &str) -> Result<Option<&str>, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_str().map(Some).ok_or_else(|| DispatchError::call(format!("`{key}` must be a string"))),
        }
    }
    fn u64(&self, key: &str) -> Result<u64, DispatchError> {
        whole(self.value(key)?).ok_or_else(|| DispatchError::call(format!("`{key}` must be a non-negative integer")))
    }
    fn opt_u64(&self, key: &str) -> Result<Option<u64>, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.u64(key).map(Some),
        }
    }
    fn u16(&self, key: &str) -> Result<u16, DispatchError> {
        u16::try_from(self.u64(key)?).map_err(|_| DispatchError::call(format!("`{key}` is out of range")))
    }
    fn u32(&self, key: &str) -> Result<u32, DispatchError> {
        u32::try_from(self.u64(key)?).map_err(|_| DispatchError::call(format!("`{key}` is out of range")))
    }
    fn opt_u32(&self, key: &str) -> Result<Option<u32>, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.u32(key).map(Some),
        }
    }
    /// Left out or null: false. Anything but a boolean (`"true"`, `1`) is a
    /// `call` error, never taken for false.
    fn bool(&self, key: &str) -> Result<bool, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(v) => v.as_bool().ok_or_else(|| DispatchError::call(format!("`{key}` must be a boolean"))),
        }
    }
    fn strings(&self, key: &str) -> Result<Vec<String>, DispatchError> {
        self.value(key)?
            .as_array()
            .and_then(|a| a.iter().map(|x| x.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| DispatchError::call(format!("`{key}` must be a list of strings")))
    }
    fn ids(&self, key: &str) -> Result<Vec<u64>, DispatchError> {
        self.value(key)?
            .as_array()
            .and_then(|a| a.iter().map(whole).collect())
            .ok_or_else(|| DispatchError::call(format!("`{key}` must be a list of integers")))
    }
    /// Left out or null: an empty list.
    fn opt_strings(&self, key: &str) -> Result<Vec<String>, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(_) => self.strings(key),
        }
    }
    /// Left out or null: an empty list.
    fn opt_ids(&self, key: &str) -> Result<Vec<u64>, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(_) => self.ids(key),
        }
    }
    fn endpoints(&self) -> Result<Endpoints, DispatchError> {
        let mut base = serde_json::to_value(Endpoints::default()).unwrap_or_default();
        let given = match self.0.get("endpoints") {
            None | Some(Value::Null) => return Ok(Endpoints::default()),
            Some(Value::Object(given)) => given,
            Some(_) => return Err(DispatchError::call("`endpoints` must be an object")),
        };
        if let Some(b) = base.as_object_mut() {
            for (k, v) in given {
                // A misspelt key would otherwise leave that URL at Kick's:
                // a host that meant a test server would talk to kick.com.
                if !b.contains_key(k) {
                    return Err(DispatchError::call(format!("`endpoints`: unknown key `{k}`")));
                }
                // null keeps Kick's URL, like a key left out.
                if !v.is_null() {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
        serde_json::from_value(base).map_err(|e| DispatchError::call(format!("`endpoints`: {e}")))
    }
    fn status_body(&self) -> Result<(u16, &str), DispatchError> {
        Ok((self.u16("status")?, self.str("body")?))
    }
    /// An object that deserializes into `T` (whose unknown keys are refused).
    fn de<T: DeserializeOwned>(&self, key: &str) -> Result<T, DispatchError> {
        serde_json::from_value(self.value(key)?.clone()).map_err(|e| DispatchError::call(format!("`{key}`: {e}")))
    }
    /// Left out or null: `T::default()`.
    fn opt_de<T: DeserializeOwned + Default>(&self, key: &str) -> Result<T, DispatchError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(T::default()),
            Some(_) => self.de(key),
        }
    }
    fn user(&self, key: &str) -> Result<UserId, DispatchError> {
        self.u64(key).map(UserId)
    }
}

/// `&str`s borrowed from owned strings, for a core function taking `&[&str]`.
fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn to<T: Serialize>(v: T) -> Out {
    serde_json::to_value(v).map_err(|e| DispatchError::call(e.to_string()))
}

fn ok<T: Serialize>(r: Result<T, Error>) -> Out {
    to(r?)
}

/// Calls one function by name. `args` is a JSON object (or null for none).
pub fn call(name: &str, args: &Value) -> Out {
    let empty = Map::new();
    let a = Args(match args {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return Err(DispatchError::call("arguments must be an object")),
    });
    match name {
        "events.parse" => to(events::parse(a.str("name")?, a.value("data").unwrap_or(&Value::Null))),
        "content.parse" => to(content::parse(a.str("text")?, a.bool("combine")?)),
        "content.emote_url" => ok(content::emote_url(a.str("id")?)),

        "realtime.chat_channels" => {
            to(realtime::chat_channels(ChatroomId(a.u64("chatroom_id")?), a.opt_u64("channel_id")?.map(ChannelId)))
        }
        "realtime.chat_connection_request" => {
            to(realtime::chat_connection_request(&a.endpoints()?, ChannelId(a.u64("channel_id")?), a.str("client_id")?))
        }
        "realtime.connection_request" => to(realtime::connection_request(&a.endpoints()?, a.str("client_id")?)),
        "realtime.parse_connection" => {
            let (s, b) = a.status_body()?;
            ok(realtime::parse_connection(s, b))
        }
        "realtime.token_request" => to(realtime::token_request(&a.endpoints()?, a.str("client_id")?)),
        "realtime.parse_token" => {
            let (s, b) = a.status_body()?;
            ok(realtime::parse_token(s, b))
        }
        "realtime.retry_after_ms" => to(realtime::retry_after_ms(a.u16("code")?)),

        "public_api.token_request" => {
            to(public_api::token_request(&a.endpoints()?, a.str("client_id")?, a.str("client_secret")?))
        }
        "public_api.parse_token" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_token(s, b))
        }
        "public_api.livestreams_requests" => {
            let ids: Vec<UserId> = a.ids("user_ids")?.into_iter().map(UserId).collect();
            ok(public_api::livestreams_requests(&a.endpoints()?, a.str("token")?, &ids))
        }
        "public_api.livestreams_page_request" => {
            let languages = a.opt_strings("language_codes")?;
            let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
            ok(public_api::livestreams_page_request(
                &a.endpoints()?,
                a.str("token")?,
                &a.opt_ids("category_ids")?,
                &languages,
                a.opt_u32("limit")?,
                a.opt_str("cursor")?,
            ))
        }
        "public_api.parse_livestreams" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_livestreams(s, b))
        }
        "public_api.parse_livestreams_page" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_livestreams_page(s, b))
        }
        "public_api.channels_by_ids_request" => {
            let ids: Vec<UserId> = a.ids("user_ids")?.into_iter().map(UserId).collect();
            ok(public_api::channels_by_ids_request(&a.endpoints()?, a.str("token")?, &ids))
        }
        "public_api.channels_by_slugs_request" => {
            let slugs = a.strings("slugs")?;
            let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
            ok(public_api::channels_by_slugs_request(&a.endpoints()?, a.str("token")?, &refs))
        }
        "public_api.parse_channels" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_channels(s, b))
        }
        "public_api.public_key_request" => to(public_api::public_key_request(&a.endpoints()?)),
        "public_api.parse_public_key" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_public_key(s, b))
        }
        "public_api.subscribe_request" => {
            let list = a.value("events")?.as_array().ok_or_else(|| DispatchError::call("`events` must be a list"))?;
            let events: Vec<(String, u32)> = list
                .iter()
                .map(|e| {
                    let name = e.get("name").and_then(Value::as_str)?;
                    let version = match e.get("version") {
                        None | Some(Value::Null) => 1,
                        Some(v) => u32::try_from(whole(v)?).ok()?,
                    };
                    Some((name.to_owned(), version))
                })
                .collect::<Option<_>>()
                .ok_or_else(|| DispatchError::call("`events` must be [{name, version}], version a u32"))?;
            let refs: Vec<(&str, u32)> = events.iter().map(|(n, v)| (n.as_str(), *v)).collect();
            ok(public_api::subscribe_request(
                &a.endpoints()?,
                a.str("token")?,
                UserId(a.u64("broadcaster_user_id")?),
                &refs,
            ))
        }
        "public_api.subscriptions_request" => ok(public_api::subscriptions_request(&a.endpoints()?, a.str("token")?)),
        "public_api.parse_subscriptions" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_subscriptions(s, b))
        }
        "public_api.unsubscribe_requests" => {
            let ids = a.strings("ids")?;
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            ok(public_api::unsubscribe_requests(&a.endpoints()?, a.str("token")?, &refs))
        }

        "public_api.users_request" => {
            let ids: Vec<UserId> = a.opt_ids("user_ids")?.into_iter().map(UserId).collect();
            ok(public_api::users_request(&a.endpoints()?, a.str("token")?, &ids))
        }
        "public_api.parse_users" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_users(s, b))
        }
        "public_api.categories_request" => {
            let (names, tags) = (a.opt_strings("names")?, a.opt_strings("tags")?);
            ok(public_api::categories_request(
                &a.endpoints()?,
                a.str("token")?,
                &refs(&names),
                &refs(&tags),
                &a.opt_ids("ids")?,
                a.opt_u32("limit")?,
                a.opt_str("cursor")?,
            ))
        }
        "public_api.parse_categories" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_categories(s, b))
        }
        "public_api.livestream_stats_request" => {
            ok(public_api::livestream_stats_request(&a.endpoints()?, a.str("token")?))
        }
        "public_api.parse_livestream_stats" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_livestream_stats(s, b))
        }
        "public_api.kicks_leaderboard_request" => {
            ok(public_api::kicks_leaderboard_request(&a.endpoints()?, a.str("token")?, a.opt_u32("top")?))
        }
        "public_api.parse_kicks_leaderboard" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_kicks_leaderboard(s, b))
        }
        "public_api.parse_done" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_done(s, b))
        }

        "chat.send_request" => {
            let sender = match a.value("sender")? {
                Value::String(s) if s == "bot" => chat::Sender::Bot,
                Value::Object(o) if o.len() == 1 && o.contains_key("user") => {
                    chat::Sender::User { broadcaster: Args(o).user("user")? }
                }
                _ => return Err(DispatchError::call("`sender` must be {\"user\": broadcaster_user_id} or \"bot\"")),
            };
            ok(chat::send_request(
                &a.endpoints()?,
                a.str("token")?,
                sender,
                a.str("content")?,
                a.opt_str("reply_to_message_id")?,
            ))
        }
        "chat.parse_send" => {
            let (s, b) = a.status_body()?;
            ok(chat::parse_send(s, b))
        }
        "chat.delete_request" => ok(chat::delete_request(&a.endpoints()?, a.str("token")?, a.str("message_id")?)),

        "moderation.ban_request" => ok(moderation::ban_request(
            &a.endpoints()?,
            a.str("token")?,
            a.user("broadcaster_user_id")?,
            a.user("user_id")?,
            a.opt_str("reason")?,
        )),
        "moderation.timeout_request" => ok(moderation::timeout_request(
            &a.endpoints()?,
            a.str("token")?,
            a.user("broadcaster_user_id")?,
            a.user("user_id")?,
            a.u32("minutes")?,
            a.opt_str("reason")?,
        )),
        "moderation.unban_request" => ok(moderation::unban_request(
            &a.endpoints()?,
            a.str("token")?,
            a.user("broadcaster_user_id")?,
            a.user("user_id")?,
        )),

        "channels.update_request" => {
            // Left out or null: unchanged; `[]` clears them.
            let tags = match a.0.get("custom_tags") {
                None | Some(Value::Null) => None,
                Some(_) => Some(a.strings("custom_tags")?),
            };
            let tags = tags.as_deref().map(refs);
            ok(channels::update_request(
                &a.endpoints()?,
                a.str("token")?,
                a.opt_u64("category_id")?,
                tags.as_deref(),
                a.opt_str("stream_title")?,
            ))
        }

        "rewards.list_request" => ok(rewards::list_request(&a.endpoints()?, a.str("token")?)),
        "rewards.parse_rewards" => {
            let (s, b) = a.status_body()?;
            ok(rewards::parse_rewards(s, b))
        }
        "rewards.create_request" => ok(rewards::create_request(&a.endpoints()?, a.str("token")?, &a.de("fields")?)),
        "rewards.update_request" => {
            ok(rewards::update_request(&a.endpoints()?, a.str("token")?, a.str("id")?, &a.de("fields")?))
        }
        "rewards.parse_reward" => {
            let (s, b) = a.status_body()?;
            ok(rewards::parse_reward(s, b))
        }
        "rewards.delete_request" => ok(rewards::delete_request(&a.endpoints()?, a.str("token")?, a.str("id")?)),
        "rewards.redemptions_request" => {
            let status = match a.opt_str("status")? {
                None => None,
                Some("pending") => Some(rewards::RedemptionStatus::Pending),
                Some("accepted") => Some(rewards::RedemptionStatus::Accepted),
                Some("rejected") => Some(rewards::RedemptionStatus::Rejected),
                Some(_) => return Err(DispatchError::call("`status` must be pending, accepted or rejected")),
            };
            ok(rewards::redemptions_request(
                &a.endpoints()?,
                a.str("token")?,
                a.opt_str("reward_id")?,
                status,
                a.opt_str("cursor")?,
            ))
        }
        "rewards.redemptions_by_ids_request" => {
            let ids = a.strings("ids")?;
            ok(rewards::redemptions_by_ids_request(&a.endpoints()?, a.str("token")?, &refs(&ids), a.opt_str("cursor")?))
        }
        "rewards.parse_redemptions" => {
            let (s, b) = a.status_body()?;
            ok(rewards::parse_redemptions(s, b))
        }
        "rewards.accept_request" => {
            let ids = a.strings("ids")?;
            ok(rewards::accept_request(&a.endpoints()?, a.str("token")?, &refs(&ids)))
        }
        "rewards.reject_request" => {
            let ids = a.strings("ids")?;
            ok(rewards::reject_request(&a.endpoints()?, a.str("token")?, &refs(&ids)))
        }
        "rewards.parse_failures" => {
            let (s, b) = a.status_body()?;
            ok(rewards::parse_failures(s, b))
        }

        "ads.ad_break_request" => {
            ok(ads::ad_break_request(&a.endpoints()?, a.str("token")?, a.str("id")?, a.u32("seconds")?))
        }
        "ads.parse_ad_break" => {
            let (s, b) = a.status_body()?;
            ok(ads::parse_ad_break(s, b))
        }
        "ads.status_request" => ok(ads::status_request(&a.endpoints()?, a.str("token")?)),
        "ads.parse_status" => {
            let (s, b) = a.status_body()?;
            ok(ads::parse_status(s, b))
        }
        "ads.enroll_request" => ok(ads::enroll_request(&a.endpoints()?, a.str("token")?)),

        "drops.claims_request" => ok(drops::claims_request(&a.endpoints()?, a.str("token")?, &a.opt_de("filter")?)),
        "drops.parse_claims" => {
            let (s, b) = a.status_body()?;
            ok(drops::parse_claims(s, b))
        }
        "drops.update_claims_request" => {
            let list = a.value("updates")?.as_array().ok_or_else(|| DispatchError::call("`updates` must be a list"))?;
            let updates: Vec<(&str, &str)> = list
                .iter()
                .map(|u| Some((u.get("claim_id")?.as_str()?, u.get("external_status")?.as_str()?)))
                .collect::<Option<_>>()
                .ok_or_else(|| DispatchError::call("`updates` must be [{claim_id, external_status}], both strings"))?;
            ok(drops::update_claims_request(&a.endpoints()?, a.str("token")?, &updates))
        }

        "website.channel_request" => ok(website::channel_request(&a.endpoints()?, a.str("slug")?)),
        "website.parse_channel" => {
            let (s, b) = a.status_body()?;
            ok(website::parse_channel(s, b))
        }
        "website.history_request" => to(website::history_request(&a.endpoints()?, ChannelId(a.u64("channel_id")?))),
        "website.parse_history" => {
            let (s, b) = a.status_body()?;
            ok(website::parse_history(s, b))
        }
        "website.channel_user_request" => {
            ok(website::channel_user_request(&a.endpoints()?, a.str("channel_slug")?, a.str("username")?))
        }
        "website.parse_channel_user" => {
            let (s, b) = a.status_body()?;
            ok(website::parse_channel_user(s, b))
        }
        "website.me_request" => ok(website::me_request(&a.endpoints()?, a.str("session")?, a.str("channel_slug")?)),
        "website.parse_me" => {
            let (s, b) = a.status_body()?;
            ok(website::parse_me(s, b))
        }
        "website.ban_request" => {
            ok(website::ban_request(&a.endpoints()?, a.str("session")?, a.str("channel_slug")?, a.str("username")?))
        }
        "website.timeout_request" => ok(website::timeout_request(
            &a.endpoints()?,
            a.str("session")?,
            a.str("channel_slug")?,
            a.str("username")?,
            a.u32("minutes")?,
        )),
        "website.unban_request" => {
            ok(website::unban_request(&a.endpoints()?, a.str("session")?, a.str("channel_slug")?, a.str("username")?))
        }
        "website.delete_message_request" => ok(website::delete_message_request(
            &a.endpoints()?,
            a.str("session")?,
            ChatroomId(a.u64("chatroom_id")?),
            a.str("message_id")?,
        )),
        "website.pin_request" => ok(website::pin_request(
            &a.endpoints()?,
            a.str("session")?,
            a.str("channel_slug")?,
            a.value("message")?,
            a.u32("minutes")?,
        )),
        "website.unpin_request" => {
            ok(website::unpin_request(&a.endpoints()?, a.str("session")?, a.str("channel_slug")?))
        }
        "website.send_request" => {
            let reply = match a.0.get("reply_to") {
                None | Some(Value::Null) => None,
                Some(r) => {
                    Some(Args(r.as_object().ok_or_else(|| DispatchError::call("`reply_to` must be an object"))?))
                }
            };
            let target = match &reply {
                None => None,
                Some(r) => Some(website::ReplyTarget {
                    message_id: r.str("message_id")?,
                    content: r.opt_str("content")?.unwrap_or(""),
                    sender_id: UserId(r.u64("sender_id")?),
                    sender_username: r.str("sender_username")?,
                }),
            };
            ok(website::send_request(
                &a.endpoints()?,
                a.str("session")?,
                ChatroomId(a.u64("chatroom_id")?),
                a.str("content")?,
                a.str("message_ref")?,
                target.as_ref(),
            ))
        }
        "website.parse_action" => {
            let (s, b) = a.status_body()?;
            ok(website::parse_action(s, b))
        }

        #[cfg(feature = "oauth")]
        "oauth.pkce" => {
            use base64::Engine;
            let random = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(a.str("random")?)
                .map_err(|_| DispatchError::call("`random` is not base64url without padding"))?;
            ok(crate::oauth::Pkce::from_random(&random))
        }
        #[cfg(feature = "oauth")]
        "oauth.pkce_from_verifier" => ok(crate::oauth::Pkce::from_verifier(a.str("verifier")?)),
        #[cfg(feature = "oauth")]
        "oauth.authorize_url" => {
            use crate::oauth::{Pkce, Scope};
            let scopes: Vec<Scope> = a.strings("scopes")?.iter().map(|s| Scope::from(s.as_str())).collect();
            ok(crate::oauth::authorize_url(
                &a.endpoints()?,
                a.str("client_id")?,
                a.str("redirect_uri")?,
                &scopes,
                a.str("state")?,
                &Pkce::from_verifier(a.str("verifier")?)?,
            ))
        }
        #[cfg(feature = "oauth")]
        "oauth.code_request" => to(crate::oauth::code_request(
            &a.endpoints()?,
            a.str("client_id")?,
            a.str("client_secret")?,
            a.str("redirect_uri")?,
            a.str("code")?,
            a.str("code_verifier")?,
        )),
        #[cfg(feature = "oauth")]
        "oauth.refresh_request" => to(crate::oauth::refresh_request(
            &a.endpoints()?,
            a.str("client_id")?,
            a.str("client_secret")?,
            a.str("refresh_token")?,
        )),
        #[cfg(feature = "oauth")]
        "oauth.parse_user_token" => {
            let (s, b) = a.status_body()?;
            ok(crate::oauth::parse_user_token(s, b))
        }
        #[cfg(feature = "oauth")]
        "oauth.revoke_request" => {
            use crate::oauth::TokenTypeHint;
            let hint = match a.opt_str("token_type_hint")? {
                None => None,
                Some("access_token") => Some(TokenTypeHint::AccessToken),
                Some("refresh_token") => Some(TokenTypeHint::RefreshToken),
                Some(_) => return Err(DispatchError::call("`token_type_hint` must be access_token or refresh_token")),
            };
            ok(crate::oauth::revoke_request(&a.endpoints()?, a.str("token")?, hint))
        }
        #[cfg(feature = "oauth")]
        "oauth.parse_revoke" => {
            let (s, b) = a.status_body()?;
            ok(crate::oauth::parse_revoke(s, b))
        }
        #[cfg(feature = "oauth")]
        "oauth.introspect_request" => ok(crate::oauth::introspect_request(&a.endpoints()?, a.str("token")?)),
        #[cfg(feature = "oauth")]
        "oauth.parse_introspect" => {
            let (s, b) = a.status_body()?;
            ok(crate::oauth::parse_introspect(s, b))
        }
        #[cfg(feature = "oauth")]
        "oauth.scopes" => to(crate::oauth::Scope::ALL.iter().map(crate::oauth::Scope::as_str).collect::<Vec<_>>()),

        #[cfg(feature = "webhooks")]
        "webhooks.verify" => webhook_verify(&a),
        #[cfg(feature = "webhooks")]
        "webhooks.parse" => ok(crate::webhooks::parse(a.str("event_type")?, a.body()?)),

        other => Err(DispatchError::call(format!("no function `{other}`"))),
    }
}

#[cfg(feature = "webhooks")]
impl Args<'_> {
    /// A webhook body: `body` as text, or `body_base64` for the raw bytes
    /// (a body that is not UTF-8 must come that way to verify).
    fn body(&self) -> Result<Vec<u8>, DispatchError> {
        use base64::Engine;
        match (self.opt_str("body")?, self.opt_str("body_base64")?) {
            (Some(text), None) => Ok(text.as_bytes().to_vec()),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|_| DispatchError::call("`body_base64` is not base64")),
            (Some(_), Some(_)) => Err(DispatchError::call("give `body` or `body_base64`, not both")),
            (None, None) => Err(DispatchError::call("missing `body` (or `body_base64`)")),
        }
    }

    /// `now` (Unix seconds) and `tolerance_secs` (default 300), or
    /// `skip_freshness: true` and neither.
    fn freshness(&self) -> Result<crate::webhooks::Freshness, DispatchError> {
        use crate::webhooks::{DEFAULT_TOLERANCE_SECS, Freshness};
        if self.bool("skip_freshness")? {
            return Ok(Freshness::Skip);
        }
        let now = match self.0.get("now") {
            None | Some(Value::Null) => {
                return Err(DispatchError::call(
                    "missing `now` (Unix seconds); pass `skip_freshness: true` to replay a stored delivery",
                ));
            }
            Some(v) => whole(v).ok_or_else(|| {
                DispatchError::call("`now` must be a non-negative integer (Unix seconds; round a float down)")
            })?,
        };
        let tolerance_secs = self.opt_u64("tolerance_secs")?.unwrap_or(DEFAULT_TOLERANCE_SECS);
        Ok(Freshness::Within { now, tolerance_secs })
    }
}

#[cfg(feature = "webhooks")]
fn webhook_verify(a: &Args<'_>) -> Out {
    use crate::webhooks;
    let pairs: Vec<(String, String)> = match a.value("headers")? {
        Value::Array(list) => {
            list.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_owned(), p.get(1)?.as_str()?.to_owned()))).collect()
        }
        Value::Object(map) => map.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect(),
        _ => return Err(DispatchError::call("`headers` must be [[name, value]] or an object")),
    };
    let refuse = |e: String| DispatchError { kind: "webhook", status: None, message: e };
    let headers = webhooks::Headers::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .map_err(|e| refuse(e.to_string()))?;
    let body = a.body()?;
    let freshness = a.freshness()?;
    webhooks::verify_pem(a.str("public_key_pem")?, &headers, &body, freshness).map_err(|e| refuse(e.to_string()))?;
    // Signed, but not the claimed type's shape (or not JSON): `shape` /
    // `json`, so a host can tell a forged delivery from a mismatched type
    // header or a body Kick changed.
    let event = webhooks::parse(&headers.event_type, &body)?;
    Ok(serde_json::json!({ "headers": to(&headers)?, "event": to(event)? }))
}

/// A realtime session for a binding to hold.
///
/// A call that panics (and is caught by the binding) leaves the handle
/// **poisoned**: the session may be half-changed, so every later call is
/// refused (kind `panic`) until `closed`, which resets the state a
/// connection uses, succeeds.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    session: realtime::Session,
    /// Set for the length of each call, so a panic leaves it set.
    poisoned: bool,
}

const POISONED: &str = "kicksdk: this realtime session panicked in an earlier call and its state cannot be trusted; \
                        call `closed` to reset it, or start a new one";

impl SessionHandle {
    /// A session for these feeds.
    pub fn new(channels: Vec<String>) -> Self {
        Self { session: realtime::Session::new(realtime::SessionOptions::new(channels)), poisoned: false }
    }

    /// Calls one of the session's methods by name: `open {token, now}`,
    /// `receive {text, now}`, `refresh {token, now}`, `tick {now}`,
    /// `closed {}`, `deadline {}`, `is_subscribed {channel}`,
    /// `is_connected {}`.
    ///
    /// `args` is a JSON object, or null for none, as for [`call`].
    pub fn call(&mut self, method: &str, args: &Value) -> Out {
        if self.poisoned && method != "closed" {
            return Err(DispatchError { kind: "panic", status: None, message: POISONED.into() });
        }
        self.poisoned = true;
        let out = self.run(method, args);
        self.poisoned = false;
        out
    }

    fn run(&mut self, method: &str, args: &Value) -> Out {
        let empty = Map::new();
        let a = Args(match args {
            Value::Object(map) => map,
            Value::Null => &empty,
            _ => return Err(DispatchError::call("arguments must be an object")),
        });
        let s = &mut self.session;
        match method {
            "open" => to(s.open(a.str("token")?, a.u64("now")?)),
            "receive" => to(s.receive(a.str("text")?, a.u64("now")?)),
            "refresh" => to(s.refresh(a.str("token")?, a.u64("now")?)),
            "tick" => to(s.tick(a.u64("now")?)),
            "closed" => to(s.closed()),
            "deadline" => to(s.deadline()),
            "is_subscribed" => to(s.is_subscribed(a.str("channel")?)),
            "is_connected" => to(s.is_connected()),
            #[cfg(test)]
            "panic" => panic!("for a test"),
            other => Err(DispatchError::call(format!("no session method `{other}`"))),
        }
    }
}

/// Every function [`call`] knows, for bindings that check at load time. The
/// `webhooks.*` ones only with the `webhooks` feature (on by default).
pub const FUNCTIONS: &[&str] = &[
    "events.parse",
    "content.parse",
    "content.emote_url",
    "realtime.chat_channels",
    "realtime.chat_connection_request",
    "realtime.connection_request",
    "realtime.parse_connection",
    "realtime.token_request",
    "realtime.parse_token",
    "realtime.retry_after_ms",
    "public_api.token_request",
    "public_api.parse_token",
    "public_api.livestreams_requests",
    "public_api.livestreams_page_request",
    "public_api.parse_livestreams",
    "public_api.parse_livestreams_page",
    "public_api.channels_by_ids_request",
    "public_api.channels_by_slugs_request",
    "public_api.parse_channels",
    "public_api.public_key_request",
    "public_api.parse_public_key",
    "public_api.subscribe_request",
    "public_api.subscriptions_request",
    "public_api.parse_subscriptions",
    "public_api.unsubscribe_requests",
    "public_api.users_request",
    "public_api.parse_users",
    "public_api.categories_request",
    "public_api.parse_categories",
    "public_api.livestream_stats_request",
    "public_api.parse_livestream_stats",
    "public_api.kicks_leaderboard_request",
    "public_api.parse_kicks_leaderboard",
    "public_api.parse_done",
    "chat.send_request",
    "chat.parse_send",
    "chat.delete_request",
    "moderation.ban_request",
    "moderation.timeout_request",
    "moderation.unban_request",
    "channels.update_request",
    "rewards.list_request",
    "rewards.parse_rewards",
    "rewards.create_request",
    "rewards.update_request",
    "rewards.parse_reward",
    "rewards.delete_request",
    "rewards.redemptions_request",
    "rewards.redemptions_by_ids_request",
    "rewards.parse_redemptions",
    "rewards.accept_request",
    "rewards.reject_request",
    "rewards.parse_failures",
    "ads.ad_break_request",
    "ads.parse_ad_break",
    "ads.status_request",
    "ads.parse_status",
    "ads.enroll_request",
    "drops.claims_request",
    "drops.parse_claims",
    "drops.update_claims_request",
    "website.channel_request",
    "website.parse_channel",
    "website.history_request",
    "website.parse_history",
    "website.channel_user_request",
    "website.parse_channel_user",
    "website.me_request",
    "website.parse_me",
    "website.ban_request",
    "website.timeout_request",
    "website.unban_request",
    "website.delete_message_request",
    "website.pin_request",
    "website.unpin_request",
    "website.send_request",
    "website.parse_action",
    #[cfg(feature = "oauth")]
    "oauth.pkce",
    #[cfg(feature = "oauth")]
    "oauth.pkce_from_verifier",
    #[cfg(feature = "oauth")]
    "oauth.authorize_url",
    #[cfg(feature = "oauth")]
    "oauth.code_request",
    #[cfg(feature = "oauth")]
    "oauth.refresh_request",
    #[cfg(feature = "oauth")]
    "oauth.parse_user_token",
    #[cfg(feature = "oauth")]
    "oauth.revoke_request",
    #[cfg(feature = "oauth")]
    "oauth.parse_revoke",
    #[cfg(feature = "oauth")]
    "oauth.introspect_request",
    #[cfg(feature = "oauth")]
    "oauth.parse_introspect",
    #[cfg(feature = "oauth")]
    "oauth.scopes",
    #[cfg(feature = "webhooks")]
    "webhooks.verify",
    #[cfg(feature = "webhooks")]
    "webhooks.parse",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_listed_function_is_dispatched() {
        for name in FUNCTIONS {
            let e = call(name, &json!({})).err();
            assert!(
                !matches!(&e, Some(DispatchError { message, .. }) if message.starts_with("no function")),
                "{name} is listed but not dispatched"
            );
        }
        assert_eq!(call("nope", &Value::Null).unwrap_err().message, "no function `nope`");
        assert_eq!(FUNCTIONS.contains(&"webhooks.verify"), cfg!(feature = "webhooks"));
        assert_eq!(FUNCTIONS.contains(&"oauth.pkce"), cfg!(feature = "oauth"));
    }

    #[test]
    fn errors_keep_kicks_status() {
        let e = call("website.parse_channel", &json!({"status": 404, "body": "{}"})).unwrap_err();
        assert_eq!((e.kind, e.status), ("status", Some(404)));
        let e = call("website.channel_request", &json!({})).unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), ("call", "missing `slug`"));
    }

    #[test]
    fn numbers_are_checked_not_squeezed() {
        let top = |limit: Value| {
            call("public_api.livestreams_page_request", &json!({"token": "t", "limit": limit}))
                .map(|r| r["url"].clone())
        };
        assert!(top(json!(10)).unwrap().as_str().unwrap().contains("limit=10"));
        assert!(top(json!(10.0)).unwrap().as_str().unwrap().contains("limit=10"));
        // 2^32 + 10 used to become 10.
        for bad in [json!(4_294_967_306u64), json!(10.5), json!(-1), json!(1e30), json!("10")] {
            assert_eq!(top(bad.clone()).unwrap_err().kind, "call", "{bad}");
        }
        let sub = |version: Value| {
            call(
                "public_api.subscribe_request",
                &json!({"token": "t", "broadcaster_user_id": 1, "events": [{"name": "channel.followed", "version": version}]}),
            )
        };
        assert!(sub(json!(1)).is_ok());
        assert!(sub(Value::Null).is_ok());
        assert_eq!(sub(json!(4_294_967_297u64)).unwrap_err().kind, "call");
        assert_eq!(sub(json!(1.5)).unwrap_err().kind, "call");
        // 2^53 + 1 as a float has already lost its last digit.
        let e = call("website.history_request", &json!({"channel_id": 9_007_199_254_740_993.0})).unwrap_err();
        assert_eq!(e.kind, "call");
        assert!(call("website.history_request", &json!({"channel_id": u64::MAX})).is_ok());
        let mut s = SessionHandle::new(vec![]);
        assert_eq!(s.call("open", &json!({"token": "t", "now": 1.5})).unwrap_err().kind, "call");
        assert_eq!(call("realtime.retry_after_ms", &json!({"code": 3501})).unwrap(), json!(300_000));
        assert_eq!(call("realtime.retry_after_ms", &json!({"code": 1006})).unwrap(), json!(0));
        assert_eq!(call("realtime.retry_after_ms", &json!({"code": 70_000})).unwrap_err().kind, "call");
        assert_eq!(call("realtime.should_reconnect", &json!({"code": 3501})).unwrap_err().kind, "call");
    }

    #[test]
    fn optional_arguments_of_the_wrong_type_are_refused() {
        let top = |extra: Value| {
            let mut a = json!({"token": "t"});
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            call("public_api.livestreams_page_request", &a)
        };
        let url = top(json!({"cursor": "c", "language_codes": ["en"], "category_ids": [3]})).unwrap()["url"].clone();
        assert_eq!(url, "https://api.kick.com/public/v2/livestreams?category_id=3&language_code=en&cursor=c");
        assert!(top(json!({"cursor": null, "language_codes": null, "category_ids": null})).is_ok());
        for bad in [
            json!({"cursor": 1}),
            json!({"cursor": ["c"]}),
            json!({"language_codes": "en"}),
            json!({"language_codes": [7]}),
            json!({"category_ids": [-1]}),
        ] {
            assert_eq!(top(bad.clone()).unwrap_err().kind, "call", "{bad}");
        }
        assert!(call("content.parse", &json!({"text": "hi", "combine": true})).is_ok());
        assert!(call("content.parse", &json!({"text": "hi", "combine": null})).is_ok());
        for bad in [json!("true"), json!(1), json!(0)] {
            let e = call("content.parse", &json!({"text": "hi", "combine": bad})).unwrap_err();
            assert_eq!((e.kind, e.message.as_str()), ("call", "`combine` must be a boolean"), "{bad}");
        }
        let reply = |content: Value| {
            call(
                "website.send_request",
                &json!({"session": "s", "chatroom_id": 1, "content": "hi", "message_ref": "1",
                    "reply_to": {"message_id": "m", "content": content, "sender_id": 2, "sender_username": "somechannel"}}),
            )
        };
        assert!(reply(json!("parent")).is_ok());
        assert!(reply(Value::Null).is_ok());
        assert_eq!(reply(json!(5)).unwrap_err().kind, "call");
    }

    /// A docs fixture's response, as `{status, body}`.
    fn answer(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/docs/public_api").join(name);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        v["response"].clone()
    }

    #[test]
    fn user_token_actions_by_name() {
        let r = call(
            "chat.send_request",
            &json!({"token": "t", "sender": {"user": 123}, "content": "Pog", "reply_to_message_id": "m"}),
        )
        .unwrap();
        assert_eq!(
            (r["method"].as_str(), r["url"].as_str()),
            (Some("POST"), Some("https://api.kick.com/public/v1/chat"))
        );
        let body: Value = serde_json::from_str(r["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"broadcaster_user_id": 123, "content": "Pog", "type": "user", "reply_to_message_id": "m"})
        );
        let bot = call("chat.send_request", &json!({"token": "t", "sender": "bot", "content": "hi"})).unwrap();
        assert!(bot["body"].as_str().unwrap().contains(r#""type":"bot""#));
        for bad in [json!("user"), json!({"user": "1"}), json!({"user": 1, "x": 2}), json!(1)] {
            let e = call("chat.send_request", &json!({"token": "t", "sender": bad, "content": "hi"})).unwrap_err();
            assert_eq!(e.kind, "call", "{bad}");
        }
        assert_eq!(call("chat.parse_send", &answer("actions-chat-send.json")).unwrap()["is_sent"], true);

        let r = call(
            "moderation.timeout_request",
            &json!({"token": "t", "broadcaster_user_id": 1, "user_id": 2, "minutes": 10}),
        )
        .unwrap();
        assert_eq!(r["body"], r#"{"broadcaster_user_id":1,"user_id":2,"duration":10}"#);
        let e = call(
            "moderation.timeout_request",
            &json!({"token": "t", "broadcaster_user_id": 1, "user_id": 2, "minutes": 0}),
        )
        .unwrap_err();
        assert_eq!(e.kind, "invalid");

        let tags = |t: Value| call("channels.update_request", &json!({"token": "t", "custom_tags": t}));
        assert_eq!(tags(json!([])).unwrap()["body"], r#"{"custom_tags":[]}"#);
        assert_eq!(tags(Value::Null).unwrap_err().kind, "invalid"); // nothing to change
        assert_eq!(tags(json!("a")).unwrap_err().kind, "call");

        let r = call("rewards.create_request", &json!({"token": "t", "fields": {"title": "Hydrate", "cost": 100}}))
            .unwrap();
        assert_eq!(r["url"], "https://api.kick.com/public/v1/channels/rewards");
        let e = call("rewards.create_request", &json!({"token": "t", "fields": {"titel": "x"}})).unwrap_err();
        assert!(e.kind == "call" && e.message.contains("titel"), "{e:?}");
        let r = call("rewards.redemptions_request", &json!({"token": "t", "status": "accepted"})).unwrap();
        assert!(r["url"].as_str().unwrap().ends_with("/redemptions?status=accepted"), "{}", r["url"]);
        assert_eq!(
            call("rewards.redemptions_request", &json!({"token": "t", "status": "done"})).unwrap_err().kind,
            "call"
        );
        assert!(call("rewards.accept_request", &json!({"token": "t", "ids": [1]})).is_err());
        assert!(call("rewards.parse_redemptions", &answer("actions-rewards-redemptions.json")).is_ok());

        let r = call("ads.ad_break_request", &json!({"token": "t", "id": "a", "seconds": 30})).unwrap();
        assert_eq!(r["method"], "POST");
        assert!(call("ads.parse_status", &answer("actions-ads-status.json")).is_ok());

        let r = call("drops.claims_request", &json!({"token": "t", "filter": {"user_id": 7, "limit": 5}})).unwrap();
        assert!(r["url"].as_str().unwrap().contains("user_id=7"), "{}", r["url"]);
        assert!(call("drops.claims_request", &json!({"token": "t"})).is_ok());
        assert_eq!(
            call("drops.claims_request", &json!({"token": "t", "filter": {"nope": 1}})).unwrap_err().kind,
            "call"
        );
        let up = |u: Value| call("drops.update_claims_request", &json!({"token": "t", "updates": u}));
        assert!(up(json!([{"claim_id": "c", "external_status": "done"}])).is_ok());
        assert_eq!(up(json!([{"claim_id": "c"}])).unwrap_err().kind, "call");
        assert_eq!(up(json!([])).unwrap_err().kind, "invalid");
    }

    #[test]
    fn new_public_reads_by_name() {
        let r = call("public_api.users_request", &json!({"token": "t", "user_ids": [123]})).unwrap();
        assert_eq!(r["url"], "https://api.kick.com/public/v1/users?id=123");
        let me = call("public_api.users_request", &json!({"token": "t"})).unwrap();
        assert_eq!(me["url"], "https://api.kick.com/public/v1/users");
        assert_eq!(call("public_api.parse_users", &answer("v1-users.json")).unwrap()[0]["user_id"], 123);
        let r = call("public_api.categories_request", &json!({"token": "t", "names": ["a"], "limit": 5})).unwrap();
        assert!(r["url"].as_str().unwrap().starts_with("https://api.kick.com/public/v2/categories?"));
        assert!(call("public_api.parse_categories", &answer("v2-categories.json")).is_ok());
        assert!(call("public_api.parse_livestream_stats", &answer("v1-livestreams-stats.json")).is_ok());
        assert!(call("public_api.parse_kicks_leaderboard", &answer("v1-kicks-leaderboard.json")).is_ok());
        assert_eq!(call("public_api.parse_done", &json!({"status": 204, "body": ""})).unwrap(), Value::Null);
        assert_eq!(call("public_api.parse_done", &json!({"status": 403, "body": ""})).unwrap_err().kind, "status");
        // Kick's reason reaches the bindings in the message.
        let e = call("ads.parse_ad_break", &json!({"status": 400, "body": "{\"message\":\"CHANNEL_IS_NOT_LIVE\"}"}))
            .unwrap_err();
        assert_eq!((e.kind, e.status), ("status", Some(400)));
        assert_eq!(e.message, r#"Kick answered 400: {"message":"CHANNEL_IS_NOT_LIVE"}"#);
    }

    #[cfg(feature = "oauth")]
    #[test]
    fn oauth_by_name() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let p = call("oauth.pkce", &json!({"random": b64.encode([7u8; 32])})).unwrap();
        let verifier = p["verifier"].as_str().unwrap().to_owned();
        assert_eq!(verifier.len(), 43);
        assert_eq!(call("oauth.pkce_from_verifier", &json!({"verifier": verifier})).unwrap(), p);
        assert_eq!(call("oauth.pkce", &json!({"random": "not base64!"})).unwrap_err().kind, "call");
        assert_eq!(call("oauth.pkce", &json!({"random": b64.encode([7u8; 31])})).unwrap_err().kind, "invalid");

        let url = call(
            "oauth.authorize_url",
            &json!({"client_id": "c", "redirect_uri": "http://localhost/cb", "scopes": ["user:read", "chat:write"],
                "state": "s", "verifier": verifier}),
        )
        .unwrap();
        let url = url.as_str().unwrap();
        assert!(url.starts_with("https://id.kick.com/oauth/authorize?response_type=code&client_id=c&"), "{url}");
        assert!(url.contains(&format!("code_challenge={}", p["challenge"].as_str().unwrap())), "{url}");
        assert!(url.contains("scope=user%3Aread%20chat%3Awrite"), "{url}");

        let r = call("oauth.revoke_request", &json!({"token": "tok", "token_type_hint": "refresh_token"})).unwrap();
        assert_eq!(r["url"], "https://id.kick.com/oauth/revoke?token=tok&token_type_hint=refresh_token");
        let e = call("oauth.revoke_request", &json!({"token": "tok", "token_type_hint": "id_token"})).unwrap_err();
        assert_eq!(e.kind, "call");
        assert_eq!(call("oauth.parse_revoke", &json!({"status": 200, "body": "OK"})).unwrap(), Value::Null);
        let t = call(
            "oauth.parse_user_token",
            &json!({"status": 200, "body": r#"{"access_token":"a","refresh_token":"r","scope":"user:read"}"#}),
        )
        .unwrap();
        assert_eq!(t["scopes"], json!(["user:read"]));
        assert_eq!(call("oauth.scopes", &json!({})).unwrap().as_array().unwrap().len(), 13);
        let r = call(
            "oauth.code_request",
            &json!({"client_id": "c", "client_secret": "s", "redirect_uri": "r",
            "code": "x", "code_verifier": "v"}),
        )
        .unwrap();
        assert_eq!(r["url"], "https://id.kick.com/oauth/token");
    }

    #[test]
    fn requests_and_partial_endpoints() {
        let r =
            call("realtime.token_request", &json!({"client_id": "c", "endpoints": {"realtime": "http://x"}})).unwrap();
        assert_eq!(r["url"], "http://x/auth/connection");
        assert_eq!(r["method"], "POST");
    }

    #[cfg(feature = "webhooks")]
    #[test]
    fn webhooks_verify_fresh_deliveries_as_text_or_bytes() {
        use base64::Engine;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing");
        let pem = std::fs::read_to_string(dir.join("test-public-key.pem")).unwrap();
        let body = std::fs::read_to_string(dir.join("body.json")).unwrap();
        let signature = std::fs::read_to_string(dir.join("signature.b64")).unwrap();
        let headers = json!({
            "kick-event-message-id": "01M38M9K3YRKV8E6Q68C18ZWG4",
            "kick-event-message-timestamp": "2026-09-24T02:35:42Z",
            "kick-event-signature": signature.trim(),
            "kick-event-type": "channel.followed",
            "kick-event-version": "1",
        });
        let args = |extra: Value| {
            let mut a = json!({"public_key_pem": pem, "headers": headers});
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            a
        };
        let ok = call("webhooks.verify", &args(json!({"body": body, "now": 1_790_217_344}))).unwrap();
        assert_eq!(ok["event"]["type"], "followed");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&body);
        assert!(call("webhooks.verify", &args(json!({"body_base64": b64, "now": 1_790_217_344}))).is_ok());
        assert!(call("webhooks.verify", &args(json!({"body": body, "skip_freshness": true}))).is_ok());
        let e = call("webhooks.verify", &args(json!({"body": body}))).unwrap_err();
        assert_eq!(e.kind, "call", "now is required: {}", e.message);
        assert!(e.message.starts_with("missing `now`"), "{}", e.message);
        // Python's time.time(), or a negative or textual `now`: not missing.
        for bad in [json!(1_790_217_344.5), json!(-1), json!("1790217344")] {
            let e = call("webhooks.verify", &args(json!({"body": body, "now": bad}))).unwrap_err();
            assert_eq!(e.kind, "call");
            assert!(e.message.starts_with("`now` must be a non-negative integer"), "{bad}: {}", e.message);
        }
        let e = call("webhooks.verify", &args(json!({"body": body, "now": 1_790_300_000}))).unwrap_err();
        assert_eq!(e.kind, "webhook", "stale: {}", e.message);
        let e = call("webhooks.verify", &args(json!({"body": body, "now": 1_790_217_400, "tolerance_secs": 10})))
            .unwrap_err();
        assert_eq!(e.kind, "webhook");
        let e = call("webhooks.verify", &args(json!({"body": body, "body_base64": b64, "now": 0}))).unwrap_err();
        assert_eq!(e.kind, "call");
        // `skip_freshness: "yes"` is neither true nor left out.
        let e = call("webhooks.verify", &args(json!({"body": body, "skip_freshness": "yes"}))).unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), ("call", "`skip_freshness` must be a boolean"));
        let e = call("webhooks.verify", &args(json!({"body": 1, "now": 1_790_217_344}))).unwrap_err();
        assert_eq!(e.kind, "call");

        // Signed, but the (unsigned) type header claims another type: not a
        // forgery, a shape failure.
        let mut retyped = headers.clone();
        retyped["kick-event-type"] = json!("kicks.gifted");
        let e = call(
            "webhooks.verify",
            &json!({"public_key_pem": pem, "headers": retyped, "body": body, "now": 1_790_217_344}),
        )
        .unwrap_err();
        assert_eq!(e.kind, "shape", "{}", e.message);
        let mut forged = headers.clone();
        forged["kick-event-message-id"] = json!("01M38M9K3YRKV8E6Q68C18ZWG5");
        let e = call(
            "webhooks.verify",
            &json!({"public_key_pem": pem, "headers": forged, "body": body, "now": 1_790_217_344}),
        )
        .unwrap_err();
        assert_eq!(e.kind, "webhook", "{}", e.message);

        let parsed = call("webhooks.parse", &json!({"event_type": "channel.followed", "body": body})).unwrap();
        assert_eq!(parsed["type"], "followed");
        let e = call("webhooks.parse", &json!({"event_type": "kicks.gifted", "body": body})).unwrap_err();
        assert_eq!(e.kind, "shape");
    }

    #[test]
    fn a_session_by_name() {
        let mut s = SessionHandle::new(vec!["a".into()]);
        let out = s.call("open", &json!({"token": "t", "now": 0})).unwrap();
        assert_eq!(out[0]["action"], "send");
        assert_eq!(s.call("is_connected", &Value::Null).unwrap(), json!(false));
        for bad in [json!([]), json!("x"), json!(1)] {
            let e = s.call("is_connected", &bad).unwrap_err();
            assert_eq!((e.kind, e.message.as_str()), ("call", "arguments must be an object"), "{bad}");
        }
        assert_eq!(s.call("deadline", &Value::Null).unwrap(), json!(45_000));
        // A close, in the order bindings see its keys.
        let out = s
            .call("receive", &json!({"text": r#"{"id":1,"error":{"code":101,"message":"unauthorized"}}"#, "now": 1}))
            .unwrap();
        assert_eq!(
            serde_json::to_string(&out).unwrap(),
            r#"[{"action":"close","code":101,"reason":"Kick refused the connection: 101 unauthorized","terminal":true,"retry_after_ms":300000}]"#
        );
    }

    #[test]
    fn a_session_that_panicked_is_refused_until_closed() {
        let mut s = SessionHandle::new(vec!["a".into()]);
        s.call("open", &json!({"token": "t", "now": 0})).unwrap();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.call("panic", &Value::Null)));
        assert!(caught.is_err());
        for (method, args) in [("is_connected", json!({})), ("tick", json!({"now": 1})), ("deadline", json!(null))] {
            let e = s.call(method, &args).unwrap_err();
            assert_eq!(e.kind, "panic", "{method}");
            assert!(e.message.contains("call `closed`"), "{}", e.message);
        }
        s.call("closed", &Value::Null).unwrap();
        assert_eq!(s.call("is_connected", &Value::Null).unwrap(), json!(false));
        assert_eq!(s.call("open", &json!({"token": "t", "now": 2})).unwrap()[0]["action"], "send");
        // An ordinary error does not poison.
        assert_eq!(s.call("tick", &json!({})).unwrap_err().kind, "call");
        assert!(s.call("deadline", &Value::Null).is_ok());
    }

    #[test]
    fn endpoints_refuse_unknown_keys() {
        let call_with = |e: Value| call("website.channel_request", &json!({"slug": "somechannel", "endpoints": e}));
        assert!(call_with(json!({"websit": "http://localhost:1"})).is_err());
        assert!(call_with(json!("http://localhost:1")).is_err());
        let r = call_with(json!({"website": "http://localhost:1", "api": null})).unwrap();
        assert!(r["url"].as_str().unwrap().starts_with("http://localhost:1/"));
        assert!(call_with(Value::Null).is_ok());
    }
}
