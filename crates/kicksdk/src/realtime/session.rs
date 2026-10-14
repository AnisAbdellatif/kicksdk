//! Kick's Centrifugo protocol as a state machine.
//!
//! A [`Session`] is one connection's worth of protocol. The host opens the
//! socket, then tells the session what happens - [`Session::open`] when it
//! opens, [`Session::receive`] for each text frame, [`Session::tick`] when the
//! clock reaches [`Session::deadline`], [`Session::closed`] when it closes -
//! and does what the returned [`Action`]s say: send a frame, close the socket,
//! fetch a new token, hand an event on. The session never reads a clock or
//! sets a timer; every call carries `now` in milliseconds.
//!
//! What it does on the wire, as Kick's server expects (JSON protocol):
//!
//! - `{"id":1,"connect":{"token":…,"name":"js"}}` on open; the reply carries
//!   the ping interval (25 s) and the token's lifetime (`ttl`, 2700 s).
//! - One `subscribe` per feed, sent together once connected.
//! - An empty frame `{}` from the server is a ping: answered with `{}`.
//! - Several replies can share one frame, a line each.
//! - A `refresh` with a new token before the old one runs out.
//! - A push with `disconnect`, or nothing at all for a ping interval and a
//!   grace, ends the connection. A connect reply without a ping interval
//!   means the server will not ping, and then silence is not a failure.
//! - A push with `unsub` and a code from 2500 to 2999 means "subscribe
//!   again": the session does so itself.
//!
//! The numbers Kick sends are bounded before use: a ping interval to 1 s -
//! 10 min, a token lifetime to 1 s - 1 day. Time arithmetic saturates, so no
//! `now` or reply can overflow it.
//!
//! **Signed URLs.** An event's data can carry a channel's `playback_url`, a
//! signed URL for its stream (`ChatMoveToSupportedChannelEvent` on
//! `channel.<id>` carries the hosting channel's). Nothing on the feed needs
//! it, so [`Action::Event`]'s `data` never has a `playback_url` key: it is
//! removed, at any depth, before the event is handed on. Only data that had
//! one is encoded again; any other string is handed on as Kick sent it,
//! byte for byte.

use crate::events::{self, KickEvent};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fmt;

/// While connecting, before Kick has said: its usual 25 s.
const DEFAULT_PING_MS: u64 = 25_000;
/// The ping intervals believed, seconds.
const PING_SECS: std::ops::RangeInclusive<u64> = 1..=600;
/// The token lifetimes believed, seconds.
const TTL_SECS: std::ops::RangeInclusive<u64> = 1..=86_400;

/// How a session behaves. [`SessionOptions::new`] has Kick's values; change a
/// field on the result for anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionOptions {
    /// The feeds to subscribe to, e.g. [`super::chat_channels`]. A feed
    /// listed twice is subscribed once.
    pub channels: Vec<String>,
    /// The client name sent on connect. Kick's page sends `js`.
    pub client_name: String,
    /// How long past a missed ping before the connection counts as dead, ms.
    pub silence_grace_ms: u64,
    /// How long before the token runs out to ask for a new one, ms.
    pub refresh_lead_ms: u64,
}

impl SessionOptions {
    /// Kick's values, for these feeds.
    pub fn new(channels: Vec<String>) -> Self {
        Self { channels, client_name: "js".into(), silence_grace_ms: 20_000, refresh_lead_ms: 120_000 }
    }
}

/// The least a host waits before reconnecting after a terminal close: 5
/// minutes. [`super::Backoff::next_delay_for`] lengthens it for terminal
/// closes in a row.
pub const TERMINAL_RETRY_MS: u64 = 300_000;

/// How long at least to wait before reconnecting after the socket closed
/// with this code, ms: [`TERMINAL_RETRY_MS`] for Centrifugo's terminal
/// codes, 3500-3999 and 4500-4999 (the server does not want this client
/// back as it is: a bad token, a ban, a refused request, too many
/// connections), and 0 - the ordinary backoff - for anything else:
/// 3000-3499, 4000-4499, the WebSocket codes below 3000 (a network drop is
/// 1006).
///
/// A terminal close is not the end: for a guest reading chat, a refused
/// token or a connection limit usually passes. Reconnect after
/// [`super::Backoff::next_delay_for`] of this, with a **new client id**
/// and so a new guest token. More than 0 means terminal.
///
/// For the close code the host's socket reports; [`Action::Close`] already
/// carries the answer in `terminal` and `retry_after_ms`.
pub fn retry_after_ms(code: u16) -> u64 {
    if matches!(code, 3500..=3999 | 4500..=4999) { TERMINAL_RETRY_MS } else { 0 }
}

/// What the host should do, or know, after a call.
///
/// `Debug` hides the token in a connect or refresh frame; `Serialize`, what
/// a binding hands on, does not.
// Actions live for one call and are handed straight on; boxing the event
// would only cost every host a pointer to chase.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    /// Send this text frame.
    Send {
        /// The frame.
        text: String,
    },
    /// Close the socket, then call [`Session::closed`]. If the connection
    /// is still wanted, reconnect after
    /// [`super::Backoff::next_delay_for`]`(retry_after_ms, random)`, with a
    /// new token; after a terminal close, for a new client id too.
    ///
    /// Serialized: `{"action":"close","code":3501,"reason":"…",
    /// "terminal":true,"retry_after_ms":300000}`.
    #[non_exhaustive]
    Close {
        /// Why: Kick's disconnect code (3000-4999, see
        /// [`retry_after_ms`]), Kick's error code when it refused the
        /// connect or a token refresh (101 unauthorized, 109 token
        /// expired, …), or 0 when the session gave up itself (silence).
        code: i64,
        /// Why, for a log line.
        reason: String,
        /// Kick does not want this client back as it is: a terminal
        /// disconnect code, or a refused connect or refresh that is not
        /// worth retrying at once. Reconnect all the same, after the long
        /// wait, with a new client id and token; worth a log line or a
        /// report.
        terminal: bool,
        /// The least to wait before reconnecting, ms: [`TERMINAL_RETRY_MS`]
        /// when `terminal`, else 0 (the ordinary backoff). Pass it to
        /// [`super::Backoff::next_delay_for`].
        retry_after_ms: u64,
    },
    /// Fetch a new token for the same client id ([`super::token_request`])
    /// and pass it to [`Session::refresh`]. If that fails, do nothing: Kick
    /// ends the connection when the old token runs out, and the reconnect
    /// gets a new one.
    RefreshToken,
    /// Kick accepted the connection.
    Connected {
        /// Kick's id for this connection.
        client: Option<String>,
    },
    /// A feed is subscribed.
    Subscribed {
        /// Which.
        channel: String,
    },
    /// A feed is not, or no longer, subscribed. Kick's codes 2500-2999 mean
    /// "subscribe again": the session has already sent the subscribe, and a
    /// [`Action::Subscribed`] or [`Action::SubscribeFailed`] follows.
    Unsubscribed {
        /// Which.
        channel: String,
        /// Kick's code; 0 when the socket closed.
        code: i64,
        /// Kick's reason.
        reason: String,
    },
    /// Kick refused a subscription (103 "permission denied" for a feed a
    /// guest may not read).
    SubscribeFailed {
        /// Which.
        channel: String,
        /// Kick's code.
        code: i64,
        /// Kick's message.
        message: String,
    },
    /// An event on a feed.
    Event {
        /// The feed it came on.
        channel: String,
        /// Kick's event name.
        name: String,
        /// Its data as Kick sent it (usually a JSON string), less any
        /// `playback_url` (see the module docs).
        data: Value,
        /// The same, typed.
        event: KickEvent,
    },
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Send { text } => f.debug_struct("Send").field("text", &redact_tokens(text)).finish(),
            Action::Close { code, reason, terminal, retry_after_ms } => f
                .debug_struct("Close")
                .field("code", code)
                .field("reason", reason)
                .field("terminal", terminal)
                .field("retry_after_ms", retry_after_ms)
                .finish(),
            Action::RefreshToken => f.write_str("RefreshToken"),
            Action::Connected { client } => f.debug_struct("Connected").field("client", client).finish(),
            Action::Subscribed { channel } => f.debug_struct("Subscribed").field("channel", channel).finish(),
            Action::Unsubscribed { channel, code, reason } => f
                .debug_struct("Unsubscribed")
                .field("channel", channel)
                .field("code", code)
                .field("reason", reason)
                .finish(),
            Action::SubscribeFailed { channel, code, message } => f
                .debug_struct("SubscribeFailed")
                .field("channel", channel)
                .field("code", code)
                .field("message", message)
                .finish(),
            Action::Event { channel, name, data, event } => f
                .debug_struct("Event")
                .field("channel", channel)
                .field("name", name)
                .field("data", data)
                .field("event", event)
                .finish(),
        }
    }
}

/// A frame with the token of any `connect` or `refresh` in it replaced.
fn redact_tokens(text: &str) -> String {
    if !text.contains("\"token\"") {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(mut v) => {
                for command in ["connect", "refresh"] {
                    if let Some(t) = v.get_mut(command).and_then(|c| c.get_mut("token")) {
                        *t = Value::String("[redacted]".into());
                    }
                }
                v.to_string()
            }
            // Not JSON, and it mentions a token: show none of it.
            Err(_) => "[redacted]".into(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `data` without `playback_url`, at any depth. A JSON string (Kick's
/// usual form) is decoded, cleaned and encoded again only when a
/// `playback_url` key was actually removed; otherwise it is passed on as
/// Kick sent it, byte for byte (a chat message can mention `playback_url`).
fn without_playback_urls(data: Value) -> Value {
    /// Whether anything was removed.
    fn strip(v: &mut Value) -> bool {
        match v {
            Value::Object(map) => {
                let before = map.len();
                map.retain(|k, _| k != "playback_url");
                let removed = map.len() != before;
                map.values_mut().fold(removed, |r, v| strip(v) | r)
            }
            Value::Array(list) => list.iter_mut().fold(false, |r, v| strip(v) | r),
            _ => false,
        }
    }
    match data {
        Value::String(s) if s.contains("playback_url") => match serde_json::from_str::<Value>(&s) {
            Ok(mut v) => {
                if strip(&mut v) {
                    Value::String(v.to_string())
                } else {
                    Value::String(s)
                }
            }
            Err(_) => Value::String(s),
        },
        mut other => {
            strip(&mut other);
            other
        }
    }
}

/// A JSON string literal.
fn quoted(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

/// Whether, after a refused connect or refresh, a new connection could fare
/// better at once: as Centrifugo's own client decides, for a transport
/// error (below 100), an error Kick marks `temporary`, and 109 (token
/// expired). Any other refusal is terminal: the long wait, then a new
/// client id.
fn retryable(code: i64, temporary: bool) -> bool {
    code < 100 || temporary || code == 109
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Connecting,
    Connected,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Connect,
    /// A subscribe; `again` when Kick asked for it (2500-2999), in which case
    /// the feed has already been reported unsubscribed.
    Subscribe {
        channel: String,
        again: bool,
    },
    Refresh,
}

/// One connection's protocol state. See the module docs.
#[derive(Debug, Clone)]
pub struct Session {
    options: SessionOptions,
    phase: Phase,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    subscribed: BTreeSet<String>,
    /// The server's ping interval; `None` once it said it does not ping.
    ping_ms: Option<u64>,
    last_traffic: u64,
    refresh_at: Option<u64>,
    refresh_asked: bool,
}

impl Session {
    /// A session for these options, not yet connected.
    pub fn new(mut options: SessionOptions) -> Self {
        let mut seen = BTreeSet::new();
        options.channels.retain(|c| seen.insert(c.clone()));
        Self {
            options,
            phase: Phase::Idle,
            next_id: 0,
            pending: HashMap::new(),
            subscribed: BTreeSet::new(),
            ping_ms: Some(DEFAULT_PING_MS),
            last_traffic: 0,
            refresh_at: None,
            refresh_asked: false,
        }
    }

    /// Whether this feed is subscribed now.
    pub fn is_subscribed(&self, channel: &str) -> bool {
        self.subscribed.contains(channel)
    }

    /// Whether Kick has accepted the connection.
    pub fn is_connected(&self) -> bool {
        self.phase == Phase::Connected
    }

    /// A command frame: `{"id":N,<body>}`. Written out rather than built as a
    /// JSON object, so the key order is Kick's whatever serde_json's map is.
    fn command(&mut self, pending: Pending, body: &str) -> String {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.pending.insert(id, pending);
        format!("{{\"id\":{id},{body}}}")
    }

    fn subscribe(&mut self, channel: String, again: bool) -> String {
        let body = format!("\"subscribe\":{{\"channel\":{}}}", quoted(&channel));
        self.command(Pending::Subscribe { channel, again }, &body)
    }

    /// The socket opened: send the connect command with this token.
    ///
    /// On a session that was not [`closed`](Session::closed) first, the
    /// feeds of the old connection are reported unsubscribed here, as
    /// `closed` would have.
    pub fn open(&mut self, token: &str, now: u64) -> Vec<Action> {
        let mut out = self.lost_feeds();
        self.reset();
        self.phase = Phase::Connecting;
        self.last_traffic = now;
        let body =
            format!("\"connect\":{{\"token\":{},\"name\":{}}}", quoted(token), quoted(&self.options.client_name));
        let text = self.command(Pending::Connect, &body);
        out.push(Action::Send { text });
        out
    }

    /// A new token, after [`Action::RefreshToken`].
    pub fn refresh(&mut self, token: &str, _now: u64) -> Vec<Action> {
        if self.phase != Phase::Connected {
            return vec![];
        }
        let text = self.command(Pending::Refresh, &format!("\"refresh\":{{\"token\":{}}}", quoted(token)));
        vec![Action::Send { text }]
    }

    /// A text frame arrived.
    pub fn receive(&mut self, text: &str, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if matches!(self.phase, Phase::Idle | Phase::Closing) {
            return out;
        }
        self.last_traffic = now;
        for line in text.split('\n') {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "{}" {
                out.push(Action::Send { text: "{}".into() });
                continue;
            }
            let Ok(Value::Object(msg)) = serde_json::from_str::<Value>(line) else { continue };
            let msg = Value::Object(msg);
            match msg.get("id").and_then(Value::as_u64) {
                Some(id) if id > 0 => self.reply(id, &msg, now, &mut out),
                _ => {
                    if let Some(push) = msg.get("push") {
                        self.push(push, &mut out);
                    }
                }
            }
            if self.phase == Phase::Closing {
                break;
            }
        }
        out
    }

    fn reply(&mut self, id: u64, msg: &Value, now: u64, out: &mut Vec<Action>) {
        let Some(pending) = self.pending.remove(&id) else { return };
        let error = msg.get("error").map(|e| {
            (
                e.get("code").and_then(Value::as_i64).unwrap_or(0),
                e.get("message").and_then(Value::as_str).unwrap_or("").to_owned(),
                e.get("temporary").and_then(Value::as_bool).unwrap_or(false),
            )
        });
        match (pending, error) {
            (Pending::Connect, Some((code, message, temporary))) => {
                let reason = format!("Kick refused the connection: {code} {message}");
                self.close(code, reason, !retryable(code, temporary), out);
            }
            (Pending::Connect, None) => {
                let reply = msg.get("connect").cloned().unwrap_or_default();
                self.phase = Phase::Connected;
                // No ping interval: the server will not ping, and a quiet
                // feed is not a dead one.
                self.ping_ms = reply
                    .get("ping")
                    .and_then(Value::as_u64)
                    .filter(|p| *p > 0)
                    .map(|p| p.clamp(*PING_SECS.start(), *PING_SECS.end()) * 1000);
                self.schedule_refresh(&reply, now);
                out.push(Action::Connected { client: reply.get("client").and_then(Value::as_str).map(str::to_owned) });
                let frames: Vec<String> =
                    self.options.channels.clone().into_iter().map(|c| self.subscribe(c, false)).collect();
                if !frames.is_empty() {
                    out.push(Action::Send { text: frames.join("\n") });
                }
            }
            (Pending::Subscribe { channel, .. }, Some((code, message, _))) => {
                out.push(Action::SubscribeFailed { channel, code, message });
            }
            (Pending::Subscribe { channel, .. }, None) => {
                self.subscribed.insert(channel.clone());
                out.push(Action::Subscribed { channel });
            }
            (Pending::Refresh, Some((code, message, temporary))) => {
                let reason = format!("Kick refused the new token: {code} {message}");
                self.close(code, reason, !retryable(code, temporary), out);
            }
            (Pending::Refresh, None) => {
                let reply = msg.get("refresh").cloned().unwrap_or_default();
                self.schedule_refresh(&reply, now);
            }
        }
    }

    fn push(&mut self, push: &Value, out: &mut Vec<Action>) {
        let channel = push.get("channel").and_then(Value::as_str).unwrap_or("").to_owned();
        if let Some(d) = push.get("disconnect") {
            let code = d.get("code").and_then(Value::as_i64).unwrap_or(0);
            let reason = d.get("reason").and_then(Value::as_str).unwrap_or("");
            let terminal = match u16::try_from(code) {
                Ok(c @ 3000..=4999) => retry_after_ms(c) > 0,
                // Outside Centrifugo's ranges: what the push says, if anything.
                _ => d.get("reconnect").and_then(Value::as_bool) == Some(false),
            };
            self.close(code, format!("Kick disconnected: {code} {reason}"), terminal, out);
        } else if let Some(u) = push.get("unsub") {
            if self.subscribed.remove(&channel) {
                let code = u.get("code").and_then(Value::as_i64).unwrap_or(0);
                let reason = u.get("reason").and_then(Value::as_str).unwrap_or("").to_owned();
                out.push(Action::Unsubscribed { channel: channel.clone(), code, reason });
                if (2500..3000).contains(&code) && self.phase == Phase::Connected {
                    let text = self.subscribe(channel, true);
                    out.push(Action::Send { text });
                }
            }
        } else if let Some(envelope) = push.get("pub").and_then(|p| p.get("data")) {
            // Kick's own envelope: {"event": name, "data": "<json>"}.
            if let Some(name) = envelope.get("event").and_then(Value::as_str) {
                let data = without_playback_urls(envelope.get("data").cloned().unwrap_or(Value::Null));
                let event = events::parse(name, &data);
                out.push(Action::Event { channel, name: name.to_owned(), data, event });
            }
        }
    }

    fn schedule_refresh(&mut self, reply: &Value, now: u64) {
        let expires = reply.get("expires").and_then(Value::as_bool).unwrap_or(false);
        let ttl = reply.get("ttl").and_then(Value::as_u64).unwrap_or(0);
        self.refresh_asked = false;
        self.refresh_at = (expires && ttl > 0).then(|| {
            let ttl_ms = ttl.clamp(*TTL_SECS.start(), *TTL_SECS.end()) * 1000;
            now.saturating_add(ttl_ms.saturating_sub(self.options.refresh_lead_ms).max(ttl_ms / 2))
        });
    }

    fn close(&mut self, code: i64, reason: String, terminal: bool, out: &mut Vec<Action>) {
        if self.phase != Phase::Closing {
            self.phase = Phase::Closing;
            let retry_after_ms = if terminal { TERMINAL_RETRY_MS } else { 0 };
            out.push(Action::Close { code, reason, terminal, retry_after_ms });
        }
    }

    /// The clock moved on: closes a silent connection, asks for a token
    /// before the current one runs out.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if matches!(self.phase, Phase::Idle | Phase::Closing) {
            return out;
        }
        if self.silence_deadline().is_some_and(|d| now >= d) {
            self.close(0, "Kick went quiet".into(), false, &mut out);
            return out;
        }
        if let Some(at) = self.refresh_at
            && !self.refresh_asked
            && now >= at
        {
            self.refresh_asked = true;
            out.push(Action::RefreshToken);
        }
        out
    }

    fn silence_deadline(&self) -> Option<u64> {
        self.ping_ms.map(|p| self.last_traffic.saturating_add(p).saturating_add(self.options.silence_grace_ms))
    }

    /// When to call [`Session::tick`] next, if at all.
    pub fn deadline(&self) -> Option<u64> {
        if matches!(self.phase, Phase::Idle | Phase::Closing) {
            return None;
        }
        let refresh = self.refresh_at.filter(|_| !self.refresh_asked);
        match (refresh, self.silence_deadline()) {
            (Some(r), Some(s)) => Some(r.min(s)),
            (r, s) => r.or(s),
        }
    }

    /// The socket closed. Reports every feed that was subscribed, or waiting
    /// for Kick's answer to its subscribe, as no longer so (code 0), and
    /// leaves the session ready for [`Session::open`] on a new socket.
    pub fn closed(&mut self) -> Vec<Action> {
        let out = self.lost_feeds();
        self.reset();
        out
    }

    /// Unsubscribed for each feed subscribed or being subscribed, each once:
    /// a feed Kick asked to subscribe again was reported when it asked.
    fn lost_feeds(&mut self) -> Vec<Action> {
        let mut lost = std::mem::take(&mut self.subscribed);
        lost.extend(self.pending.values().filter_map(|p| match p {
            Pending::Subscribe { channel, again: false } => Some(channel.clone()),
            _ => None,
        }));
        lost.into_iter()
            .map(|channel| Action::Unsubscribed { channel, code: 0, reason: "socket closed".into() })
            .collect()
    }

    fn reset(&mut self) {
        self.phase = Phase::Idle;
        // Command ids are per connection: a new socket starts at 1.
        self.next_id = 0;
        self.pending.clear();
        self.subscribed.clear();
        self.ping_ms = Some(DEFAULT_PING_MS);
        self.refresh_at = None;
        self.refresh_asked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn debug_hides_tokens() {
        let mut s = Session::new(SessionOptions::new(vec![]));
        let out = s.open("sekrit.jwt", 0);
        let shown = format!("{out:?}");
        assert!(!shown.contains("sekrit"), "{shown}");
        assert!(shown.contains("[redacted]"));
        // What goes on the wire is untouched.
        assert!(matches!(&out[0], Action::Send { text } if text.contains("sekrit.jwt")));
        assert_eq!(redact_tokens("{}"), "{}");
        assert_eq!(redact_tokens("\"token\" garbage"), "[redacted]");
    }

    #[test]
    fn playback_urls_are_removed_at_any_depth() {
        let inner = json!({"slug": "otherchannel", "channel": {"id": 1, "playback_url": "https://x?sig=sekrit",
            "list": [{"playback_url": "y"}]}, "hosted": {"slug": "somechannel"}});
        let cleaned = without_playback_urls(Value::String(inner.to_string()));
        let text = cleaned.as_str().unwrap();
        assert!(!text.contains("playback_url") && !text.contains("sekrit"), "{text}");
        let back: Value = serde_json::from_str(text).unwrap();
        assert_eq!(back["hosted"]["slug"], "somechannel");
        assert_eq!(back["channel"]["id"], 1);
        assert_eq!(without_playback_urls(inner.clone())["channel"].get("playback_url"), None);
        // Without one, the string is passed on as it was.
        let plain = Value::String("{\"b\":1,\"a\":2}".into());
        assert_eq!(without_playback_urls(plain.clone()), plain);
        // Mentioned, not a key: Kick's string, byte for byte (escapes and
        // key order kept).
        let said = r#"{"id":"m1","content":"what is a playback_url? caf\u00e9 \/ ok","chatroom_id":1}"#;
        assert_eq!(without_playback_urls(Value::String(said.into())), Value::String(said.into()));
        // Not JSON at all: passed on too.
        let odd = Value::String("playback_url {".into());
        assert_eq!(without_playback_urls(odd.clone()), odd);
    }

    #[test]
    fn terminal_codes() {
        for code in [0, 1000, 1006, 2999, 3000, 3499, 4000, 4499, 5000, u16::MAX] {
            assert_eq!(retry_after_ms(code), 0, "{code}");
        }
        for code in [3500, 3501, 3999, 4500, 4501, 4999] {
            assert_eq!(retry_after_ms(code), 300_000, "{code}");
        }
        assert!(retryable(109, false) && retryable(100, true) && retryable(0, false));
        assert!(!retryable(101, false) && !retryable(103, false));
    }
}
