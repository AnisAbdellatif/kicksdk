//! kicksdk for JavaScript.
//!
//! Every function here is a thin wrapper: arguments in, the core's answer out
//! as plain JS objects (`snake_case` keys, as Kick writes them). Requests come
//! back as `{method, url, headers: [[name, value]], body}` for the caller's
//! `fetch`; parsers take `(status, bodyText)` and throw an `Error` with a
//! `status` property when Kick answered with an error status.
//!
//! The hand-written driver in `bindings/js` puts these together with `fetch`,
//! `WebSocket` and timers; most callers want that rather than these.

use kicksdk::http::Endpoints;
use kicksdk::ids::{ChannelId, ChatroomId, UserId};
use kicksdk::realtime;
use kicksdk::{content, events, public_api, webhooks, website};
use serde::Serialize;
use wasm_bindgen::prelude::*;

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

fn endpoints(value: JsValue) -> Result<Endpoints, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(Endpoints::default());
    }
    // Partial overrides are filled from the defaults.
    let mut e = serde_json::to_value(Endpoints::default()).unwrap();
    let given: serde_json::Value =
        serde_wasm_bindgen::from_value(value).map_err(|err| JsValue::from(js_sys::Error::new(&err.to_string())))?;
    if let (Some(base), Some(over)) = (e.as_object_mut(), given.as_object()) {
        for (k, v) in over {
            base.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(e).map_err(|err| js_sys::Error::new(&err.to_string()).into())
}

fn json_arg(value: JsValue) -> Result<serde_json::Value, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|e| js_sys::Error::new(&e.to_string()).into())
}

fn ids<T: From<u64>>(list: Vec<f64>) -> Vec<T> {
    list.into_iter().map(|n| T::from(n as u64)).collect()
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
pub fn chat_channels(chatroom_id: f64, channel_id: Option<f64>) -> Vec<String> {
    realtime::chat_channels(ChatroomId(chatroom_id as u64), channel_id.map(|c| ChannelId(c as u64)))
}

/// Where a channel's chat is served.
#[wasm_bindgen(js_name = chatConnectionRequest)]
pub fn chat_connection_request(channel_id: f64, client_id: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&realtime::chat_connection_request(&endpoints(endpoints_)?, ChannelId(channel_id as u64), client_id))
}

/// Where Kick's realtime is served in general.
#[wasm_bindgen(js_name = connectionRequest)]
pub fn connection_request(client_id: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&realtime::connection_request(&endpoints(endpoints_)?, client_id))
}

/// Reads either connection answer: `[{provider, url}…]`.
#[wasm_bindgen(js_name = parseConnection)]
pub fn parse_connection(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(realtime::parse_connection(status, body))
}

/// A guest token request for `clientId`.
#[wasm_bindgen(js_name = tokenRequest)]
pub fn token_request(client_id: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&realtime::token_request(&endpoints(endpoints_)?, client_id))
}

/// The token from a token answer.
#[wasm_bindgen(js_name = parseToken)]
pub fn parse_token(status: u16, body: &str) -> Result<String, JsValue> {
    realtime::parse_token(status, body).map_err(fail)
}

/// One connection's protocol. Each method returns the actions to carry out.
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
        out(&self.0.open(token, now as u64))
    }

    /// A text frame arrived.
    pub fn receive(&mut self, text: &str, now: f64) -> Result<JsValue, JsValue> {
        out(&self.0.receive(text, now as u64))
    }

    /// A new token, after a `refresh_token` action.
    pub fn refresh(&mut self, token: &str, now: f64) -> Result<JsValue, JsValue> {
        out(&self.0.refresh(token, now as u64))
    }

    /// The clock reached `deadline()`.
    pub fn tick(&mut self, now: f64) -> Result<JsValue, JsValue> {
        out(&self.0.tick(now as u64))
    }

    /// When to call `tick` next, in ms, or undefined.
    pub fn deadline(&self) -> Option<f64> {
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
    /// From `initialMs`, doubling to `maxMs`.
    #[wasm_bindgen(constructor)]
    pub fn new(initial_ms: Option<f64>, max_ms: Option<f64>) -> Backoff {
        Backoff(realtime::Backoff::new(initial_ms.unwrap_or(1000.0) as u64, max_ms.unwrap_or(30000.0) as u64))
    }

    /// The next wait, ms.
    #[wasm_bindgen(js_name = nextDelay)]
    pub fn next_delay(&mut self) -> f64 {
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
pub fn app_token_request(client_id: &str, client_secret: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&public_api::token_request(&endpoints(endpoints_)?, client_id, client_secret))
}

/// Reads an app token answer.
#[wasm_bindgen(js_name = parseAppToken)]
pub fn parse_app_token(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_token(status, body))
}

/// `/livestreams` for any number of user ids: one request per 50.
#[wasm_bindgen(js_name = livestreamsRequests)]
pub fn livestreams_requests(token: &str, user_ids: Vec<f64>, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(public_api::livestreams_requests(&endpoints(endpoints_)?, token, &ids::<UserId>(user_ids)))
}

/// Reads a `/livestreams` answer.
#[wasm_bindgen(js_name = parseLivestreams)]
pub fn parse_livestreams(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_livestreams(status, body))
}

/// `/channels` for up to 50 user ids.
#[wasm_bindgen(js_name = channelsByIdsRequest)]
pub fn channels_by_ids_request(token: &str, user_ids: Vec<f64>, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(public_api::channels_by_ids_request(&endpoints(endpoints_)?, token, &ids::<UserId>(user_ids)))
}

/// `/channels` for up to 50 slugs (one unknown slug fails them all).
#[wasm_bindgen(js_name = channelsBySlugsRequest)]
pub fn channels_by_slugs_request(token: &str, slugs: Vec<String>, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
    result(public_api::channels_by_slugs_request(&endpoints(endpoints_)?, token, &refs))
}

/// Reads a `/channels` answer.
#[wasm_bindgen(js_name = parseChannels)]
pub fn parse_channels(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_channels(status, body))
}

/// The webhook signing key request.
#[wasm_bindgen(js_name = publicKeyRequest)]
pub fn public_key_request(endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&public_api::public_key_request(&endpoints(endpoints_)?))
}

/// The PEM from a public key answer.
#[wasm_bindgen(js_name = parsePublicKey)]
pub fn parse_public_key(status: u16, body: &str) -> Result<String, JsValue> {
    public_api::parse_public_key(status, body).map_err(fail)
}

// ---- Website API -------------------------------------------------------------

/// A channel by slug (`kick.com/api/v2`): chatroom id, followers, live state.
#[wasm_bindgen(js_name = channelRequest)]
pub fn channel_request(slug: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(website::channel_request(&endpoints(endpoints_)?, slug))
}

/// Reads a website channel answer.
#[wasm_bindgen(js_name = parseChannel)]
pub fn parse_channel(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel(status, body))
}

/// Recent chat and the pin, by channel id.
#[wasm_bindgen(js_name = historyRequest)]
pub fn history_request(channel_id: f64, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    out(&website::history_request(&endpoints(endpoints_)?, ChannelId(channel_id as u64)))
}

/// Reads a history answer: `{messages, pinned}`.
#[wasm_bindgen(js_name = parseHistory)]
pub fn parse_history(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_history(status, body))
}

/// A user as a channel sees them.
#[wasm_bindgen(js_name = channelUserRequest)]
pub fn channel_user_request(channel_slug: &str, username: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(website::channel_user_request(&endpoints(endpoints_)?, channel_slug, username))
}

/// Reads a channel user answer.
#[wasm_bindgen(js_name = parseChannelUser)]
pub fn parse_channel_user(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_channel_user(status, body))
}

/// The signed-in viewer's standing on a channel.
#[wasm_bindgen(js_name = meRequest)]
pub fn me_request(session: &str, channel_slug: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(website::me_request(&endpoints(endpoints_)?, session, channel_slug))
}

/// Reads a `me` answer.
#[wasm_bindgen(js_name = parseMe)]
pub fn parse_me(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(website::parse_me(status, body))
}

/// Bans a user for good.
#[wasm_bindgen(js_name = banRequest)]
pub fn ban_request(session: &str, channel_slug: &str, username: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(website::ban_request(&endpoints(endpoints_)?, session, channel_slug, username))
}

/// Times a user out.
#[wasm_bindgen(js_name = timeoutRequest)]
pub fn timeout_request(
    session: &str,
    channel_slug: &str,
    username: &str,
    minutes: u32,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    result(website::timeout_request(&endpoints(endpoints_)?, session, channel_slug, username, minutes))
}

/// Lifts a ban or timeout.
#[wasm_bindgen(js_name = unbanRequest)]
pub fn unban_request(
    session: &str,
    channel_slug: &str,
    username: &str,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    result(website::unban_request(&endpoints(endpoints_)?, session, channel_slug, username))
}

/// Deletes a message.
#[wasm_bindgen(js_name = deleteMessageRequest)]
pub fn delete_message_request(
    session: &str,
    chatroom_id: f64,
    message_id: &str,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    result(website::delete_message_request(
        &endpoints(endpoints_)?,
        session,
        ChatroomId(chatroom_id as u64),
        message_id,
    ))
}

/// Pins a message (the feed's data for it, whole) for `minutes`.
#[wasm_bindgen(js_name = pinRequest)]
pub fn pin_request(
    session: &str,
    channel_slug: &str,
    message: JsValue,
    minutes: u32,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    result(website::pin_request(&endpoints(endpoints_)?, session, channel_slug, &json_arg(message)?, minutes))
}

/// Removes the pin.
#[wasm_bindgen(js_name = unpinRequest)]
pub fn unpin_request(session: &str, channel_slug: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(website::unpin_request(&endpoints(endpoints_)?, session, channel_slug))
}

/// Sends a message, or a reply when `replyTo` is
/// `{message_id, content, sender_id, sender_username}`.
#[wasm_bindgen(js_name = sendRequest)]
pub fn send_request(
    session: &str,
    chatroom_id: f64,
    text: &str,
    message_ref: &str,
    reply_to: JsValue,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    let reply: Option<serde_json::Value> =
        if reply_to.is_undefined() || reply_to.is_null() { None } else { Some(json_arg(reply_to)?) };
    let target = match &reply {
        None => None,
        Some(r) => Some(website::ReplyTarget {
            message_id: r["message_id"].as_str().ok_or_else(|| js_sys::Error::new("replyTo.message_id"))?,
            content: r["content"].as_str().unwrap_or(""),
            sender_id: r["sender_id"].as_u64().ok_or_else(|| js_sys::Error::new("replyTo.sender_id"))?,
            sender_username: r["sender_username"]
                .as_str()
                .ok_or_else(|| js_sys::Error::new("replyTo.sender_username"))?,
        }),
    };
    result(website::send_request(
        &endpoints(endpoints_)?,
        session,
        ChatroomId(chatroom_id as u64),
        text,
        message_ref,
        target.as_ref(),
    ))
}

/// Whether an acting call worked; throws with `status` if not.
#[wasm_bindgen(js_name = parseAction)]
pub fn parse_action(status: u16, body: &str) -> Result<(), JsValue> {
    website::parse_action(status, body).map_err(fail)
}

/// The top live streams, overall or in a category or language: up to 100.
/// `sort` is `"viewer_count"` (the default) or `"started_at"`.
#[wasm_bindgen(js_name = topLivestreamsRequest)]
pub fn top_livestreams_request(
    token: &str,
    category_id: Option<f64>,
    language: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    let sort = match sort.as_deref().unwrap_or("viewer_count") {
        "started_at" => public_api::Sort::StartedAt,
        _ => public_api::Sort::ViewerCount,
    };
    result(public_api::top_livestreams_request(
        &endpoints(endpoints_)?,
        token,
        category_id.map(|c| c as u64),
        language.as_deref(),
        sort,
        limit.unwrap_or(100),
    ))
}

/// Subscribes the app to webhook `events` (`[{name, version}]`) for one channel.
#[wasm_bindgen(js_name = subscribeRequest)]
pub fn subscribe_request(
    token: &str,
    broadcaster_user_id: f64,
    events: JsValue,
    endpoints_: JsValue,
) -> Result<JsValue, JsValue> {
    let list = json_arg(events)?;
    let pairs: Vec<(String, u32)> = list
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    Some((
                        e.get("name")?.as_str()?.to_owned(),
                        e.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let refs: Vec<(&str, u32)> = pairs.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    result(public_api::subscribe_request(&endpoints(endpoints_)?, token, UserId(broadcaster_user_id as u64), &refs))
}

/// Every webhook subscription the app has.
#[wasm_bindgen(js_name = subscriptionsRequest)]
pub fn subscriptions_request(token: &str, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    result(public_api::subscriptions_request(&endpoints(endpoints_)?, token))
}

/// Reads a subscriptions answer.
#[wasm_bindgen(js_name = parseSubscriptions)]
pub fn parse_subscriptions(status: u16, body: &str) -> Result<JsValue, JsValue> {
    result(public_api::parse_subscriptions(status, body))
}

/// Removes subscriptions, 50 per request.
#[wasm_bindgen(js_name = unsubscribeRequests)]
pub fn unsubscribe_requests(token: &str, ids: Vec<String>, endpoints_: JsValue) -> Result<JsValue, JsValue> {
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    result(public_api::unsubscribe_requests(&endpoints(endpoints_)?, token, &refs))
}

// ---- Webhooks ----------------------------------------------------------------

/// Reads a delivery's body by its type header, without checking anything.
#[wasm_bindgen(js_name = parseWebhook)]
pub fn parse_webhook(event_type: &str, body: &str) -> Result<JsValue, JsValue> {
    out(&webhooks::parse(event_type, body).map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?)
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
    webhooks::verify(public_key_pem, &h, body.as_bytes())
        .map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    let event = webhooks::parse(&h.event_type, body).map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    #[derive(Serialize)]
    struct Verified<'a> {
        headers: &'a webhooks::Headers,
        event: webhooks::WebhookEvent,
    }
    out(&Verified { headers: &h, event })
}
