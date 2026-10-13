//! Kick's realtime chat feed.
//!
//! Since October 2026 Kick serves chat over [Centrifugo] (it was Pusher until
//! then; that app key is gone). Connecting takes three steps, and all three
//! only work from kick.com:
//!
//! 1. Ask which server carries the chat: [`chat_connection_request`] for a
//!    channel (each channel's chat lives on one region) or
//!    [`connection_request`] for Kick in general. Read the answer with
//!    [`parse_connection`].
//! 2. Get a guest token for a client id you make up: [`token_request`],
//!    [`parse_token`]. It lasts 45 minutes; a [`Session`] asks for a new one
//!    before then, for the **same** client id.
//! 3. Open a WebSocket to the server's URL and drive a [`Session`] with what
//!    arrives on it.
//!
//! **Origin.** The WebSocket server refuses the handshake (403) unless its
//! Origin is `https://kick.com`, and the two endpoints above answer CORS for
//! that origin alone. A server sets the header ([`REQUIRED_ORIGIN`]); a
//! browser cannot, so in a browser this only works from a page or content
//! script on kick.com. Everywhere else, relay it through a server.
//!
//! [Centrifugo]: https://centrifugal.dev

mod backoff;
mod session;

pub use backoff::Backoff;
pub use session::{Action, Session, SessionOptions};

use crate::http::{self, Endpoints, Error, Method, Request};
use crate::ids::{ChannelId, ChatroomId};
use crate::lenient::string;
use serde::Serialize;
use serde_json::json;

/// The Origin Kick's realtime server and endpoints accept.
pub const REQUIRED_ORIGIN: &str = "https://kick.com";

/// The feeds Kick's own page joins for one chat, the chat feed first. What
/// each carries, as seen:
///
/// | Feed | Carries |
/// |---|---|
/// | `chatrooms.<chatroom>.v2` | chat, deletions, bans, subs, clears, pins, polls, hosts arriving |
/// | `chatrooms.<chatroom>` | the same hosts again, in another shape |
/// | `chatroom_<chatroom>` | gifted subs, reward redemptions |
/// | `channel.<channel>` | live / offline, hosts leaving |
/// | `channel_<channel>` | Kicks |
///
/// Feeds named `private-…` need a user's token and are refused to a guest.
pub fn chat_channels(chatroom: ChatroomId, channel: Option<ChannelId>) -> Vec<String> {
    let mut names =
        vec![format!("chatrooms.{chatroom}.v2"), format!("chatrooms.{chatroom}"), format!("chatroom_{chatroom}")];
    if let Some(channel) = channel {
        names.push(format!("channel.{channel}"));
        names.push(format!("channel_{channel}"));
    }
    names
}

/// A way to connect, as Kick offers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Connection {
    /// A Centrifugo server. What Kick offers today.
    Centrifugo {
        /// `wss://realtime.<region>.platform.kick.com/connection/websocket`
        url: String,
    },
    /// A Pusher app. Kick's client still accepts one; Kick no longer offers it.
    Pusher {
        /// The app key.
        app_key: String,
        /// The cluster.
        cluster: String,
    },
    /// A provider this crate does not know.
    Other {
        /// Its name.
        name: String,
    },
}

impl Connection {
    /// The Centrifugo URL, if this is one.
    pub fn centrifugo_url(&self) -> Option<&str> {
        match self {
            Connection::Centrifugo { url } => Some(url),
            _ => None,
        }
    }
}

fn client_body(client_id: &str) -> serde_json::Value {
    json!({
        "client": { "id": client_id, "type": "web" },
        "capabilities": { "accepted_providers": [{ "provider": "centrifugo" }] }
    })
}

fn kick_origin(req: Request) -> Request {
    req.header("origin", REQUIRED_ORIGIN).header("referer", format!("{REQUIRED_ORIGIN}/"))
}

/// Where Kick's chat for `channel` is served. `client_id` is any id you make
/// up (a UUID); use the same one for the token.
pub fn chat_connection_request(endpoints: &Endpoints, channel: ChannelId, client_id: &str) -> Request {
    let url = format!("{}/channels/{channel}/chat/connection", endpoints.realtime);
    kick_origin(Request::new(Method::Post, url).json(&client_body(client_id)))
}

/// Where Kick's realtime is served in general, for feeds not tied to one
/// channel's chat.
pub fn connection_request(endpoints: &Endpoints, client_id: &str) -> Request {
    let url = format!("{}/connection", endpoints.realtime);
    kick_origin(Request::new(Method::Post, url).json(&client_body(client_id)))
}

/// Reads either connection answer: the ways Kick offers, best first.
pub fn parse_connection(status: u16, body: &str) -> Result<Vec<Connection>, Error> {
    let data = http::data(status, body)?;
    let list = data.get("connections").and_then(|c| c.as_array()).ok_or_else(|| Error::shape("no `connections`"))?;
    Ok(list
        .iter()
        .map(|c| {
            let creds = c.get("credentials").cloned().unwrap_or_default();
            match string(c, "provider").as_deref() {
                Some("centrifugo") => match string(&creds, "url") {
                    Some(url) => Connection::Centrifugo { url },
                    None => Connection::Other { name: "centrifugo".into() },
                },
                Some("pusher") => match (string(&creds, "app_key"), string(&creds, "cluster")) {
                    (Some(app_key), Some(cluster)) => Connection::Pusher { app_key, cluster },
                    _ => Connection::Other { name: "pusher".into() },
                },
                other => Connection::Other { name: other.unwrap_or("").into() },
            }
        })
        .collect())
}

/// A guest connection token for `client_id`. No session goes with it.
pub fn token_request(endpoints: &Endpoints, client_id: &str) -> Request {
    let url = format!("{}/auth/connection", endpoints.realtime);
    kick_origin(Request::new(Method::Post, url).json(&json!({ "client_id": client_id })))
}

/// The token from a [`token_request`] answer.
pub fn parse_token(status: u16, body: &str) -> Result<String, Error> {
    let data = http::data(status, body)?;
    string(&data, "token").filter(|t| !t.is_empty()).ok_or_else(|| Error::shape("no `token`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_carry_kicks_origin() {
        let e = Endpoints::default();
        let r = chat_connection_request(&e, ChannelId(668), "c1");
        assert_eq!(r.url, "https://web.kick.com/api/v1/realtime/channels/668/chat/connection");
        assert!(r.headers.contains(&("origin".into(), "https://kick.com".into())));
        let body: serde_json::Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["client"]["id"], "c1");
        let t = token_request(&e, "c1");
        assert_eq!(t.body.as_deref(), Some(r#"{"client_id":"c1"}"#));
    }

    #[test]
    fn connections_are_read_in_order() {
        let body = r#"{"data":{"connections":[{"credentials":{"url":"wss://a/connection/websocket"},"provider":"centrifugo"},
            {"credentials":{"app_key":"k","cluster":"us2"},"provider":"pusher"},{"provider":"ably"}],"mode":"websocket"},"message":"success"}"#;
        let c = parse_connection(200, body).unwrap();
        assert_eq!(c[0].centrifugo_url(), Some("wss://a/connection/websocket"));
        assert_eq!(c[1], Connection::Pusher { app_key: "k".into(), cluster: "us2".into() });
        assert_eq!(c[2], Connection::Other { name: "ably".into() });
        assert_eq!(parse_token(200, r#"{"data":{"token":"t"},"message":"success"}"#).unwrap(), "t");
        assert!(parse_token(200, r#"{"data":{},"message":"success"}"#).is_err());
    }

    #[test]
    fn feeds_for_a_chat() {
        assert_eq!(
            chat_channels(ChatroomId(1), Some(ChannelId(2))),
            ["chatrooms.1.v2", "chatrooms.1", "chatroom_1", "channel.2", "channel_2"]
        );
        assert_eq!(chat_channels(ChatroomId(1), None).len(), 3);
    }
}
