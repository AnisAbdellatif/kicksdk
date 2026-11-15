//! kicksdk for JavaScript.
//!
//! Every function here is a thin wrapper: arguments in, the core's answer out
//! as plain JS objects (`snake_case` keys, as Kick writes them). Requests come
//! back as `{method, url, headers: [[name, value]], body}` for the caller's
//! `fetch`; parsers take `(status, bodyText)` and throw an `Error` with a
//! `status` property when Kick answered with an error status (and an `excerpt`
//! property, the start of Kick's answer, also in the message).
//!
//! Numbers are checked, never cast: every numeric argument must be a JS
//! number (a numeric string, a boolean or a BigInt is a `TypeError`), and an
//! id, a time, a status or a count must be a whole number in its range (ids at
//! most `Number.MAX_SAFE_INTEGER`, the largest a JS number holds exactly), or
//! the call throws a `RangeError`. Results are held to the same limit: one
//! with an integer above `Number.MAX_SAFE_INTEGER` in it throws an `Error`
//! rather than come back rounded.
//!
//! The hand-written driver in `bindings/js` puts these together with `fetch`,
//! `WebSocket` and timers; most callers want that rather than these.

use kicksdk::http::Endpoints;
use kicksdk::ids::{ChannelId, ChatroomId, UserId};
use kicksdk::public_api::{ads, channels, chat, drops, moderation, rewards};
use kicksdk::realtime;
use kicksdk::{content, events, oauth, public_api, webhooks, website};
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** Base URLs to use instead of Kick's. Any left out, or undefined, stay Kick's. */
export interface Endpoints {
  /** The official API's host, without a version: `https://api.kick.com`. */
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

/** A webhook delivery's headers: `[name, value]` pairs (a fetch `Headers` will do) or an object. */
export type WebhookHeaders = Iterable<readonly [string, string]> | Record<string, string>;

/** How a delivery's timestamp is checked. By default: within 5 minutes of this machine's clock. */
export interface VerifyOptions {
  /** Now, Unix **seconds** (not `Date.now()`'s milliseconds). Default: this machine's clock. */
  now?: number | undefined;
  /** How far from `now` the timestamp may be, either way, in seconds. Default 300. */
  toleranceSecs?: number | undefined;
  /** No freshness check: to re-check a delivery stored earlier. A live receiver never skips it. */
  skipFreshness?: boolean | undefined;
}

/** A delivery that passed: its headers and the typed event. */
export interface VerifiedWebhook {
  headers: {
    message_id: string;
    timestamp: string;
    signature: string;
    event_type: string;
    version: string;
    subscription_id: string | null;
  };
  event: { type: string; [key: string]: unknown };
}

/** A request for `fetch`: what every `…Request` function returns. */
export interface HttpRequest {
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
}

/** A user token's scope, as Kick spells it: the 13 Kick documents, or one added since. */
export type Scope =
  | 'ads:read'
  | 'ads:write'
  | 'channel:read'
  | 'channel:write'
  | 'channel:rewards:read'
  | 'channel:rewards:write'
  | 'chat:write'
  | 'events:subscribe'
  | 'kicks:read'
  | 'moderation:ban'
  | 'moderation:chat_message:manage'
  | 'streamkey:read'
  | 'user:read'
  | (string & {});

/**
 * A PKCE pair: `challenge` goes in `oauthAuthorizeUrl`, `verifier` is a
 * secret kept until the user comes back, for `oauthCodeRequest`. Never log it.
 */
export interface Pkce {
  verifier: string;
  challenge: string;
}

/** A user token. Both tokens are secrets: never log them. */
export interface UserToken {
  access_token: string;
  /** For `oauthRefreshRequest`; each refresh gives a new one. */
  refresh_token: string | null;
  /** Seconds until the access token expires. */
  expires_in: number | null;
  /** Seconds until the refresh token expires, from every refresh. */
  refresh_expires_in: number | null;
  token_type: string | null;
  scopes: Scope[];
}

/** Which kind of token `oauthRevokeRequest` revokes. */
export type TokenTypeHint = 'access_token' | 'refresh_token';

/** What Kick says about a token. When it is not `active`, nothing else. */
export interface Introspection {
  active: boolean;
  client_id: string | null;
  /** Unix seconds. */
  exp: number | null;
  scopes: Scope[];
  /** `user` or `app`. */
  token_type: string | null;
}

/** One page of a paginated answer: pass `next_cursor` back for the next. */
export interface Page<T> {
  data: T[];
  next_cursor: string | null;
}

export interface User {
  user_id: number;
  name: string | null;
  profile_picture: string | null;
  /** Only for a user token with `user:read`, for its own user. */
  email: string | null;
}

export interface Category {
  id: number | null;
  name: string | null;
  thumbnail: string | null;
  /** Only `categoriesRequest` gives them; empty elsewhere. */
  tags: string[];
}

export interface LivestreamStats {
  total_count: number | null;
}

export interface KicksEntry {
  user_id: number;
  username: string | null;
  rank: number | null;
  gifted_amount: number | null;
}

export interface KicksLeaderboard {
  lifetime: KicksEntry[];
  month: KicksEntry[];
  week: KicksEntry[];
}

/** Who `chatSendRequest` sends as: the token's user, into a channel by its broadcaster's user id, or the app's bot. */
export type ChatSender = { user: number } | 'bot';

/** What Kick says about a message sent. */
export interface Sent {
  is_sent: boolean;
  /** The new message's id, for `chatDeleteRequest`. */
  message_id: string | null;
}

export interface Reward {
  id: string;
  title: string | null;
  description: string | null;
  cost: number | null;
  background_color: string | null;
  is_enabled: boolean | null;
  is_paused: boolean | null;
  is_user_input_required: boolean | null;
  should_redemptions_skip_request_queue: boolean | null;
}

/** A reward's settings: what is left out is not sent. Creating needs `title` and `cost`. */
export interface RewardFields {
  title?: string | undefined;
  cost?: number | undefined;
  description?: string | undefined;
  background_color?: string | undefined;
  is_enabled?: boolean | undefined;
  is_paused?: boolean | undefined;
  is_user_input_required?: boolean | undefined;
  should_redemptions_skip_request_queue?: boolean | undefined;
}

export type RedemptionStatus = 'pending' | 'accepted' | 'rejected';

export interface Redemption {
  id: string;
  status: string | null;
  redeemed_at: string | null;
  /** The redeemer's user id. */
  redeemer: number | null;
  user_input: string | null;
}

export interface RedeemedReward {
  id: string;
  title: string | null;
  description: string | null;
  cost: number | null;
  can_manage: boolean | null;
  is_deleted: boolean;
}

export interface RedemptionPage {
  rewards: { reward: RedeemedReward; redemptions: Redemption[] }[];
  next_cursor: string | null;
}

/** A redemption Kick did not accept or reject: `NOT_PENDING`, `NOT_FOUND`, `NOT_OWNED`, `UNKNOWN`. */
export interface FailedRedemption {
  id: string;
  reason: string | null;
}

export interface AdBreak {
  id: string | null;
  ad_break_id: string | null;
  remaining_ad_breaks: number | null;
}

export interface AdStatus {
  opted_in: boolean | null;
  ads_blocked: boolean | null;
  auto_ads_active: boolean | null;
  remaining_ad_breaks: number | null;
  limits: { max: number | null; period_seconds: number | null } | null;
  ad_breaks: {
    id: string | null;
    ad_break_id: string | null;
    status: string | null;
    source: string | null;
    break_duration_seconds: number | null;
    created_at: string | null;
  }[];
}

/** What `dropsClaimsRequest` lists by: what is left out is not filtered. */
export interface ClaimFilter {
  campaign_id?: string | undefined;
  claim_id?: string | undefined;
  user_id?: number | undefined;
  external_status?: string | undefined;
  /** 1 to 1000. */
  limit?: number | undefined;
  cursor?: string | undefined;
}

export interface Claim {
  claim_id: string;
  user_id: number | null;
  campaign_id: string | null;
  reward_id: string | null;
  external_id: string | null;
  external_status: string | null;
  created_at: string | null;
  updated_at: string | null;
}

export interface Claims {
  claims: Claim[];
  /** The next page's, for `ClaimFilter.cursor`. */
  cursor: string | null;
}

/** One claim's new status, for `dropsUpdateClaimsRequest`. */
export interface ClaimUpdate {
  claim_id: string;
  external_status: string;
}
"#;

#[wasm_bindgen]
extern "C" {
    /// `Endpoints` on the TypeScript side.
    #[wasm_bindgen(typescript_type = "Endpoints")]
    pub type EndpointsArg;
    /// `WebhookHeaders` on the TypeScript side.
    #[wasm_bindgen(typescript_type = "WebhookHeaders")]
    pub type HeadersArg;
    /// `VerifyOptions` on the TypeScript side.
    #[wasm_bindgen(typescript_type = "VerifyOptions")]
    pub type VerifyOptionsArg;
    /// `VerifiedWebhook` on the TypeScript side.
    #[wasm_bindgen(typescript_type = "VerifiedWebhook")]
    pub type VerifiedWebhook;
    /// An id: a JS number, checked by [`id`].
    #[wasm_bindgen(typescript_type = "number")]
    pub type Id;
    /// Any other number (a time, a count, a status): a JS number, checked
    /// by [`number`] and [`uint`]. Taken as a `JsValue` rather than an `f64`,
    /// which wasm-bindgen would get from `'10'` or `true` by coercion.
    #[wasm_bindgen(typescript_type = "number")]
    pub type Num;
    /// A flag: a JS boolean, checked by [`flag`] rather than coerced
    /// (wasm-bindgen would read `'false'` or `1` as true).
    #[wasm_bindgen(typescript_type = "boolean")]
    pub type Flag;
    /// Ids: an array of JS numbers, checked by [`id_list`].
    #[wasm_bindgen(typescript_type = "readonly number[]")]
    pub type IdList;
    /// A webhook body: text or bytes.
    #[wasm_bindgen(typescript_type = "string | Uint8Array | ArrayBuffer")]
    pub type Body;
    /// Strings: an array of JS strings, checked by [`string_list`].
    #[wasm_bindgen(typescript_type = "readonly string[]")]
    pub type StringList;
    /// `sendRequest`'s reply target.
    #[wasm_bindgen(typescript_type = "ReplyTo")]
    pub type ReplyToArg;
    /// `subscribeRequest`'s events.
    #[wasm_bindgen(typescript_type = "ReadonlyArray<{ name: string; version?: number }>")]
    pub type WebhookEventList;
    /// Bytes from a CSPRNG, for `oauthPkce`.
    #[wasm_bindgen(typescript_type = "Uint8Array")]
    pub type Bytes;
    /// Scopes: an array of strings.
    #[wasm_bindgen(typescript_type = "readonly Scope[]")]
    pub type ScopeList;
    /// `oauthRevokeRequest`'s hint.
    #[wasm_bindgen(typescript_type = "TokenTypeHint")]
    pub type TokenTypeHintArg;
    /// `chatSendRequest`'s sender.
    #[wasm_bindgen(typescript_type = "ChatSender")]
    pub type ChatSenderArg;
    /// A reward's settings.
    #[wasm_bindgen(typescript_type = "RewardFields")]
    pub type RewardFieldsArg;
    /// A redemption status.
    #[wasm_bindgen(typescript_type = "RedemptionStatus")]
    pub type RedemptionStatusArg;
    /// `dropsClaimsRequest`'s filter.
    #[wasm_bindgen(typescript_type = "ClaimFilter")]
    pub type ClaimFilterArg;
    /// `dropsUpdateClaimsRequest`'s updates.
    #[wasm_bindgen(typescript_type = "readonly ClaimUpdate[]")]
    pub type ClaimUpdateList;
}

// ---- Conversions -------------------------------------------------------------

/// A result as plain JS. An integer above `Number.MAX_SAFE_INTEGER` in it
/// is an error (serde-wasm-bindgen's JSON-compatible mode), not a rounded
/// number.
fn out<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))
}

fn fail(e: kicksdk::Error) -> JsValue {
    // A status error also carries the start of Kick's answer, which often
    // says why (`CHANNEL_IS_NOT_LIVE`): in the message, and as `excerpt`.
    let excerpt = match &e {
        kicksdk::Error::Status { excerpt, .. } if !excerpt.is_empty() => Some(excerpt.clone()),
        _ => None,
    };
    let message = match &excerpt {
        Some(x) => format!("{e}: {x}"),
        None => e.to_string(),
    };
    let err = js_sys::Error::new(&message);
    if let Some(status) = e.status() {
        let _ = js_sys::Reflect::set(&err, &"status".into(), &JsValue::from(status));
    }
    if let Some(x) = excerpt {
        let _ = js_sys::Reflect::set(&err, &"excerpt".into(), &JsValue::from(x));
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

/// A JS number, any (NaN included); anything else is a `TypeError`.
fn number(what: &str, value: &JsValue) -> Result<f64, JsValue> {
    value.as_f64().ok_or_else(|| type_error(&format!("{what} must be a number")))
}

/// A JS boolean; anything else is a `TypeError`.
fn flag(what: &str, value: Flag) -> Result<bool, JsValue> {
    let value: JsValue = value.into();
    value.as_bool().ok_or_else(|| type_error(&format!("{what} must be a boolean")))
}

/// [`whole`], for a value that must be a JS number.
fn uint(what: &str, value: &JsValue, max: u64) -> Result<u64, JsValue> {
    whole(what, number(what, value)?, max)
}

/// [`uint`], or `None` for undefined or null.
fn opt_uint(what: &str, value: Option<Num>, max: u64) -> Result<Option<u64>, JsValue> {
    let value = or_undefined(value);
    if nothing(&value) { Ok(None) } else { uint(what, &value, max).map(Some) }
}

/// An HTTP status, 0 to 65535 (a parser's first argument).
fn http_status(value: &Num) -> Result<u16, JsValue> {
    // In range: `uint` checked it.
    Ok(uint("status", value, u16::MAX.into())? as u16)
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

fn u32_arg(what: &str, value: &JsValue) -> Result<u32, JsValue> {
    // In range: `uint` checked it.
    Ok(uint(what, value, u32::MAX.into())? as u32)
}

/// A list of strings, or none for undefined or null.
fn string_list(what: &str, value: &JsValue) -> Result<Vec<String>, JsValue> {
    if nothing(value) {
        return Ok(vec![]);
    }
    if !js_sys::Array::is_array(value) {
        return Err(type_error(&format!("{what} must be an array of strings")));
    }
    let list: &js_sys::Array = value.unchecked_ref();
    list.iter()
        .enumerate()
        .map(|(i, v)| v.as_string().ok_or_else(|| type_error(&format!("{what}[{i}] must be a string"))))
        .collect()
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
        // Whatever its value: a misspelt key would otherwise leave that URL
        // at Kick's.
        if !base.contains_key(&k) {
            let known: Vec<&str> = base.keys().map(String::as_str).collect();
            return Err(type_error(&format!("endpoints: unknown key `{k}` (known: {})", known.join(", "))));
        }
        if v.is_null() {
            continue;
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
pub fn parse_content(text: &str, combine: Flag) -> Result<JsValue, JsValue> {
    out(&content::parse(text, flag("combine", combine)?))
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
pub fn parse_connection(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(realtime::parse_connection(http_status(&status)?, body))
}

/// A guest token request for `clientId`.
#[wasm_bindgen(js_name = tokenRequest)]
pub fn token_request(client_id: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    out(&realtime::token_request(&to_endpoints(endpoints)?, client_id))
}

/// The token from a token answer.
#[wasm_bindgen(js_name = parseToken)]
pub fn parse_token(status: Num, body: &str) -> Result<String, JsValue> {
    realtime::parse_token(http_status(&status)?, body).map_err(fail)
}

/// The least to wait, ms, before reconnecting after the socket closed with
/// this code: 300000 (5 minutes) for Kick's terminal codes (3500-3999 and
/// 4500-4999: a bad token, a ban, a refused request), then reconnect with a
/// new client id; 0, the ordinary backoff, for the rest (1006, a network
/// drop, among them).
#[wasm_bindgen(js_name = retryAfterMs)]
pub fn retry_after_ms(code: Num) -> Result<f64, JsValue> {
    // In range: `uint` checked it.
    let code = uint("code", &code, u16::MAX.into())? as u16;
    // At most 300000: exact as a JS number.
    Ok(realtime::retry_after_ms(code) as f64)
}

fn now_ms(now: &Num) -> Result<u64, JsValue> {
    uint("now (ms)", now, MAX_SAFE)
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
    pub fn open(&mut self, token: &str, now: Num) -> Result<JsValue, JsValue> {
        let now = now_ms(&now)?;
        out(&self.0.open(token, now))
    }

    /// A text frame arrived.
    pub fn receive(&mut self, text: &str, now: Num) -> Result<JsValue, JsValue> {
        let now = now_ms(&now)?;
        out(&self.0.receive(text, now))
    }

    /// A new token, after a `refresh_token` action.
    pub fn refresh(&mut self, token: &str, now: Num) -> Result<JsValue, JsValue> {
        let now = now_ms(&now)?;
        out(&self.0.refresh(token, now))
    }

    /// The clock reached `deadline()`.
    pub fn tick(&mut self, now: Num) -> Result<JsValue, JsValue> {
        let now = now_ms(&now)?;
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

/// The largest `retryAfterMs` `nextDelayFor` takes: its wait, at most 7.5
/// times this, stays within what a JS number holds exactly.
const MAX_RETRY_AFTER_MS: u64 = MAX_SAFE / 8;

/// Reconnect delays: 1 s doubling to 30 s, and a slower stage for terminal
/// closes (`nextDelayFor`).
#[wasm_bindgen]
pub struct Backoff(realtime::Backoff);

#[wasm_bindgen]
impl Backoff {
    /// From `initialMs`, doubling to `maxMs` (whole milliseconds).
    #[wasm_bindgen(constructor)]
    pub fn new(initial_ms: Option<Num>, max_ms: Option<Num>) -> Result<Backoff, JsValue> {
        let initial = opt_uint("initialMs", initial_ms, MAX_SAFE)?.unwrap_or(1_000);
        let max = opt_uint("maxMs", max_ms, MAX_SAFE)?.unwrap_or(30_000);
        Ok(Backoff(realtime::Backoff::new(initial, max)))
    }

    /// The next wait, ms.
    #[wasm_bindgen(js_name = nextDelay)]
    pub fn next_delay(&mut self) -> f64 {
        // At most `maxMs`, itself at most 2^53 - 1: exact as a JS number.
        self.0.next_delay() as f64
    }

    /// The next wait, ms, spread over [half of it, all of it] by `random`,
    /// drawn uniformly from [0, 1) (`Math.random()`), so that clients dropped
    /// together do not all come back together. Out of range is clamped, NaN
    /// taken as 0.
    #[wasm_bindgen(js_name = nextDelayJittered)]
    pub fn next_delay_jittered(&mut self, random: Num) -> Result<f64, JsValue> {
        let random = number("random", &random)?;
        // At most `nextDelay()`'s, which a JS number holds exactly.
        Ok(self.0.next_delay_jittered(random) as f64)
    }

    /// The wait before reconnecting after a connection ended, ms.
    /// `retryAfterMs` is what the end asked for: a `close` action's
    /// `retry_after_ms`, `retryAfterMs(code)` for a socket's close code, or 0
    /// for a failure without a code. With 0, exactly `nextDelayJittered`.
    /// With more (a terminal close), at least that, doubling with each
    /// terminal close since `reset()` up to six times it (300000: 5, 10, 20,
    /// then 30 minutes), plus up to a quarter more by `random`. Reconnect
    /// with a new client id after a terminal close.
    #[wasm_bindgen(js_name = nextDelayFor)]
    pub fn next_delay_for(&mut self, retry_after_ms: Num, random: Num) -> Result<f64, JsValue> {
        let retry_after_ms = uint("retryAfterMs", &retry_after_ms, MAX_RETRY_AFTER_MS)?;
        let random = number("random", &random)?;
        // At most 7.5 times `retryAfterMs`, or `nextDelay()`'s: exact as a JS
        // number.
        Ok(self.0.next_delay_for(retry_after_ms, random) as f64)
    }

    /// The chat feed subscribed: back to the first wait, and the terminal
    /// streak ends.
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
pub fn parse_app_token(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_token(http_status(&status)?, body))
}

/// `/public/v1/users/livestreams` for any number of user ids: one request
/// per 100.
#[wasm_bindgen(js_name = livestreamsRequests)]
pub fn livestreams_requests(
    token: &str,
    user_ids: IdList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let users = id_list::<UserId>("userIds", &user_ids)?;
    result(public_api::livestreams_requests(&to_endpoints(endpoints)?, token, &users))
}

/// Reads a `livestreamsRequests` answer (or a page's streams alone).
#[wasm_bindgen(js_name = parseLivestreams)]
pub fn parse_livestreams(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_livestreams(http_status(&status)?, body))
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
pub fn parse_channels(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_channels(http_status(&status)?, body))
}

/// The webhook signing key request.
#[wasm_bindgen(js_name = publicKeyRequest)]
pub fn public_key_request(endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    out(&public_api::public_key_request(&to_endpoints(endpoints)?))
}

/// The PEM from a public key answer.
#[wasm_bindgen(js_name = parsePublicKey)]
pub fn parse_public_key(status: Num, body: &str) -> Result<String, JsValue> {
    public_api::parse_public_key(http_status(&status)?, body).map_err(fail)
}

// ---- Website API -------------------------------------------------------------

/// A channel by slug (`kick.com/api/v2`): chatroom id, followers, live state.
#[wasm_bindgen(js_name = channelRequest)]
pub fn channel_request(slug: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(website::channel_request(&to_endpoints(endpoints)?, slug))
}

/// Reads a website channel answer.
#[wasm_bindgen(js_name = parseChannel)]
pub fn parse_channel(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel(http_status(&status)?, body))
}

/// Recent chat and the pin, by channel id.
#[wasm_bindgen(js_name = historyRequest)]
pub fn history_request(channel_id: Id, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    let channel = ChannelId(id("channelId", &channel_id)?);
    out(&website::history_request(&to_endpoints(endpoints)?, channel))
}

/// Reads a history answer: `{messages, pinned}`.
#[wasm_bindgen(js_name = parseHistory)]
pub fn parse_history(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_history(http_status(&status)?, body))
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
pub fn parse_channel_user(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel_user(http_status(&status)?, body))
}

/// The signed-in viewer's standing on a channel.
#[wasm_bindgen(js_name = meRequest)]
pub fn me_request(session: &str, channel_slug: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(website::me_request(&to_endpoints(endpoints)?, session, channel_slug))
}

/// Reads a `me` answer.
#[wasm_bindgen(js_name = parseMe)]
pub fn parse_me(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_me(http_status(&status)?, body))
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
    minutes: Num,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let minutes = u32_arg("minutes", &minutes)?;
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
    minutes: Num,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let minutes = u32_arg("minutes", &minutes)?;
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
pub fn parse_action(status: Num, body: &str) -> Result<(), JsValue> {
    website::parse_action(http_status(&status)?, body).map_err(fail)
}

/// One page of the streams live now (`/public/v2/livestreams`), in up to 25
/// categories and 25 languages (none: all). `limit` is 1 to 1000 (Kick's
/// default 100); `cursor` is the previous page's `next_cursor`. Kick has no
/// sort: streams come oldest first.
#[wasm_bindgen(js_name = livestreamsPageRequest)]
pub fn livestreams_page_request(
    token: &str,
    category_ids: Option<IdList>,
    language_codes: Option<StringList>,
    limit: Option<Num>,
    cursor: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let categories = or_undefined(category_ids);
    let categories: Vec<u64> = if nothing(&categories) { vec![] } else { id_list("categoryIds", &categories)? };
    let languages = string_list("languageCodes", &or_undefined(language_codes))?;
    let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
    // In range: `opt_uint` checked it.
    let limit = opt_uint("limit", limit, u32::MAX.into())?.map(|n| n as u32);
    result(public_api::livestreams_page_request(
        &to_endpoints(endpoints)?,
        token,
        &categories,
        &languages,
        limit,
        cursor.as_deref(),
    ))
}

/// Reads a `livestreamsPageRequest` answer: `{data, next_cursor}`.
#[wasm_bindgen(js_name = parseLivestreamsPage)]
pub fn parse_livestreams_page(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_livestreams_page(http_status(&status)?, body))
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
pub fn parse_subscriptions(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_subscriptions(http_status(&status)?, body))
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

/// Users by id, up to 50; none, with a user token: the token's own user.
#[wasm_bindgen(js_name = usersRequest, unchecked_return_type = "HttpRequest")]
pub fn users_request(
    token: &str,
    user_ids: Option<IdList>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let users = or_undefined(user_ids);
    let users: Vec<UserId> = if nothing(&users) { vec![] } else { id_list("userIds", &users)? };
    result(public_api::users_request(&to_endpoints(endpoints)?, token, &users))
}

/// Reads a `usersRequest` answer.
#[wasm_bindgen(js_name = parseUsers, unchecked_return_type = "User[]")]
pub fn parse_users(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_users(http_status(&status)?, body))
}

/// One page of categories (`/public/v2/categories`): by `names` and `tags`
/// (no commas in them), or by `ids`; none of them, all. `limit` is 1 to 1000
/// (Kick's default 25); `cursor` is the previous page's `next_cursor`.
#[wasm_bindgen(js_name = categoriesRequest, unchecked_return_type = "HttpRequest")]
pub fn categories_request(
    token: &str,
    names: Option<StringList>,
    tags: Option<StringList>,
    ids: Option<IdList>,
    limit: Option<Num>,
    cursor: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let names = string_list("names", &or_undefined(names))?;
    let tags = string_list("tags", &or_undefined(tags))?;
    let ids = or_undefined(ids);
    let ids: Vec<u64> = if nothing(&ids) { vec![] } else { id_list("ids", &ids)? };
    // In range: `opt_uint` checked it.
    let limit = opt_uint("limit", limit, u32::MAX.into())?.map(|n| n as u32);
    result(public_api::categories_request(
        &to_endpoints(endpoints)?,
        token,
        &refs(&names),
        &refs(&tags),
        &ids,
        limit,
        cursor.as_deref(),
    ))
}

/// Reads a `categoriesRequest` answer: `{data, next_cursor}`.
#[wasm_bindgen(js_name = parseCategories, unchecked_return_type = "Page<Category>")]
pub fn parse_categories(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_categories(http_status(&status)?, body))
}

/// How many streams are live on Kick.
#[wasm_bindgen(js_name = livestreamStatsRequest, unchecked_return_type = "HttpRequest")]
pub fn livestream_stats_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(public_api::livestream_stats_request(&to_endpoints(endpoints)?, token))
}

/// Reads a `livestreamStatsRequest` answer.
#[wasm_bindgen(js_name = parseLivestreamStats, unchecked_return_type = "LivestreamStats")]
pub fn parse_livestream_stats(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_livestream_stats(http_status(&status)?, body))
}

/// The top `top` Kicks gifters (1 to 100; Kick's default 10) of the token's
/// own channel. A user token with `kicks:read`.
#[wasm_bindgen(js_name = kicksLeaderboardRequest, unchecked_return_type = "HttpRequest")]
pub fn kicks_leaderboard_request(
    token: &str,
    top: Option<Num>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    // In range: `opt_uint` checked it.
    let top = opt_uint("top", top, u32::MAX.into())?.map(|n| n as u32);
    result(public_api::kicks_leaderboard_request(&to_endpoints(endpoints)?, token, top))
}

/// Reads a `kicksLeaderboardRequest` answer.
#[wasm_bindgen(js_name = parseKicksLeaderboard, unchecked_return_type = "KicksLeaderboard")]
pub fn parse_kicks_leaderboard(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_kicks_leaderboard(http_status(&status)?, body))
}

/// Whether a user-token call whose answer carries nothing worked (any 2xx);
/// throws with `status` if not.
#[wasm_bindgen(js_name = parseDone)]
pub fn parse_done(status: Num, body: &str) -> Result<(), JsValue> {
    public_api::parse_done(http_status(&status)?, body).map_err(fail)
}

fn refs(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

/// A plain object read into one of the core's types: its keys as the core
/// names them, an unknown one refused.
fn plain<T: serde::de::DeserializeOwned>(what: &str, value: &JsValue) -> Result<T, JsValue> {
    if !value.is_object() || js_sys::Array::is_array(value) {
        return Err(type_error(&format!("{what} must be an object")));
    }
    // Through JSON: serde-wasm-bindgen reads a struct's own keys alone, and
    // would let a misspelt one through.
    let json = json_arg(value.clone())?;
    serde_json::from_value(json).map_err(|e| type_error(&format!("{what}: {e}")))
}

// ---- User tokens (OAuth) ---------------------------------------------------

/// A PKCE pair from 32 to 96 bytes drawn from a CSPRNG (`newPkce()` draws
/// them). The verifier is a secret: keep it, never log it.
#[wasm_bindgen(js_name = oauthPkce, unchecked_return_type = "Pkce")]
pub fn oauth_pkce(random: Bytes) -> Result<JsValue, JsValue> {
    let random: JsValue = random.into();
    let bytes = random.dyn_ref::<js_sys::Uint8Array>().ok_or_else(|| type_error("random must be a Uint8Array"))?;
    result(oauth::Pkce::from_random(&bytes.to_vec()))
}

/// The PKCE pair for a verifier kept from before (43 to 128 characters of
/// `A-Z a-z 0-9 - . _ ~`).
#[wasm_bindgen(js_name = oauthPkceFromVerifier, unchecked_return_type = "Pkce")]
pub fn oauth_pkce_from_verifier(verifier: &str) -> Result<JsValue, JsValue> {
    result(oauth::Pkce::from_verifier(verifier))
}

/// Where to send the user's browser to approve the app. `state` (from
/// `newState()`) must come back unchanged on the redirect; `verifier` is the
/// `Pkce`'s.
#[wasm_bindgen(js_name = oauthAuthorizeUrl)]
pub fn oauth_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    scopes: ScopeList,
    state: &str,
    verifier: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<String, JsValue> {
    let scopes = string_list("scopes", &scopes)?;
    let scopes: Vec<oauth::Scope> = scopes.iter().map(|s| oauth::Scope::from(s.as_str())).collect();
    let pkce = oauth::Pkce::from_verifier(verifier).map_err(fail)?;
    oauth::authorize_url(&to_endpoints(endpoints)?, client_id, redirect_uri, &scopes, state, &pkce).map_err(fail)
}

/// A user token for the `code` on the redirect, with the PKCE verifier.
#[wasm_bindgen(js_name = oauthCodeRequest, unchecked_return_type = "HttpRequest")]
pub fn oauth_code_request(
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    out(&oauth::code_request(&to_endpoints(endpoints)?, client_id, client_secret, redirect_uri, code, code_verifier))
}

/// A new user token (and refresh token) for a refresh token.
#[wasm_bindgen(js_name = oauthRefreshRequest, unchecked_return_type = "HttpRequest")]
pub fn oauth_refresh_request(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    out(&oauth::refresh_request(&to_endpoints(endpoints)?, client_id, client_secret, refresh_token))
}

/// Reads an `oauthCodeRequest` or `oauthRefreshRequest` answer.
#[wasm_bindgen(js_name = oauthParseUserToken, unchecked_return_type = "UserToken")]
pub fn oauth_parse_user_token(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(oauth::parse_user_token(http_status(&status)?, body))
}

/// Revokes a token. It goes in the URL: keep the URL out of logs.
#[wasm_bindgen(js_name = oauthRevokeRequest, unchecked_return_type = "HttpRequest")]
pub fn oauth_revoke_request(
    token: &str,
    token_type_hint: Option<TokenTypeHintArg>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let hint = or_undefined(token_type_hint);
    let hint = match hint.as_string().as_deref() {
        _ if nothing(&hint) => None,
        Some("access_token") => Some(oauth::TokenTypeHint::AccessToken),
        Some("refresh_token") => Some(oauth::TokenTypeHint::RefreshToken),
        _ => return Err(type_error("tokenTypeHint must be 'access_token' or 'refresh_token'")),
    };
    result(oauth::revoke_request(&to_endpoints(endpoints)?, token, hint))
}

/// Whether a revoke worked; throws with `status` if not.
#[wasm_bindgen(js_name = oauthParseRevoke)]
pub fn oauth_parse_revoke(status: Num, body: &str) -> Result<(), JsValue> {
    oauth::parse_revoke(http_status(&status)?, body).map_err(fail)
}

/// Asks Kick about the token it is sent with (a user or an app token).
#[wasm_bindgen(js_name = oauthIntrospectRequest, unchecked_return_type = "HttpRequest")]
pub fn oauth_introspect_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(oauth::introspect_request(&to_endpoints(endpoints)?, token))
}

/// Reads an `oauthIntrospectRequest` answer.
#[wasm_bindgen(js_name = oauthParseIntrospect, unchecked_return_type = "Introspection")]
pub fn oauth_parse_introspect(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(oauth::parse_introspect(http_status(&status)?, body))
}

/// Every scope Kick documents.
#[wasm_bindgen(js_name = oauthScopes, unchecked_return_type = "Scope[]")]
pub fn oauth_scopes() -> Vec<String> {
    oauth::Scope::ALL.iter().map(|s| s.as_str().to_owned()).collect()
}

// ---- Official API, user tokens ---------------------------------------------

/// Sends `content` as the token's user into a channel (`{user:
/// broadcasterUserId}`) or as the app's bot (`'bot'`), as a reply if
/// `replyToMessageId` is given. Scope `chat:write`.
#[wasm_bindgen(js_name = chatSendRequest, unchecked_return_type = "HttpRequest")]
pub fn chat_send_request(
    token: &str,
    sender: ChatSenderArg,
    content: &str,
    reply_to_message_id: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let sender: JsValue = sender.into();
    let wrong = || type_error("sender must be {user: broadcasterUserId} or 'bot'");
    let sender = if sender.as_string().as_deref() == Some("bot") {
        chat::Sender::Bot
    } else if sender.is_object() && !js_sys::Array::is_array(&sender) {
        let keys = js_sys::Object::keys(sender.unchecked_ref::<js_sys::Object>());
        if keys.length() != 1 || keys.get(0).as_string().as_deref() != Some("user") {
            return Err(wrong());
        }
        chat::Sender::User { broadcaster: UserId(id("sender.user", &js_sys::Reflect::get(&sender, &"user".into())?)?) }
    } else {
        return Err(wrong());
    };
    result(chat::send_request(&to_endpoints(endpoints)?, token, sender, content, reply_to_message_id.as_deref()))
}

/// Reads a `chatSendRequest` answer.
#[wasm_bindgen(js_name = chatParseSend, unchecked_return_type = "Sent")]
pub fn chat_parse_send(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(chat::parse_send(http_status(&status)?, body))
}

/// Deletes a message by id. Scope `moderation:chat_message:manage`;
/// `parseDone` reads the answer.
#[wasm_bindgen(js_name = chatDeleteRequest, unchecked_return_type = "HttpRequest")]
pub fn chat_delete_request(token: &str, message_id: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(chat::delete_request(&to_endpoints(endpoints)?, token, message_id))
}

/// Bans a user for good. Scope `moderation:ban`; `parseDone` reads the answer.
#[wasm_bindgen(js_name = moderationBanRequest, unchecked_return_type = "HttpRequest")]
pub fn moderation_ban_request(
    token: &str,
    broadcaster_user_id: Id,
    user_id: Id,
    reason: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let broadcaster = UserId(id("broadcasterUserId", &broadcaster_user_id)?);
    let user = UserId(id("userId", &user_id)?);
    result(moderation::ban_request(&to_endpoints(endpoints)?, token, broadcaster, user, reason.as_deref()))
}

/// Times a user out for 1 to 10080 minutes. Scope `moderation:ban`.
#[wasm_bindgen(js_name = moderationTimeoutRequest, unchecked_return_type = "HttpRequest")]
pub fn moderation_timeout_request(
    token: &str,
    broadcaster_user_id: Id,
    user_id: Id,
    minutes: Num,
    reason: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let broadcaster = UserId(id("broadcasterUserId", &broadcaster_user_id)?);
    let user = UserId(id("userId", &user_id)?);
    let minutes = u32_arg("minutes", &minutes)?;
    result(moderation::timeout_request(&to_endpoints(endpoints)?, token, broadcaster, user, minutes, reason.as_deref()))
}

/// Lifts a ban or a timeout. Scope `moderation:ban`.
#[wasm_bindgen(js_name = moderationUnbanRequest, unchecked_return_type = "HttpRequest")]
pub fn moderation_unban_request(
    token: &str,
    broadcaster_user_id: Id,
    user_id: Id,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let broadcaster = UserId(id("broadcasterUserId", &broadcaster_user_id)?);
    let user = UserId(id("userId", &user_id)?);
    result(moderation::unban_request(&to_endpoints(endpoints)?, token, broadcaster, user))
}

/// Sets what is given of the token's channel's category, custom tags (`[]`
/// clears them, up to 10) and stream title. Scope `channel:write`;
/// `parseDone` reads the answer.
#[wasm_bindgen(js_name = channelsUpdateRequest, unchecked_return_type = "HttpRequest")]
pub fn channels_update_request(
    token: &str,
    category_id: Option<Id>,
    custom_tags: Option<StringList>,
    stream_title: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let category = opt_id("categoryId", &or_undefined(category_id))?;
    let tags = or_undefined(custom_tags);
    let tags = if nothing(&tags) { None } else { Some(string_list("customTags", &tags)?) };
    let tags = tags.as_deref().map(refs);
    result(channels::update_request(
        &to_endpoints(endpoints)?,
        token,
        category,
        tags.as_deref(),
        stream_title.as_deref(),
    ))
}

/// Every channel-points reward of the token's channel.
#[wasm_bindgen(js_name = rewardsListRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_list_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(rewards::list_request(&to_endpoints(endpoints)?, token))
}

/// Reads a `rewardsListRequest` answer.
#[wasm_bindgen(js_name = rewardsParseRewards, unchecked_return_type = "Reward[]")]
pub fn rewards_parse_rewards(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(rewards::parse_rewards(http_status(&status)?, body))
}

/// Creates a reward: `fields` needs `title` and `cost`.
#[wasm_bindgen(js_name = rewardsCreateRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_create_request(
    token: &str,
    fields: RewardFieldsArg,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let fields: rewards::RewardFields = plain("fields", &fields)?;
    result(rewards::create_request(&to_endpoints(endpoints)?, token, &fields))
}

/// Changes what `fields` sets of reward `id`.
#[wasm_bindgen(js_name = rewardsUpdateRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_update_request(
    token: &str,
    id: &str,
    fields: RewardFieldsArg,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let fields: rewards::RewardFields = plain("fields", &fields)?;
    result(rewards::update_request(&to_endpoints(endpoints)?, token, id, &fields))
}

/// Reads a `rewardsCreateRequest` or `rewardsUpdateRequest` answer.
#[wasm_bindgen(js_name = rewardsParseReward, unchecked_return_type = "Reward")]
pub fn rewards_parse_reward(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(rewards::parse_reward(http_status(&status)?, body))
}

/// Deletes reward `id`; `parseDone` reads the answer.
#[wasm_bindgen(js_name = rewardsDeleteRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_delete_request(token: &str, id: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(rewards::delete_request(&to_endpoints(endpoints)?, token, id))
}

/// Redemptions, grouped by reward: of one reward or all, with one `status`
/// (Kick's default `'pending'`), from `cursor`.
#[wasm_bindgen(js_name = rewardsRedemptionsRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_redemptions_request(
    token: &str,
    reward_id: Option<String>,
    status: Option<RedemptionStatusArg>,
    cursor: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let status = or_undefined(status);
    let status = match status.as_string().as_deref() {
        _ if nothing(&status) => None,
        Some("pending") => Some(rewards::RedemptionStatus::Pending),
        Some("accepted") => Some(rewards::RedemptionStatus::Accepted),
        Some("rejected") => Some(rewards::RedemptionStatus::Rejected),
        _ => return Err(type_error("status must be 'pending', 'accepted' or 'rejected'")),
    };
    result(rewards::redemptions_request(
        &to_endpoints(endpoints)?,
        token,
        reward_id.as_deref(),
        status,
        cursor.as_deref(),
    ))
}

/// Redemptions by id, whatever their status.
#[wasm_bindgen(js_name = rewardsRedemptionsByIdsRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_redemptions_by_ids_request(
    token: &str,
    ids: StringList,
    cursor: Option<String>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let ids = string_list("ids", &ids)?;
    result(rewards::redemptions_by_ids_request(&to_endpoints(endpoints)?, token, &refs(&ids), cursor.as_deref()))
}

/// Reads a `rewardsRedemptionsRequest` or `rewardsRedemptionsByIdsRequest`
/// answer.
#[wasm_bindgen(js_name = rewardsParseRedemptions, unchecked_return_type = "RedemptionPage")]
pub fn rewards_parse_redemptions(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(rewards::parse_redemptions(http_status(&status)?, body))
}

/// Accepts 1 to 25 pending redemptions; `rewardsParseFailures` reads the
/// answer.
#[wasm_bindgen(js_name = rewardsAcceptRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_accept_request(
    token: &str,
    ids: StringList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let ids = string_list("ids", &ids)?;
    result(rewards::accept_request(&to_endpoints(endpoints)?, token, &refs(&ids)))
}

/// Rejects 1 to 25 pending redemptions; `rewardsParseFailures` reads the
/// answer.
#[wasm_bindgen(js_name = rewardsRejectRequest, unchecked_return_type = "HttpRequest")]
pub fn rewards_reject_request(
    token: &str,
    ids: StringList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let ids = string_list("ids", &ids)?;
    result(rewards::reject_request(&to_endpoints(endpoints)?, token, &refs(&ids)))
}

/// The redemptions an accept or reject failed for; empty is all done.
#[wasm_bindgen(js_name = rewardsParseFailures, unchecked_return_type = "FailedRedemption[]")]
pub fn rewards_parse_failures(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(rewards::parse_failures(http_status(&status)?, body))
}

/// Starts an ad break of 7 to 90 `seconds`; `id` is a UUID you make up
/// (`crypto.randomUUID()`).
#[wasm_bindgen(js_name = adsAdBreakRequest, unchecked_return_type = "HttpRequest")]
pub fn ads_ad_break_request(
    token: &str,
    id: &str,
    seconds: Num,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let seconds = u32_arg("seconds", &seconds)?;
    result(ads::ad_break_request(&to_endpoints(endpoints)?, token, id, seconds))
}

/// Reads an `adsAdBreakRequest` answer.
#[wasm_bindgen(js_name = adsParseAdBreak, unchecked_return_type = "AdBreak")]
pub fn ads_parse_ad_break(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(ads::parse_ad_break(http_status(&status)?, body))
}

/// The channel's ads: enrolled or not, limits, recent ad breaks.
#[wasm_bindgen(js_name = adsStatusRequest, unchecked_return_type = "HttpRequest")]
pub fn ads_status_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(ads::status_request(&to_endpoints(endpoints)?, token))
}

/// Reads an `adsStatusRequest` answer.
#[wasm_bindgen(js_name = adsParseStatus, unchecked_return_type = "AdStatus")]
pub fn ads_parse_status(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(ads::parse_status(http_status(&status)?, body))
}

/// Enrolls the channel in ads; `parseDone` reads the answer.
#[wasm_bindgen(js_name = adsEnrollRequest, unchecked_return_type = "HttpRequest")]
pub fn ads_enroll_request(token: &str, endpoints: Option<EndpointsArg>) -> Result<JsValue, JsValue> {
    result(ads::enroll_request(&to_endpoints(endpoints)?, token))
}

/// Drops reward claims, with an **app** token. `filter`: what is left out
/// is not filtered.
#[wasm_bindgen(js_name = dropsClaimsRequest, unchecked_return_type = "HttpRequest")]
pub fn drops_claims_request(
    token: &str,
    filter: Option<ClaimFilterArg>,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    let filter = or_undefined(filter);
    let filter: drops::ClaimFilter = if nothing(&filter) { Default::default() } else { plain("filter", &filter)? };
    result(drops::claims_request(&to_endpoints(endpoints)?, token, &filter))
}

/// Reads a `dropsClaimsRequest` answer.
#[wasm_bindgen(js_name = dropsParseClaims, unchecked_return_type = "Claims")]
pub fn drops_parse_claims(status: Num, body: &str) -> Result<JsValue, JsValue> {
    result(drops::parse_claims(http_status(&status)?, body))
}

/// Sets 1 to 100 claims' `external_status`; `parseDone` reads the answer.
#[wasm_bindgen(js_name = dropsUpdateClaimsRequest, unchecked_return_type = "HttpRequest")]
pub fn drops_update_claims_request(
    token: &str,
    updates: ClaimUpdateList,
    endpoints: Option<EndpointsArg>,
) -> Result<JsValue, JsValue> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Update {
        claim_id: String,
        external_status: String,
    }
    if !js_sys::Array::is_array(&updates) {
        return Err(type_error("updates must be an array of {claim_id, external_status}"));
    }
    let list: &js_sys::Array = updates.unchecked_ref();
    let updates: Vec<Update> =
        list.iter().enumerate().map(|(i, u)| plain(&format!("updates[{i}]"), &u)).collect::<Result<_, _>>()?;
    let pairs: Vec<(&str, &str)> = updates.iter().map(|u| (u.claim_id.as_str(), u.external_status.as_str())).collect();
    result(drops::update_claims_request(&to_endpoints(endpoints)?, token, &pairs))
}

// ---- Webhooks ----------------------------------------------------------------

/// A body as Kick sent it: text, or the raw bytes.
fn body_bytes(value: &JsValue) -> Result<Vec<u8>, JsValue> {
    if let Some(text) = value.as_string() {
        Ok(text.into_bytes())
    } else if let Some(bytes) = value.dyn_ref::<js_sys::Uint8Array>() {
        Ok(bytes.to_vec())
    } else if let Some(buffer) = value.dyn_ref::<js_sys::ArrayBuffer>() {
        Ok(js_sys::Uint8Array::new(buffer).to_vec())
    } else {
        Err(type_error("body must be a string, a Uint8Array or an ArrayBuffer"))
    }
}

/// A header, or `None` for one that is not Kick's and whose value is not a
/// string (Node's `set-cookie` is an array): such a header is not read,
/// whatever it holds. Kick's own must be strings.
fn header_pair(pair: JsValue) -> Result<Option<(String, String)>, JsValue> {
    if !js_sys::Array::is_array(&pair) {
        return Err(type_error("headers: each must be a [name, value] pair"));
    }
    let pair: js_sys::Array = pair.unchecked_into();
    let name = pair.get(0).as_string().ok_or_else(|| type_error("headers: a name must be a string"))?;
    match pair.get(1).as_string() {
        Some(value) => Ok(Some((name, value))),
        None if name.get(..11).is_some_and(|p| p.eq_ignore_ascii_case("kick-event-")) => {
            Err(type_error(&format!("headers: {name} must be a string")))
        }
        None => Ok(None),
    }
}

/// `[[name, value]…]`, any iterable of pairs (a fetch `Headers`, a `Map`),
/// or an object.
fn webhook_headers(value: HeadersArg) -> Result<webhooks::Headers, JsValue> {
    let value: JsValue = value.into();
    if !value.is_object() {
        return Err(type_error("headers must be [name, value] pairs, a Headers or an object"));
    }
    let pairs: Vec<Option<(String, String)>> = match js_sys::try_iter(&value)? {
        Some(iter) => iter.map(|pair| header_pair(pair?)).collect::<Result<_, _>>()?,
        None => js_sys::Object::entries(value.unchecked_ref()).iter().map(header_pair).collect::<Result<_, _>>()?,
    };
    let pairs: Vec<(String, String)> = pairs.into_iter().flatten().collect();
    webhooks::Headers::from_pairs(pairs.iter().map(|(a, b)| (a.as_str(), b.as_str()))).map_err(webhook_error)
}

/// A `now` in seconds this large is far in the future (the year 5138): it
/// is `Date.now()`'s milliseconds by mistake.
const NOW_SECS_LIMIT: u64 = 100_000_000_000;

/// This machine's clock, Unix seconds.
fn machine_now() -> u64 {
    // Date.now() is a whole number of ms, positive and far below 2^53.
    (js_sys::Date::now() / 1000.0).floor().max(0.0) as u64
}

/// `{now?, toleranceSecs?, skipFreshness?}`; by default within 5 minutes of
/// this machine's clock.
fn freshness(options: Option<VerifyOptionsArg>) -> Result<webhooks::Freshness, JsValue> {
    let options: JsValue = options.map(Into::into).unwrap_or(JsValue::UNDEFINED);
    if nothing(&options) {
        return Ok(webhooks::Freshness::at(machine_now()));
    }
    if !options.is_object() {
        return Err(type_error("options must be an object: {now?, toleranceSecs?, skipFreshness?}"));
    }
    for key in js_sys::Object::keys(options.unchecked_ref::<js_sys::Object>()).iter() {
        let key = key.as_string().unwrap_or_default();
        if !matches!(key.as_str(), "now" | "toleranceSecs" | "skipFreshness") {
            return Err(type_error(&format!("options: unknown key `{key}` (now, toleranceSecs, skipFreshness)")));
        }
    }
    let get = |key: &str| js_sys::Reflect::get(&options, &key.into());
    let (now, tolerance, skip) = (get("now")?, get("toleranceSecs")?, get("skipFreshness")?);
    let skip = match skip.as_bool() {
        Some(skip) => skip,
        None if nothing(&skip) => false,
        None => return Err(type_error("options.skipFreshness must be a boolean")),
    };
    if skip {
        if !nothing(&now) || !nothing(&tolerance) {
            return Err(type_error("options: give now and toleranceSecs, or skipFreshness, not both"));
        }
        return Ok(webhooks::Freshness::Skip);
    }
    let now = if nothing(&now) { machine_now() } else { uint("options.now (Unix seconds)", &now, MAX_SAFE)? };
    if now >= NOW_SECS_LIMIT {
        return Err(range_error(
            "options.now is Unix seconds; this looks like milliseconds (Math.floor(Date.now() / 1000))",
        ));
    }
    let tolerance_secs = if nothing(&tolerance) {
        webhooks::DEFAULT_TOLERANCE_SECS
    } else {
        uint("options.toleranceSecs", &tolerance, MAX_SAFE)?
    };
    Ok(webhooks::Freshness::Within { now, tolerance_secs })
}

fn webhook_error(e: webhooks::WebhookError) -> JsValue {
    error(&e.to_string())
}

/// What a delivery that passed comes out as: its headers and typed event.
fn verified(headers: &webhooks::Headers, body: &[u8]) -> Result<VerifiedWebhook, JsValue> {
    let event = webhooks::parse(&headers.event_type, body).map_err(fail)?;
    #[derive(Serialize)]
    struct Verified<'a> {
        headers: &'a webhooks::Headers,
        event: webhooks::WebhookEvent,
    }
    Ok(out(&Verified { headers, event })?.unchecked_into())
}

/// Reads a delivery's body by its type header, without checking anything.
/// Throws when the body is not that type's shape. `body` is text or bytes.
#[wasm_bindgen(js_name = parseWebhook)]
pub fn parse_webhook(event_type: &str, body: Body) -> Result<JsValue, JsValue> {
    result(webhooks::parse(event_type, body_bytes(&body)?))
}

/// Checks a delivery and returns `{headers, event}`, or throws. `headers` as
/// `[[name, value]…]`, a fetch `Headers` or an object; `body` the raw body,
/// as text or bytes. The timestamp must be within 5 minutes of this
/// machine's clock, unless `options` says otherwise: `now` (Unix seconds),
/// `toleranceSecs`, or `skipFreshness: true` to re-check a stored delivery.
///
/// A receiver checking many deliveries reads the key once, with
/// `new WebhookPublicKey(pem)`, and calls its `verify`.
#[wasm_bindgen(js_name = verifyWebhook)]
pub fn verify_webhook(
    public_key_pem: &str,
    headers: HeadersArg,
    body: Body,
    options: Option<VerifyOptionsArg>,
) -> Result<VerifiedWebhook, JsValue> {
    let (h, body, freshness) = (webhook_headers(headers)?, body_bytes(&body)?, freshness(options)?);
    let key = webhooks::PublicKey::from_pem(public_key_pem).map_err(webhook_error)?;
    webhooks::verify(&key, &h, &body, freshness).map_err(webhook_error)?;
    verified(&h, &body)
}

/// Kick's webhook key, read once: `new WebhookPublicKey(pem)`, then
/// `key.verify(headers, body, options?)` for each delivery.
#[wasm_bindgen]
pub struct WebhookPublicKey(webhooks::PublicKey);

#[wasm_bindgen]
impl WebhookPublicKey {
    /// Reads a PEM key (`BEGIN PUBLIC KEY`, as Kick serves it, or
    /// `BEGIN RSA PUBLIC KEY`); throws if it is not one.
    #[wasm_bindgen(constructor)]
    pub fn new(pem: &str) -> Result<WebhookPublicKey, JsValue> {
        webhooks::PublicKey::from_pem(pem).map(WebhookPublicKey).map_err(webhook_error)
    }

    /// `verifyWebhook`, with this key.
    pub fn verify(
        &self,
        headers: HeadersArg,
        body: Body,
        options: Option<VerifyOptionsArg>,
    ) -> Result<VerifiedWebhook, JsValue> {
        let (h, body, freshness) = (webhook_headers(headers)?, body_bytes(&body)?, freshness(options)?);
        webhooks::verify(&self.0, &h, &body, freshness).map_err(webhook_error)?;
        verified(&h, &body)
    }
}
