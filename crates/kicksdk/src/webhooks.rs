//! Kick's official webhooks: the headers, the signature, the bodies.
//!
//! A delivery is a POST with the event in the body and the rest in headers
//! ([`Headers::from_pairs`]). Kick signs `<message id>.<timestamp>.<raw body>`
//! with RSA (SHA-256, PKCS#1 v1.5); [`verify`] checks it against the key from
//! [`crate::public_api::public_key_request`], read once into a [`PublicKey`].
//!
//! What a valid signature means, and what it does not:
//!
//! - **Kick sent this body, at that timestamp, under that message id, to
//!   some app.** Not necessarily to yours: every app gets the same key, so
//!   anyone who receives deliveries can replay one to you. Only the message
//!   id, the timestamp and the body are signed.
//! - **Replays**: [`verify`] refuses a timestamp more than
//!   [`DEFAULT_TOLERANCE_SECS`] (5 minutes) from `now` ([`Freshness::at`]),
//!   and the receiver should also keep the message ids it has seen for at
//!   least that long and drop repeats - which Kick sends anyway (below).
//!   [`Freshness::Skip`] is for re-checking deliveries stored earlier.
//! - **The type, version and subscription-id headers are not signed.**
//!   [`parse`] reads the body by the type header and refuses a body that is
//!   not that type's shape, so a delivery cannot be passed off as another
//!   kind. Two cases it cannot tell apart: `channel.subscription.new` and
//!   `.renewal` have one shape, and a type this crate does not know comes
//!   out as [`WebhookEvent::Other`] with the header's claim unchecked.
//! - The message id must be a ULID and the timestamp RFC 3339
//!   ([`WebhookError::MalformedHeader`]). Neither can then hold the `.`
//!   that separates them in the signed text, so no header can move bytes
//!   between the id, the timestamp and the body.
//!
//! What to expect (`docs/KICK.md` §4): the same event can arrive **twice**
//! (key on the message id); a failed delivery has **not** been seen to be
//! retried, so treat it as lost; metadata and follow bodies carry **no time**
//! (use the timestamp header); a renewal's `created_at` is when the
//! subscription first started.
//!
//! The RSA here is the `rsa` crate's, used for verifying only.
//! RUSTSEC-2023-0071 (the Marvin timing attack) is about operations with the
//! private key, which this crate never has; see `deny.toml`.

use crate::http::Error;
use crate::lenient::{array, boolean, get, int, string, uint};
use base64::Engine;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

/// How far a delivery's timestamp may be from now, either way, by default:
/// 5 minutes. Kick's deliveries arrive within a second or two.
pub const DEFAULT_TOLERANCE_SECS: u64 = 300;

const NAMES: [&str; 6] = [
    "kick-event-message-id",
    "kick-event-message-timestamp",
    "kick-event-signature",
    "kick-event-type",
    "kick-event-version",
    "kick-event-subscription-id",
];

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
    /// A header is not in its format: the message id a ULID, the timestamp
    /// RFC 3339.
    #[error("malformed header {0}")]
    MalformedHeader(&'static str),
    /// The public key could not be read.
    #[error("unreadable public key")]
    BadKey,
    /// The signature is not base64 or not a signature.
    #[error("malformed signature")]
    MalformedSignature,
    /// The signature does not match.
    #[error("signature does not match")]
    Mismatch,
    /// The timestamp is further from now than the tolerance: a replay, or a
    /// clock that is off.
    #[error("the delivery's timestamp is {offset_secs} s from now, beyond the tolerance")]
    Stale {
        /// The timestamp less now, seconds: negative for the past.
        offset_secs: i64,
    },
}

impl Headers {
    /// Reads the headers from (name, value) pairs, names in any case. The
    /// message id must be a ULID and the timestamp RFC 3339.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Self, WebhookError> {
        let mut found: [Option<String>; 6] = Default::default();
        for (name, value) in pairs {
            if let Some(i) = NAMES.iter().position(|n| n.eq_ignore_ascii_case(name)) {
                found[i] = Some(value.to_owned());
            }
        }
        let [id, ts, sig, ty, ver, sub] = found;
        let headers = Headers {
            message_id: id.ok_or(WebhookError::MissingHeader(NAMES[0]))?,
            timestamp: ts.ok_or(WebhookError::MissingHeader(NAMES[1]))?,
            signature: sig.ok_or(WebhookError::MissingHeader(NAMES[2]))?,
            event_type: ty.ok_or(WebhookError::MissingHeader(NAMES[3]))?,
            version: ver.ok_or(WebhookError::MissingHeader(NAMES[4]))?,
            subscription_id: sub,
        };
        headers.check()?;
        Ok(headers)
    }

    /// The signed headers in their formats; the timestamp as Unix seconds.
    fn check(&self) -> Result<i64, WebhookError> {
        if !is_ulid(&self.message_id) {
            return Err(WebhookError::MalformedHeader(NAMES[0]));
        }
        rfc3339_secs(&self.timestamp).ok_or(WebhookError::MalformedHeader(NAMES[1]))
    }
}

/// 26 characters of Crockford's base32, the first at most `7`.
fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes().enumerate().all(|(i, b)| {
            let b = b.to_ascii_uppercase();
            match i {
                0 => (b'0'..=b'7').contains(&b),
                _ => b.is_ascii_digit() || (b.is_ascii_uppercase() && !matches!(b, b'I' | b'L' | b'O' | b'U')),
            }
        })
}

/// An RFC 3339 date-time (`2026-09-24T02:35:42Z`, a fraction and an offset
/// allowed), as Unix seconds.
fn rfc3339_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter().all(u8::is_ascii_digit).then(|| part.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let sep = |at: usize, c: &[u8]| b.get(at).is_some_and(|x| c.contains(x));
    if !(sep(4, b"-") && sep(7, b"-") && sep(10, b"Tt") && sep(13, b":") && sep(16, b":")) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let mut rest = 19;
    if sep(rest, b".") {
        let digits = b[rest + 1..].iter().take_while(|d| d.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        rest += 1 + digits;
    }
    let offset = match b.get(rest..)? {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let (h, m) = (num(rest + 1, 2)?, num(rest + 4, 2)?);
            if h > 23 || m > 59 {
                return None;
            }
            (h * 60 + m) * 60 * if *sign == b'-' { -1 } else { 1 }
        }
        _ => return None,
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days_in_month || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Days from 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// Kick's webhook key, read once.
#[derive(Clone)]
pub struct PublicKey(VerifyingKey<sha2::Sha256>);

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        AsRef::<rsa::RsaPublicKey>::as_ref(&self.0) == other.0.as_ref()
    }
}

impl Eq for PublicKey {}

impl PublicKey {
    /// Reads a PEM key: SubjectPublicKeyInfo (`BEGIN PUBLIC KEY`, what Kick
    /// serves) or PKCS#1 (`BEGIN RSA PUBLIC KEY`).
    pub fn from_pem(pem: &str) -> Result<Self, WebhookError> {
        let pem = pem.trim();
        let key = rsa::RsaPublicKey::from_public_key_pem(pem)
            .or_else(|_| rsa::RsaPublicKey::from_pkcs1_pem(pem))
            .map_err(|_| WebhookError::BadKey)?;
        Ok(Self(VerifyingKey::new(key)))
    }
}

impl std::str::FromStr for PublicKey {
    type Err = WebhookError;
    fn from_str(pem: &str) -> Result<Self, WebhookError> {
        Self::from_pem(pem)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(RSA)")
    }
}

/// Whether, and how, [`verify`] checks the delivery's timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Freshness {
    /// The timestamp must be at most `tolerance_secs` from `now`, before or
    /// after.
    Within {
        /// Now, Unix seconds.
        now: u64,
        /// How far off the timestamp may be, seconds.
        tolerance_secs: u64,
    },
    /// No check: for deliveries stored earlier and checked again. A live
    /// receiver should never skip it.
    Skip,
}

impl Freshness {
    /// Within [`DEFAULT_TOLERANCE_SECS`] of `now` (Unix seconds).
    pub fn at(now: u64) -> Self {
        Freshness::Within { now, tolerance_secs: DEFAULT_TOLERANCE_SECS }
    }
}

/// Checks a delivery: the headers' formats, the timestamp's freshness, and
/// the signature. `body` is the raw body, byte for byte (`&[u8]`, `&str`,
/// `Vec<u8>`, …): not re-encoded, and not necessarily UTF-8.
pub fn verify(
    key: &PublicKey,
    headers: &Headers,
    body: impl AsRef<[u8]>,
    freshness: Freshness,
) -> Result<(), WebhookError> {
    let timestamp = headers.check()?;
    if let Freshness::Within { now, tolerance_secs } = freshness {
        let offset = i128::from(timestamp) - i128::from(now);
        if offset.unsigned_abs() > u128::from(tolerance_secs) {
            let offset_secs = i64::try_from(offset).unwrap_or(if offset < 0 { i64::MIN } else { i64::MAX });
            return Err(WebhookError::Stale { offset_secs });
        }
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(headers.signature.trim())
        .map_err(|_| WebhookError::MalformedSignature)?;
    let signature = Signature::try_from(raw.as_slice()).map_err(|_| WebhookError::MalformedSignature)?;
    let body = body.as_ref();
    let mut signed = Vec::with_capacity(headers.message_id.len() + headers.timestamp.len() + body.len() + 2);
    signed.extend_from_slice(headers.message_id.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(headers.timestamp.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);
    key.0.verify(&signed, &signature).map_err(|_| WebhookError::Mismatch)
}

/// [`verify`] with the key as PEM, read on every call. A receiver checking
/// many deliveries should read it once with [`PublicKey::from_pem`].
pub fn verify_pem(
    public_key_pem: &str,
    headers: &Headers,
    body: impl AsRef<[u8]>,
    freshness: Freshness,
) -> Result<(), WebhookError> {
    verify(&PublicKey::from_pem(public_key_pem)?, headers, body, freshness)
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

/// The kinds of body this crate reads, by the field only that kind has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Status,
    Metadata,
    Followed,
    Subscription,
    Gifts,
    Kicks,
}

impl Shape {
    const ALL: [Shape; 6] =
        [Shape::Status, Shape::Metadata, Shape::Followed, Shape::Subscription, Shape::Gifts, Shape::Kicks];

    fn of_type(event_type: &str) -> Option<Shape> {
        Some(match event_type {
            "livestream.status.updated" => Shape::Status,
            "livestream.metadata.updated" => Shape::Metadata,
            "channel.followed" => Shape::Followed,
            "channel.subscription.new" | "channel.subscription.renewal" => Shape::Subscription,
            "channel.subscription.gifts" => Shape::Gifts,
            "kicks.gifted" => Shape::Kicks,
            _ => return None,
        })
    }

    /// Whether `v` has this kind's own field.
    fn fits(self, v: &Value) -> bool {
        let object = |key| get(v, key).is_some_and(Value::is_object);
        match self {
            Shape::Status => boolean(v, "is_live").is_some(),
            // moderation.banned has a `metadata` too, of other fields.
            Shape::Metadata => get(v, "metadata").is_some_and(|m| {
                m.is_object()
                    && ["title", "language", "has_mature_content", "category"].iter().any(|k| m.get(k).is_some())
            }),
            Shape::Followed => object("follower"),
            Shape::Subscription => object("subscriber"),
            Shape::Gifts => get(v, "giftees").is_some_and(Value::is_array),
            Shape::Kicks => object("gift"),
        }
    }
}

/// Reads a delivery's body (raw bytes or text) by its type header.
///
/// The type header is not signed, so the body must be that type's shape:
/// have the type's own field (`follower` for a follow, `is_live` for a
/// status, …) and no other type's. Otherwise [`Error::Shape`]; not JSON,
/// [`Error::Json`]. A type this crate does not read comes out as
/// [`WebhookEvent::Other`], unchecked.
pub fn parse(event_type: &str, body: impl AsRef<[u8]>) -> Result<WebhookEvent, Error> {
    let v: Value = serde_json::from_slice(body.as_ref())?;
    if !v.is_object() {
        return Err(Error::shape("a webhook body is an object"));
    }
    if let Some(claimed) = Shape::of_type(event_type) {
        let fits: Vec<Shape> = Shape::ALL.into_iter().filter(|s| s.fits(&v)).collect();
        if fits != [claimed] {
            return Err(Error::shape(format!("the body is not a {event_type}")));
        }
    }
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

    const ID: &str = "01M38M9K3YRKV8E6Q68C18ZWG4";

    #[test]
    fn headers_in_any_case_and_missing_ones_named() {
        let pairs = [
            ("Kick-Event-Message-Id", ID),
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

    #[test]
    fn signed_headers_must_be_in_their_formats() {
        let with = |id: &str, ts: &str| {
            Headers::from_pairs([
                ("kick-event-message-id", id),
                ("kick-event-message-timestamp", ts),
                ("kick-event-signature", "c2ln"),
                ("kick-event-type", "channel.followed"),
                ("kick-event-version", "1"),
            ])
        };
        assert!(with(ID, "2026-09-24T02:35:42Z").is_ok());
        assert!(with(&ID.to_lowercase(), "2026-09-24t02:35:42.123456+02:00").is_ok());
        let bad_id = Err(WebhookError::MalformedHeader("kick-event-message-id"));
        for id in [
            "",
            "01M",
            "01M38M9K3YRKV8E6Q68C18ZWG4X",
            "81M38M9K3YRKV8E6Q68C18ZWG4",
            "01M38M9K3YRKV8E6Q68C18ZWG.",
            "01M38M9K3YRKV8E6Q68C18ZWGI",
        ] {
            assert_eq!(with(id, "2026-09-24T02:35:42Z"), bad_id, "{id:?}");
        }
        let bad_ts = Err(WebhookError::MalformedHeader("kick-event-message-timestamp"));
        for ts in [
            "",
            "2026-09-24T02:35:42",
            "2026-09-24T02:35:42Z.",
            "2026-09-24T02:35:42Z.x",
            "2026-09-24 02:35:42Z",
            "2026-02-30T02:35:42Z",
            "2026-09-24T24:00:00Z",
            "2026-09-24T02:35:42.Z",
            "2026-09-24T02:35:42+0200",
            "1758681342",
        ] {
            assert_eq!(with(ID, ts), bad_ts, "{ts:?}");
        }
    }

    #[test]
    fn rfc3339_as_unix_seconds() {
        assert_eq!(rfc3339_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_secs("2026-09-24T02:35:42Z"), Some(1_790_217_342));
        assert_eq!(rfc3339_secs("2026-09-24T04:35:42.5+02:00"), Some(1_790_217_342));
        assert_eq!(rfc3339_secs("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(rfc3339_secs("2023-02-29T00:00:00Z"), None);
        assert_eq!(rfc3339_secs("1969-12-31T23:59:59Z"), Some(-1));
    }

    #[test]
    fn a_body_must_be_its_types_shape() {
        let follow = r#"{"broadcaster":{"user_id":1},"follower":{"user_id":2}}"#;
        assert!(matches!(parse("channel.followed", follow), Ok(WebhookEvent::Followed { .. })));
        assert!(matches!(parse("channel.followed", follow.as_bytes()), Ok(WebhookEvent::Followed { .. })));
        for other in [
            "livestream.status.updated",
            "livestream.metadata.updated",
            "channel.subscription.new",
            "channel.subscription.gifts",
            "kicks.gifted",
        ] {
            assert!(matches!(parse(other, follow), Err(Error::Shape(_))), "{other}");
        }
        let status = r#"{"broadcaster":{},"is_live":true,"started_at":"x","follower":{}}"#;
        assert!(matches!(parse("livestream.status.updated", status), Err(Error::Shape(_))), "two shapes at once");
        let banned = r#"{"broadcaster":{},"moderator":{},"banned_user":{},"metadata":{"reason":"x"}}"#;
        assert!(matches!(parse("livestream.metadata.updated", banned), Err(Error::Shape(_))));
        assert!(matches!(parse("moderation.banned", banned), Ok(WebhookEvent::Other { .. })));
        assert!(matches!(parse("channel.followed", "[]"), Err(Error::Shape(_))));
        assert!(matches!(parse("channel.followed", "{"), Err(Error::Json(_))));
        assert!(matches!(parse("channel.followed", b"\xff"), Err(Error::Json(_))));
    }

    /// A throwaway 1024-bit key made for this test alone, so a delivery can
    /// be signed here: small, because only the arithmetic is under test.
    const TEST_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIICdwIBADANBgkqhkiG9w0BAQEFAASCAmEwggJdAgEAAoGBAKTXCWzDTIcS22mQ\nfM9o95xMQjMdqRybYz//5F/Hb/jBkamd/N1C2HpGI/4Zuf9LPVdtsu5k9P3VSmFn\nrh1ajRF1FVv89Ak2ZmUhZ675KeRbJploDwf5rKvDWoKFkDo7ZDqrzZ1OicU5KTKD\nIUMtq6bzzxvNLqDn0w7eZXNd6yNzAgMBAAECgYEAnDH3CEx283b22azD13VLS5sn\nD3CHkkfbYAWFzQwaGcix0duWg8KXhiV4bYrLKBGPaozUA/k9/FhjRrEL11y6Vc7G\nDxc+1cSg/Vkqm2Uhd5GwrSUvymz074MPt3tbuQsEJTt72JR5Wcvmj0V3VmeD2wIS\nnHVjZ1gJdTSGRxnVgQECQQDWZ60FaLSBSByW575Bq/qMrINQRY6iFn0TFRL9MTtv\nAAnksnGIf8kQkQsiH2trNioJv/v10chlgdDpqfdhZd+TAkEAxNG9BvQ7rbg4s8G7\nMVzIb2nqNOMd55NtO74tlBDANZb19gwh3LAKrkHNMpQSkOYI/CQ0PSL7H3WUb3dR\ncWVYoQJBAJH3CTL6gpDVUCloYf2BoNRuigN3J1CwMLxyB3h6MPUzuIiK94IvROpT\nZJn5e/8EZAA5RMao6F46z8iUXwde/KMCQG82tMFBMJexCRlJ1yTFcNzD1Hud+j/o\npkij5c1Myc9+Re2KXXf+YYql0/VIhlUqJtmS+5GwOoZEQmPZ4ENbKQECQGWzNgyw\nzCq2MXXCjHhj/r8zXrq+05vASZIz1Qan9CSkoxnm3SR1DwPJK6msvrrC8qieLjYY\nGG1ADCau3QXMLV0=\n-----END PRIVATE KEY-----";

    fn signed(id: &str, ts: &str, body: &[u8]) -> (PublicKey, Headers) {
        use rsa::pkcs8::DecodePrivateKey;
        use rsa::signature::{SignatureEncoding, Signer};
        let private = rsa::RsaPrivateKey::from_pkcs8_pem(TEST_PRIVATE_KEY).unwrap();
        let public = PublicKey(VerifyingKey::new(private.to_public_key()));
        let mut text = format!("{id}.{ts}.").into_bytes();
        text.extend_from_slice(body);
        let signature = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(private).sign(&text).to_vec();
        let signature = base64::engine::general_purpose::STANDARD.encode(signature);
        let headers = Headers::from_pairs([
            ("kick-event-message-id", id),
            ("kick-event-message-timestamp", ts),
            ("kick-event-signature", signature.as_str()),
            ("kick-event-type", "channel.followed"),
            ("kick-event-version", "1"),
        ])
        .unwrap();
        (public, headers)
    }

    #[test]
    fn bodies_verify_byte_for_byte_and_fresh() {
        let at = 1_790_217_342; // 2026-09-24T02:35:42Z
        // Not UTF-8: verified as the bytes that came.
        let body = b"{\"x\":\"\xff\xfe\"}";
        let (key, headers) = signed(ID, "2026-09-24T02:35:42Z", body);
        assert_eq!(verify(&key, &headers, body, Freshness::at(at)), Ok(()));
        assert_eq!(verify(&key, &headers, Vec::from(&body[..]), Freshness::at(at + 300)), Ok(()));
        assert_eq!(verify(&key, &headers, body, Freshness::at(at - 300)), Ok(()));
        assert_eq!(
            verify(&key, &headers, body, Freshness::at(at + 301)),
            Err(WebhookError::Stale { offset_secs: -301 })
        );
        assert_eq!(
            verify(&key, &headers, body, Freshness::at(at - 301)),
            Err(WebhookError::Stale { offset_secs: 301 })
        );
        let tight = Freshness::Within { now: at + 10, tolerance_secs: 5 };
        assert_eq!(verify(&key, &headers, body, tight), Err(WebhookError::Stale { offset_secs: -10 }));
        // A replay of a stored delivery, on purpose.
        assert_eq!(verify(&key, &headers, body, Freshness::Skip), Ok(()));
        assert_eq!(verify(&key, &headers, b"{}", Freshness::Skip), Err(WebhookError::Mismatch));
        // Fields set by hand are checked as from_pairs checks them.
        let mut moved = headers.clone();
        moved.timestamp = "2026-09-24T02:35:42Z.{".into();
        assert_eq!(
            verify(&key, &moved, body, Freshness::Skip),
            Err(WebhookError::MalformedHeader("kick-event-message-timestamp"))
        );
        let mut moved = headers.clone();
        moved.message_id = format!("{ID}.2026");
        assert_eq!(
            verify(&key, &moved, body, Freshness::Skip),
            Err(WebhookError::MalformedHeader("kick-event-message-id"))
        );
        let mut garbled = headers;
        garbled.signature = "not base64!".into();
        assert_eq!(verify(&key, &garbled, body, Freshness::Skip), Err(WebhookError::MalformedSignature));
    }

    #[test]
    fn keys_are_read_once() {
        assert_eq!(PublicKey::from_pem("nope"), Err(WebhookError::BadKey));
        assert_eq!("nope".parse::<PublicKey>(), Err(WebhookError::BadKey));
    }
}
