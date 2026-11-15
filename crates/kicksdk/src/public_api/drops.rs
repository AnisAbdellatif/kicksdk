//! Drops reward claims: `GET` and `PATCH /public/v1/drops/claims`.
//!
//! Only an OAuth app that belongs to the organization running the
//! campaigns may call these, with an **app** token
//! ([`token_request`](super::token_request)), not a user's. They are in
//! Kick's drops guide (`drops/public-api.md`), not in its API spec
//! (`docs/KICK.md` §3).

use super::common::{query, v1};
use crate::http::{self, Endpoints, Error, Method, Request};
use crate::ids::UserId;
use crate::lenient::{array, id_string, string, uint};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The most claims one page gives (Kick's guide; its default is 10).
pub const MAX_LIMIT: u32 = 1000;
/// The most claims one update takes (Kick's guide).
pub const MAX_UPDATES: usize = 100;

/// What to list claims by, for [`claims_request`]; `None` is "not
/// filtered". Start from `ClaimFilter::default()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ClaimFilter {
    /// One campaign.
    pub campaign_id: Option<String>,
    /// One claim.
    pub claim_id: Option<String>,
    /// One user's claims.
    pub user_id: Option<UserId>,
    /// Claims with this `external_status`.
    pub external_status: Option<String>,
    /// Claims per page, 1 to [`MAX_LIMIT`].
    pub limit: Option<u32>,
    /// From here: a page's [`Claims::cursor`].
    pub cursor: Option<String>,
}

/// Lists claims. Read the answer with [`parse_claims`].
pub fn claims_request(endpoints: &Endpoints, token: &str, filter: &ClaimFilter) -> Result<Request, Error> {
    let mut q = Vec::new();
    if let Some(c) = &filter.campaign_id {
        q.push(("campaign_id", c.clone()));
    }
    if let Some(c) = &filter.claim_id {
        q.push(("claim_id", c.clone()));
    }
    if let Some(u) = filter.user_id {
        q.push(("user_id", u.to_string()));
    }
    if let Some(s) = &filter.external_status {
        q.push(("external_status", s.clone()));
    }
    if let Some(n) = filter.limit {
        if !(1..=MAX_LIMIT).contains(&n) {
            return Err(Error::Invalid(format!("limit {n}; Kick takes 1 to {MAX_LIMIT}")));
        }
        q.push(("limit", n.to_string()));
    }
    if let Some(c) = &filter.cursor {
        q.push(("cursor", c.clone()));
    }
    Request::new(Method::Get, v1(endpoints, &format!("/drops/claims{}", query(&q)))).bearer(token)
}

/// A viewer's claim of a drops reward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Claim {
    /// Its id (a ULID).
    pub claim_id: String,
    /// Who claimed.
    pub user_id: Option<UserId>,
    /// The campaign.
    pub campaign_id: Option<String>,
    /// The reward.
    pub reward_id: Option<String>,
    /// The organization's id for it.
    pub external_id: Option<String>,
    /// The organization's fulfilment status: `initiated` when the claim is
    /// made, then whatever [`update_claims_request`] set.
    pub external_status: Option<String>,
    /// When it was made.
    pub created_at: Option<String>,
    /// When it last changed.
    pub updated_at: Option<String>,
}

/// One page of claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Claims {
    /// The claims.
    pub claims: Vec<Claim>,
    /// Where the next page starts, for [`ClaimFilter::cursor`]. The guide
    /// does not say what the last page gives; an empty string is `None`.
    pub cursor: Option<String>,
}

/// Reads a [`claims_request`] answer. A claim without an id is skipped.
pub fn parse_claims(status: u16, body: &str) -> Result<Claims, Error> {
    let data = http::data(status, body)?;
    if !data.is_object() {
        return Err(Error::shape("`data` is not an object"));
    }
    let claims = array(&data, "claims")
        .iter()
        .filter_map(|c: &Value| {
            Some(Claim {
                claim_id: id_string(c, "claim_id")?,
                user_id: uint(c, "user_id").map(UserId),
                campaign_id: string(c, "campaign_id"),
                reward_id: string(c, "reward_id"),
                external_id: string(c, "external_id"),
                external_status: string(c, "external_status"),
                created_at: string(c, "created_at"),
                updated_at: string(c, "updated_at"),
            })
        })
        .collect();
    let cursor = string(&data, "cursor").filter(|c| !c.is_empty());
    Ok(Claims { claims, cursor })
}

/// Sets claims' `external_status`: `(claim_id, external_status)` pairs, 1
/// to [`MAX_UPDATES`]. Kick answers 204: read it with
/// [`parse_done`](super::parse_done).
pub fn update_claims_request(endpoints: &Endpoints, token: &str, updates: &[(&str, &str)]) -> Result<Request, Error> {
    match updates.len() {
        0 => return Err(Error::Invalid("no claims".into())),
        n if n > MAX_UPDATES => return Err(Error::Invalid(format!("{n} claims; Kick takes at most {MAX_UPDATES}"))),
        _ => {}
    }
    let claims: Vec<Value> =
        updates.iter().map(|(id, status)| json!({ "claim_id": id, "external_status": status })).collect();
    Ok(Request::new(Method::Patch, v1(endpoints, "/drops/claims")).bearer(token)?.json(&json!({ "claims": claims })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_by_filter() {
        let e = Endpoints::default();
        let base = "https://api.kick.com/public/v1/drops/claims";
        assert_eq!(claims_request(&e, "t", &ClaimFilter::default()).unwrap().url, base);
        let f = ClaimFilter {
            campaign_id: Some("01JAXK".into()),
            limit: Some(10),
            cursor: Some("01K0TD".into()),
            user_id: Some(UserId(1234)),
            ..Default::default()
        };
        let r = claims_request(&e, "t", &f).unwrap();
        assert_eq!(r.url, format!("{base}?campaign_id=01JAXK&user_id=1234&limit=10&cursor=01K0TD"));
        for bad in [0, MAX_LIMIT + 1] {
            let f = ClaimFilter { limit: Some(bad), ..Default::default() };
            assert!(claims_request(&e, "t", &f).is_err());
        }
    }

    #[test]
    fn updates_up_to_a_hundred() {
        let e = Endpoints::default();
        let r = update_claims_request(&e, "t", &[("a", "processed")]).unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Patch, "https://api.kick.com/public/v1/drops/claims"));
        assert_eq!(r.body.as_deref(), Some(r#"{"claims":[{"claim_id":"a","external_status":"processed"}]}"#));
        assert!(update_claims_request(&e, "t", &[]).is_err());
        assert!(update_claims_request(&e, "t", &[("a", "x"); MAX_UPDATES]).is_ok());
        assert!(update_claims_request(&e, "t", &[("a", "x"); MAX_UPDATES + 1]).is_err());
    }

    #[test]
    fn last_page_has_no_cursor() {
        let c = parse_claims(200, r#"{"data":{"claims":[{"claim_id":"a","user_id":5},{}],"cursor":""}}"#).unwrap();
        assert_eq!((c.claims.len(), c.claims[0].user_id, c.cursor), (1, Some(UserId(5)), None));
    }
}
