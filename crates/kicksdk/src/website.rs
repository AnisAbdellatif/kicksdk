//! The endpoints kick.com's own frontend uses, `kick.com/api/v2`.
//!
//! Undocumented: Kick can change or block them at any time, and using them is
//! a grey area under Kick's terms. Treat a failure as a gap, never a zero.
//! They are here because some things exist nowhere else: the **chatroom id**
//! (which chat needs), the **follower total**, chat history and the pinned
//! message, and acting as a viewer (moderating, sending).
//!
//! Reading needs nothing. Acting needs the viewer's session: kick.com keeps it
//! in the `session_token` cookie and sends it as a bearer token, and so do the
//! `*_request` builders that take a `session`. Never log or store it.
//!
//! Builders that put a slug, a username or a message id in the URL's path
//! refuse an empty one, `.` and `..` ([`Error::Invalid`]): those would
//! move the path to another endpoint.
//!
//! The channel answer carries a `playback_url` with a signed token. It is
//! passed on ([`WebsiteChannel::playback_url`]) because a player needs it;
//! treat it like a credential - never log or store it.

use crate::events::{self, ChatMessage, KickEvent};
use crate::http::{self, Endpoints, Error, Method, Request, segment};
use crate::ids::{ChannelId, ChatroomId, UserId};
use crate::lenient::{array, boolean, get, int, string, uint};
use serde::Serialize;
use serde_json::{Value, json};

fn v2(endpoints: &Endpoints, path: &str) -> String {
    format!("{}/api/v2{path}", endpoints.website)
}

fn acting(req: Request, session: &str) -> Result<Request, Error> {
    // Kick's frontend sends this on every call; the moderation endpoints
    // have been seen to want it.
    Ok(req.bearer(session)?.header("x-app-platform", "web"))
}

/// A subscriber badge tier of a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct SubscriberBadge {
    /// From this many months.
    pub months: i64,
    /// The image.
    pub src: String,
}

/// A channel, as the website API gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct WebsiteChannel {
    /// The channel id.
    pub id: ChannelId,
    /// The owner's user id.
    pub user_id: Option<UserId>,
    /// The slug.
    pub slug: Option<String>,
    /// The chatroom id: the one thing chat needs that no official endpoint gives.
    pub chatroom_id: ChatroomId,
    /// The follower total: a number in one answer, a string in another.
    pub followers_count: Option<u64>,
    /// Whether it is live now.
    pub live: bool,
    /// Viewers now, when live.
    pub viewer_count: Option<i64>,
    /// Kick's livestream id, when live.
    pub livestream_id: Option<u64>,
    /// The stream's title, when live.
    pub session_title: Option<String>,
    /// The owner's username, case kept.
    pub username: Option<String>,
    /// The owner's picture.
    pub profile_pic: Option<String>,
    /// The live stream's HLS master playlist, **signed**: a credential for the
    /// stream. Present while live. Never log or store it.
    pub playback_url: Option<String>,
    /// The channel's own subscriber badges, by months ascending.
    pub subscriber_badges: Vec<SubscriberBadge>,
    /// Verified.
    pub verified: Option<bool>,
}

/// A channel by slug. A rename makes the old slug 404 until the new one is known.
pub fn channel_request(endpoints: &Endpoints, slug: &str) -> Result<Request, Error> {
    Ok(Request::new(Method::Get, v2(endpoints, &format!("/channels/{}", segment("slug", slug)?))))
}

/// Reads a [`channel_request`] answer (not enveloped in `data`).
pub fn parse_channel(status: u16, body: &str) -> Result<WebsiteChannel, Error> {
    let v = http::json(status, body)?;
    let chatroom = get(&v, "chatroom").ok_or_else(|| Error::shape("no `chatroom`"))?;
    let livestream = get(&v, "livestream");
    let mut subscriber_badges: Vec<SubscriberBadge> = array(&v, "subscriber_badges")
        .iter()
        .filter_map(|b| {
            Some(SubscriberBadge {
                months: int(b, "months")?,
                src: get(b, "badge_image").and_then(|i| string(i, "src"))?,
            })
        })
        .collect();
    subscriber_badges.sort_by_key(|b| b.months);
    Ok(WebsiteChannel {
        id: ChannelId(uint(&v, "id").ok_or_else(|| Error::shape("no channel `id`"))?),
        user_id: uint(&v, "user_id").map(UserId),
        slug: string(&v, "slug"),
        chatroom_id: ChatroomId(uint(chatroom, "id").ok_or_else(|| Error::shape("no `chatroom.id`"))?),
        followers_count: uint(&v, "followers_count"),
        live: livestream.is_some_and(Value::is_object),
        viewer_count: livestream.and_then(|l| int(l, "viewer_count")),
        livestream_id: livestream.and_then(|l| uint(l, "id")),
        session_title: livestream.and_then(|l| string(l, "session_title")),
        username: get(&v, "user").and_then(|u| string(u, "username")),
        profile_pic: get(&v, "user").and_then(|u| string(u, "profile_pic")),
        playback_url: string(&v, "playback_url").filter(|u| !u.is_empty()),
        subscriber_badges,
        verified: boolean(&v, "verified").or_else(|| get(&v, "verified").map(Value::is_object)),
    })
}

/// The recent chat and the pinned message.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct History {
    /// Recent messages, oldest first.
    pub messages: Vec<ChatMessage>,
    /// The pinned message, as a [`KickEvent::PinCreated`].
    pub pinned: Option<KickEvent>,
}

/// A channel's recent chat and its pin, by channel id.
pub fn history_request(endpoints: &Endpoints, channel: ChannelId) -> Request {
    Request::new(Method::Get, v2(endpoints, &format!("/channels/{channel}/messages")))
}

/// Reads a [`history_request`] answer.
pub fn parse_history(status: u16, body: &str) -> Result<History, Error> {
    let data = http::data(status, body)?;
    let mut messages: Vec<(usize, ChatMessage)> =
        array(&data, "messages").iter().filter_map(events::chat_message).enumerate().collect();
    // Oldest first; ties keep Kick's order.
    messages.sort_by(|(ia, a), (ib, b)| a.created_at.cmp(&b.created_at).then(ia.cmp(ib)));
    Ok(History {
        messages: messages.into_iter().map(|(_, m)| m).collect(),
        pinned: get(&data, "pinned_message").and_then(events::pin_from_history),
    })
}

/// A user as a channel sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ChannelUser {
    /// Their user id.
    pub id: Option<u64>,
    /// Their username.
    pub username: Option<String>,
    /// Their slug.
    pub slug: Option<String>,
    /// Their picture.
    pub profile_pic: Option<String>,
    /// When they joined Kick.
    pub created_at: Option<String>,
    /// Following this channel since.
    pub following_since: Option<String>,
    /// Months subscribed to this channel.
    pub subscribed_for: Option<i64>,
    /// A moderator here.
    pub is_moderator: bool,
    /// Kick staff.
    pub is_staff: bool,
    /// This channel's owner.
    pub is_channel_owner: bool,
    /// Banned here: Kick's ban record, as sent.
    pub banned: Option<Value>,
    /// Their badges here.
    pub badges: Vec<events::Badge>,
}

/// A user as `channel` sees them. A 404 means the channel does not know the
/// name: it answers whether an `@word` is a real user.
pub fn channel_user_request(endpoints: &Endpoints, channel_slug: &str, username: &str) -> Result<Request, Error> {
    let path = format!("/channels/{}/users/{}", segment("slug", channel_slug)?, segment("username", username)?);
    Ok(Request::new(Method::Get, v2(endpoints, &path)))
}

/// Reads a [`channel_user_request`] answer.
pub fn parse_channel_user(status: u16, body: &str) -> Result<ChannelUser, Error> {
    let v = http::json(status, body)?;
    Ok(ChannelUser {
        id: uint(&v, "id"),
        username: string(&v, "username"),
        slug: string(&v, "slug"),
        profile_pic: string(&v, "profile_pic"),
        created_at: string(&v, "created_at"),
        following_since: string(&v, "following_since"),
        subscribed_for: int(&v, "subscribed_for"),
        is_moderator: boolean(&v, "is_moderator").unwrap_or(false),
        is_staff: boolean(&v, "is_staff").unwrap_or(false),
        is_channel_owner: boolean(&v, "is_channel_owner").unwrap_or(false),
        banned: get(&v, "banned").cloned(),
        badges: array(&v, "badges")
            .iter()
            .filter_map(|b| {
                Some(events::Badge { kind: string(b, "type")?, text: string(b, "text"), count: int(b, "count") })
            })
            .collect(),
    })
}

/// What the signed-in viewer may do on a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Me {
    /// A moderator here.
    pub is_moderator: bool,
    /// The channel's owner.
    pub is_broadcaster: bool,
    /// Kick staff.
    pub is_super_admin: bool,
}

impl Me {
    /// May moderate this channel.
    pub fn can_moderate(&self) -> bool {
        self.is_moderator || self.is_broadcaster || self.is_super_admin
    }
}

/// The signed-in viewer's standing on a channel. 401/403: not signed in.
pub fn me_request(endpoints: &Endpoints, session: &str, channel_slug: &str) -> Result<Request, Error> {
    let url = v2(endpoints, &format!("/channels/{}/me", segment("slug", channel_slug)?));
    acting(Request::new(Method::Get, url), session)
}

/// Reads a [`me_request`] answer.
pub fn parse_me(status: u16, body: &str) -> Result<Me, Error> {
    let v = http::json(status, body)?;
    Ok(Me {
        is_moderator: boolean(&v, "is_moderator").unwrap_or(false),
        is_broadcaster: boolean(&v, "is_broadcaster").unwrap_or(false),
        is_super_admin: boolean(&v, "is_super_admin").unwrap_or(false),
    })
}

/// Bans `username` from a channel for good.
pub fn ban_request(endpoints: &Endpoints, session: &str, channel_slug: &str, username: &str) -> Result<Request, Error> {
    let url = v2(endpoints, &format!("/channels/{}/bans", segment("slug", channel_slug)?));
    Ok(acting(Request::new(Method::Post, url), session)?
        .json(&json!({ "permanent": true, "banned_username": username, "reason": null })))
}

/// Times `username` out for `minutes`.
pub fn timeout_request(
    endpoints: &Endpoints,
    session: &str,
    channel_slug: &str,
    username: &str,
    minutes: u32,
) -> Result<Request, Error> {
    if !(1..=10_080).contains(&minutes) {
        return Err(Error::Invalid(format!("{minutes} minutes; 1 to 10080 (a week)")));
    }
    let url = v2(endpoints, &format!("/channels/{}/bans", segment("slug", channel_slug)?));
    Ok(acting(Request::new(Method::Post, url), session)?
        .json(&json!({ "permanent": false, "banned_username": username, "reason": null, "duration": minutes })))
}

/// Lifts a ban or timeout.
pub fn unban_request(
    endpoints: &Endpoints,
    session: &str,
    channel_slug: &str,
    username: &str,
) -> Result<Request, Error> {
    let path = format!("/channels/{}/bans/{}", segment("slug", channel_slug)?, segment("username", username)?);
    acting(Request::new(Method::Delete, v2(endpoints, &path)), session)
}

/// Deletes a message. Needs the chatroom id, not the channel's.
pub fn delete_message_request(
    endpoints: &Endpoints,
    session: &str,
    chatroom: ChatroomId,
    message_id: &str,
) -> Result<Request, Error> {
    let url = v2(endpoints, &format!("/chatrooms/{chatroom}/messages/{}", segment("message id", message_id)?));
    acting(Request::new(Method::Delete, url), session)
}

/// Pins a message for `minutes`. Kick takes the whole message back - sender,
/// badges and all - and shows it to every viewer as the banner, so `message`
/// should be the message as the feed delivered it (the event's `data`).
pub fn pin_request(
    endpoints: &Endpoints,
    session: &str,
    channel_slug: &str,
    message: &Value,
    minutes: u32,
) -> Result<Request, Error> {
    if !(1..=10_080).contains(&minutes) {
        return Err(Error::Invalid(format!("{minutes} minutes; 1 to 10080 (a week)")));
    }
    let url = v2(endpoints, &format!("/channels/{}/pinned-message", segment("slug", channel_slug)?));
    Ok(acting(Request::new(Method::Post, url), session)?.json(&json!({ "message": message, "duration": minutes })))
}

/// Removes the channel's pin (there is only ever one).
pub fn unpin_request(endpoints: &Endpoints, session: &str, channel_slug: &str) -> Result<Request, Error> {
    let url = v2(endpoints, &format!("/channels/{}/pinned-message", segment("slug", channel_slug)?));
    acting(Request::new(Method::Delete, url), session)
}

/// What a reply answers, for [`send_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget<'a> {
    /// The parent message's id.
    pub message_id: &'a str,
    /// Its text.
    pub content: &'a str,
    /// Its sender's id.
    pub sender_id: u64,
    /// Its sender's username.
    pub sender_username: &'a str,
}

/// Sends a message, or a reply, to a chatroom as the viewer. `message_ref` is
/// any string (Kick's page sends the clock); the feed echoes it back so the
/// sender can recognise its own message.
pub fn send_request(
    endpoints: &Endpoints,
    session: &str,
    chatroom: ChatroomId,
    content: &str,
    message_ref: &str,
    reply_to: Option<&ReplyTarget<'_>>,
) -> Result<Request, Error> {
    if content.trim().is_empty() || content.chars().count() > 500 {
        return Err(Error::Invalid("a message is 1 to 500 characters".into()));
    }
    let body = match reply_to {
        None => json!({ "content": content, "type": "message", "message_ref": message_ref }),
        Some(r) => json!({
            "content": content,
            "type": "reply",
            "message_ref": message_ref,
            "metadata": {
                "original_message": { "id": r.message_id, "content": r.content },
                "original_sender": { "id": r.sender_id, "username": r.sender_username }
            },
            "thread_parent_id": r.message_id
        }),
    };
    let url = v2(endpoints, &format!("/messages/send/{chatroom}"));
    Ok(acting(Request::new(Method::Post, url), session)?.json(&body))
}

/// Whether an acting call worked: Kick's answer to these carries nothing else
/// worth reading, and the feed reports the effect.
pub fn parse_action(status: u16, body: &str) -> Result<(), Error> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::Status { status, excerpt: body.chars().take(200).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn followers_come_as_numbers_or_strings() {
        for n in ["123", "\"123\""] {
            let body = format!(
                r#"{{"id":5,"user_id":6,"slug":"a","followers_count":{n},"chatroom":{{"id":7}},"livestream":null}}"#
            );
            let c = parse_channel(200, &body).unwrap();
            assert_eq!(c.followers_count, Some(123));
            assert_eq!(c.chatroom_id, ChatroomId(7));
            assert!(!c.live);
        }
        assert!(parse_channel(200, r#"{"id":5}"#).is_err());
        assert_eq!(parse_channel(404, "{}").unwrap_err().status(), Some(404));
    }

    #[test]
    fn acting_carries_the_session() {
        let e = Endpoints::default();
        let r = timeout_request(&e, "sess", "somechannel", "bob", 5).unwrap();
        assert_eq!(r.url, "https://kick.com/api/v2/channels/somechannel/bans");
        assert!(r.headers.contains(&("authorization".into(), "Bearer sess".into())));
        assert!(timeout_request(&e, "s", "x", "b", 0).is_err());
        let reply = ReplyTarget { message_id: "m1", content: "hi", sender_id: 9, sender_username: "Ann" };
        let r = send_request(&e, "s", ChatroomId(3), "yo", "123", Some(&reply)).unwrap();
        let body: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["thread_parent_id"], "m1");
        assert_eq!(body["metadata"]["original_sender"]["id"], 9);
        assert!(send_request(&e, "s", ChatroomId(3), "  ", "1", None).is_err());
        assert!(send_request(&e, "s\r\nx: y", ChatroomId(3), "yo", "1", None).is_err());
    }

    #[test]
    fn path_segments_cannot_move_the_path() {
        let e = Endpoints::default();
        for bad in ["", ".", ".."] {
            assert!(matches!(channel_request(&e, bad), Err(Error::Invalid(_))), "{bad:?}");
            assert!(channel_user_request(&e, bad, "bob").is_err());
            assert!(channel_user_request(&e, "somechannel", bad).is_err());
            assert!(me_request(&e, "s", bad).is_err());
            assert!(ban_request(&e, "s", bad, "bob").is_err());
            assert!(timeout_request(&e, "s", bad, "bob", 5).is_err());
            assert!(unban_request(&e, "s", bad, "bob").is_err());
            assert!(unban_request(&e, "s", "somechannel", bad).is_err());
            assert!(delete_message_request(&e, "s", ChatroomId(1), bad).is_err());
            assert!(pin_request(&e, "s", bad, &json!({}), 5).is_err());
            assert!(unpin_request(&e, "s", bad).is_err());
        }
        // A dot inside a name is fine, and other characters are encoded.
        let r = channel_user_request(&e, "some.channel", "a/b").unwrap();
        assert_eq!(r.url, "https://kick.com/api/v2/channels/some.channel/users/a%2Fb");
        let r = delete_message_request(&e, "s", ChatroomId(1), "m-1").unwrap();
        assert_eq!(r.url, "https://kick.com/api/v2/chatrooms/1/messages/m-1");
    }
}
