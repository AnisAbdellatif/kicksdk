//! Bans and timeouts with a user token: `POST` and `DELETE
//! /public/v1/moderation/bans`, scope `moderation:ban`.
//!
//! The official versions of [`website::ban_request`],
//! [`website::timeout_request`] and [`website::unban_request`]: an OAuth
//! user token instead of the viewer's kick.com session, and the channel and
//! the user by user id instead of slug and username (`docs/KICK.md` §3).
//! Kick's answer carries nothing: read it with
//! [`parse_done`](super::parse_done).
//!
//! [`website::ban_request`]: crate::website::ban_request
//! [`website::timeout_request`]: crate::website::timeout_request
//! [`website::unban_request`]: crate::website::unban_request

use super::common::{at_most, v1};
use crate::http::{Endpoints, Error, Method, Request};
use crate::ids::UserId;
use serde_json::{Value, json};

/// The longest timeout, in minutes: a week (Kick's spec).
pub const MAX_TIMEOUT_MINUTES: u32 = 10_080;

/// The most characters in a ban's reason (Kick's spec).
pub const MAX_REASON: usize = 100;

fn bans(endpoints: &Endpoints, token: &str, method: Method, body: &Value) -> Result<Request, Error> {
    Ok(Request::new(method, v1(endpoints, "/moderation/bans")).bearer(token)?.json(body))
}

fn ban(broadcaster: UserId, user: UserId, minutes: Option<u32>, reason: Option<&str>) -> Result<Value, Error> {
    let mut body = json!({ "broadcaster_user_id": broadcaster.0, "user_id": user.0 });
    if let Some(m) = minutes {
        body["duration"] = m.into();
    }
    if let Some(r) = reason {
        at_most("reason", r, MAX_REASON)?;
        body["reason"] = r.into();
    }
    Ok(body)
}

/// Bans `user` from `broadcaster`'s chat for good.
pub fn ban_request(
    endpoints: &Endpoints,
    token: &str,
    broadcaster: UserId,
    user: UserId,
    reason: Option<&str>,
) -> Result<Request, Error> {
    bans(endpoints, token, Method::Post, &ban(broadcaster, user, None, reason)?)
}

/// Times `user` out of `broadcaster`'s chat for `minutes`, 1 to
/// [`MAX_TIMEOUT_MINUTES`].
pub fn timeout_request(
    endpoints: &Endpoints,
    token: &str,
    broadcaster: UserId,
    user: UserId,
    minutes: u32,
    reason: Option<&str>,
) -> Result<Request, Error> {
    if !(1..=MAX_TIMEOUT_MINUTES).contains(&minutes) {
        return Err(Error::Invalid(format!("{minutes} minutes; 1 to {MAX_TIMEOUT_MINUTES} (a week)")));
    }
    bans(endpoints, token, Method::Post, &ban(broadcaster, user, Some(minutes), reason)?)
}

/// Lifts a ban or a timeout. The ids go in a JSON body, on a `DELETE`.
pub fn unban_request(endpoints: &Endpoints, token: &str, broadcaster: UserId, user: UserId) -> Result<Request, Error> {
    bans(endpoints, token, Method::Delete, &json!({ "broadcaster_user_id": broadcaster.0, "user_id": user.0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(r: &Request) -> Value {
        serde_json::from_str(r.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn bans_timeouts_and_unbans() {
        let e = Endpoints::default();
        let url = "https://api.kick.com/public/v1/moderation/bans";
        let r = ban_request(&e, "t", UserId(123), UserId(333), None).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, url));
        assert_eq!(body(&r), json!({ "broadcaster_user_id": 123, "user_id": 333 }));
        let r = timeout_request(&e, "t", UserId(123), UserId(333), 10, Some("Not vibing")).unwrap();
        assert_eq!(
            body(&r),
            json!({ "broadcaster_user_id": 123, "user_id": 333, "duration": 10, "reason": "Not vibing" })
        );
        let r = unban_request(&e, "t", UserId(123), UserId(333)).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Delete, url));
        assert_eq!(body(&r), json!({ "broadcaster_user_id": 123, "user_id": 333 }));
        assert!(r.headers.contains(&("content-type".into(), "application/json".into())));
    }

    #[test]
    fn refuses_what_kick_would() {
        let e = Endpoints::default();
        let (b, u) = (UserId(1), UserId(2));
        assert!(timeout_request(&e, "t", b, u, 0, None).is_err());
        assert!(timeout_request(&e, "t", b, u, MAX_TIMEOUT_MINUTES, None).is_ok());
        assert!(timeout_request(&e, "t", b, u, MAX_TIMEOUT_MINUTES + 1, None).is_err());
        assert!(ban_request(&e, "t", b, u, Some(&"x".repeat(MAX_REASON))).is_ok());
        assert!(matches!(ban_request(&e, "t", b, u, Some(&"x".repeat(MAX_REASON + 1))), Err(Error::Invalid(_))));
        assert!(unban_request(&e, "t\nx", b, u).is_err());
    }
}
