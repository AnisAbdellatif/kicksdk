//! What the user-token modules share.

use crate::http::{Endpoints, Error, encode};

/// `<api>/public/v1<path>`.
pub(super) fn v1(endpoints: &Endpoints, path: &str) -> String {
    format!("{}/public/v1{path}", endpoints.api)
}

/// Whether a call whose answer carries nothing worth reading worked: a
/// `204` with no body, or `{"message": "OK"}` and the like. Any 2xx is
/// success and the body is not read; anything else is
/// [`Error::Status`].
///
/// For [`chat::delete_request`](super::chat::delete_request), the
/// [`moderation`](super::moderation) builders,
/// [`channels::update_request`](super::channels::update_request),
/// [`rewards::delete_request`](super::rewards::delete_request),
/// [`ads::enroll_request`](super::ads::enroll_request) and
/// [`drops::update_claims_request`](super::drops::update_claims_request).
pub fn parse_done(status: u16, body: &str) -> Result<(), Error> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::Status { status, excerpt: body.chars().take(200).collect() })
    }
}

/// Refuses text over `max` characters (Unicode scalar values).
pub(super) fn at_most(what: &str, s: &str, max: usize) -> Result<(), Error> {
    match s.chars().count() {
        n if n > max => Err(Error::Invalid(format!("{what} of {n} characters; Kick takes at most {max}"))),
        _ => Ok(()),
    }
}

/// Refuses empty or all-whitespace text.
pub(super) fn not_blank(what: &str, s: &str) -> Result<(), Error> {
    match s.trim() {
        "" => Err(Error::Invalid(format!("an empty {what}"))),
        _ => Ok(()),
    }
}

/// `?k=v&…` with the values encoded, or nothing when there are no pairs.
pub(super) fn query(pairs: &[(&str, String)]) -> String {
    match pairs {
        [] => String::new(),
        _ => format!("?{}", pairs.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect::<Vec<_>>().join("&")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_is_any_success() {
        assert!(parse_done(204, "").is_ok());
        assert!(parse_done(200, r#"{"data":{},"message":"OK"}"#).is_ok());
        assert_eq!(parse_done(403, r#"{"message":"Forbidden"}"#).unwrap_err().status(), Some(403));
    }

    #[test]
    fn queries_are_encoded() {
        assert_eq!(query(&[]), "");
        assert_eq!(query(&[("a", "x y".into()), ("b", "1".into())]), "?a=x%20y&b=1");
        assert!(at_most("t", "ééé", 3).is_ok() && at_most("t", "éééé", 3).is_err());
        assert!(not_blank("t", " \n").is_err());
    }
}
