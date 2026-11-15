//! Ads on the token's own channel: `POST /public/v1/ads/ad-break` and
//! `POST /public/v1/ads/enroll` (scope `ads:write`), `GET
//! /public/v1/ads/ad-break-status` (`ads:read` or `ads:write`).
//!
//! A refusal names its reason in the error answer's `data`, which
//! [`Error::Status`]'s excerpt keeps: for an ad break `CHANNEL_IS_NOT_LIVE`,
//! `CHANNEL_NOT_OPTED_IN`, `ADS_BLOCKED`, `ADS_RESTRICTED`,
//! `AD_BREAK_ACTIVE`, `MAX_AD_BREAKS_REACHED` or `AUTO_ADS_ACTIVE` (400, 409
//! or 429); for enrolling `CHANNEL_NOT_FOUND`, `CHANNEL_IS_LIVE` or
//! `CHANNEL_NEVER_STREAMED` (Kick's spec).

use super::common::{not_blank, v1};
use crate::http::{self, Endpoints, Error, Method, Request};
use crate::lenient::{array, boolean, get, int, string};
use serde::Serialize;
use serde_json::{Value, json};

/// The shortest ad break, in seconds (Kick's spec).
pub const MIN_BREAK_SECONDS: u32 = 7;
/// The longest ad break, in seconds (Kick's spec).
pub const MAX_BREAK_SECONDS: u32 = 90;

/// Starts an ad break of `seconds` ([`MIN_BREAK_SECONDS`] to
/// [`MAX_BREAK_SECONDS`]). `id` is a UUID the caller makes up for it (the
/// core draws no random numbers). Kick answers 202: read it with
/// [`parse_ad_break`].
pub fn ad_break_request(endpoints: &Endpoints, token: &str, id: &str, seconds: u32) -> Result<Request, Error> {
    not_blank("ad break id", id)?;
    if !(MIN_BREAK_SECONDS..=MAX_BREAK_SECONDS).contains(&seconds) {
        return Err(Error::Invalid(format!(
            "{seconds} seconds; Kick takes {MIN_BREAK_SECONDS} to {MAX_BREAK_SECONDS}"
        )));
    }
    Ok(Request::new(Method::Post, v1(endpoints, "/ads/ad-break"))
        .bearer(token)?
        .json(&json!({ "id": id, "break_duration_seconds": seconds })))
}

/// An ad break started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct AdBreak {
    /// The id it was started with.
    pub id: Option<String>,
    /// Its id in ad server requests.
    pub ad_break_id: Option<String>,
    /// How many more the channel may start in the current period.
    pub remaining_ad_breaks: Option<i64>,
}

/// Reads an [`ad_break_request`] answer.
pub fn parse_ad_break(status: u16, body: &str) -> Result<AdBreak, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    Ok(AdBreak {
        id: string(&data, "id"),
        ad_break_id: string(&data, "ad_break_id"),
        remaining_ad_breaks: int(&data, "remaining_ad_breaks"),
    })
}

/// The channel's ads: enrolled or not, limits, recent ad breaks. Read the
/// answer with [`parse_status`].
pub fn status_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Get, v1(endpoints, "/ads/ad-break-status")).bearer(token)
}

/// How many ad breaks a channel may start, and over how long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct AdLimits {
    /// At most this many…
    pub max: Option<i64>,
    /// …per this many seconds.
    pub period_seconds: Option<i64>,
}

/// One ad break, in [`AdStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct AdBreakState {
    /// The id it was started with.
    pub id: Option<String>,
    /// Its id in ad server requests; missing until it is accepted.
    pub ad_break_id: Option<String>,
    /// Its state, as Kick wrote it (`inserted` in Kick's example).
    pub status: Option<String>,
    /// What started it (`public_api` in Kick's example).
    pub source: Option<String>,
    /// How long, in seconds.
    pub break_duration_seconds: Option<i64>,
    /// When, `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_at: Option<String>,
}

/// A channel's ads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct AdStatus {
    /// Enrolled ([`enroll_request`]).
    pub opted_in: Option<bool>,
    /// Ads are blocked on the channel.
    pub ads_blocked: Option<bool>,
    /// Kick is running ads by itself.
    pub auto_ads_active: Option<bool>,
    /// How many more ad breaks may start in the current period.
    pub remaining_ad_breaks: Option<i64>,
    /// The limits.
    pub limits: Option<AdLimits>,
    /// Recent ad breaks.
    pub ad_breaks: Vec<AdBreakState>,
}

/// Reads a [`status_request`] answer.
pub fn parse_status(status: u16, body: &str) -> Result<AdStatus, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    Ok(AdStatus {
        opted_in: boolean(&data, "opted_in"),
        ads_blocked: boolean(&data, "ads_blocked"),
        auto_ads_active: boolean(&data, "auto_ads_active"),
        remaining_ad_breaks: int(&data, "remaining_ad_breaks"),
        limits: get(&data, "limits").map(|l| AdLimits { max: int(l, "max"), period_seconds: int(l, "period_seconds") }),
        ad_breaks: array(&data, "ad_breaks").iter().map(state).collect(),
    })
}

fn state(b: &Value) -> AdBreakState {
    AdBreakState {
        id: string(b, "id"),
        ad_break_id: string(b, "ad_break_id"),
        status: string(b, "status"),
        source: string(b, "source"),
        break_duration_seconds: int(b, "break_duration_seconds"),
        created_at: string(b, "created_at"),
    }
}

/// Enrolls the channel in ads; Kick refuses while it is live. Read the
/// answer with [`parse_done`](super::parse_done).
pub fn enroll_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Post, v1(endpoints, "/ads/enroll")).bearer(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_three() {
        let e = Endpoints::default();
        let r = ad_break_request(&e, "t", "123e4567-e89b-12d3-a456-426614174000", 30).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, "https://api.kick.com/public/v1/ads/ad-break"));
        let body: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(body, json!({ "id": "123e4567-e89b-12d3-a456-426614174000", "break_duration_seconds": 30 }));
        let r = status_request(&e, "t").unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Get, "https://api.kick.com/public/v1/ads/ad-break-status"));
        let r = enroll_request(&e, "t").unwrap();
        assert_eq!(
            (r.method, r.url.as_str(), r.body),
            (Method::Post, "https://api.kick.com/public/v1/ads/enroll", None)
        );
    }

    #[test]
    fn refuses_what_kick_would() {
        let e = Endpoints::default();
        assert!(ad_break_request(&e, "t", "u", MIN_BREAK_SECONDS).is_ok());
        assert!(ad_break_request(&e, "t", "u", MAX_BREAK_SECONDS).is_ok());
        assert!(ad_break_request(&e, "t", "u", MIN_BREAK_SECONDS - 1).is_err());
        assert!(ad_break_request(&e, "t", "u", MAX_BREAK_SECONDS + 1).is_err());
        assert!(ad_break_request(&e, "t", "", 30).is_err());
        assert!(enroll_request(&e, "t\r\n").is_err());
    }

    #[test]
    fn refusals_keep_their_reason() {
        let body = r#"{"data":"MAX_AD_BREAKS_REACHED","message":"Bad request"}"#;
        match parse_ad_break(429, body).unwrap_err() {
            Error::Status { status: 429, excerpt } => assert!(excerpt.contains("MAX_AD_BREAKS_REACHED")),
            other => panic!("{other:?}"),
        }
        let s = parse_status(200, r#"{"data":{"opted_in":true,"ad_breaks":[{"id":"x"}]},"message":"OK"}"#).unwrap();
        assert_eq!((s.opted_in, s.ad_breaks.len(), s.limits), (Some(true), 1, None));
    }
}
