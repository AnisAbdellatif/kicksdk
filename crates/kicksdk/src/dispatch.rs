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
//! Numbers must be integers in range: `1.0` is 1, but `1.5`, `-1`, `1e30`
//! or a value too big for its argument is a `call` error, never rounded or
//! clamped. Every argument must be of its type: an optional one may be
//! left out or null, but a number for `sort` or `"true"` for `combine` is a
//! `call` error, never taken for "left out".
//!
//! Failures come back as a [`DispatchError`]: `kind` is `status` (Kick
//! answered an error status, in `status`), `shape`, `json`, `invalid`
//! (refused before sending), `webhook` (a delivery that does not verify: a
//! header missing or malformed, a bad key, a signature that does not match,
//! a stale timestamp), or `call` (no such function, or arguments that do
//! not fit it). `webhooks.verify` on a delivery that verifies but whose body
//! is not the claimed type's shape fails with `shape` (`json` if it is not
//! JSON): the type header is not signed, so that is a mismatched header or
//! a body Kick changed, not a forgery.

use crate::http::{Endpoints, Error};
use crate::ids::{ChannelId, ChatroomId, UserId};
use crate::{content, events, public_api, realtime, website};
use serde::Serialize;
use serde_json::{Map, Value};

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct DispatchError {
    /// `status`, `shape`, `json`, `invalid`, `webhook` or `call`.
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
        Self { kind, status: e.status(), message: e.to_string() }
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
        "realtime.should_reconnect" => to(realtime::should_reconnect(a.u16("code")?)),

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
        "public_api.top_livestreams_request" => {
            let sort = match a.opt_str("sort")?.unwrap_or("viewer_count") {
                "viewer_count" => public_api::Sort::ViewerCount,
                "started_at" => public_api::Sort::StartedAt,
                other => return Err(DispatchError::call(format!("unknown sort `{other}`"))),
            };
            ok(public_api::top_livestreams_request(
                &a.endpoints()?,
                a.str("token")?,
                a.opt_u64("category_id")?,
                a.opt_str("language")?,
                sort,
                a.opt_u32("limit")?.unwrap_or(100),
            ))
        }
        "public_api.parse_livestreams" => {
            let (s, b) = a.status_body()?;
            ok(public_api::parse_livestreams(s, b))
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
        let now = self.u64("now").map_err(|_| {
            DispatchError::call("missing `now` (Unix seconds); pass `skip_freshness: true` to replay a stored delivery")
        })?;
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
#[derive(Debug, Clone)]
pub struct SessionHandle(realtime::Session);

impl SessionHandle {
    /// A session for these feeds.
    pub fn new(channels: Vec<String>) -> Self {
        Self(realtime::Session::new(realtime::SessionOptions::new(channels)))
    }

    /// Calls one of the session's methods by name: `open {token, now}`,
    /// `receive {text, now}`, `refresh {token, now}`, `tick {now}`,
    /// `closed {}`, `deadline {}`, `is_subscribed {channel}`,
    /// `is_connected {}`.
    pub fn call(&mut self, method: &str, args: &Value) -> Out {
        let empty = Map::new();
        let a = Args(args.as_object().unwrap_or(&empty));
        let s = &mut self.0;
        match method {
            "open" => to(s.open(a.str("token")?, a.u64("now")?)),
            "receive" => to(s.receive(a.str("text")?, a.u64("now")?)),
            "refresh" => to(s.refresh(a.str("token")?, a.u64("now")?)),
            "tick" => to(s.tick(a.u64("now")?)),
            "closed" => to(s.closed()),
            "deadline" => to(s.deadline()),
            "is_subscribed" => to(s.is_subscribed(a.str("channel")?)),
            "is_connected" => to(s.is_connected()),
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
    "realtime.should_reconnect",
    "public_api.token_request",
    "public_api.parse_token",
    "public_api.livestreams_requests",
    "public_api.top_livestreams_request",
    "public_api.parse_livestreams",
    "public_api.channels_by_ids_request",
    "public_api.channels_by_slugs_request",
    "public_api.parse_channels",
    "public_api.public_key_request",
    "public_api.parse_public_key",
    "public_api.subscribe_request",
    "public_api.subscriptions_request",
    "public_api.parse_subscriptions",
    "public_api.unsubscribe_requests",
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
            call("public_api.top_livestreams_request", &json!({"token": "t", "limit": limit})).map(|r| r["url"].clone())
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
        assert_eq!(call("realtime.should_reconnect", &json!({"code": 3501})).unwrap(), json!(false));
        assert_eq!(call("realtime.should_reconnect", &json!({"code": 70_000})).unwrap_err().kind, "call");
    }

    #[test]
    fn optional_arguments_of_the_wrong_type_are_refused() {
        let top = |extra: Value| {
            let mut a = json!({"token": "t"});
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            call("public_api.top_livestreams_request", &a)
        };
        assert!(top(json!({"sort": "started_at", "language": "en"})).is_ok());
        assert!(top(json!({"sort": null, "language": null})).is_ok());
        for bad in
            [json!({"sort": 1}), json!({"sort": ["started_at"]}), json!({"language": 7}), json!({"language": {}})]
        {
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
        assert_eq!(s.call("deadline", &Value::Null).unwrap(), json!(45_000));
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
