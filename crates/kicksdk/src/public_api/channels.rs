//! Editing the token's own channel: `PATCH /public/v1/channels`, scope
//! `channel:write`. Kick answers 204: read it with
//! [`parse_done`](super::parse_done).

use super::common::{not_blank, v1};
use crate::http::{Endpoints, Error, Method, Request};
use serde_json::{Map, Value};

/// The most custom tags a channel takes (Kick's spec).
pub const MAX_TAGS: usize = 10;

/// Sets what is given of the stream's category (by id), custom tags
/// (`Some(&[])` clears them) and title; what is `None` stays. Refuses a
/// request that sets nothing, category 0, an empty title and more than
/// [`MAX_TAGS`] tags.
pub fn update_request(
    endpoints: &Endpoints,
    token: &str,
    category_id: Option<u64>,
    custom_tags: Option<&[&str]>,
    stream_title: Option<&str>,
) -> Result<Request, Error> {
    let mut body = Map::new();
    if let Some(c) = category_id {
        if c == 0 {
            return Err(Error::Invalid("category 0".into()));
        }
        body.insert("category_id".into(), c.into());
    }
    if let Some(tags) = custom_tags {
        if tags.len() > MAX_TAGS {
            return Err(Error::Invalid(format!("{} tags; Kick takes at most {MAX_TAGS}", tags.len())));
        }
        body.insert("custom_tags".into(), tags.into());
    }
    if let Some(t) = stream_title {
        not_blank("title", t)?;
        body.insert("stream_title".into(), t.into());
    }
    if body.is_empty() {
        return Err(Error::Invalid("nothing to change".into()));
    }
    Ok(Request::new(Method::Patch, v1(endpoints, "/channels")).bearer(token)?.json(&Value::Object(body)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_what_is_given() {
        let e = Endpoints::default();
        let r = update_request(&e, "t", Some(123), Some(&["pog", "chad"]), Some("My first stream")).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Patch, "https://api.kick.com/public/v1/channels"));
        let body: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({ "category_id": 123, "custom_tags": ["pog", "chad"], "stream_title": "My first stream" })
        );
        let r = update_request(&e, "t", None, Some(&[]), None).unwrap();
        assert_eq!(r.body.as_deref(), Some(r#"{"custom_tags":[]}"#));
    }

    #[test]
    fn refuses_what_kick_would() {
        let e = Endpoints::default();
        assert!(matches!(update_request(&e, "t", None, None, None), Err(Error::Invalid(_))));
        assert!(update_request(&e, "t", Some(0), None, None).is_err());
        assert!(update_request(&e, "t", None, None, Some(" ")).is_err());
        assert!(update_request(&e, "t", None, Some(&["x"; MAX_TAGS]), None).is_ok());
        assert!(update_request(&e, "t", None, Some(&["x"; MAX_TAGS + 1]), None).is_err());
    }
}
