//! Requests to make, and what goes wrong reading the answers.
//!
//! A request builder returns a [`Request`]; the host sends it however it
//! sends things and hands the status and body to the matching parser. Nothing
//! here retries or waits: when to try again is the host's call.

use serde::{Deserialize, Serialize};

/// Where each part of Kick lives. Every builder takes one, so a test, a
/// simulator or a proxy can stand in for any of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    /// GET
    Get,
    /// POST
    Post,
    /// DELETE
    Delete,
}

/// A request for the host to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub(crate) fn new(method: Method, url: String) -> Self {
        Self { method, url, headers: vec![("accept".into(), "application/json".into())], body: None }
    }

    pub(crate) fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub(crate) fn bearer(self, token: &str) -> Self {
        self.header("authorization", format!("Bearer {token}"))
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
    }
}
