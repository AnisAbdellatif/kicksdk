//! Requests to make, and what goes wrong reading the answers.
//!
//! A request builder returns a [`Request`]; the host sends it however it
//! sends things and hands the status and body to the matching parser. Nothing
//! here retries or waits: when to try again is the host's call.
//!
//! Builders refuse, with [`Error::Invalid`], what would make a request other
//! than the one asked for: a path segment that is empty, `.` or `..` (it
//! would move the path; percent-encoding a dot does not help, since URLs
//! read `%2e%2e` as `..` too), and a token with a control character (a CR
//! or LF would add a header of its own).
//!
//! A [`Request`]'s `Debug` hides credentials (the `authorization` header,
//! a form's `client_secret`); its `Serialize`, which is what a host sends,
//! does not.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Where each part of Kick lives. Every builder takes one, so a test, a
/// simulator or a proxy can stand in for any of them.
///
/// Start from [`Endpoints::default`] and replace a part with the `with_*`
/// methods (or assign the field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Endpoints {
    /// The official API: `https://api.kick.com/public/v1`.
    pub api: String,
    /// OAuth: `https://id.kick.com`.
    pub id: String,
    /// kick.com itself, whose `/api/v2` is the website API.
    pub website: String,
    /// The realtime endpoints: `https://web.kick.com/api/v1/realtime`.
    pub realtime: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api: "https://api.kick.com/public/v1".into(),
            id: "https://id.kick.com".into(),
            website: "https://kick.com".into(),
            realtime: "https://web.kick.com/api/v1/realtime".into(),
        }
    }
}

impl Endpoints {
    /// The official API somewhere else.
    pub fn with_api(mut self, url: impl Into<String>) -> Self {
        self.api = url.into();
        self
    }

    /// OAuth somewhere else.
    pub fn with_id(mut self, url: impl Into<String>) -> Self {
        self.id = url.into();
        self
    }

    /// The website (and its `/api/v2`) somewhere else.
    pub fn with_website(mut self, url: impl Into<String>) -> Self {
        self.website = url.into();
        self
    }

    /// The realtime endpoints somewhere else.
    pub fn with_realtime(mut self, url: impl Into<String>) -> Self {
        self.realtime = url.into();
        self
    }
}

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
#[non_exhaustive]
pub enum Method {
    /// GET
    Get,
    /// POST
    Post,
    /// DELETE
    Delete,
}

/// A request for the host to send.
///
/// Its `Debug` hides credentials; see the module docs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The full URL, query string included.
    pub url: String,
    /// Headers, lower-case names. A browser drops the ones it forbids
    /// (`origin`, `user-agent`); see each builder for what that costs.
    pub headers: Vec<(String, String)>,
    /// The body, already encoded (JSON or a form), if there is one.
    pub body: Option<String>,
}

impl Request {
    /// A request with no body, asking for JSON (`accept: application/json`),
    /// as every builder starts. For a host's own requests and for tests.
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self { method, url: url.into(), headers: vec![("accept".into(), "application/json".into())], body: None }
    }

    /// Adds a header whose value this crate wrote (no user input).
    pub(crate) fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        debug_assert!(header_value(&value).is_ok(), "{name}: {value:?}");
        self.headers.push((name.into(), value));
        self
    }

    /// `authorization: Bearer <token>`. A token with a control character is
    /// refused: a CR or LF in it would end the header and start another.
    pub(crate) fn bearer(self, token: &str) -> Result<Self, Error> {
        header_value(token).map_err(|_| Error::Invalid("the token has a control character".into()))?;
        Ok(self.header("authorization", format!("Bearer {token}")))
    }

    pub(crate) fn json(mut self, body: &serde_json::Value) -> Self {
        self.headers.push(("content-type".into(), "application/json".into()));
        self.body = Some(body.to_string());
        self
    }

    pub(crate) fn form(mut self, pairs: &[(&str, &str)]) -> Self {
        self.headers.push(("content-type".into(), "application/x-www-form-urlencoded".into()));
        self.body =
            Some(pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&"));
        self
    }
}

/// Header names whose values `Debug` hides.
const SECRET_HEADERS: [&str; 3] = ["authorization", "cookie", "x-csrf-token"];
/// Form fields whose values `Debug` hides.
const SECRET_FIELDS: [&str; 5] = ["client_secret", "refresh_token", "code", "code_verifier", "password"];
const REDACTED: &str = "[redacted]";

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let secret = SECRET_HEADERS.iter().any(|s| s.eq_ignore_ascii_case(k));
                (k.as_str(), if secret { REDACTED } else { v.as_str() })
            })
            .collect();
        let form = self
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.starts_with("application/x-www-form-urlencoded"));
        let body = self.body.as_deref().map(|b| if form { redact_form(b) } else { b.to_owned() });
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &body)
            .finish()
    }
}

fn redact_form(body: &str) -> String {
    body.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if SECRET_FIELDS.contains(&k) => format!("{k}={REDACTED}"),
            _ => pair.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Why an answer could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Kick answered with a status other than success.
    #[error("Kick answered {status}")]
    Status {
        /// The HTTP status.
        status: u16,
        /// The start of the body, for a log line.
        excerpt: String,
    },
    /// The body was not JSON.
    #[error("Kick's answer was not JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The body was JSON but not the shape this parser knows.
    #[error("Kick's answer had an unexpected shape: {0}")]
    Shape(String),
    /// The arguments were refused before anything was sent.
    #[error("invalid request: {0}")]
    Invalid(String),
}

impl Error {
    /// The HTTP status, when Kick answered with an error status.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether this is Kick saying "slow down" (429).
    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(429)
    }

    pub(crate) fn shape(what: impl Into<String>) -> Self {
        Error::Shape(what.into())
    }
}

/// Checks the status and decodes the body as JSON.
pub(crate) fn json(status: u16, body: &str) -> Result<serde_json::Value, Error> {
    if !(200..300).contains(&status) {
        return Err(Error::Status { status, excerpt: body.chars().take(200).collect() });
    }
    Ok(serde_json::from_str(body)?)
}

/// Kick's usual envelope, `{"data": …, "message": …}`: the `data`.
pub(crate) fn data(status: u16, body: &str) -> Result<serde_json::Value, Error> {
    match json(status, body)? {
        serde_json::Value::Object(mut map) => map.remove("data").ok_or_else(|| Error::shape("no `data` in the answer")),
        _ => Err(Error::shape("the answer is not an object")),
    }
}

/// A header value without control characters (a tab is allowed, as HTTP
/// allows it).
pub(crate) fn header_value(s: &str) -> Result<(), Error> {
    match s.chars().find(|c| c.is_control() && *c != '\t') {
        Some(c) => Err(Error::Invalid(format!("control character {c:?} in a header value"))),
        None => Ok(()),
    }
}

/// One path segment, percent-encoded. Empty, `.` and `..` are refused: a
/// URL reads them (and `%2e`, `%2e%2e`) as "here" and "up one", so they
/// would send the request somewhere else.
pub(crate) fn segment(what: &str, s: &str) -> Result<String, Error> {
    match s {
        "" | "." | ".." => Err(Error::Invalid(format!("{what} {s:?} is not a name"))),
        _ => Ok(encode(s)),
    }
}

/// Percent-encodes one path segment or query value.
pub(crate) fn encode(s: &str) -> String {
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
    const SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');
    utf8_percent_encode(s, SET).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_went_wrong() {
        let e = json(429, "slow down").unwrap_err();
        assert!(e.is_rate_limited());
        assert_eq!(e.status(), Some(429));
        assert!(matches!(data(200, "[]"), Err(Error::Shape(_))));
        assert!(matches!(data(200, "{\"message\":\"OK\"}"), Err(Error::Shape(_))));
        assert!(matches!(json(200, "<html>"), Err(Error::Json(_))));
    }

    #[test]
    fn segments_are_encoded() {
        assert_eq!(encode("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(encode("user_01.x-y~"), "user_01.x-y~");
        assert_eq!(segment("slug", "a.b").unwrap(), "a.b");
        assert_eq!(segment("slug", "...").unwrap(), "...");
        for bad in ["", ".", ".."] {
            assert!(matches!(segment("slug", bad), Err(Error::Invalid(_))), "{bad:?}");
        }
    }

    #[test]
    fn tokens_cannot_add_headers() {
        let r = Request::new(Method::Get, "https://x");
        assert!(r.clone().bearer("abc.def-ghi").is_ok());
        for bad in ["a\r\nx-evil: 1", "a\nb", "a\0b", "a\u{7f}"] {
            assert!(matches!(r.clone().bearer(bad), Err(Error::Invalid(_))), "{bad:?}");
        }
    }

    #[test]
    fn debug_hides_credentials() {
        let r = Request::new(Method::Post, "https://x").bearer("sekrit-token").unwrap().form(&[
            ("grant_type", "client_credentials"),
            ("client_id", "app"),
            ("client_secret", "sekrit-secret"),
        ]);
        let shown = format!("{r:?}");
        assert!(!shown.contains("sekrit"), "{shown}");
        assert!(shown.contains("client_id=app") && shown.contains("client_secret=[redacted]"), "{shown}");
        // What the host sends is untouched.
        assert!(r.body.as_deref().unwrap().contains("client_secret=sekrit-secret"));
        assert!(r.headers.contains(&("authorization".into(), "Bearer sekrit-token".into())));
    }

    #[test]
    fn endpoints_by_parts() {
        let e = Endpoints::default().with_api("http://a").with_website("http://w");
        assert_eq!(
            (e.api.as_str(), e.website.as_str(), e.id.as_str()),
            ("http://a", "http://w", "https://id.kick.com")
        );
    }
}
