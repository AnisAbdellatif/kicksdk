//! Kick's three numeric ids, kept apart.
//!
//! One channel has three different numbers, and passing one where another
//! belongs fails quietly - the request just finds nothing:
//!
//! - the **user id** (`broadcaster_user_id`): what the official API and the
//!   webhooks use;
//! - the **channel id**: `/livestreams`' `channel_id`, the website API's `id`,
//!   the `channel.<id>` feeds, the realtime connection endpoint;
//! - the **chatroom id**: the website API's `chatroom.id`, the chat feeds.
//!   No official endpoint gives it.
//!
//! On some channels two of them happen to be equal. Never rely on it.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<u64> for $name {
            fn from(v: u64) -> Self {
                Self(v)
            }
        }
    };
}

id!(
    /// A user's id: `broadcaster_user_id` in the official API and webhooks.
    UserId
);
id!(
    /// A channel's id: `channel_id` in `/livestreams`, `id` in the website API.
    ChannelId
);
id!(
    /// A chatroom's id: `chatroom.id` in the website API.
    ChatroomId
);
