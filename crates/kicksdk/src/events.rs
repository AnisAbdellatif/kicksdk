//! Kick's chat-feed events, typed.
//!
//! The realtime feed delivers each event as Kick's own envelope, an event name
//! and its `data` (usually a JSON string). [`parse`] turns that into a
//! [`KickEvent`]. Parsing is lenient: a field Kick leaves out or sends in
//! another type comes out as `None`, and an event this crate does not know
//! comes out as [`KickEvent::Unknown`] rather than an error, so a new event
//! from Kick never breaks a client that does not care about it.
//!
//! Which feed carries which event is in [`crate::realtime::chat_channels`].

use crate::lenient::{self, array, boolean, get, id_string, int, string, uint};
use serde::Serialize;
use serde_json::Value;

/// Kick's event names, as they arrive.
pub mod names {
    /// A chat message (or a reply). On `chatrooms.<id>.v2`.
    pub const CHAT_MESSAGE: &str = r"App\Events\ChatMessageEvent";
    /// A message removed by a moderator or Kick's AI moderation.
    pub const MESSAGE_DELETED: &str = r"App\Events\MessageDeletedEvent";
    /// A ban or a timeout.
    pub const USER_BANNED: &str = r"App\Events\UserBannedEvent";
    /// A ban or timeout lifted.
    pub const USER_UNBANNED: &str = r"App\Events\UserUnbannedEvent";
    /// The chat cleared by a moderator.
    pub const CHATROOM_CLEAR: &str = r"App\Events\ChatroomClearEvent";
    /// A message pinned.
    pub const PINNED_MESSAGE_CREATED: &str = r"App\Events\PinnedMessageCreatedEvent";
    /// The pin removed.
    pub const PINNED_MESSAGE_DELETED: &str = r"App\Events\PinnedMessageDeletedEvent";
    /// A subscription or resubscription.
    pub const SUBSCRIPTION: &str = r"App\Events\SubscriptionEvent";
    /// Gifted subscriptions, prefixed form.
    pub const GIFTED_SUBSCRIPTIONS: &str = r"App\Events\GiftedSubscriptionsEvent";
    /// Gifted subscriptions as Kick's page binds them: no prefix, on `chatroom_<id>`.
    pub const GIFTED_SUBSCRIPTIONS_PLAIN: &str = "GiftedSubscriptionsEvent";
    /// Kicks (paid gifts). No prefix, on `channel_<id>`.
    pub const KICKS_GIFTED: &str = "KicksGifted";
    /// A channel-point reward redeemed. No prefix, on `chatroom_<id>`.
    pub const REWARD_REDEEMED: &str = "RewardRedeemedEvent";
    /// The same, prefixed, in case Kick ever sends it that way.
    pub const REWARD_REDEEMED_PREFIXED: &str = r"App\Events\RewardRedeemedEvent";
    /// A host (Kick's raid) arriving, on the receiving chatroom's `.v2` feed.
    pub const STREAM_HOST: &str = r"App\Events\StreamHostEvent";
    /// The same host in another shape, on `chatrooms.<id>`. Mapping both
    /// would count every host twice; see [`super::KickEvent::HostDuplicate`].
    pub const STREAM_HOSTED: &str = r"App\Events\StreamHostedEvent";
    /// A host leaving, on the hosting channel's `channel.<id>` feed.
    pub const CHAT_MOVE_TO_SUPPORTED_CHANNEL: &str = r"App\Events\ChatMoveToSupportedChannelEvent";
    /// The stream went live, on `channel.<id>`.
    pub const STREAMER_IS_LIVE: &str = r"App\Events\StreamerIsLive";
    /// The stream ended, on `channel.<id>`.
    pub const STOP_STREAM_BROADCAST: &str = r"App\Events\StopStreamBroadcast";
    /// A poll updated.
    pub const POLL_UPDATE: &str = r"App\Events\PollUpdateEvent";
    /// A poll removed.
    pub const POLL_DELETE: &str = r"App\Events\PollDeleteEvent";
}

/// Someone named in an event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[non_exhaustive]
pub struct UserRef {
    /// The user id, when Kick sent one.
    pub id: Option<u64>,
    /// The username, as Kick spells it (case kept).
    pub username: Option<String>,
    /// The slug, when Kick sent one.
    pub slug: Option<String>,
}

impl UserRef {
    fn from(v: &Value) -> Option<Self> {
        let u = UserRef { id: uint(v, "id"), username: string(v, "username"), slug: string(v, "slug") };
        (u.id.is_some() || u.username.is_some() || u.slug.is_some()).then_some(u)
    }
}

/// A badge in Kick's first format (`identity.badges`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Badge {
    /// `moderator`, `vip`, `og`, `broadcaster`, `founder`, `subscriber`, …
    pub kind: String,
    /// Kick's label for it.
    pub text: Option<String>,
    /// Months, on a subscriber badge.
    pub count: Option<i64>,
}

/// A badge in Kick's image format (`identity.badges_v2`). Badges the user
/// chose not to show (`selected: false`) are left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct BadgeImage {
    /// `level`, or an event badge's name.
    pub name: String,
    /// `global`, …
    pub badge_type: Option<String>,
    /// The image, on Kick's CDN.
    pub image_url: String,
    /// The chat level, on a `level` badge.
    pub level: Option<i64>,
    /// Kick's ordering.
    pub sort_order: i64,
}

/// Who sent a chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Sender {
    /// The user id. Missing on the odd malformed event; `None` then.
    pub id: Option<u64>,
    /// The username, case kept.
    pub username: String,
    /// The slug.
    pub slug: Option<String>,
    /// Their chat colour, `#RRGGBB`.
    pub color: Option<String>,
    /// Badges, first format.
    pub badges: Vec<Badge>,
    /// Badges, image format.
    pub badges_v2: Vec<BadgeImage>,
}

/// What a reply answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ReplyTo {
    /// The parent message's id.
    pub message_id: Option<String>,
    /// The parent's text.
    pub content: Option<String>,
    /// The parent's sender's id: replying in turn needs it.
    pub sender_id: Option<u64>,
    /// The parent's sender.
    pub sender_username: Option<String>,
}

/// A chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ChatMessage {
    /// The message id (a UUID).
    pub id: String,
    /// The chatroom.
    pub chatroom_id: Option<u64>,
    /// The text, emotes inline as `[emote:<id>:<name>]` (see [`crate::content`]).
    pub content: String,
    /// `message` or `reply` (others may come; kept as sent).
    pub kind: String,
    /// When, as Kick wrote it: `…+00:00`, not `Z`.
    pub created_at: Option<String>,
    /// Who.
    pub sender: Sender,
    /// What it replies to.
    pub reply_to: Option<ReplyTo>,
    /// The thread it belongs to, on a reply.
    pub thread_parent_id: Option<String>,
    /// The sender's own reference, echoed back from the send call.
    pub message_ref: Option<String>,
}

/// A Kicks gift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct KicksGift {
    /// Kick's gift id.
    pub gift_id: Option<String>,
    /// The display name ("Hell Yeah", "Rage Quit", …).
    pub name: Option<String>,
    /// Kicks spent.
    pub amount: i64,
    /// `BASIC`, `LEVEL_UP`, …
    pub kind: Option<String>,
    /// `BASIC`, `MID`, …
    pub tier: Option<String>,
    /// How long it stays pinned, seconds.
    pub pinned_time: Option<i64>,
}

/// The channel a host went to, on the hosting side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Hosted {
    /// The hosted channel's id (a channel id, not a user id).
    pub channel_id: Option<u64>,
    /// Its slug.
    pub slug: Option<String>,
    /// Its username.
    pub username: Option<String>,
    /// The viewers taken along: the hosting channel's own count at the time.
    pub viewers: Option<i64>,
    /// Whether it was live.
    pub is_live: Option<bool>,
}

/// One event from the chat feed.
///
/// Durations are kept as Kick sent them, in Kick's unit, which is not
/// confirmed everywhere: see each field and `docs/KICK.md` §8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KickEvent {
    /// A chat message.
    ChatMessage(ChatMessage),
    /// A message removed.
    MessageDeleted {
        /// Which message. The feed never carries its text.
        message_id: String,
        /// Removed by Kick's AI moderation rather than a person.
        ai_moderated: Option<bool>,
    },
    /// A ban or timeout.
    UserBanned {
        /// Who.
        user: Option<UserRef>,
        /// By whom.
        banned_by: Option<UserRef>,
        /// A ban rather than a timeout.
        permanent: bool,
        /// A timeout's length, minutes (inferred from values 1 to 5).
        duration: Option<i64>,
        /// When a timeout ends.
        expires_at: Option<String>,
    },
    /// A ban or timeout lifted.
    UserUnbanned {
        /// Who.
        user: Option<UserRef>,
        /// By whom.
        unbanned_by: Option<UserRef>,
        /// It was a ban rather than a timeout.
        permanent: bool,
    },
    /// The chat cleared.
    ChatroomCleared,
    /// A message pinned.
    PinCreated {
        /// The message, whole.
        message: Option<ChatMessage>,
        /// Who pinned it.
        pinned_by: Option<UserRef>,
        /// How long, as Kick sent it (`"1200"` so far). The unit is not
        /// settled: Kick's pin call is sent minutes, and this may be seconds.
        duration: Option<i64>,
        /// When it ends, where Kick says (the history endpoint does).
        finish_at: Option<String>,
    },
    /// The pin removed.
    PinDeleted,
    /// A subscription.
    Subscription {
        /// Who.
        username: Option<String>,
        /// For how many months.
        months: Option<i64>,
    },
    /// Gifted subscriptions.
    GiftedSubscriptions {
        /// Who gave them.
        gifter: Option<String>,
        /// Who got them.
        recipients: Vec<String>,
    },
    /// Kicks.
    KicksGifted {
        /// Who sent them.
        sender: Option<UserRef>,
        /// Their name colour.
        sender_color: Option<String>,
        /// What.
        gift: KicksGift,
        /// The sender's message; only the larger gifts allow one.
        message: Option<String>,
    },
    /// A channel-point reward redeemed.
    RewardRedeemed {
        /// Who.
        username: Option<String>,
        /// Which reward.
        reward: Option<String>,
        /// What they typed, for rewards that ask.
        input: Option<String>,
    },
    /// A host arriving here.
    Host {
        /// The hosting channel, by username only: no id, no slug, and its case
        /// can differ from the slug's.
        host_username: Option<String>,
        /// The hosting channel's viewer count at the time.
        viewers: Option<i64>,
        /// Their message, if any.
        message: Option<String>,
    },
    /// The same host as a [`KickEvent::Host`], in the other shape Kick sends
    /// it on `chatrooms.<id>`. Kept apart so it is never counted twice.
    HostDuplicate,
    /// This channel hosting another.
    HostingOut {
        /// Where to.
        hosted: Hosted,
    },
    /// The stream went live.
    StreamLive {
        /// Its title.
        title: Option<String>,
        /// Kick's livestream id.
        livestream_id: Option<u64>,
    },
    /// The stream ended.
    StreamEnded {
        /// Kick's livestream id.
        livestream_id: Option<u64>,
    },
    /// A poll changed.
    PollUpdated {
        /// The poll, as Kick sent it.
        poll: Value,
    },
    /// The poll removed.
    PollDeleted,
    /// An event this crate does not know. The name is kept; the data is the
    /// caller's to read.
    Unknown {
        /// Kick's event name.
        name: String,
    },
}

/// Reads one feed event: its name, and its `data` as Kick sent it (a JSON
/// string, or a value).
pub fn parse(name: &str, data: &Value) -> KickEvent {
    let d = lenient::decode_twice(data);
    use names::*;
    let parsed = match name {
        CHAT_MESSAGE => chat_message(&d).map(KickEvent::ChatMessage),
        MESSAGE_DELETED => get(&d, "message")
            .and_then(|m| id_string(m, "id"))
            .or_else(|| id_string(&d, "message_id"))
            .map(|message_id| KickEvent::MessageDeleted { message_id, ai_moderated: boolean(&d, "aiModerated") }),
        USER_BANNED => Some(KickEvent::UserBanned {
            user: get(&d, "user").and_then(UserRef::from),
            banned_by: get(&d, "banned_by").and_then(UserRef::from),
            permanent: boolean(&d, "permanent").unwrap_or(false),
            duration: int(&d, "duration"),
            expires_at: string(&d, "expires_at"),
        }),
        USER_UNBANNED => Some(KickEvent::UserUnbanned {
            user: get(&d, "user").and_then(UserRef::from),
            unbanned_by: get(&d, "unbanned_by").and_then(UserRef::from),
            permanent: boolean(&d, "permanent").unwrap_or(false),
        }),
        CHATROOM_CLEAR => Some(KickEvent::ChatroomCleared),
        PINNED_MESSAGE_CREATED => Some(pin(&d)),
        PINNED_MESSAGE_DELETED => Some(KickEvent::PinDeleted),
        SUBSCRIPTION => Some(KickEvent::Subscription { username: string(&d, "username"), months: int(&d, "months") }),
        GIFTED_SUBSCRIPTIONS | GIFTED_SUBSCRIPTIONS_PLAIN => Some(KickEvent::GiftedSubscriptions {
            gifter: string(&d, "gifter_username"),
            recipients: array(&d, "gifted_usernames").iter().filter_map(|u| u.as_str().map(str::to_owned)).collect(),
        }),
        KICKS_GIFTED => Some(kicks(&d)),
        REWARD_REDEEMED | REWARD_REDEEMED_PREFIXED => {
            let reward = get(&d, "reward").cloned().unwrap_or(Value::Null);
            Some(KickEvent::RewardRedeemed {
                username: string(&d, "username").or_else(|| get(&d, "user").and_then(|u| string(u, "username"))),
                reward: string(&d, "reward_title").or_else(|| string(&reward, "title")).or_else(|| string(&d, "title")),
                input: string(&d, "user_input").or_else(|| string(&d, "input")).or_else(|| string(&d, "message")),
            })
        }
        STREAM_HOST => Some(KickEvent::Host {
            host_username: string(&d, "host_username"),
            viewers: int(&d, "number_viewers"),
            message: string(&d, "optional_message").filter(|m| !m.is_empty()),
        }),
        STREAM_HOSTED => Some(KickEvent::HostDuplicate),
        CHAT_MOVE_TO_SUPPORTED_CHANNEL => {
            let h = get(&d, "hosted").cloned().unwrap_or(Value::Null);
            Some(KickEvent::HostingOut {
                hosted: Hosted {
                    channel_id: uint(&h, "id"),
                    slug: string(&h, "slug").or_else(|| string(&d, "slug")),
                    username: string(&h, "username"),
                    viewers: int(&h, "viewers_count"),
                    is_live: boolean(&h, "is_live"),
                },
            })
        }
        STREAMER_IS_LIVE => {
            let l = get(&d, "livestream").cloned().unwrap_or(Value::Null);
            Some(KickEvent::StreamLive { title: string(&l, "session_title"), livestream_id: uint(&l, "id") })
        }
        STOP_STREAM_BROADCAST => {
            Some(KickEvent::StreamEnded { livestream_id: get(&d, "livestream").and_then(|l| uint(l, "id")) })
        }
        POLL_UPDATE => Some(KickEvent::PollUpdated { poll: get(&d, "poll").cloned().unwrap_or(Value::Null) }),
        POLL_DELETE => Some(KickEvent::PollDeleted),
        _ => None,
    };
    parsed.unwrap_or_else(|| KickEvent::Unknown { name: name.to_owned() })
}

/// A chat message from its data, or `None` if it has no id, sender name or
/// text. Also reads the messages in the website API's history.
pub fn chat_message(d: &Value) -> Option<ChatMessage> {
    let sender = get(d, "sender")?;
    let identity = get(sender, "identity").cloned().unwrap_or(Value::Null);
    let metadata = get(d, "metadata").cloned().unwrap_or(Value::Null);
    Some(ChatMessage {
        id: id_string(d, "id")?,
        chatroom_id: uint(d, "chatroom_id"),
        content: string(d, "content")?,
        kind: string(d, "type").unwrap_or_else(|| "message".into()),
        created_at: string(d, "created_at"),
        sender: Sender {
            id: uint(sender, "id"),
            username: string(sender, "username")?,
            slug: string(sender, "slug"),
            color: string(&identity, "color"),
            badges: array(&identity, "badges")
                .iter()
                .filter_map(|b| {
                    Some(Badge { kind: string(b, "type")?, text: string(b, "text"), count: int(b, "count") })
                })
                .collect(),
            badges_v2: array(&identity, "badges_v2")
                .iter()
                .filter(|b| boolean(b, "selected") != Some(false))
                .filter_map(|b| {
                    Some(BadgeImage {
                        name: string(b, "name")?,
                        badge_type: string(b, "badge_type"),
                        image_url: string(b, "image_url")?,
                        level: get(b, "metadata").and_then(|m| int(m, "level")),
                        sort_order: int(b, "sort_order").unwrap_or(0),
                    })
                })
                .collect(),
        },
        reply_to: get(&metadata, "original_message").map(|original| {
            let parent_sender = get(&metadata, "original_sender").cloned().unwrap_or(Value::Null);
            ReplyTo {
                message_id: id_string(original, "id"),
                content: string(original, "content"),
                sender_id: uint(&parent_sender, "id"),
                sender_username: string(&parent_sender, "username"),
            }
        }),
        thread_parent_id: id_string(d, "thread_parent_id"),
        message_ref: id_string(&metadata, "message_ref"),
    })
}

fn pin(d: &Value) -> KickEvent {
    KickEvent::PinCreated {
        message: get(d, "message").and_then(chat_message),
        pinned_by: get(d, "pinnedBy").or_else(|| get(d, "pinned_by")).and_then(UserRef::from),
        duration: int(d, "duration"),
        finish_at: string(d, "finish_at"),
    }
}

fn kicks(d: &Value) -> KickEvent {
    let gift = get(d, "gift").cloned().unwrap_or(Value::Null);
    let sender = get(d, "sender").cloned().unwrap_or(Value::Null);
    KickEvent::KicksGifted {
        sender: UserRef::from(&sender),
        sender_color: string(&sender, "username_color"),
        gift: KicksGift {
            gift_id: id_string(&gift, "gift_id"),
            name: string(&gift, "name"),
            amount: int(&gift, "amount").unwrap_or(0),
            kind: string(&gift, "type"),
            tier: string(&gift, "tier"),
            pinned_time: int(&gift, "pinned_time"),
        },
        // Beside `gift`, not inside it.
        message: string(d, "message").filter(|m| !m.is_empty()),
    }
}

/// Reads the pinned message as the website API's history gives it (the same
/// shape as the event, plus `finish_at`).
pub(crate) fn pin_from_history(d: &Value) -> Option<KickEvent> {
    match pin(d) {
        KickEvent::PinCreated { message: None, .. } => None,
        event => Some(event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_events_are_kept_by_name() {
        assert_eq!(
            parse("App\\Events\\Whatever", &json!("{}")),
            KickEvent::Unknown { name: "App\\Events\\Whatever".into() }
        );
    }

    #[test]
    fn a_message_without_text_is_unknown_rather_than_an_error() {
        let data = json!({"id": "x", "sender": {"username": "a"}});
        assert!(matches!(parse(names::CHAT_MESSAGE, &data), KickEvent::Unknown { .. }));
    }

    #[test]
    fn kicks_message_is_beside_the_gift() {
        let data = json!({"sender": {"id": 1, "username": "a", "username_color": "#fff"},
            "gift": {"gift_id": "rage_quit", "name": "Rage Quit", "amount": 500, "type": "LEVEL_UP", "tier": "MID", "pinned_time": 600},
            "message": "gg"});
        let KickEvent::KicksGifted { gift, message, .. } = parse(names::KICKS_GIFTED, &data) else { panic!() };
        assert_eq!(gift.amount, 500);
        assert_eq!(message.as_deref(), Some("gg"));
    }

    #[test]
    fn hidden_badges_are_left_out() {
        let data = json!({"id": "1", "content": "hi", "sender": {"id": 2, "username": "a", "identity": {"badges_v2": [
            {"name": "level", "image_url": "u", "selected": false},
            {"name": "sub", "image_url": "v", "metadata": {"level": 3}}]}}});
        let KickEvent::ChatMessage(m) = parse(names::CHAT_MESSAGE, &data) else { panic!() };
        assert_eq!(m.sender.badges_v2.len(), 1);
        assert_eq!(m.sender.badges_v2[0].level, Some(3));
    }
}
