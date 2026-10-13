//! Kick (kick.com) without the I/O.
//!
//! `kicksdk` knows Kick: which requests to make, what the answers look like,
//! how the realtime chat feed speaks, and how webhooks are signed. It never
//! opens a socket or sends a request itself. The host does that, with whatever
//! it has: `fetch` and `WebSocket` in a browser or an extension, Mint or Gun
//! under Elixir, reqwest and tokio-tungstenite in a Rust service. That is what
//! lets one implementation run everywhere, and it is the only way some of it
//! can work at all: Kick's realtime server only accepts a socket whose Origin
//! is `https://kick.com`, which in a browser means a socket opened by a
//! kick.com page.
//!
//! The pieces:
//!
//! - [`http`]: a [`Request`] to send, and the [`Error`] a parser returns
//!   when Kick's answer is not what it should be.
//! - [`public_api`]: Kick's official API (`api.kick.com/public/v1`) and the
//!   app token.
//! - [`website`]: the endpoints kick.com's own frontend uses (`kick.com/api/v2`):
//!   the chatroom id, follower totals, chat history, and moderation with a
//!   viewer's session.
//! - [`realtime`]: the chat feed: where to connect, the guest token, and a
//!   [`Session`](realtime::Session) state machine for Kick's Centrifugo
//!   protocol.
//! - [`events`]: Kick's chat-feed events, typed.
//! - [`content`]: chat message text, with its `[emote:id:name]` tokens.
//! - [`webhooks`]: the official webhooks: headers, signature and bodies.
//! - `dispatch` (feature `dispatch`): every function by name, JSON in and out,
//!   for bindings that cross a C ABI or a dynamic language.
//!
//! Everything here is grounded in `docs/KICK.md`, which records each fact
//! about Kick with where it was seen. When Kick changes, that file changes
//! first, then the fixtures, then this code.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod content;
#[cfg(feature = "dispatch")]
pub mod dispatch;
pub mod events;
pub mod http;
pub mod ids;
mod lenient;
pub mod public_api;
pub mod realtime;
#[cfg(feature = "webhooks")]
pub mod webhooks;
pub mod website;

pub use events::KickEvent;
pub use http::{Endpoints, Error, Method, Request};
pub use ids::{ChannelId, ChatroomId, UserId};
