//! Chat with a user token: sending (`POST /public/v1/chat`, scope
//! `chat:write`) and deleting (`DELETE /public/v1/chat/{message_id}`, scope
//! `moderation:chat_message:manage`).
//!
//! These are the official versions of [`website::send_request`] and
//! [`website::delete_message_request`]: they take an OAuth user token
//! instead of the viewer's kick.com session, a channel by user id instead
//! of chatroom id, and a reply by message id alone (`docs/KICK.md` §3).
//!
//! [`website::send_request`]: crate::website::send_request
//! [`website::delete_message_request`]: crate::website::delete_message_request

use super::common::{not_blank, v1};
use crate::http::{self, Endpoints, Error, Method, Request, segment};
use crate::ids::UserId;
use crate::lenient::{boolean, id_string};
use serde::Serialize;
use serde_json::json;

/// The most bytes a message's `content` may take in UTF-8 (Kick's spec).
pub const MAX_BYTES: usize = 2048;

/// The most user-perceived characters (grapheme clusters) in a message
/// (Kick's spec). Not checked here: counting grapheme clusters needs Unicode
/// tables the core does not carry, so Kick refuses (400) what is over.
pub const MAX_CHARACTERS: usize = 500;

/// Who a message goes out as, and so where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sender {
    /// As the token's user, into `broadcaster`'s chat.
    User {
        /// The channel, by its broadcaster's user id.
        broadcaster: UserId,
    },
    /// As the app's bot, into the chat of the channel the token belongs to
    /// (Kick ignores any other channel).
    Bot,
}

/// Sends `content`, as a reply to the message `reply_to` (its id) if
/// given. Refuses an empty message and one over [`MAX_BYTES`].
pub fn send_request(
    endpoints: &Endpoints,
    token: &str,
    sender: Sender,
    content: &str,
    reply_to: Option<&str>,
) -> Result<Request, Error> {
    not_blank("message", content)?;
    if content.len() > MAX_BYTES {
        return Err(Error::Invalid(format!("a message of {} bytes; Kick takes at most {MAX_BYTES}", content.len())));
    }
    let mut body = match sender {
        Sender::User { broadcaster } => {
            json!({ "broadcaster_user_id": broadcaster.0, "content": content, "type": "user" })
        }
        Sender::Bot => json!({ "content": content, "type": "bot" }),
    };
    if let Some(id) = reply_to {
        body["reply_to_message_id"] = id.into();
    }
    Ok(Request::new(Method::Post, v1(endpoints, "/chat")).bearer(token)?.json(&body))
}

/// What Kick says about a message sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Sent {
    /// Kick says it went out. `false` when Kick said so, or said nothing.
    pub is_sent: bool,
    /// The new message's id (a UUID): what the chat feed and the webhooks
    /// call it, and what [`delete_request`] takes.
    pub message_id: Option<String>,
}

/// Reads a [`send_request`] answer.
pub fn parse_send(status: u16, body: &str) -> Result<Sent, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    Ok(Sent { is_sent: boolean(&data, "is_sent").unwrap_or(false), message_id: id_string(&data, "message_id") })
}

/// Deletes a message by its id. Kick answers 204: read it with
/// [`parse_done`](super::parse_done).
pub fn delete_request(endpoints: &Endpoints, token: &str, message_id: &str) -> Result<Request, Error> {
    let url = v1(endpoints, &format!("/chat/{}", segment("message id", message_id)?));
    Request::new(Method::Delete, url).bearer(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn body(r: &Request) -> Value {
        serde_json::from_str(r.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn sends_as_a_user_or_a_bot() {
        let e = Endpoints::default();
        let r = send_request(&e, "t", Sender::User { broadcaster: UserId(123) }, "Pog", Some("m-1")).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, "https://api.kick.com/public/v1/chat"));
        assert!(r.headers.contains(&("authorization".into(), "Bearer t".into())));
        assert_eq!(
            body(&r),
            json!({ "broadcaster_user_id": 123, "content": "Pog", "type": "user", "reply_to_message_id": "m-1" })
        );
        let r = send_request(&e, "t", Sender::Bot, "hi", None).unwrap();
        assert_eq!(body(&r), json!({ "content": "hi", "type": "bot" }));
    }

    #[test]
    fn refuses_what_kick_would() {
        let e = Endpoints::default();
        assert!(matches!(send_request(&e, "t", Sender::Bot, " ", None), Err(Error::Invalid(_))));
        assert!(send_request(&e, "t", Sender::Bot, &"é".repeat(1024), None).is_ok());
        assert!(send_request(&e, "t", Sender::Bot, &"é".repeat(1025), None).is_err());
        assert!(send_request(&e, "t\r\nx: y", Sender::Bot, "hi", None).is_err());
        for bad in ["", ".", ".."] {
            assert!(matches!(delete_request(&e, "t", bad), Err(Error::Invalid(_))), "{bad:?}");
        }
        let r = delete_request(&e, "t", "828f83bb-e391").unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Delete, "https://api.kick.com/public/v1/chat/828f83bb-e391"));
    }

    #[test]
    fn reads_the_answer() {
        let s = parse_send(200, r#"{"data":{"is_sent":true,"message_id":"m"},"message":"OK"}"#).unwrap();
        assert_eq!(s, Sent { is_sent: true, message_id: Some("m".into()) });
        assert!(!parse_send(200, r#"{"data":{},"message":"OK"}"#).unwrap().is_sent);
        assert!(matches!(parse_send(200, r#"{"data":null}"#), Err(Error::Shape(_))));
        assert!(parse_send(429, "{}").unwrap_err().is_rate_limited());
    }
}
