//! kicksdk for JavaScript.
//!
//! Every function here is a thin wrapper: arguments in, the core's answer out
//! as plain JS objects (`snake_case` keys, as Kick writes them). Requests come
//! back as `{method, url, headers: [[name, value]], body}` for the caller's
//! `fetch`; parsers take `(status, bodyText)` and throw an `Error` with a
//! `status` property when Kick answered with an error status.
//!
//! Numbers are checked, never cast: an id, a time or a count must be a whole
//! number in its range (ids at most `Number.MAX_SAFE_INTEGER`, the largest a
//! JS number holds exactly), or the call throws a `RangeError`. An id must be
//! a number, not a numeric string (`TypeError`).
//!
//! The hand-written driver in `bindings/js` puts these together with `fetch`,
//! `WebSocket` and timers; most callers want that rather than these.

use kicksdk::http::Endpoints;
use kicksdk::ids::{ChannelId, ChatroomId, UserId};
use kicksdk::realtime;
use kicksdk::{content, events, public_api, webhooks, website};
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** Base URLs to use instead of Kick's. Any left out, or undefined, stay Kick's. */
export interface Endpoints {
  /** The official API: `https://api.kick.com/public/v1`. */
  api?: string | undefined;
  /** OAuth: `https://id.kick.com`. */
  id?: string | undefined;
  /** kick.com itself, whose `/api/v2` is the website API. */
  website?: string | undefined;
  /** The realtime endpoints: `https://web.kick.com/api/v1/realtime`. */
  realtime?: string | undefined;
}

/** The message a reply answers, from its chat event: `sender_id` is the sender's user id. */
export interface ReplyTo {
  message_id: string;
  content?: string | undefined;
  sender_id: number;
  sender_username: string;
}

"#;

#[wasm_bindgen]
extern "C" {
    /// `Endpoints` on the TypeScript side.
    #[wasm_bindgen(typescript_type = "Endpoints")]
    pub type EndpointsArg;
    /// An id: a JS number, checked by [`id`].
    #[wasm_bindgen(typescript_type = "number")]
    pub type Id;
    /// Ids: an array of JS numbers, checked by [`id_list`].
    #[wasm_bindgen(typescript_type = "readonly number[]")]
    pub type IdList;
    /// `topLivestreamsRequest`'s order.
    #[wasm_bindgen(typescript_type = "'viewer_count' | 'started_at'")]
    pub type SortArg;
    /// `sendRequest`'s reply target.
    #[wasm_bindgen(typescript_type = "ReplyTo")]
    pub type ReplyToArg;
    /// `subscribeRequest`'s events.
    #[wasm_bindgen(typescript_type = "ReadonlyArray<{ name: string; version?: number }>")]
    pub type WebhookEventList;
}

// ---- Conversions -------------------------------------------------------------

fn out<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))
}

fn fail(e: kicksdk::Error) -> JsValue {
    let err = js_sys::Error::new(&e.to_string());
    if let Some(status) = e.status() {
        let _ = js_sys::Reflect::set(&err, &"status".into(), &JsValue::from(status));
    }
    err.into()
}

fn result<T: Serialize>(r: Result<T, kicksdk::Error>) -> Result<JsValue, JsValue> {
    out(&r.map_err(fail)?)
}

fn error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

fn type_error(message: &str) -> JsValue {
    js_sys::TypeError::new(message).into()
}

fn range_error(message: &str) -> JsValue {
    js_sys::RangeError::new(message).into()
}

/// `Number.MAX_SAFE_INTEGER`: above it a JS number no longer holds every
/// whole number, so an id there may already be another id.
const MAX_SAFE: u64 = (1 << 53) - 1;

fn nothing(value: &JsValue) -> bool {
    value.is_undefined() || value.is_null()
}

/// A whole number from 0 to `max` (at most [`MAX_SAFE`]).
fn whole(what: &str, n: f64, max: u64) -> Result<u64, JsValue> {
    let max = max.min(MAX_SAFE);
    // `max` is at most 2^53 - 1, so it is exact as an f64, and so is `n`
    // once it is whole and within it: the cast below is lossless.
    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n <= max as f64 {
        Ok(n as u64)
    } else {
        Err(range_error(&format!("{what} must be a whole number from 0 to {max}, not {n}")))
    }
}

/// [`whole`], for a value that must be a JS number.
fn uint(what: &str, value: &JsValue, max: u64) -> Result<u64, JsValue> {
    match value.as_f64() {
        Some(n) => whole(what, n, max),
        None => Err(type_error(&format!("{what} must be a number"))),
    }
}

fn id(what: &str, value: &JsValue) -> Result<u64, JsValue> {
    uint(what, value, MAX_SAFE)
}

fn or_undefined<T: Into<JsValue>>(value: Option<T>) -> JsValue {
    value.map(Into::into).unwrap_or(JsValue::UNDEFINED)
}

fn opt_id(what: &str, value: &JsValue) -> Result<Option<u64>, JsValue> {
    if nothing(value) { Ok(None) } else { id(what, value).map(Some) }
}

fn u32_arg(what: &str, n: f64) -> Result<u32, JsValue> {
    // In range: `whole` checked it.
    Ok(whole(what, n, u32::MAX.into())? as u32)
}

/// A list of ids: an array of numbers.
fn id_list<T: From<u64>>(what: &str, value: &JsValue) -> Result<Vec<T>, JsValue> {
    if !js_sys::Array::is_array(value) {
        return Err(type_error(&format!("{what} must be an array of numbers")));
    }
    let list: &js_sys::Array = value.unchecked_ref();
    list.iter().enumerate().map(|(i, v)| id(&format!("{what}[{i}]"), &v).map(T::from)).collect()
}

fn to_endpoints(value: Option<EndpointsArg>) -> Result<Endpoints, JsValue> {
    let value: JsValue = match value {
        Some(v) => v.into(),
        None => return Ok(Endpoints::default()),
    };
    if nothing(&value) {
        return Ok(Endpoints::default());
    }
    if !value.is_object() || js_sys::Array::is_array(&value) {
        return Err(type_error("endpoints must be an object: {api?, id?, website?, realtime?}"));
    }
    // Partial overrides are filled from the defaults; a key given as
    // undefined (or null) keeps Kick's URL.
    let mut e = serde_json::to_value(Endpoints::default()).map_err(|err| error(&err.to_string()))?;
    let base = e.as_object_mut().ok_or_else(|| error("endpoints"))?;
    let given: serde_json::Map<String, serde_json::Value> =
        serde_wasm_bindgen::from_value(value).map_err(|err| type_error(&format!("endpoints: {err}")))?;
    for (k, v) in given {
        if v.is_null() {
            continue;
        }
        if !base.contains_key(&k) {
            let known: Vec<&str> = base.keys().map(String::as_str).collect();
            return Err(type_error(&format!("endpoints: unknown key `{k}` (known: {})", known.join(", "))));
        }
        if !v.is_string() {
            return Err(type_error(&format!("endpoints.{k} must be a string")));
        }
        base.insert(k, v);
    }
    serde_json::from_value(e).map_err(|err| type_error(&format!("endpoints: {err}")))
}

fn json_arg(value: JsValue) -> Result<serde_json::Value, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| type_error(&e.to_string()))
}

// ---- Events and text -------------------------------------------------------

/// A feed event, typed: `{type: "chat_message", …}`. `data` as Kick sent it.
#[wasm_bindgen(js_name = parseEvent)]
pub fn parse_event(name: &str, data: JsValue) -> Result<JsValue, JsValue> {
    out(&events::parse(name, &json_arg(data)?))
}

/// A message's text and emotes.
#[wasm_bindgen(js_name = parseContent)]
pub fn parse_content(text: &str, combine: bool) -> Result<JsValue, JsValue> {
    out(&content::parse(text, combine))
}

/// An emote's image URL.
#[wasm_bindgen(js_name = emoteUrl)]
pub fn emote_url(id: &str) -> Result<String, JsValue> {
    content::emote_url(id).map_err(fail)
}

// ---- Realtime ----------------------------------------------------------------

/// The feeds for one chat, the chat feed first.
#[wasm_bindgen(js_name = chatChannels)]
pub fn chat_channels(chatroom_id: Id, channel_id: Option<Id>) -> Result<Vec<String>, JsValue> {
    let chatroom = ChatroomId(id("chatroomId", &chatroom_id)?);
    let channel = opt_id("channelId", &or_undefined(channel_id))?.map(ChannelId);
    Ok(realtime::chat_channels(chatroom, channel))
}

/// Where a channel's chat is served.
#[wasm_bindgen(js_name = chatConnectionRequest)]
pub fn chat_connection_request(
    channel_id: Id,
    client_id: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let channel = ChannelId(id("channelId", &channel_id)?);
    out(&realtime::chat_connection_request(&to_endpoints(endpoints)?, channel, client_id))
}

/// Where Kick's realtime is served in general.
#[wasm_bindgen(js_name = connectionRequest)]
pub fn connection_request(client_id: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    out(&realtime::connection_request(&to_endpoints(endpoints)?, client_id))
}

/// Reads either connection answer: `[{provider, url}…]`.
#[wasm_bindgen(js_name = parseConnection)]
pub fn parse_connection(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(realtime::parse_connection(status, body))
}

/// A guest token request for `clientId`.
#[wasm_bindgen(js_name = tokenRequest)]
pub fn token_request(client_id: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    out(&realtime::token_request(&to_endpoints(endpoints)?, client_id))
}

/// The token from a token answer.
#[wasm_bindgen(js_name = parseToken)]
pub fn parse_token(status: u16, body: &str) -> Result<String, JsValue> {
    realtime::parse_token(status, body).map_err(fail)
}

fn now_ms(now: f64) -> Result<u64, JsValue> {
    whole("now (ms)", now, MAX_SAFE)
}

/// One connection's protocol. Each method returns the actions to carry out.
/// `now` is whole milliseconds from a clock that only goes forward
/// (`Math.floor(performance.now())`, or `Date.now()`).
#[wasm_bindgen]
pub struct Session(realtime::Session);

#[wasm_bindgen]
impl Session {
    /// A session for these feeds.
    #[wasm_bindgen(constructor)]
    pub fn new(channels: Vec<String>) -> Session {
        Session(realtime::Session::new(realtime::SessionOptions::new(channels)))
    }

    /// The socket opened.
    pub fn open(&mut self, token: &str, now: f64) -> Result<JsValue, JsValue> {
        let now = now_ms(now)?;
        out(&self.0.open(token, now))
    }

    /// A text frame arrived.
    pub fn receive(&mut self, text: &str, now: f64) -> Result<JsValue, JsValue> {
        let now = now_ms(now)?;
        out(&self.0.receive(text, now))
    }

    /// A new token, after a `refresh_token` action.
    pub fn refresh(&mut self, token: &str, now: f64) -> Result<JsValue, JsValue> {
        let now = now_ms(now)?;
        out(&self.0.refresh(token, now))
    }

    /// The clock reached `deadline()`.
    pub fn tick(&mut self, now: f64) -> Result<JsValue, JsValue> {
        let now = now_ms(now)?;
        out(&self.0.tick(now))
    }

    /// When to call `tick` next, in ms, or undefined.
    pub fn deadline(&self) -> Option<f64> {
        // `now` is at most 2^53 - 1 and Kick's intervals are bounded, so a
        // JS number holds the deadline.
        self.0.deadline().map(|d| d as f64)
    }

    /// The socket closed.
    pub fn closed(&mut self) -> Result<JsValue, JsValue> {
        out(&self.0.closed())
    }

    /// Whether a feed is subscribed.
    #[wasm_bindgen(js_name = isSubscribed)]
    pub fn is_subscribed(&self, channel: &str) -> bool {
        self.0.is_subscribed(channel)
    }

    /// Whether Kick accepted the connection.
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.0.is_connected()
    }
}

/// Reconnect delays: 1 s doubling to 30 s.
#[wasm_bindgen]
pub struct Backoff(realtime::Backoff);

#[wasm_bindgen]
impl Backoff {
    /// From `initialMs`, doubling to `maxMs` (whole milliseconds).
    #[wasm_bindgen(constructor)]
    pub fn new(initial_ms: Option<f64>, max_ms: Option<f64>) -> Result<Backoff, JsValue> {
        let initial = initial_ms.map(|n| whole("initialMs", n, MAX_SAFE)).transpose()?.unwrap_or(1_000);
        let max = max_ms.map(|n| whole("maxMs", n, MAX_SAFE)).transpose()?.unwrap_or(30_000);
        Ok(Backoff(realtime::Backoff::new(initial, max)))
    }

    /// The next wait, ms.
    #[wasm_bindgen(js_name = nextDelay)]
    pub fn next_delay(&mut self) -> f64 {
        // At most `maxMs`, itself at most 2^53 - 1: exact as a JS number.
        self.0.next_delay() as f64
    }

    /// A connection worked.
    pub fn reset(&mut self) {
        self.0.reset()
    }
}

// ---- Official API ------------------------------------------------------------

/// An app token request (client credentials).
#[wasm_bindgen(js_name = appTokenRequest)]
pub fn app_token_request(
    client_id: &str,
    client_secret: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    out(&public_api::token_request(&to_endpoints(endpoints)?, client_id, client_secret))
}

/// Reads an app token answer.
#[wasm_bindgen(js_name = parseAppToken)]
pub fn parse_app_token(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_token(status, body))
}

/// `/livestreams` for any number of user ids: one request per 50.
#[wasm_bindgen(js_name = livestreamsRequests)]
pub fn livestreams_requests(
    token: &str,
    user_ids: IdList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let users = id_list::<UserId>("userIds", &user_ids)?;
    result(public_api::livestreams_requests(&to_endpoints(endpoints)?, token, &users))
}

/// Reads a `/livestreams` answer.
#[wasm_bindgen(js_name = parseLivestreams)]
pub fn parse_livestreams(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_livestreams(status, body))
}

/// `/channels` for up to 50 user ids.
#[wasm_bindgen(js_name = channelsByIdsRequest)]
pub fn channels_by_ids_request(
    token: &str,
    user_ids: IdList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let users = id_list::<UserId>("userIds", &user_ids)?;
    result(public_api::channels_by_ids_request(&to_endpoints(endpoints)?, token, &users))
}

/// `/channels` for up to 50 slugs (one unknown slug fails them all).
#[wasm_bindgen(js_name = channelsBySlugsRequest)]
pub fn channels_by_slugs_request(
    token: &str,
    slugs: Vec<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
    result(public_api::channels_by_slugs_request(&to_endpoints(endpoints)?, token, &refs))
}

/// Reads a `/channels` answer.
#[wasm_bindgen(js_name = parseChannels)]
pub fn parse_channels(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_channels(status, body))
}

/// The webhook signing key request.
#[wasm_bindgen(js_name = publicKeyRequest)]
pub fn public_key_request(endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    out(&public_api::public_key_request(&to_endpoints(endpoints)?))
}

/// The PEM from a public key answer.
#[wasm_bindgen(js_name = parsePublicKey)]
pub fn parse_public_key(status: u16, body: &str) -> Result<String, JsValue> {
    public_api::parse_public_key(status, body).map_err(fail)
}

// ---- Website API -------------------------------------------------------------

/// A channel by slug (`kick.com/api/v2`): chatroom id, followers, live state.
#[wasm_bindgen(js_name = channelRequest)]
pub fn channel_request(slug: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(website::channel_request(&to_endpoints(endpoints)?, slug))
}

/// Reads a website channel answer.
#[wasm_bindgen(js_name = parseChannel)]
pub fn parse_channel(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel(status, body))
}

/// Recent chat and the pin, by channel id.
#[wasm_bindgen(js_name = historyRequest)]
pub fn history_request(channel_id: Id, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    let channel = ChannelId(id("channelId", &channel_id)?);
    out(&website::history_request(&to_endpoints(endpoints)?, channel))
}

/// Reads a history answer: `{messages, pinned}`.
#[wasm_bindgen(js_name = parseHistory)]
pub fn parse_history(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_history(status, body))
}

/// A user as a channel sees them.
#[wasm_bindgen(js_name = channelUserRequest)]
pub fn channel_user_request(
    channel_slug: &str,
    username: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    result(website::channel_user_request(&to_endpoints(endpoints)?, channel_slug, username))
}

/// Reads a channel user answer.
#[wasm_bindgen(js_name = parseChannelUser)]
pub fn parse_channel_user(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel_user(status, body))
}

/// The signed-in viewer's standing on a channel.
#[wasm_bindgen(js_name = meRequest)]
pub fn me_request(session: &str, channel_slug: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(website::me_request(&to_endpoints(endpoints)?, session, channel_slug))
}

/// Reads a `me` answer.
#[wasm_bindgen(js_name = parseMe)]
pub fn parse_me(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_me(status, body))
}

/// Bans a user for good.
#[wasm_bindgen(js_name = banRequest)]
pub fn ban_request(
    session: &str,
    channel_slug: &str,
    username: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    result(website::ban_request(&to_endpoints(endpoints)?, session, channel_slug, username))
}

/// Times a user out for 1 to 10080 minutes.
#[wasm_bindgen(js_name = timeoutRequest)]
pub fn timeout_request(
    session: &str,
    channel_slug: &str,
    username: &str,
    minutes: f64,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let minutes = u32_arg("minutes", minutes)?;
    result(website::timeout_request(&to_endpoints(endpoints)?, session, channel_slug, username, minutes))
}

/// Lifts a ban or timeout.
#[wasm_bindgen(js_name = unbanRequest)]
pub fn unban_request(
    session: &str,
    channel_slug: &str,
    username: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    result(website::unban_request(&to_endpoints(endpoints)?, session, channel_slug, username))
}

/// Deletes a message.
#[wasm_bindgen(js_name = deleteMessageRequest)]
pub fn delete_message_request(
    session: &str,
    chatroom_id: Id,
    message_id: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let chatroom = ChatroomId(id("chatroomId", &chatroom_id)?);
    result(website::delete_message_request(&to_endpoints(endpoints)?, session, chatroom, message_id))
}

/// Pins a message (the feed's data for it, whole) for 1 to 10080 minutes.
#[wasm_bindgen(js_name = pinRequest)]
pub fn pin_request(
    session: &str,
    channel_slug: &str,
    message: JsValue,
    minutes: f64,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let minutes = u32_arg("minutes", minutes)?;
    result(website::pin_request(&to_endpoints(endpoints)?, session, channel_slug, &json_arg(message)?, minutes))
}

/// Removes the pin.
#[wasm_bindgen(js_name = unpinRequest)]
pub fn unpin_request(session: &str, channel_slug: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(website::unpin_request(&to_endpoints(endpoints)?, session, channel_slug))
}

/// Sends a message, or a reply when `replyTo` is
/// `{message_id, content, sender_id, sender_username}`.
#[wasm_bindgen(js_name = sendRequest)]
pub fn send_request(
    session: &str,
    chatroom_id: Id,
    text: &str,
    message_ref: &str,
    reply_to: Option<ReplyToArg>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let chatroom = ChatroomId(id("chatroomId", &chatroom_id)?);
    let reply_to = or_undefined(reply_to);
    let field = |name: &str| js_sys::Reflect::get(&reply_to, &name.into());
    let string = |name: &str| -> Result<Option<String>, JsValue> {
        let v = field(name)?;
        if nothing(&v) {
            return Ok(None);
        }
        v.as_string().map(Some).ok_or_else(|| type_error(&format!("replyTo.{name} must be a string")))
    };
    let reply = if nothing(&reply_to) {
        None
    } else {
        let missing = |name: &str| type_error(&format!("replyTo.{name} is missing"));
        Some((
            string("message_id")?.ok_or_else(|| missing("message_id"))?,
            string("content")?.unwrap_or_default(),
            UserId(id("replyTo.sender_id", &field("sender_id")?)?),
            string("sender_username")?.ok_or_else(|| missing("sender_username"))?,
        ))
    };
    let target = reply.as_ref().map(|(message_id, content, sender_id, sender_username)| website::ReplyTarget {
        message_id,
        content,
        sender_id: *sender_id,
        sender_username,
    });
    result(website::send_request(&to_endpoints(endpoints)?, session, chatroom, text, message_ref, target.as_ref()))
}

/// Whether an acting call worked; throws with `status` if not.
#[wasm_bindgen(js_name = parseAction)]
pub fn parse_action(status: u16, body: &str) -> Result<(), JsValue> {
    website::parse_action(status, body).map_err(fail)
}

/// The top live streams, overall or in a category or language: 1 to 100
/// (100 by default). `sort` is `"viewer_count"` (the default) or
/// `"started_at"`.
#[wasm_bindgen(js_name = topLivestreamsRequest)]
pub fn top_livestreams_request(
    token: &str,
    category_id: Option<Id>,
    language: Option<String>,
    sort: Option<SortArg>,
    limit: Option<f64>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let category = opt_id("categoryId", &or_undefined(category_id))?;
    let sort = sort.map(|s| s.as_string().ok_or_else(|| type_error("sort must be a string"))).transpose()?;
    let sort = match sort.as_deref() {
        None | Some("viewer_count") => public_api::Sort::ViewerCount,
        Some("started_at") => public_api::Sort::StartedAt,
        Some(other) => {
            return Err(range_error(&format!("sort must be \"viewer_count\" or \"started_at\", not {other:?}")));
        }
    };
    let limit = limit.map(|n| u32_arg("limit", n)).transpose()?.unwrap_or(100);
    result(public_api::top_livestreams_request(
        &to_endpoints(endpoints)?,
        token,
        category,
        language.as_deref(),
        sort,
        limit,
    ))
}

/// Subscribes the app to webhook `events` (`[{name, version?}]`, version 1
/// when left out) for one channel.
#[wasm_bindgen(js_name = subscribeRequest)]
pub fn subscribe_request(
    token: &str,
    broadcaster_user_id: Id,
    events: WebhookEventList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let broadcaster = UserId(id("broadcasterUserId", &broadcaster_user_id)?);
    if !js_sys::Array::is_array(&events) {
        return Err(type_error("events must be an array of {name, version?}"));
    }
    let list: &js_sys::Array = events.unchecked_ref();
    let mut pairs: Vec<(String, u32)> = Vec::new();
    for (i, e) in list.iter().enumerate() {
        if !e.is_object() {
            return Err(type_error(&format!("events[{i}] must be {{name, version?}}")));
        }
        let name = js_sys::Reflect::get(&e, &"name".into())?
            .as_string()
            .ok_or_else(|| type_error(&format!("events[{i}].name must be a string")))?;
        let version = js_sys::Reflect::get(&e, &"version".into())?;
        let version = if nothing(&version) {
            1
        } else {
            // In range: `uint` checked it.
            uint(&format!("events[{i}].version"), &version, u32::MAX.into())? as u32
        };
        pairs.push((name, version));
    }
    let refs: Vec<(&str, u32)> = pairs.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    result(public_api::subscribe_request(&to_endpoints(endpoints)?, token, broadcaster, &refs))
}

/// Every webhook subscription the app has.
#[wasm_bindgen(js_name = subscriptionsRequest)]
pub fn subscriptions_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(public_api::subscriptions_request(&to_endpoints(endpoints)?, token))
}

/// Reads a subscriptions answer.
#[wasm_bindgen(js_name = parseSubscriptions)]
pub fn parse_subscriptions(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_subscriptions(status, body))
}

/// Removes subscriptions, 50 per request.
#[wasm_bindgen(js_name = unsubscribeRequests)]
pub fn unsubscribe_requests(
    token: &str,
    ids: Vec<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    result(public_api::unsubscribe_requests(&to_endpoints(endpoints)?, token, &refs))
}

// ---- Webhooks ----------------------------------------------------------------

/// Reads a delivery's body by its type header, without checking anything.
#[wasm_bindgen(js_name = parseWebhook)]
pub fn parse_webhook(event_type: &str, body: &str) -> Result<JsValue, JsValue> {
    result(webhooks::parse(event_type, body))
}

/// Checks a delivery: `headers` as `[[name, value]…]` or an object, `body` the
/// raw text. Returns the typed event, or throws.
#[wasm_bindgen(js_name = verifyWebhook)]
pub fn verify_webhook(public_key_pem: &str, headers: JsValue, body: &str) -> Result<JsValue, JsValue> {
    let pairs: Vec<(String, String)> = match json_arg(headers)? {
        serde_json::Value::Array(list) => {
            list.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_owned(), p.get(1)?.as_str()?.to_owned()))).collect()
        }
        serde_json::Value::Object(map) => {
            map.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect()
        }
        _ => return Err(js_sys::Error::new("headers must be pairs or an object").into()),
    };
    let h = webhooks::Headers::from_pairs(pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())))
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    // The delivery must be fresh: within 5 minutes of this machine's clock.
    let now = (js_sys::Date::now() / 1000.0) as u64;
    let key =
        webhooks::PublicKey::from_pem(public_key_pem).map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    webhooks::verify(&key, &h, body.as_bytes(), webhooks::Freshness::at(now))
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    let event = webhooks::parse(&h.event_type, body).map_err(fail)?;
    #[derive(Serialize)]
    struct Verified<'a> {
        headers: &'a webhooks::Headers,
        event: webhooks::WebhookEvent,
    }
    out(&Verified { headers: &h, event })
}
