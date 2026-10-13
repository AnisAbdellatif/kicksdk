//! Kick's official webhooks: the headers, the signature, the bodies.
//!
//! A delivery is a POST with the event in the body and the rest in headers
//! ([`Headers::from_pairs`]). Kick signs `<message id>.<timestamp>.<raw body>`
//! with RSA (SHA-256, PKCS#1 v1.5); [`verify`] checks it against the key from
//! [`crate::public_api::public_key_request`]. **The type, version and
//! subscription-id headers are not signed**: trust them only as far as the
//! body agrees.
//!
//! What to expect (`docs/KICK.md` §4): the same event can arrive **twice**
//! (key on the message id); a failed delivery has **not** been seen to be
//! retried, so treat it as lost; metadata and follow bodies carry **no time**
//! (use the timestamp header); a renewal's `created_at` is when the
//! subscription first started.

use crate::lenient::{array, boolean, get, int, string, uint};
use base64::Engine;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use serde::Serialize;
use serde_json::Value;

/// The headers of one delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Headers {
    /// `Kick-Event-Message-Id`: a ULID. Deduplicate on it.
    pub message_id: String,
    /// `Kick-Event-Message-Timestamp`: RFC 3339, second precision.
    pub timestamp: String,
    /// `Kick-Event-Signature`: base64.
    pub signature: String,
    /// `Kick-Event-Type` (unsigned).
    pub event_type: String,
    /// `Kick-Event-Version` (unsigned).
    pub version: String,
    /// `Kick-Event-Subscription-Id` (unsigned).
    pub subscription_id: Option<String>,
}

/// Why a delivery was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebhookError {
    /// A header is missing.
    #[error("missing header {0}")]
    MissingHeader(&'static str),
    /// The public key could not be read.
    #[error("unreadable public key")]
    BadKey,
    /// The signature is not base64 or not a signature.
    #[error("malformed signature")]
    MalformedSignature,
    /// The signature does not match.
    #[error("signature does not match")]
    Mismatch,
}

impl Headers {
    /// Reads the headers from (name, value) pairs, names in any case.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Self, WebhookError> {
        let mut found: [Option<String>; 6] = Default::default();
        const NAMES: [&str; 6] = [
            "kick-event-message-id",
            "kick-event-message-timestamp",
            "kick-event-signature",
            "kick-event-type",
            "kick-event-version",
            "kick-event-subscription-id",
        ];
        for (name, value) in pairs {
            if let Some(i) = NAMES.iter().position(|n| n.eq_ignore_ascii_case(name)) {
                found[i] = Some(value.to_owned());
            }
        }
        let [id, ts, sig, ty, ver, sub] = found;
        Ok(Headers {
            message_id: id.ok_or(WebhookError::MissingHeader(NAMES[0]))?,
            timestamp: ts.ok_or(WebhookError::MissingHeader(NAMES[1]))?,
            signature: sig.ok_or(WebhookError::MissingHeader(NAMES[2]))?,
            event_type: ty.ok_or(WebhookError::MissingHeader(NAMES[3]))?,
            version: ver.ok_or(WebhookError::MissingHeader(NAMES[4]))?,
            subscription_id: sub,
        })
    }
}

/// Checks a delivery's signature. `body` is the raw body, byte for byte.
pub fn verify(public_key_pem: &str, headers: &Headers, body: &[u8]) -> Result<(), WebhookError> {
    // Kick serves SubjectPublicKeyInfo ("BEGIN PUBLIC KEY"); PKCS#1 is read too.
    let pem = public_key_pem.trim();
    let key = rsa::RsaPublicKey::from_public_key_pem(pem)
        .or_else(|_| rsa::RsaPublicKey::from_pkcs1_pem(pem))
        .map_err(|_| WebhookError::BadKey)?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(headers.signature.trim())
        .map_err(|_| WebhookError::MalformedSignature)?;
    let signature = Signature::try_from(raw.as_slice()).map_err(|_| WebhookError::MalformedSignature)?;
    let mut signed = Vec::with_capacity(headers.message_id.len() + headers.timestamp.len() + body.len() + 2);
    signed.extend_from_slice(headers.message_id.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(headers.timestamp.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    VerifyingKey::<sha2::Sha256>::new(key).verify(&signed, &signature).map_err(|_| WebhookError::Mismatch)
}

/// Someone in a webhook body: broadcaster, follower, subscriber, gifter,
/// giftee, sender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Person {
    /// Their user id. Null for an anonymous gifter.
    pub user_id: Option<u64>,
    /// Username. Null for an anonymous gifter.
    pub username: Option<String>,
    /// Channel slug.
    pub channel_slug: Option<String>,
    /// Verified.
    pub is_verified: Option<bool>,
    /// Anonymous (gifts).
    pub is_anonymous: Option<bool>,
    /// Their picture.
    pub profile_picture: Option<String>,
}

impl Person {
    fn from(v: Option<&Value>) -> Option<Self> {
        let v = v?;
        Some(Person {
            user_id: uint(v, "user_id"),
            username: string(v, "username"),
            channel_slug: string(v, "channel_slug"),
            is_verified: boolean(v, "is_verified"),
            is_anonymous: boolean(v, "is_anonymous"),
            profile_picture: string(v, "profile_picture"),
        })
    }
}

/// A category in a metadata update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Category {
    /// Kick's id.
    pub id: Option<u64>,
    /// Its name.
    pub name: Option<String>,
    /// Its picture.
    pub thumbnail: Option<String>,
}

/// One webhook event, typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum WebhookEvent {
    /// `livestream.status.updated`: start (`is_live`, no `ended_at`) and end
    /// are one type. Both carry the same `started_at`.
    LivestreamStatus {
        /// The channel.
        broadcaster: Option<Person>,
        /// Live now.
        is_live: bool,
        /// The title at that moment (not a source of title changes).
        title: Option<String>,
        /// The stream's start: with the channel, its identity.
        started_at: Option<String>,
        /// The end, on an end.
        ended_at: Option<String>,
    },
    /// `livestream.metadata.updated`: a full snapshot; compare with the last
    /// one to see what changed.
    LivestreamMetadata {
        /// The channel.
        broadcaster: Option<Person>,
        /// The title.
        title: Option<String>,
        /// The language.
        language: Option<String>,
        /// Mature.
        has_mature_content: Option<bool>,
        /// The category (sent as `category` and `Category`; same value).
        category: Option<Category>,
    },
    /// `channel.followed`.
    Followed {
        /// The channel.
        broadcaster: Option<Person>,
        /// Who followed.
        follower: Option<Person>,
    },
    /// `channel.subscription.new` and `.renewal`.
    Subscription {
        /// A renewal rather than a new subscription.
        renewal: bool,
        /// The channel.
        broadcaster: Option<Person>,
        /// Who.
        subscriber: Option<Person>,
        /// Months (1 for a new one).
        duration: Option<i64>,
        /// On a renewal, when the subscription first started.
        created_at: Option<String>,
        /// When it runs out.
        expires_at: Option<String>,
    },
    /// `channel.subscription.gifts`: 1 to 25 giftees seen in one.
    SubscriptionGifts {
        /// The channel.
        broadcaster: Option<Person>,
        /// Who gave; `is_anonymous` with nulls for an anonymous gift.
        gifter: Option<Person>,
        /// Who got them.
        giftees: Vec<Person>,
        /// When.
        created_at: Option<String>,
        /// When they run out.
        expires_at: Option<String>,
    },
    /// `kicks.gifted`.
    KicksGifted {
        /// The channel.
        broadcaster: Option<Person>,
        /// Who sent them.
        sender: Option<Person>,
        /// Kicks spent.
        amount: Option<i64>,
        /// The gift's name.
        name: Option<String>,
        /// `BASIC`, `LEVEL_UP`, …
        kind: Option<String>,
        /// `BASIC`, `MID`, …
        tier: Option<String>,
        /// The sender's message.
        message: Option<String>,
        /// How long it is pinned, seconds.
        pinned_time_seconds: Option<i64>,
        /// When.
        created_at: Option<String>,
    },
    /// A type this crate does not read yet (`moderation.banned`,
    /// `channel.reward.redemption.updated`, `chat.message.sent`, and anything
    /// new): the body as sent.
    Other {
        /// The type header.
        event_type: String,
        /// The body.
        body: Value,
    },
}

/// Reads a delivery's body, by its type header.
pub fn parse(event_type: &str, body: &str) -> Result<WebhookEvent, serde_json::Error> {
    let v: Value = serde_json::from_str(body)?;
    let broadcaster = || Person::from(get(&v, "broadcaster"));
    Ok(match event_type {
        "livestream.status.updated" => WebhookEvent::LivestreamStatus {
            broadcaster: broadcaster(),
            is_live: boolean(&v, "is_live").unwrap_or(false),
            title: string(&v, "title"),
            started_at: string(&v, "started_at"),
            ended_at: string(&v, "ended_at"),
        },
        "livestream.metadata.updated" => {
            let m = get(&v, "metadata").cloned().unwrap_or_default();
            let c = get(&m, "category").or_else(|| get(&m, "Category"));
            WebhookEvent::LivestreamMetadata {
                broadcaster: broadcaster(),
                title: string(&m, "title"),
                language: string(&m, "language"),
                has_mature_content: boolean(&m, "has_mature_content"),
                category: c.map(|c| Category {
                    id: uint(c, "id"),
                    name: string(c, "name"),
                    thumbnail: string(c, "thumbnail"),
                }),
            }
        }
        "channel.followed" => {
            WebhookEvent::Followed { broadcaster: broadcaster(), follower: Person::from(get(&v, "follower")) }
        }
        "channel.subscription.new" | "channel.subscription.renewal" => WebhookEvent::Subscription {
            renewal: event_type.ends_with("renewal"),
            broadcaster: broadcaster(),
            subscriber: Person::from(get(&v, "subscriber")),
            duration: int(&v, "duration"),
            created_at: string(&v, "created_at"),
            expires_at: string(&v, "expires_at"),
        },
        "channel.subscription.gifts" => WebhookEvent::SubscriptionGifts {
            broadcaster: broadcaster(),
            gifter: Person::from(get(&v, "gifter")),
            giftees: array(&v, "giftees").iter().filter_map(|g| Person::from(Some(g))).collect(),
            created_at: string(&v, "created_at"),
            expires_at: string(&v, "expires_at"),
        },
        "kicks.gifted" => {
            let g = get(&v, "gift").cloned().unwrap_or_default();
            WebhookEvent::KicksGifted {
                broadcaster: broadcaster(),
                sender: Person::from(get(&v, "sender")),
                amount: int(&g, "amount"),
                name: string(&g, "name"),
                kind: string(&g, "type"),
                tier: string(&g, "tier"),
                message: string(&g, "message"),
                pinned_time_seconds: int(&g, "pinned_time_seconds"),
                created_at: string(&v, "created_at"),
            }
        }
        other => WebhookEvent::Other { event_type: other.to_owned(), body: v },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_in_any_case_and_missing_ones_named() {
        let pairs = [
            ("Kick-Event-Message-Id", "01M"),
            ("kick-event-message-timestamp", "2026-09-24T02:35:42Z"),
            ("KICK-EVENT-SIGNATURE", "c2ln"),
            ("kick-event-type", "channel.followed"),
            ("kick-event-version", "1"),
        ];
        let h = Headers::from_pairs(pairs).unwrap();
        assert_eq!(h.event_type, "channel.followed");
        assert_eq!(h.subscription_id, None);
        assert_eq!(
            Headers::from_pairs(pairs[1..].iter().copied()),
            Err(WebhookError::MissingHeader("kick-event-message-id"))
        );
    }
}
