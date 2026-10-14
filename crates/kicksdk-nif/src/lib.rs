//! kicksdk for Elixir: the core's functions as NIFs on `Kicksdk.Native`.
//!
//! The Elixir modules in `lib/` are the interface; this file only converts.
//! Values cross the way JSON decodes in Elixir: maps with **string keys**,
//! lists, binaries, numbers, booleans and `nil`. Not atoms: much of what comes
//! back is Kick's own data, and turning its keys into atoms would let Kick
//! grow the atom table without limit. JSON arguments (the endpoints, `call`'s
//! arguments) arrive already encoded by the Elixir side.
//!
//! Integers come back exact up to 64 bits (`i64` and `u64`). The core reads
//! JSON without arbitrary precision, so a bigger integer in Kick's data (Kick
//! sends none) is already a float when it gets here, and comes back as one.
//!
//! Text arrives as binaries, whatever they hold. What can fail on the data
//! returns `{:ok, value}` or `{:error, reason}`, and text that is not UTF-8
//! or a status out of range is one more such failure. What returns a plain
//! value raises instead: `ArgumentError` for an argument it cannot use,
//! `RuntimeError` for a session that a panic left unusable.
//!
//! Scheduling: a NIF should hand its scheduler back within about a
//! millisecond. None here waits on anything, but the ones that read input
//! take time in proportion to it: about 20 µs per KiB of JSON turned into
//! terms and 100-150 µs per KiB of message text split into parts, so a 4 MB
//! answer holds a scheduler for over 100 ms (measured on the release
//! build). Each of those has a `_dirty` twin, the same function on a dirty
//! CPU scheduler, which the Elixir side picks for an input over 4 KiB
//! (`Kicksdk.Native.dirty?/1`). Below that the normal scheduler is used:
//! switching to a dirty one costs about 8 µs, more than the work on a
//! typical chat frame. With one scheduler (`+S 1`), a 1 ms timer was held
//! up 109 ms by a 3.8 MB `/livestreams` answer on the normal scheduler and
//! 3 ms on the dirty one.

// `|s, b| website::parse_history(s, b)` rather than the bare path: the API
// table (scripts/docs/api_table.py) finds the core function each NIF calls
// by `module::function(`.
#![allow(clippy::redundant_closure)]

use kicksdk::http::Endpoints;
use kicksdk::ids::{ChannelId, ChatroomId, UserId};
use kicksdk::{content, events, public_api, realtime, webhooks, website};
use rustler::types::atom::Atom;
use rustler::{Binary, Encoder, Env, NifResult, Resource, ResourceArc, Term};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Mutex, MutexGuard, PoisonError};

mod atoms {
    rustler::atoms! {
        ok, error, status, shape, json, invalid, call, webhook, skip,
        struct_ = "__struct__",
        exception = "__exception__",
        message,
        argument_error = "Elixir.ArgumentError",
        runtime_error = "Elixir.RuntimeError",
    }
}

// ---- Conversions ---------------------------------------------------------------

/// An Elixir exception, raised from a NIF: `%ArgumentError{message: …}`.
struct Exception {
    module: Atom,
    message: String,
}

impl Encoder for Exception {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = rustler::types::map::map_new(env);
        map.map_put(atoms::struct_(), self.module)
            .and_then(|m| m.map_put(atoms::exception(), true))
            .and_then(|m| m.map_put(atoms::message(), self.message.as_str()))
            .unwrap_or(map)
    }
}

fn argument_error(message: impl Into<String>) -> rustler::Error {
    rustler::Error::RaiseTerm(Box::new(Exception { module: atoms::argument_error(), message: message.into() }))
}

fn runtime_error(message: impl Into<String>) -> rustler::Error {
    rustler::Error::RaiseTerm(Box::new(Exception { module: atoms::runtime_error(), message: message.into() }))
}

/// Any value as terms. It goes through `serde_json::Value`, which cannot
/// fail for the core's types; if it ever did, that is raised, never returned
/// as an `{:error, _}` where the caller expects a list or a map.
fn term<'a, T: Serialize + ?Sized>(env: Env<'a>, v: &T) -> NifResult<Term<'a>> {
    let value = serde_json::to_value(v).map_err(|e| runtime_error(format!("kicksdk: cannot convert a value: {e}")))?;
    Ok(encode(env, &value))
}

fn encode<'a>(env: Env<'a>, v: &Value) -> Term<'a> {
    match v {
        Value::Null => rustler::types::atom::nil().encode(env),
        Value::Bool(b) => b.encode(env),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.encode(env),
            (None, Some(u)) => u.encode(env),
            // A float, or an integer beyond 64 bits, which serde_json has
            // already read as a float (see the module docs).
            _ => n.as_f64().unwrap_or(0.0).encode(env),
        },
        Value::String(s) => s.encode(env),
        Value::Array(list) => list.iter().map(|x| encode(env, x)).collect::<Vec<_>>().encode(env),
        Value::Object(map) => {
            let keys: Vec<Term<'a>> = map.keys().map(|k| k.encode(env)).collect();
            let values: Vec<Term<'a>> = map.values().map(|x| encode(env, x)).collect();
            // The keys of a JSON object are distinct, so this cannot fail.
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

/// Text from a binary, refused (`{:invalid, _}`) when it is not UTF-8.
fn utf8<'b>(b: &'b [u8], what: &str) -> Result<&'b str, kicksdk::Error> {
    std::str::from_utf8(b).map_err(|_| kicksdk::Error::Invalid(format!("{what} is not UTF-8")))
}

/// Text for a function that returns a plain value: raised when not UTF-8.
fn text<'b>(b: &'b [u8], what: &str) -> NifResult<&'b str> {
    utf8(b, what).map_err(|e| argument_error(e.to_string()))
}

fn texts(list: &[Binary], what: &str) -> Result<Vec<String>, kicksdk::Error> {
    list.iter().map(|b| utf8(b.as_slice(), what).map(str::to_owned)).collect()
}

/// An HTTP status as the core takes it: an integer from 0 to 65535, never
/// wrapped.
fn http_status(t: Term) -> Result<u16, kicksdk::Error> {
    t.decode::<i64>()
        .ok()
        .and_then(|s| u16::try_from(s).ok())
        .ok_or_else(|| kicksdk::Error::Invalid("the status must be an integer from 0 to 65535".into()))
}

/// A parser over `(status, body)`. A body that is not UTF-8 is not JSON
/// either, so it fails as `{:json, _}` - unless Kick answered an error
/// status, which says more and keeps its excerpt.
fn parsed<'a, T: Serialize>(
    env: Env<'a>,
    status: Term<'a>,
    body: &[u8],
    f: impl FnOnce(u16, &str) -> Result<T, kicksdk::Error>,
) -> NifResult<Term<'a>> {
    let status = match http_status(status) {
        Ok(s) => s,
        Err(e) => return result::<()>(env, Err(e)),
    };
    match std::str::from_utf8(body) {
        Ok(text) => result(env, f(status, text)),
        Err(_) => match f(status, &String::from_utf8_lossy(body)) {
            Err(e @ kicksdk::Error::Status { .. }) => result::<()>(env, Err(e)),
            _ => Ok((atoms::error(), (atoms::json(), "the body is not UTF-8")).encode(env)),
        },
    }
}

/// The endpoints override: a JSON object with any of `api`, `id`, `website`,
/// `realtime` as URL strings (null for the default). Anything else is
/// raised, an unknown key even with a null value (as `dispatch` does): a
/// typo must not quietly send the request to kick.com.
fn endpoints(json: &[u8]) -> NifResult<Endpoints> {
    let mut e = Endpoints::default();
    if json.is_empty() {
        return Ok(e);
    }
    let given: serde_json::Map<String, Value> =
        serde_json::from_slice(json).map_err(|_| argument_error("endpoints must be a map of URL strings"))?;
    for (key, value) in given {
        let set: fn(Endpoints, String) -> Endpoints = match key.as_str() {
            "api" => Endpoints::with_api,
            "id" => Endpoints::with_id,
            "website" => Endpoints::with_website,
            "realtime" => Endpoints::with_realtime,
            other => {
                return Err(argument_error(format!(
                    "endpoints: unknown key `{other}` (expected api, id, website, realtime)"
                )));
            }
        };
        e = match value {
            Value::Null => e,
            Value::String(url) => set(e, url),
            _ => return Err(argument_error(format!("endpoints: `{key}` must be a URL string"))),
        };
    }
    Ok(e)
}

// ---- Events and text -------------------------------------------------------

#[rustler::nif]
fn parse_event<'a>(env: Env<'a>, name: Binary<'a>, data: Binary<'a>) -> NifResult<Term<'a>> {
    event(env, &name, &data)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_event_dirty<'a>(env: Env<'a>, name: Binary<'a>, data: Binary<'a>) -> NifResult<Term<'a>> {
    event(env, &name, &data)
}

fn event<'a>(env: Env<'a>, name: &[u8], data: &[u8]) -> NifResult<Term<'a>> {
    // Kick's data is usually a JSON string inside the JSON; the caller may
    // hand over either that string's content or the outer value. Text that
    // is not JSON (or not UTF-8) is read as text.
    let data = serde_json::from_slice(data).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(data).into()));
    term(env, &events::parse(text(name, "the event name")?, &data))
}

#[rustler::nif]
fn parse_content<'a>(env: Env<'a>, message: Binary<'a>, combine: bool) -> NifResult<Term<'a>> {
    term(env, &content::parse(text(&message, "the message")?, combine))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_content_dirty<'a>(env: Env<'a>, message: Binary<'a>, combine: bool) -> NifResult<Term<'a>> {
    term(env, &content::parse(text(&message, "the message")?, combine))
}

#[rustler::nif]
fn emote_url<'a>(env: Env<'a>, id: Binary<'a>) -> NifResult<Term<'a>> {
    result(env, utf8(&id, "the emote id").and_then(|id| content::emote_url(id)))
}

// ---- Realtime ----------------------------------------------------------------

#[rustler::nif]
fn chat_channels(chatroom: u64, channel: Option<u64>) -> Vec<String> {
    realtime::chat_channels(ChatroomId(chatroom), channel.map(ChannelId))
}

#[rustler::nif]
fn chat_connection_request<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    channel: u64,
    client_id: Binary<'a>,
) -> NifResult<Term<'a>> {
    let client_id = text(&client_id, "the client id")?;
    term(env, &realtime::chat_connection_request(&endpoints(&e)?, ChannelId(channel), client_id))
}

#[rustler::nif]
fn connection_request<'a>(env: Env<'a>, e: Binary<'a>, client_id: Binary<'a>) -> NifResult<Term<'a>> {
    term(env, &realtime::connection_request(&endpoints(&e)?, text(&client_id, "the client id")?))
}

#[rustler::nif]
fn parse_connection<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| realtime::parse_connection(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_connection_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| realtime::parse_connection(s, b))
}

#[rustler::nif]
fn token_request<'a>(env: Env<'a>, e: Binary<'a>, client_id: Binary<'a>) -> NifResult<Term<'a>> {
    term(env, &realtime::token_request(&endpoints(&e)?, text(&client_id, "the client id")?))
}

#[rustler::nif]
fn parse_token<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| realtime::parse_token(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_token_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| realtime::parse_token(s, b))
}

#[rustler::nif]
fn retry_after_ms(code: u16) -> u64 {
    realtime::retry_after_ms(code)
}

/// `realtime::Backoff` as four integers, `{initial_ms, max_ms, next_ms,
/// terminal_closes}` (its serde fields), so that Elixir keeps it as an
/// immutable struct. Every call takes all four and returns all four: a field
/// left out would be lost between calls (the terminal count, and with it the
/// escalation from 5 to 30 minutes).
type BackoffFields = (u64, u64, u64, u32);

fn backoff(fields: BackoffFields) -> NifResult<realtime::Backoff> {
    let (initial_ms, max_ms, next_ms, terminal_closes) = fields;
    serde_json::from_value(serde_json::json!({
        "initial_ms": initial_ms,
        "max_ms": max_ms,
        "next_ms": next_ms,
        "terminal_closes": terminal_closes,
    }))
    .map_err(|e| runtime_error(format!("kicksdk: cannot rebuild a backoff: {e}")))
}

fn fields(b: &realtime::Backoff) -> NifResult<BackoffFields> {
    let v = serde_json::to_value(b).map_err(|e| runtime_error(format!("kicksdk: cannot read a backoff: {e}")))?;
    let field = |k: &str| {
        v.get(k).and_then(Value::as_u64).ok_or_else(|| runtime_error(format!("kicksdk: no {k} in a backoff")))
    };
    let terminal_closes = u32::try_from(field("terminal_closes")?)
        .map_err(|_| runtime_error("kicksdk: terminal_closes out of range in a backoff"))?;
    Ok((field("initial_ms")?, field("max_ms")?, field("next_ms")?, terminal_closes))
}

#[rustler::nif]
fn backoff_new(initial_ms: u64, max_ms: u64) -> NifResult<BackoffFields> {
    fields(&realtime::Backoff::new(initial_ms, max_ms))
}

/// `{delay_ms, backoff}`; `random`, from `[0, 1)`, spreads the delay.
#[rustler::nif]
fn backoff_next(b: BackoffFields, random: Option<f64>) -> NifResult<(u64, BackoffFields)> {
    let mut b = backoff(b)?;
    let delay = match random {
        Some(r) => b.next_delay_jittered(r),
        None => b.next_delay(),
    };
    Ok((delay, fields(&b)?))
}

/// `{delay_ms, backoff}` after a close that asks for at least
/// `retry_after_ms` (0 for an ordinary drop).
#[rustler::nif]
fn backoff_next_for(b: BackoffFields, retry_after_ms: u64, random: f64) -> NifResult<(u64, BackoffFields)> {
    let mut b = backoff(b)?;
    let delay = b.next_delay_for(retry_after_ms, random);
    Ok((delay, fields(&b)?))
}

#[rustler::nif]
fn backoff_reset(b: BackoffFields) -> NifResult<BackoffFields> {
    let mut b = backoff(b)?;
    b.reset();
    fields(&b)
}

/// A session and the clock it is driven by.
///
/// The core counts time in unsigned milliseconds; a host's clock need not
/// (`System.monotonic_time/1` is usually negative). The first `now` a
/// session is given becomes its zero, every `now` after it reaches the core
/// relative to that, and `deadline/1` adds it back.
struct SessionState {
    session: realtime::Session,
    zero: Option<i64>,
}

impl SessionState {
    /// The host's `now` on the core's clock (an earlier time counts as 0).
    fn at(&mut self, now: i64) -> u64 {
        let zero = *self.zero.get_or_insert(now);
        u64::try_from(i128::from(now) - i128::from(zero)).unwrap_or(0)
    }

    /// The core's time on the host's clock.
    fn host(&self, t: u64) -> i64 {
        i64::try_from(i128::from(self.zero.unwrap_or(0)) + i128::from(t)).unwrap_or(i64::MAX)
    }
}

struct SessionResource(Mutex<SessionState>);

#[rustler::resource_impl]
impl Resource for SessionResource {}

const POISONED: &str = "kicksdk: this realtime session panicked in an earlier call and its state cannot be \
                        trusted; call Kicksdk.Realtime.Session.closed/1 to reset it, or start a new one";

/// The session, unless a panic in an earlier call poisoned it: its state may
/// be half-changed then, so it is refused until `closed/1` resets it.
fn lock(m: &Mutex<SessionState>) -> NifResult<MutexGuard<'_, SessionState>> {
    m.lock().map_err(|_| runtime_error(POISONED))
}

/// The session even if poisoned, for `closed/1`, which resets every part of
/// its state that a connection uses; the lock is usable again afterwards.
fn lock_to_reset(m: &Mutex<SessionState>) -> MutexGuard<'_, SessionState> {
    let guard = m.lock().unwrap_or_else(PoisonError::into_inner);
    m.clear_poison();
    guard
}

/// Runs `f` on the session with `now` on the core's clock.
fn with<'a, F>(env: Env<'a>, s: &ResourceArc<SessionResource>, now: i64, f: F) -> NifResult<Term<'a>>
where
    F: FnOnce(&mut realtime::Session, u64) -> Vec<realtime::Action>,
{
    let mut state = lock(&s.0)?;
    let now = state.at(now);
    term(env, &f(&mut state.session, now))
}

#[rustler::nif]
fn session_new(channels: Vec<String>) -> ResourceArc<SessionResource> {
    let session = realtime::Session::new(realtime::SessionOptions::new(channels));
    ResourceArc::new(SessionResource(Mutex::new(SessionState { session, zero: None })))
}

#[rustler::nif]
fn session_open<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, token: Binary<'a>, now: i64) -> NifResult<Term<'a>> {
    let token = text(&token, "the token")?;
    with(env, &s, now, |x, now| x.open(token, now))
}

#[rustler::nif]
fn session_receive<'a>(
    env: Env<'a>,
    s: ResourceArc<SessionResource>,
    frame: Binary<'a>,
    now: i64,
) -> NifResult<Term<'a>> {
    receive(env, &s, &frame, now)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn session_receive_dirty<'a>(
    env: Env<'a>,
    s: ResourceArc<SessionResource>,
    frame: Binary<'a>,
    now: i64,
) -> NifResult<Term<'a>> {
    receive(env, &s, &frame, now)
}

/// A frame is lines of JSON. A line that is not UTF-8 is skipped like one
/// that is not JSON (the frame still counts as traffic).
fn receive<'a>(env: Env<'a>, s: &ResourceArc<SessionResource>, frame: &[u8], now: i64) -> NifResult<Term<'a>> {
    let owned;
    let frame = match std::str::from_utf8(frame) {
        Ok(f) => f,
        Err(_) => {
            owned =
                frame.split(|b| *b == b'\n').filter_map(|l| std::str::from_utf8(l).ok()).collect::<Vec<_>>().join("\n");
            &owned
        }
    };
    with(env, s, now, |x, now| x.receive(frame, now))
}

#[rustler::nif]
fn session_refresh<'a>(
    env: Env<'a>,
    s: ResourceArc<SessionResource>,
    token: Binary<'a>,
    now: i64,
) -> NifResult<Term<'a>> {
    let token = text(&token, "the token")?;
    with(env, &s, now, |x, now| x.refresh(token, now))
}

#[rustler::nif]
fn session_tick<'a>(env: Env<'a>, s: ResourceArc<SessionResource>, now: i64) -> NifResult<Term<'a>> {
    with(env, &s, now, |x, now| x.tick(now))
}

#[rustler::nif]
fn session_closed<'a>(env: Env<'a>, s: ResourceArc<SessionResource>) -> NifResult<Term<'a>> {
    term(env, &lock_to_reset(&s.0).session.closed())
}

/// On the host's clock, the one `now` is given in.
#[rustler::nif]
fn session_deadline(s: ResourceArc<SessionResource>) -> NifResult<Option<i64>> {
    let state = lock(&s.0)?;
    Ok(state.session.deadline().map(|t| state.host(t)))
}

#[rustler::nif]
fn session_subscribed(s: ResourceArc<SessionResource>, channel: Binary) -> NifResult<bool> {
    let channel = text(&channel, "the channel")?;
    Ok(lock(&s.0)?.session.is_subscribed(channel))
}

#[rustler::nif]
fn session_connected(s: ResourceArc<SessionResource>) -> NifResult<bool> {
    Ok(lock(&s.0)?.session.is_connected())
}

// ---- Official API ------------------------------------------------------------

#[rustler::nif]
fn app_token_request<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    client_id: Binary<'a>,
    secret: Binary<'a>,
) -> NifResult<Term<'a>> {
    let (client_id, secret) = (text(&client_id, "the client id")?, text(&secret, "the client secret")?);
    term(env, &public_api::token_request(&endpoints(&e)?, client_id, secret))
}

#[rustler::nif]
fn parse_app_token<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_token(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_app_token_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_token(s, b))
}

#[rustler::nif]
fn livestreams_requests<'a>(env: Env<'a>, e: Binary<'a>, token: Binary<'a>, ids: Vec<u64>) -> NifResult<Term<'a>> {
    let ids: Vec<UserId> = ids.into_iter().map(UserId).collect();
    let e = endpoints(&e)?;
    result(env, utf8(&token, "the token").and_then(|t| public_api::livestreams_requests(&e, t, &ids)))
}

#[rustler::nif]
fn parse_livestreams<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_livestreams(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_livestreams_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_livestreams(s, b))
}

#[rustler::nif]
fn channels_by_ids_request<'a>(env: Env<'a>, e: Binary<'a>, token: Binary<'a>, ids: Vec<u64>) -> NifResult<Term<'a>> {
    let ids: Vec<UserId> = ids.into_iter().map(UserId).collect();
    let e = endpoints(&e)?;
    result(env, utf8(&token, "the token").and_then(|t| public_api::channels_by_ids_request(&e, t, &ids)))
}

#[rustler::nif]
fn channels_by_slugs_request<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    token: Binary<'a>,
    slugs: Vec<Binary<'a>>,
) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    let r = utf8(&token, "the token").and_then(|t| {
        let slugs = texts(&slugs, "a slug")?;
        let refs: Vec<&str> = slugs.iter().map(String::as_str).collect();
        public_api::channels_by_slugs_request(&e, t, &refs)
    });
    result(env, r)
}

#[rustler::nif]
fn parse_channels<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_channels(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_channels_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_channels(s, b))
}

#[rustler::nif]
fn public_key_request<'a>(env: Env<'a>, e: Binary<'a>) -> NifResult<Term<'a>> {
    term(env, &public_api::public_key_request(&endpoints(&e)?))
}

#[rustler::nif]
fn parse_public_key<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_public_key(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_public_key_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_public_key(s, b))
}

#[rustler::nif]
fn subscribe_request<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    token: Binary<'a>,
    broadcaster: u64,
    events: Vec<(Binary<'a>, u32)>,
) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    let r = utf8(&token, "the token").and_then(|t| {
        let names = events.iter().map(|(n, _)| utf8(n, "an event name")).collect::<Result<Vec<_>, _>>()?;
        let refs: Vec<(&str, u32)> = names.into_iter().zip(events.iter().map(|(_, v)| *v)).collect();
        public_api::subscribe_request(&e, t, UserId(broadcaster), &refs)
    });
    result(env, r)
}

#[rustler::nif]
fn subscriptions_request<'a>(env: Env<'a>, e: Binary<'a>, token: Binary<'a>) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    result(env, utf8(&token, "the token").and_then(|t| public_api::subscriptions_request(&e, t)))
}

#[rustler::nif]
fn parse_subscriptions<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_subscriptions(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_subscriptions_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| public_api::parse_subscriptions(s, b))
}

#[rustler::nif]
fn unsubscribe_requests<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    token: Binary<'a>,
    ids: Vec<Binary<'a>>,
) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    let r = utf8(&token, "the token").and_then(|t| {
        let ids = texts(&ids, "a subscription id")?;
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        public_api::unsubscribe_requests(&e, t, &refs)
    });
    result(env, r)
}

// ---- Website API -------------------------------------------------------------

#[rustler::nif]
fn channel_request<'a>(env: Env<'a>, e: Binary<'a>, slug: Binary<'a>) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    result(env, utf8(&slug, "the slug").and_then(|s| website::channel_request(&e, s)))
}

#[rustler::nif]
fn parse_channel<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_channel(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_channel_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_channel(s, b))
}

#[rustler::nif]
fn history_request<'a>(env: Env<'a>, e: Binary<'a>, channel: u64) -> NifResult<Term<'a>> {
    term(env, &website::history_request(&endpoints(&e)?, ChannelId(channel)))
}

#[rustler::nif]
fn parse_history<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_history(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_history_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_history(s, b))
}

#[rustler::nif]
fn channel_user_request<'a>(
    env: Env<'a>,
    e: Binary<'a>,
    slug: Binary<'a>,
    username: Binary<'a>,
) -> NifResult<Term<'a>> {
    let e = endpoints(&e)?;
    let r = utf8(&slug, "the slug")
        .and_then(|s| Ok((s, utf8(&username, "the username")?)))
        .and_then(|(s, u)| website::channel_user_request(&e, s, u));
    result(env, r)
}

#[rustler::nif]
fn parse_channel_user<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_channel_user(s, b))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_channel_user_dirty<'a>(env: Env<'a>, status: Term<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    parsed(env, status, &body, |s, b| website::parse_channel_user(s, b))
}

// ---- Webhooks ----------------------------------------------------------------

/// Kick's webhook key, read once (`Kicksdk.Webhooks.public_key/1`).
struct KeyResource(webhooks::PublicKey);

#[rustler::resource_impl]
impl Resource for KeyResource {}

fn webhook_error(env: Env, message: impl Encoder) -> NifResult<Term> {
    Ok((atoms::error(), (atoms::webhook(), message)).encode(env))
}

fn read_key(pem: &[u8]) -> Result<webhooks::PublicKey, webhooks::WebhookError> {
    std::str::from_utf8(pem).map_err(|_| webhooks::WebhookError::BadKey).and_then(webhooks::PublicKey::from_pem)
}

#[rustler::nif]
fn webhook_public_key<'a>(env: Env<'a>, pem: Binary<'a>) -> NifResult<Term<'a>> {
    match read_key(&pem) {
        Ok(key) => Ok((atoms::ok(), ResourceArc::new(KeyResource(key))).encode(env)),
        Err(e) => webhook_error(env, e.to_string()),
    }
}

/// `{:ok, %{"headers" => …, "event" => …}}` for a delivery whose signature
/// checks out, `{:error, {:webhook, message}}` for one that does not, and
/// the parser's `{:error, reason}` for a signed body that is not the shape
/// of its type. `key` is a key resource or a PEM, `headers` `[{name,
/// value}]`, `body` the raw bytes, `freshness` `{now_unix_secs,
/// tolerance_secs}` or `:skip`.
#[rustler::nif]
fn verify_webhook<'a>(
    env: Env<'a>,
    key: Term<'a>,
    headers: Vec<(Binary<'a>, Binary<'a>)>,
    body: Binary<'a>,
    freshness: Term<'a>,
) -> NifResult<Term<'a>> {
    verify(env, key, &headers, &body, freshness)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn verify_webhook_dirty<'a>(
    env: Env<'a>,
    key: Term<'a>,
    headers: Vec<(Binary<'a>, Binary<'a>)>,
    body: Binary<'a>,
    freshness: Term<'a>,
) -> NifResult<Term<'a>> {
    verify(env, key, &headers, &body, freshness)
}

fn verify<'a>(
    env: Env<'a>,
    key: Term<'a>,
    headers: &[(Binary, Binary)],
    body: &[u8],
    freshness: Term<'a>,
) -> NifResult<Term<'a>> {
    let freshness = if freshness.decode::<Atom>().is_ok_and(|a| a == atoms::skip()) {
        webhooks::Freshness::Skip
    } else {
        let (now, tolerance_secs) = freshness
            .decode::<(u64, u64)>()
            .map_err(|_| argument_error("freshness must be {now_unix_secs, tolerance_secs} or :skip"))?;
        webhooks::Freshness::Within { now, tolerance_secs }
    };
    // Kick's headers must be text; any other header is not read, whatever it
    // holds.
    let mut pairs = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        let Ok(name) = std::str::from_utf8(name) else { continue };
        let kicks = name.get(..11).is_some_and(|p| p.eq_ignore_ascii_case("kick-event-"));
        match std::str::from_utf8(value) {
            Ok(value) => pairs.push((name, value)),
            Err(_) if kicks => return webhook_error(env, format!("header {name} is not UTF-8")),
            Err(_) => {}
        }
    }
    let h = match webhooks::Headers::from_pairs(pairs) {
        Ok(h) => h,
        Err(e) => return webhook_error(env, e.to_string()),
    };
    let checked = match key.decode::<ResourceArc<KeyResource>>() {
        Ok(key) => webhooks::verify(&key.0, &h, body, freshness),
        Err(_) => {
            let pem = key.decode::<Binary>().map_err(|_| argument_error("the key must be a public key or a PEM"))?;
            read_key(&pem).and_then(|key| webhooks::verify(&key, &h, body, freshness))
        }
    };
    if let Err(e) = checked {
        return webhook_error(env, e.to_string());
    }
    let event = match webhooks::parse(&h.event_type, body) {
        Ok(event) => event,
        Err(e) => return result::<()>(env, Err(e)),
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
fn parse_webhook<'a>(env: Env<'a>, event_type: Binary<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    result(env, utf8(&event_type, "the event type").and_then(|t| webhooks::parse(t, body.as_slice())))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn parse_webhook_dirty<'a>(env: Env<'a>, event_type: Binary<'a>, body: Binary<'a>) -> NifResult<Term<'a>> {
    result(env, utf8(&event_type, "the event type").and_then(|t| webhooks::parse(t, body.as_slice())))
}

// ---- Any function by name ------------------------------------------------------

/// Any core function by name (`kicksdk::dispatch`), arguments as JSON:
/// `{:ok, value}` or `{:error, reason}` shaped as everywhere else here, plus
/// `{:call, message}` for a call that does not fit and `{:webhook, message}`
/// for a delivery that does not verify (one that verifies but is not its
/// type's shape is `{:shape, _}`, the dispatch error's kind, like any other).
#[rustler::nif]
fn call<'a>(env: Env<'a>, name: Binary<'a>, args_json: Binary<'a>) -> NifResult<Term<'a>> {
    dispatch(env, &name, &args_json)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn call_dirty<'a>(env: Env<'a>, name: Binary<'a>, args_json: Binary<'a>) -> NifResult<Term<'a>> {
    dispatch(env, &name, &args_json)
}

fn dispatch<'a>(env: Env<'a>, name: &[u8], args_json: &[u8]) -> NifResult<Term<'a>> {
    let refuse = |message: &str| Ok((atoms::error(), (atoms::call(), message)).encode(env));
    let Ok(name) = std::str::from_utf8(name) else { return refuse("the function name is not UTF-8") };
    let args = if args_json.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(args_json) {
            Ok(v) => v,
            Err(_) => return refuse("the arguments are not JSON"),
        }
    };
    Ok(match kicksdk::dispatch::call(name, &args) {
        Ok(v) => (atoms::ok(), encode(env, &v)).encode(env),
        Err(e) => {
            let reason = match (e.kind, e.status) {
                ("status", Some(status)) => (atoms::status(), status, e.message.as_str()).encode(env),
                (kind, _) => (Atom::from_str(env, kind)?, e.message.as_str()).encode(env),
            };
            (atoms::error(), reason).encode(env)
        }
    })
}

rustler::init!("Elixir.Kicksdk.Native");

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SessionState {
        SessionState { session: realtime::Session::new(realtime::SessionOptions::new(vec![])), zero: None }
    }

    #[test]
    fn a_negative_clock_is_counted_from_its_first_reading() {
        let start = -576_460_752_303_i64;
        let mut s = state();
        assert_eq!(s.at(start), 0);
        assert_eq!(s.at(start + 25_000), 25_000);
        // Earlier than the first reading: the core's zero, never a wrap.
        assert_eq!(s.at(start - 1), 0);
        assert_eq!(s.host(25_000), start + 25_000);
        let mut far = state();
        assert_eq!(far.at(i64::MIN), 0);
        assert_eq!(far.at(i64::MAX), u64::MAX);
        assert_eq!(far.host(u64::MAX), i64::MAX);
    }

    #[test]
    fn a_backoff_keeps_its_terminal_count_across_calls() {
        let mut b = backoff(fields(&realtime::Backoff::default()).unwrap()).unwrap();
        assert_eq!(b.next_delay_for(300_000, 0.0), 300_000);
        let carried = fields(&b).unwrap();
        assert_eq!(carried.3, 1);
        let mut back = backoff(carried).unwrap();
        assert_eq!(back.next_delay_for(300_000, 0.0), 600_000);
        back.reset();
        assert_eq!(fields(&back).unwrap(), (1_000, 30_000, 1_000, 0));
    }

    #[test]
    fn a_poisoned_session_is_refused_until_reset() {
        let m = std::sync::Arc::new(Mutex::new(state()));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _guard = m2.lock().unwrap();
            panic!("poison it");
        })
        .join();
        assert!(m.lock().is_err());
        let _ = lock_to_reset(&m).session.closed();
        assert!(m.lock().is_ok());
    }
}
