//! kicksdk for Elixir: the core's functions as NIFs on `Kicksdk.Native`.
//!
//! The Elixir modules in `lib/` are the interface; this file only converts.
//! Values cross the way JSON decodes in Elixir: maps with **string keys**,
//! lists, binaries, numbers, booleans and `nil`. Not atoms: much of what comes
//! back is Kick's own data, and turning its keys into atoms would let Kick
//! grow the atom table without limit. `{:ok, _}` / `{:error, _}` where Kick's
//! answer can be refused. JSON arguments (the endpoints, a message to pin)
//! arrive already encoded by the Elixir side.
//!
//! Every NIF here returns in microseconds - no I/O, no waiting - so none needs
//! a dirty scheduler. Webhook verification (one RSA public-key operation) is
//! the slowest, at well under a millisecond.

use kicksdk::http::Endpoints;
use kicksdk::ids::{ChannelId, ChatroomId, UserId};
use kicksdk::{content, events, public_api, realtime, webhooks, website};
use rustler::{Encoder, Env, NifResult, Resource, ResourceArc, Term};
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;

mod atoms {
    rustler::atoms! { ok, error, status, shape, json, invalid }
}

fn term<'a, T: Serialize + ?Sized>(env: Env<'a>, v: &T) -> NifResult<Term<'a>> {
    let value = serde_json::to_value(v).map_err(|e| rustler::Error::Term(Box::new(e.to_string())))?;
    Ok(encode(env, &value))
}

fn encode<'a>(env: Env<'a>, v: &Value) -> Term<'a> {
    match v {
        Value::Null => rustler::types::atom::nil().encode(env),
        Value::Bool(b) => b.encode(env),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.encode(env),
            (None, Some(u)) => u.encode(env),
            _ => n.as_f64().unwrap_or(0.0).encode(env),
        },
        Value::String(s) => s.encode(env),
        Value::Array(list) => list.iter().map(|x| encode(env, x)).collect::<Vec<_>>().encode(env),
        Value::Object(map) => {
            let keys: Vec<Term<'a>> = map.keys().map(|k| k.encode(env)).collect();
            let values: Vec<Term<'a>> = map.values().map(|x| encode(env, x)).collect();
            Term::map_from_term_arrays(env, &keys, &values).unwrap_or_else(|_| rustler::types::atom::nil().encode(env))
        }
    }
}

fn error_term<'a>(env: Env<'a>, e: &kicksdk::Error) -> Term<'a> {
    match e {
        kicksdk::Error::Status { status, excerpt } => (atoms::status(), *status, excerpt.as_str()).encode(env),
        kicksdk::Error::Json(err) => (atoms::json(), err.to_string()).encode(env),
        kicksdk::Error::Shape(what) => (atoms::shape(), what.as_str()).encode(env),
        kicksdk::Error::Invalid(what) => (atoms::invalid(), what.as_str()).encode(env),
        // The core may grow new errors.
        other => (atoms::shape(), other.to_string()).encode(env),
    }
}

/// `{:ok, value}` or `{:error, reason}`.
fn result<'a, T: Serialize>(env: Env<'a>, r: Result<T, kicksdk::Error>) -> NifResult<Term<'a>> {
    Ok(match r {
        Ok(v) => (atoms::ok(), term(env, &v)?).encode(env),
        Err(e) => (atoms::error(), error_term(env, &e)).encode(env),
    })
}

fn endpoints(json: &str) -> NifResult<Endpoints> {
    if json.is_empty() {
        return Ok(Endpoints::default());
    }
    let mut base = serde_json::to_value(Endpoints::default()).unwrap();
    let given: serde_json::Value = serde_json::from_str(json).map_err(|_| rustler::Error::BadArg)?;
    if let (Some(b), Some(g)) = (base.as_object_mut(), given.as_object()) {
        for (k, v) in g {
            b.insert(k.clone(), v.clone());
        }
    }
    serde_json::from_value(base).map_err(|_| rustler::Error::BadArg)
}

fn json_value(text: &str) -> NifResult<serde_json::Value> {
    serde_json::from_str(text).map_err(|_| rustler::Error::BadArg)
}

// ---- Events and text -------------------------------------------------------

#[rustler::nif]
fn parse_event<'a>(env: Env<'a>, name: &str, data_json: &str) -> NifResult<Term<'a>> {
    // Kick's data is usually a JSON string inside the JSON; the caller may
    // hand over either that string's content or the outer value.
    let data = serde_json::from_str(data_json).unwrap_or_else(|_| serde_json::Value::String(data_json.to_owned()));
    term(env, &events::parse(name, &data))
}

#[rustler::nif]
fn parse_content<'a>(env: Env<'a>, text: &str, combine: bool) -> NifResult<Term<'a>> {
    term(env, &content::parse(text, combine))
}

#[rustler::nif]
fn emote_url(id: &str) -> String {
    content::emote_url(id)
}

// ---- Realtime ----------------------------------------------------------------

#[rustler::nif]
fn chat_channels(chatroom: u64, channel: Option<u64>) -> Vec<String> {
    realtime::chat_channels(ChatroomId(chatroom), channel.map(ChannelId))
}

#[rustler::nif]
fn chat_connection_request<'a>(env: Env<'a>, e: &str, channel: u64, client_id: &str) -> NifResult<Term<'a>> {
    term(env, &realtime::chat_connection_request(&endpoints(e)?, ChannelId(channel), client_id))
}

#[rustler::nif]
fn connection_request<'a>(env: Env<'a>, e: &str, client_id: &str) -> NifResult<Term<'a>> {
    term(env, &realtime::connection_request(&endpoints(e)?, client_id))
}

#[rustler::nif]
fn parse_connection<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, realtime::parse_connection(status, body))
}

#[rustler::nif]
fn token_request<'a>(env: Env<'a>, e: &str, client_id: &str) -> NifResult<Term<'a>> {
    term(env, &realtime::token_request(&endpoints(e)?, client_id))
}

#[rustler::nif]
fn parse_token<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, realtime::parse_token(status, body))
}

struct SessionResource(Mutex<realtime::Session>);

#[rustler::resource_impl]
impl Resource for SessionResource {}

fn with<'a, F>(env: Env<'a>, s: &ResourceArc<SessionResource>, f: F) -> NifResult<Term<'a>>
where
    F: FnOnce(&mut realtime::Session) -> Vec<realtime::Action>,
{
    let mut guard = s.0.lock().map_err(|_| rustler::Error::Atom("poisoned"))?;
    term(env, &f(&mut guard))
}

#[rustler::nif]
fn session_new(channels: Vec<String>) -> ResourceArc<SessionResource> {
    ResourceArc::new(SessionResource(Mutex::new(realtime::Session::new(realtime::SessionOptions::new(channels)))))
}

#[rustler::nif]
fn session_open<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, token: &str, now: u64) -> NifResult<Term<'a>> {
    with(env, &s, |x| x.open(token, now))
}

#[rustler::nif]
fn session_receive<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, text: &str, now: u64) -> NifResult<Term<'a>> {
    with(env, &s, |x| x.receive(text, now))
}

#[rustler::nif]
fn session_refresh<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, token: &str, now: u64) -> NifResult<Term<'a>> {
    with(env, &s, |x| x.refresh(token, now))
}

#[rustler::nif]
fn session_tick<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, now: u64) -> NifResult<Term<'a>> {
    with(env, &s, |x| x.tick(now))
}

#[rustler::nif]
fn session_closed<'a>(env: Env<'a>, s: ResourceArc<SessionResource>) -> NifResult<Term<'a>> {
    with(env, &s, |x| x.closed())
}

#[rustler::nif]
fn session_deadline(s: ResourceArc<SessionResource>) -> Option<u64> {
    s.0.lock().ok().and_then(|x| x.deadline())
}

#[rustler::nif]
fn session_subscribed(s: ResourceArc<SessionResource>, channel: &str) -> bool {
    s.0.lock().map(|x| x.is_subscribed(channel)).unwrap_or(false)
}

#[rustler::nif]
fn session_connected(s: ResourceArc<SessionResource>) -> bool {
    s.0.lock().map(|x| x.is_connected()).unwrap_or(false)
}

// ---- Official API ------------------------------------------------------------

#[rustler::nif]
fn app_token_request<'a>(env: Env<'a>, e: &str, client_id: &str, secret: &str) -> NifResult<Term<'a>> {
    term(env, &public_api::token_request(&endpoints(e)?, client_id, secret))
}

#[rustler::nif]
fn parse_app_token<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, public_api::parse_token(status, body))
}

#[rustler::nif]
fn livestreams_requests<'a>(env: Env<'a>, e: &str, token: &str, ids: Vec<u64>) -> NifResult<Term<'a>> {
    let ids: Vec<UserId> = ids.into_iter().map(UserId).collect();
    term(env, &public_api::livestreams_requests(&endpoints(e)?, token, &ids))
}

#[rustler::nif]
fn parse_livestreams<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, public_api::parse_livestreams(status, body))
}

#[rustler::nif]
fn channels_by_ids_request<'a>(env: Env<'a>, e: &str, token: &str, ids: Vec<u64>) -> NifResult<Term<'a>> {
    let ids: Vec<UserId> = ids.into_iter().map(UserId).collect();
    result(env, public_api::channels_by_ids_request(&endpoints(e)?, token, &ids))
}

#[rustler::nif]
fn channels_by_slugs_request<'a>(env: Env<'a>, e: &str, token: &str, slugs: Vec<String>) -> NifResult<Term<'a>> {
    let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
    result(env, public_api::channels_by_slugs_request(&endpoints(e)?, token, &refs))
}

#[rustler::nif]
fn parse_channels<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, public_api::parse_channels(status, body))
}

#[rustler::nif]
fn public_key_request<'a>(env: Env<'a>, e: &str) -> NifResult<Term<'a>> {
    term(env, &public_api::public_key_request(&endpoints(e)?))
}

#[rustler::nif]
fn parse_public_key<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, public_api::parse_public_key(status, body))
}

#[rustler::nif]
fn subscribe_request<'a>(
    env: Env<'a>,
    e: &str,
    token: &str,
    broadcaster: u64,
    events: Vec<(String, u32)>,
) -> NifResult<Term<'a>> {
    let refs: Vec<(&str, u32)> = events.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    result(env, public_api::subscribe_request(&endpoints(e)?, token, UserId(broadcaster), &refs))
}

#[rustler::nif]
fn subscriptions_request<'a>(env: Env<'a>, e: &str, token: &str) -> NifResult<Term<'a>> {
    term(env, &public_api::subscriptions_request(&endpoints(e)?, token))
}

#[rustler::nif]
fn parse_subscriptions<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, public_api::parse_subscriptions(status, body))
}

#[rustler::nif]
fn unsubscribe_requests<'a>(env: Env<'a>, e: &str, token: &str, ids: Vec<String>) -> NifResult<Term<'a>> {
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    term(env, &public_api::unsubscribe_requests(&endpoints(e)?, token, &refs))
}

// ---- Website API -------------------------------------------------------------

#[rustler::nif]
fn channel_request<'a>(env: Env<'a>, e: &str, slug: &str) -> NifResult<Term<'a>> {
    term(env, &website::channel_request(&endpoints(e)?, slug))
}

#[rustler::nif]
fn parse_channel<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, website::parse_channel(status, body))
}

#[rustler::nif]
fn history_request<'a>(env: Env<'a>, e: &str, channel: u64) -> NifResult<Term<'a>> {
    term(env, &website::history_request(&endpoints(e)?, ChannelId(channel)))
}

#[rustler::nif]
fn parse_history<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, website::parse_history(status, body))
}

#[rustler::nif]
fn channel_user_request<'a>(env: Env<'a>, e: &str, slug: &str, username: &str) -> NifResult<Term<'a>> {
    term(env, &website::channel_user_request(&endpoints(e)?, slug, username))
}

#[rustler::nif]
fn parse_channel_user<'a>(env: Env<'a>, status: u16, body: &str) -> NifResult<Term<'a>> {
    result(env, website::parse_channel_user(status, body))
}

// ---- Webhooks ----------------------------------------------------------------

/// `{:ok, %{headers: …, event: …}}` for a delivery whose signature checks
/// out, `{:error, reason}` otherwise.
#[rustler::nif]
fn verify_webhook<'a>(env: Env<'a>, pem: &str, headers: Vec<(String, String)>, body: &str) -> NifResult<Term<'a>> {
    let refuse = |reason: String| Ok((atoms::error(), reason).encode(env));
    let h = match webhooks::Headers::from_pairs(headers.iter().map(|(a, b)| (a.as_str(), b.as_str()))) {
        Ok(h) => h,
        Err(e) => return refuse(e.to_string()),
    };
    if let Err(e) = webhooks::verify(pem, &h, body.as_bytes()) {
        return refuse(e.to_string());
    }
    let event = match webhooks::parse(&h.event_type, body) {
        Ok(event) => event,
        Err(e) => return refuse(e.to_string()),
    };
    #[derive(Serialize)]
    struct Verified<'b> {
        headers: &'b webhooks::Headers,
        event: webhooks::WebhookEvent,
    }
    Ok((atoms::ok(), term(env, &Verified { headers: &h, event })?).encode(env))
}

/// Reads a body without checking its signature (for a receiver that already
/// checked it, or a test).
#[rustler::nif]
fn parse_webhook<'a>(env: Env<'a>, event_type: &str, body: &str) -> NifResult<Term<'a>> {
    Ok(match webhooks::parse(event_type, body) {
        Ok(event) => (atoms::ok(), term(env, &event)?).encode(env),
        Err(e) => (atoms::error(), (atoms::json(), e.to_string())).encode(env),
    })
}

#[rustler::nif]
fn pin_request<'a>(
    env: Env<'a>,
    e: &str,
    session: &str,
    slug: &str,
    message_json: &str,
    minutes: u32,
) -> NifResult<Term<'a>> {
    result(env, website::pin_request(&endpoints(e)?, session, slug, &json_value(message_json)?, minutes))
}

rustler::init!("Elixir.Kicksdk.Native");
