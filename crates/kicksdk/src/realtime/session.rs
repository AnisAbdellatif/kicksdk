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
//!   grace, ends the connection.

use crate::events::{self, KickEvent};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

/// How a session behaves. [`SessionOptions::new`] has Kick's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    /// The feeds to subscribe to, e.g. [`super::chat_channels`].
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

/// What the host should do, or know, after a call.
// Actions live for one call and are handed straight on; boxing the event
// would only cost every host a pointer to chase.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    /// Send this text frame.
    Send {
        /// The frame.
        text: String,
    },
    /// Close the socket, then call [`Session::closed`] and reconnect (with a
    /// [`super::Backoff`]) if the connection is still wanted.
    Close {
        /// Why, for a log line.
        reason: String,
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
    /// A feed is not, or no longer, subscribed.
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
        /// Its data as Kick sent it (usually a JSON string).
        data: Value,
        /// The same, typed.
        event: KickEvent,
    },
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
    Subscribe(String),
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
    ping_ms: u64,
    last_traffic: u64,
    refresh_at: Option<u64>,
    refresh_asked: bool,
}

impl Session {
    /// A session for these options, not yet connected.
    pub fn new(options: SessionOptions) -> Self {
        Self {
            options,
            phase: Phase::Idle,
            next_id: 0,
            pending: HashMap::new(),
            subscribed: BTreeSet::new(),
            ping_ms: 25_000,
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

    fn command(&mut self, pending: Pending, body: Value) -> String {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(id, pending);
        let mut frame = json!({ "id": id });
        if let (Value::Object(f), Value::Object(b)) = (&mut frame, body) {
            f.extend(b);
        }
        frame.to_string()
    }

    /// The socket opened: send the connect command with this token.
    pub fn open(&mut self, token: &str, now: u64) -> Vec<Action> {
        self.reset();
        self.phase = Phase::Connecting;
        self.last_traffic = now;
        let name = self.options.client_name.clone();
        let text = self.command(Pending::Connect, json!({ "connect": { "token": token, "name": name } }));
        vec![Action::Send { text }]
    }

    /// A new token, after [`Action::RefreshToken`].
    pub fn refresh(&mut self, token: &str, _now: u64) -> Vec<Action> {
        if self.phase != Phase::Connected {
            return vec![];
        }
        let text = self.command(Pending::Refresh, json!({ "refresh": { "token": token } }));
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
            )
        });
        match (pending, error) {
            (Pending::Connect, Some((code, message))) => {
                self.close(format!("Kick refused the connection: {code} {message}"), out);
            }
            (Pending::Connect, None) => {
                let reply = msg.get("connect").cloned().unwrap_or_default();
                self.phase = Phase::Connected;
                self.schedule(&reply, now);
                out.push(Action::Connected { client: reply.get("client").and_then(Value::as_str).map(str::to_owned) });
                let frames: Vec<String> = self
                    .options
                    .channels
                    .clone()
                    .into_iter()
                    .map(|channel| {
                        self.command(
                            Pending::Subscribe(channel.clone()),
                            json!({ "subscribe": { "channel": channel } }),
                        )
                    })
                    .collect();
                if !frames.is_empty() {
                    out.push(Action::Send { text: frames.join("\n") });
                }
            }
            (Pending::Subscribe(channel), Some((code, message))) => {
                out.push(Action::SubscribeFailed { channel, code, message });
            }
            (Pending::Subscribe(channel), None) => {
                self.subscribed.insert(channel.clone());
                out.push(Action::Subscribed { channel });
            }
            (Pending::Refresh, Some((code, message))) => {
                self.close(format!("Kick refused the new token: {code} {message}"), out);
            }
            (Pending::Refresh, None) => {
                let reply = msg.get("refresh").cloned().unwrap_or_default();
                self.schedule(&reply, now);
            }
        }
    }

    fn push(&mut self, push: &Value, out: &mut Vec<Action>) {
        let channel = push.get("channel").and_then(Value::as_str).unwrap_or("").to_owned();
        if let Some(d) = push.get("disconnect") {
            let code = d.get("code").and_then(Value::as_i64).unwrap_or(0);
            let reason = d.get("reason").and_then(Value::as_str).unwrap_or("");
            self.close(format!("Kick disconnected: {code} {reason}"), out);
        } else if let Some(u) = push.get("unsub") {
            if self.subscribed.remove(&channel) {
                out.push(Action::Unsubscribed {
                    channel,
                    code: u.get("code").and_then(Value::as_i64).unwrap_or(0),
                    reason: u.get("reason").and_then(Value::as_str).unwrap_or("").to_owned(),
                });
            }
        } else if let Some(envelope) = push.get("pub").and_then(|p| p.get("data")) {
            // Kick's own envelope: {"event": name, "data": "<json>"}.
            if let Some(name) = envelope.get("event").and_then(Value::as_str) {
                let data = envelope.get("data").cloned().unwrap_or(Value::Null);
                let event = events::parse(name, &data);
                out.push(Action::Event { channel, name: name.to_owned(), data, event });
            }
        }
    }

    fn schedule(&mut self, reply: &Value, now: u64) {
        if let Some(ping) = reply.get("ping").and_then(Value::as_u64).filter(|p| *p > 0) {
            self.ping_ms = ping * 1000;
        }
        let expires = reply.get("expires").and_then(Value::as_bool).unwrap_or(false);
        let ttl_ms = reply.get("ttl").and_then(Value::as_u64).unwrap_or(0) * 1000;
        self.refresh_asked = false;
        self.refresh_at =
            (expires && ttl_ms > 0).then(|| now + ttl_ms.saturating_sub(self.options.refresh_lead_ms).max(ttl_ms / 2));
    }

    fn close(&mut self, reason: String, out: &mut Vec<Action>) {
        if self.phase != Phase::Closing {
            self.phase = Phase::Closing;
            out.push(Action::Close { reason });
        }
    }

    /// The clock moved on: closes a silent connection, asks for a token
    /// before the current one runs out.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if matches!(self.phase, Phase::Idle | Phase::Closing) {
            return out;
        }
        if now >= self.silence_deadline() {
            self.close("Kick went quiet".into(), &mut out);
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

    fn silence_deadline(&self) -> u64 {
        self.last_traffic + self.ping_ms + self.options.silence_grace_ms
    }

    /// When to call [`Session::tick`] next, if at all.
    pub fn deadline(&self) -> Option<u64> {
        if matches!(self.phase, Phase::Idle | Phase::Closing) {
            return None;
        }
        let refresh = self.refresh_at.filter(|_| !self.refresh_asked);
        Some(refresh.map_or(self.silence_deadline(), |r| r.min(self.silence_deadline())))
    }

    /// The socket closed. Reports every feed that was subscribed as no longer
    /// so, and leaves the session ready for [`Session::open`] on a new socket.
    pub fn closed(&mut self) -> Vec<Action> {
        let out = std::mem::take(&mut self.subscribed)
            .into_iter()
            .map(|channel| Action::Unsubscribed { channel, code: 0, reason: "socket closed".into() })
            .collect();
        self.reset();
        out
    }

    fn reset(&mut self) {
        self.phase = Phase::Idle;
        // Command ids are per connection: a new socket starts at 1.
        self.next_id = 0;
        self.pending.clear();
        self.subscribed.clear();
        self.refresh_at = None;
        self.refresh_asked = false;
    }
}
