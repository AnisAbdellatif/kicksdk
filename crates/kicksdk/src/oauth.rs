//! User tokens: OAuth 2.1, authorization code with PKCE, on `id.kick.com`.
//!
//! A user token acts as one Kick user, with the [`Scope`]s they approved:
//! sending chat, moderating, editing their channel, reading their stream key.
//! To read public data or subscribe to webhooks, the app token is enough and
//! simpler: [`public_api::token_request`](crate::public_api::token_request)
//! and [`public_api::parse_token`](crate::public_api::parse_token).
//!
//! The flow (`docs/KICK.md` §2):
//!
//! 1. Make a [`Pkce`] and a `state` from the platform's CSPRNG. This crate
//!    draws no random numbers: the caller hands them in, which keeps it
//!    deterministic and lets it build anywhere, wasm included.
//! 2. Send the user's browser to [`authorize_url`]. Keep the verifier and
//!    the state (a session, a cookie) until the user comes back.
//! 3. Kick redirects to `redirect_uri?code=…&state=…`. Check the state is
//!    the one you made, then send [`code_request`] and read the answer with
//!    [`parse_user_token`].
//! 4. Before the access token expires, [`refresh_request`]; the answer
//!    carries a new refresh token too, which replaces the old one.
//! 5. [`revoke_request`] when the user signs out; [`introspect_request`] to
//!    ask Kick whether a token is still good and what it may do.
//!
//! Nothing here is from a recording yet: it follows Kick's docs and API spec
//! (`fixtures/docs/oauth/`), so the parsers are lenient where they disagree.
//!
//! `Debug` hides the secrets (the verifier, tokens, and a request's
//! `client_secret`, `code`, `refresh_token` and `code_verifier`); `Serialize`,
//! what a binding hands on, does not.

use crate::http::{self, Endpoints, Error, Method, Request, encode};
use crate::lenient::{boolean, int, string};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;

const REDACTED: &str = "[redacted]";

/// What a user token may do. Kick shows the user these on its consent
/// screen.
///
/// Read one with [`Scope::from`] (or `parse`), which never gives
/// [`Other`](Scope::Other) for a scope listed here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Scope {
    /// `ads:read`: a channel's ad settings and ad breaks.
    AdsRead,
    /// `ads:write`: manage a channel's ad settings and run ad breaks.
    AdsWrite,
    /// `channel:read`: channel information (description, category, …).
    ChannelRead,
    /// `channel:write`: update the livestream's metadata.
    ChannelWrite,
    /// `channel:rewards:read`: channel-points rewards.
    ChannelRewardsRead,
    /// `channel:rewards:write`: add, edit and delete channel-points rewards.
    ChannelRewardsWrite,
    /// `chat:write`: send chat messages, also as a bot.
    ChatWrite,
    /// `events:subscribe`: webhooks for the user's own channel.
    EventsSubscribe,
    /// `kicks:read`: KICKs information (leaderboards, …).
    KicksRead,
    /// `moderation:ban`: ban and unban.
    ModerationBan,
    /// `moderation:chat_message:manage`: moderate chat messages.
    ModerationChatMessageManage,
    /// `streamkey:read`: the stream URL and stream key.
    StreamkeyRead,
    /// `user:read`: user information (username, streamer id, …).
    UserRead,
    /// A scope Kick added after this crate was written, as Kick spells it.
    Other(String),
}

impl Scope {
    /// Every scope Kick documents (2026-11-15).
    pub const ALL: [Scope; 13] = [
        Scope::AdsRead,
        Scope::AdsWrite,
        Scope::ChannelRead,
        Scope::ChannelWrite,
        Scope::ChannelRewardsRead,
        Scope::ChannelRewardsWrite,
        Scope::ChatWrite,
        Scope::EventsSubscribe,
        Scope::KicksRead,
        Scope::ModerationBan,
        Scope::ModerationChatMessageManage,
        Scope::StreamkeyRead,
        Scope::UserRead,
    ];

    /// As Kick spells it.
    pub fn as_str(&self) -> &str {
        match self {
            Scope::AdsRead => "ads:read",
            Scope::AdsWrite => "ads:write",
            Scope::ChannelRead => "channel:read",
            Scope::ChannelWrite => "channel:write",
            Scope::ChannelRewardsRead => "channel:rewards:read",
            Scope::ChannelRewardsWrite => "channel:rewards:write",
            Scope::ChatWrite => "chat:write",
            Scope::EventsSubscribe => "events:subscribe",
            Scope::KicksRead => "kicks:read",
            Scope::ModerationBan => "moderation:ban",
            Scope::ModerationChatMessageManage => "moderation:chat_message:manage",
            Scope::StreamkeyRead => "streamkey:read",
            Scope::UserRead => "user:read",
            Scope::Other(s) => s,
        }
    }

    /// The scopes in a space-separated `scope`, as a token answer and an
    /// introspection carry them. Extra spaces are ignored.
    pub fn parse_list(s: &str) -> Vec<Scope> {
        s.split_ascii_whitespace().map(Scope::from).collect()
    }

    /// Scopes as a space-separated `scope`.
    pub fn join(scopes: &[Scope]) -> String {
        scopes.iter().map(Scope::as_str).collect::<Vec<_>>().join(" ")
    }

    /// One scope-token by OAuth's grammar (RFC 6749 §3.3): printable ASCII
    /// but space, `"` and `\`. A space would make it two scopes.
    fn check(&self) -> Result<(), Error> {
        let s = self.as_str();
        let ok = !s.is_empty() && s.bytes().all(|b| matches!(b, 0x21 | 0x23..=0x5b | 0x5d..=0x7e));
        if ok { Ok(()) } else { Err(Error::Invalid(format!("scope {s:?} is not one OAuth scope"))) }
    }
}

impl From<&str> for Scope {
    fn from(s: &str) -> Self {
        Scope::ALL.into_iter().find(|k| k.as_str() == s).unwrap_or_else(|| Scope::Other(s.to_owned()))
    }
}

impl std::str::FromStr for Scope {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Scope::from(s))
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Scope {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Scope::from(String::deserialize(d)?.as_str()))
    }
}

/// A PKCE pair (RFC 7636): the verifier the app keeps, and its S256
/// challenge, which goes in the [`authorize_url`].
///
/// Its `Debug` hides the verifier; `Serialize` does not (a binding hands it
/// to the app, which keeps it until the user comes back).
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Pkce {
    verifier: String,
    challenge: String,
}

impl Pkce {
    /// The challenge method: SHA-256, the only one Kick takes.
    pub const METHOD: &'static str = "S256";

    /// A pair from random bytes the caller drew from a CSPRNG: the verifier
    /// is their base64url. 32 bytes (a 43-character verifier) is what the
    /// RFC recommends; 32 to 96 are taken, which keeps the verifier within
    /// the RFC's 43 to 128 characters.
    pub fn from_random(bytes: &[u8]) -> Result<Self, Error> {
        if !(32..=96).contains(&bytes.len()) {
            return Err(Error::Invalid(format!("{} random bytes; PKCE takes 32 to 96", bytes.len())));
        }
        Ok(Self::new(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// The pair for a verifier made elsewhere (or kept from before): 43 to
    /// 128 characters of `A-Z a-z 0-9 - . _ ~`.
    pub fn from_verifier(verifier: &str) -> Result<Self, Error> {
        let len = verifier.len();
        let chars = verifier.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
        if !(43..=128).contains(&len) || !chars {
            return Err(Error::Invalid("a PKCE verifier is 43 to 128 characters of A-Z a-z 0-9 - . _ ~".into()));
        }
        Ok(Self::new(verifier.to_owned()))
    }

    fn new(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self { verifier, challenge }
    }

    /// The verifier: a secret, for [`code_request`].
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    /// The S256 challenge: base64url (no padding) of the verifier's SHA-256.
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pkce").field("verifier", &REDACTED).field("challenge", &self.challenge).finish()
    }
}

/// Where to send the user's browser to approve the app: Kick's
/// `GET /oauth/authorize` URL, every value percent-encoded.
///
/// `state` is the caller's, random from a CSPRNG, and must come back
/// unchanged on the redirect; it may not be empty (Kick requires it). So
/// may `client_id` and `redirect_uri` not, which must be exactly the
/// redirect URL set in the app's settings. At least one scope is needed.
pub fn authorize_url(
    endpoints: &Endpoints,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[Scope],
    state: &str,
    pkce: &Pkce,
) -> Result<String, Error> {
    for (what, v) in [("client_id", client_id), ("redirect_uri", redirect_uri), ("state", state)] {
        if v.is_empty() {
            return Err(Error::Invalid(format!("an empty {what}")));
        }
    }
    if scopes.is_empty() {
        return Err(Error::Invalid("no scopes".into()));
    }
    scopes.iter().try_for_each(Scope::check)?;
    let q = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", &Scope::join(scopes)),
        ("code_challenge", pkce.challenge()),
        ("code_challenge_method", Pkce::METHOD),
        ("state", state),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", encode(v)))
    .collect::<Vec<_>>()
    .join("&");
    Ok(format!("{}/oauth/authorize?{q}", endpoints.id))
}

/// A user token for the `code` Kick put on the redirect, with the verifier
/// of the [`Pkce`] whose challenge was in the [`authorize_url`]. The
/// `redirect_uri` must be the same one.
pub fn code_request(
    endpoints: &Endpoints,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
) -> Request {
    Request::new(Method::Post, format!("{}/oauth/token", endpoints.id)).form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ])
}

/// A new user token (and refresh token) for a refresh token.
pub fn refresh_request(endpoints: &Endpoints, client_id: &str, client_secret: &str, refresh_token: &str) -> Request {
    Request::new(Method::Post, format!("{}/oauth/token", endpoints.id)).form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ])
}

/// A user access token, with what refreshes it.
///
/// Its `Debug` hides both tokens; `Serialize`, what a binding hands on, does
/// not.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct UserToken {
    /// The token, for `authorization: Bearer`.
    pub access_token: String,
    /// For [`refresh_request`]. Each refresh gives a new one.
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires (the spec's example: 7 200).
    pub expires_in: Option<i64>,
    /// Seconds until the refresh token expires, counted again from every
    /// refresh (the spec's example: 2 592 000, 30 days). The docs leave it out.
    pub refresh_expires_in: Option<i64>,
    /// `Bearer`.
    pub token_type: Option<String>,
    /// What the user approved.
    pub scopes: Vec<Scope>,
}

impl fmt::Debug for UserToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { access_token: _, refresh_token, expires_in, refresh_expires_in, token_type, scopes } = self;
        f.debug_struct("UserToken")
            .field("access_token", &REDACTED)
            .field("refresh_token", &refresh_token.as_ref().map(|_| REDACTED))
            .field("expires_in", expires_in)
            .field("refresh_expires_in", refresh_expires_in)
            .field("token_type", token_type)
            .field("scopes", scopes)
            .finish()
    }
}

/// Reads a [`code_request`] or [`refresh_request`] answer. OAuth's answer
/// is not in Kick's usual `data` envelope; numbers may come as strings.
///
/// Kick's refusal (`400 {"error": "Invalid request"}`) is [`Error::Status`];
/// an `error` with a success status is [`Error::Shape`] saying what Kick said.
pub fn parse_user_token(status: u16, body: &str) -> Result<UserToken, Error> {
    let v = http::json(status, body)?;
    let Some(access_token) = string(&v, "access_token") else {
        return Err(Error::shape(match string(&v, "error") {
            Some(e) => format!("no `access_token`; Kick said {e:?}"),
            None => "no `access_token`".into(),
        }));
    };
    Ok(UserToken {
        access_token,
        refresh_token: string(&v, "refresh_token"),
        expires_in: int(&v, "expires_in"),
        refresh_expires_in: int(&v, "refresh_expires_in"),
        token_type: string(&v, "token_type"),
        scopes: string(&v, "scope").as_deref().map(Scope::parse_list).unwrap_or_default(),
    })
}

/// Which kind of token a [`revoke_request`] is for, to spare Kick looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TokenTypeHint {
    /// `access_token`
    AccessToken,
    /// `refresh_token`
    RefreshToken,
}

impl TokenTypeHint {
    /// As OAuth spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenTypeHint::AccessToken => "access_token",
            TokenTypeHint::RefreshToken => "refresh_token",
        }
    }
}

/// Revokes a token: `POST /oauth/revoke?token=…&token_type_hint=…`.
///
/// The token goes in the query, as both Kick's docs and its spec show (the
/// docs call the hint `token_hint_type`; the spec and RFC 7009 call it
/// `token_type_hint`, which is what this sends). Mind that the token is in
/// the URL, so keep the URL out of logs. An empty token is refused.
pub fn revoke_request(endpoints: &Endpoints, token: &str, hint: Option<TokenTypeHint>) -> Result<Request, Error> {
    if token.is_empty() {
        return Err(Error::Invalid("an empty token".into()));
    }
    let mut q = format!("token={}", encode(token));
    if let Some(h) = hint {
        q.push_str("&token_type_hint=");
        q.push_str(h.as_str());
    }
    Ok(Request::new(Method::Post, format!("{}/oauth/revoke?{q}", endpoints.id))
        .header("content-type", "application/x-www-form-urlencoded"))
}

/// Reads a [`revoke_request`] answer: any success status is done, whatever
/// the body (the docs show `OK`; it may be empty).
pub fn parse_revoke(status: u16, body: &str) -> Result<(), Error> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::Status { status, excerpt: body.chars().take(200).collect() })
    }
}

/// Asks Kick about the token it is sent with:
/// `POST /oauth/token/introspect`, `authorization: Bearer <token>`. Works
/// for app tokens too. (`/public/v1/token/introspect` is the deprecated
/// path to the same thing.)
pub fn introspect_request(endpoints: &Endpoints, token: &str) -> Result<Request, Error> {
    Request::new(Method::Post, format!("{}/oauth/token/introspect", endpoints.id)).bearer(token)
}

/// What Kick says about a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Introspection {
    /// Whether the token is good. When it is not, Kick says nothing else.
    pub active: bool,
    /// The app it belongs to.
    pub client_id: Option<String>,
    /// When it expires, in Unix seconds.
    pub exp: Option<i64>,
    /// What it may do (none for an app token).
    pub scopes: Vec<Scope>,
    /// `user` or `app`.
    pub token_type: Option<String>,
}

/// Reads an [`introspect_request`] answer, `{"data": {"active": …, …}}`. A
/// token Kick does not know at all may get a 401 ([`Error::Status`]) rather
/// than `active: false`.
pub fn parse_introspect(status: u16, body: &str) -> Result<Introspection, Error> {
    let d = http::data(status, body)?;
    Ok(Introspection {
        active: boolean(&d, "active").ok_or_else(|| Error::shape("no `active`"))?,
        client_id: string(&d, "client_id"),
        exp: int(&d, "exp"),
        scopes: string(&d, "scope").as_deref().map(Scope::parse_list).unwrap_or_default(),
        token_type: string(&d, "token_type"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/docs/oauth").join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// RFC 7636, Appendix B.
    const RFC_BYTES: [u8; 32] = [
        116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212, 37, 77, 105, 214, 191,
        240, 91, 88, 5, 88, 83, 132, 141, 121,
    ];
    const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn pkce_matches_the_rfc() {
        let p = Pkce::from_random(&RFC_BYTES).unwrap();
        assert_eq!((p.verifier(), p.challenge()), (RFC_VERIFIER, RFC_CHALLENGE));
        assert_eq!(Pkce::from_verifier(RFC_VERIFIER).unwrap(), p);
    }

    #[test]
    fn pkce_refuses_what_the_rfc_does() {
        for n in [0, 31, 97] {
            assert!(matches!(Pkce::from_random(&vec![7; n]), Err(Error::Invalid(_))), "{n} bytes");
        }
        assert_eq!(Pkce::from_random(&[7; 96]).unwrap().verifier().len(), 128);
        let ok43 = "a".repeat(43);
        assert!(Pkce::from_verifier(&ok43).is_ok());
        assert!(Pkce::from_verifier(&"A-._~z9".repeat(19)[..128]).is_ok());
        for bad in ["a".repeat(42), "a".repeat(129), format!("{}+", "a".repeat(42)), format!("{}=", "a".repeat(42))] {
            assert!(matches!(Pkce::from_verifier(&bad), Err(Error::Invalid(_))), "{bad:?}");
        }
        assert!(Pkce::from_verifier(&format!("{}é", "a".repeat(42))).is_err());
    }

    #[test]
    fn debug_hides_the_verifier() {
        let p = Pkce::from_random(&RFC_BYTES).unwrap();
        let shown = format!("{p:?}");
        assert!(!shown.contains(RFC_VERIFIER) && shown.contains(RFC_CHALLENGE), "{shown}");
        assert!(serde_json::to_string(&p).unwrap().contains(RFC_VERIFIER), "what a binding hands on is untouched");
    }

    #[test]
    fn scopes_round_trip() {
        let all = "ads:read ads:write channel:read channel:write channel:rewards:read channel:rewards:write \
                   chat:write events:subscribe kicks:read moderation:ban moderation:chat_message:manage \
                   streamkey:read user:read";
        let parsed = Scope::parse_list(all);
        assert_eq!(parsed, Scope::ALL.to_vec());
        assert_eq!(Scope::join(&parsed), all);
        for s in Scope::ALL {
            assert_eq!(Scope::from(s.as_str()), s);
            assert_eq!(s.to_string().parse::<Scope>().unwrap(), s);
        }
        assert_eq!(Scope::from("chat:read"), Scope::Other("chat:read".into()));
        assert_eq!(
            Scope::parse_list("  user:read   chat:read "),
            vec![Scope::UserRead, Scope::Other("chat:read".into())]
        );
        assert_eq!(Scope::parse_list(""), vec![]);
        let json = serde_json::to_string(&[Scope::UserRead, Scope::Other("x:y".into())]).unwrap();
        assert_eq!(json, r#"["user:read","x:y"]"#);
        assert_eq!(
            serde_json::from_str::<Vec<Scope>>(&json).unwrap(),
            vec![Scope::UserRead, Scope::Other("x:y".into())]
        );
    }

    #[test]
    fn authorize_url_is_the_documented_get() {
        let p = Pkce::from_random(&RFC_BYTES).unwrap();
        let url = authorize_url(
            &Endpoints::default(),
            "app id",
            "https://app.example/cb?x=1&y=2",
            &[Scope::UserRead, Scope::ChatWrite],
            "st&ate",
            &p,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://id.kick.com/oauth/authorize?response_type=code&client_id=app%20id\
             &redirect_uri=https%3A%2F%2Fapp.example%2Fcb%3Fx%3D1%26y%3D2&scope=user%3Aread%20chat%3Awrite\
             &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&state=st%26ate"
        );
        let other = Endpoints::default().with_id("http://localhost:9");
        assert!(
            authorize_url(&other, "a", "r", &[Scope::UserRead], "s", &p).unwrap().starts_with("http://localhost:9/")
        );
    }

    #[test]
    fn authorize_url_refuses_what_kick_would() {
        let p = Pkce::from_random(&RFC_BYTES).unwrap();
        let e = Endpoints::default();
        let s = [Scope::UserRead];
        for (c, r, sc, st) in [
            ("", "r", &s[..], "s"),
            ("c", "", &s[..], "s"),
            ("c", "r", &s[..], ""),
            ("c", "r", &[][..], "s"),
            ("c", "r", &[Scope::Other("a b".into())][..], "s"),
            ("c", "r", &[Scope::Other(String::new())][..], "s"),
            ("c", "r", &[Scope::Other("a\"b".into())][..], "s"),
        ] {
            assert!(matches!(authorize_url(&e, c, r, sc, st, &p), Err(Error::Invalid(_))), "{c:?} {r:?} {sc:?} {st:?}");
        }
    }

    #[test]
    fn code_and_refresh_are_forms() {
        let e = Endpoints::default();
        let r = code_request(&e, "id", "s&cret", "https://app.example/cb", "c0de", RFC_VERIFIER);
        assert_eq!((r.method, r.url.as_str()), (Method::Post, "https://id.kick.com/oauth/token"));
        assert!(r.headers.contains(&("content-type".into(), "application/x-www-form-urlencoded".into())));
        assert_eq!(
            r.body.as_deref(),
            Some(
                "grant_type=authorization_code&code=c0de&client_id=id&client_secret=s%26cret\
                 &redirect_uri=https%3A%2F%2Fapp.example%2Fcb&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
            )
        );
        let shown = format!("{r:?}");
        assert!(!shown.contains("c0de") && !shown.contains("s%26cret") && !shown.contains(RFC_VERIFIER), "{shown}");

        let r = refresh_request(&e, "id", "sekrit", "r/t");
        assert_eq!((r.method, r.url.as_str()), (Method::Post, "https://id.kick.com/oauth/token"));
        assert_eq!(
            r.body.as_deref(),
            Some("grant_type=refresh_token&refresh_token=r%2Ft&client_id=id&client_secret=sekrit")
        );
        let shown = format!("{r:?}");
        assert!(!shown.contains("sekrit") && !shown.contains("r%2Ft"), "{shown}");
    }

    #[test]
    fn user_token_reads_numbers_either_way() {
        let t = parse_user_token(
            200,
            r#"{"access_token":"a","refresh_token":"r","expires_in":"7200","token_type":"Bearer",
                "scope":"user:read chat:write","refresh_expires_in":2592000,"new_field":1}"#,
        )
        .unwrap();
        assert_eq!(
            t,
            UserToken {
                access_token: "a".into(),
                refresh_token: Some("r".into()),
                expires_in: Some(7200),
                refresh_expires_in: Some(2_592_000),
                token_type: Some("Bearer".into()),
                scopes: vec![Scope::UserRead, Scope::ChatWrite],
            }
        );
    }

    #[test]
    fn debug_hides_the_user_token() {
        let t = parse_user_token(200, r#"{"access_token":"sekrit-a","refresh_token":"sekrit-r","expires_in":7200}"#)
            .unwrap();
        let shown = format!("{t:?}");
        assert!(!shown.contains("sekrit") && shown.contains("[redacted]") && shown.contains("7200"), "{shown}");
        let sent = serde_json::to_string(&t).unwrap();
        assert!(sent.contains("sekrit-a") && sent.contains("sekrit-r"), "what a binding hands on is untouched");
    }

    #[test]
    fn token_errors() {
        let e = parse_user_token(400, &fixture("error.json")).unwrap_err();
        assert_eq!(e.status(), Some(400));
        assert!(matches!(e, Error::Status { ref excerpt, .. } if excerpt.contains("Invalid request")));
        let e = parse_user_token(200, r#"{"error":"invalid_grant"}"#).unwrap_err();
        assert!(matches!(e, Error::Shape(ref s) if s.contains("invalid_grant")), "{e}");
        assert!(matches!(parse_user_token(200, "{}"), Err(Error::Shape(_))));
        assert!(matches!(parse_user_token(200, "OK"), Err(Error::Json(_))));
        assert_eq!(parse_user_token(401, "").unwrap_err().status(), Some(401));
    }

    #[test]
    fn revoke_is_a_query() {
        let e = Endpoints::default();
        let r = revoke_request(&e, "t&k=1", Some(TokenTypeHint::RefreshToken)).unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "https://id.kick.com/oauth/revoke?token=t%26k%3D1&token_type_hint=refresh_token");
        assert_eq!(r.body, None);
        let r = revoke_request(&e, "tok", None).unwrap();
        assert_eq!(r.url, "https://id.kick.com/oauth/revoke?token=tok");
        assert!(matches!(revoke_request(&e, "", None), Err(Error::Invalid(_))));
        assert_eq!(serde_json::to_string(&TokenTypeHint::AccessToken).unwrap(), r#""access_token""#);
    }

    #[test]
    fn revoke_answers() {
        assert!(parse_revoke(200, &fixture("revoke.txt")).is_ok());
        assert!(parse_revoke(200, "").is_ok());
        assert!(parse_revoke(204, "").is_ok());
        let e = parse_revoke(400, &fixture("error.json")).unwrap_err();
        assert_eq!(e.status(), Some(400));
    }

    #[test]
    fn introspect_is_a_bearer_post() {
        let r = introspect_request(&Endpoints::default(), "tok").unwrap();
        assert_eq!((r.method, r.url.as_str()), (Method::Post, "https://id.kick.com/oauth/token/introspect"));
        assert!(r.headers.contains(&("authorization".into(), "Bearer tok".into())));
        assert!(matches!(introspect_request(&Endpoints::default(), "t\r\nx: 1"), Err(Error::Invalid(_))));
    }

    #[test]
    fn introspect_answers() {
        assert_eq!(parse_introspect(401, r#"{"message":"Unauthorized"}"#).unwrap_err().status(), Some(401));
        assert!(matches!(parse_introspect(200, r#"{"data":{},"message":"OK"}"#), Err(Error::Shape(_))));
        assert!(matches!(parse_introspect(200, r#"{"message":"OK"}"#), Err(Error::Shape(_))));
    }

    #[test]
    fn docs_fixtures() {
        // The docs' example: every value an empty string.
        let t = parse_user_token(200, &fixture("token.json")).unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.refresh_token.as_deref(), t.expires_in, t.scopes.len()),
            ("", Some(""), None, 0)
        );
        // The spec's example: numbers, and refresh_expires_in.
        let t = parse_user_token(200, &fixture("token-spec.json")).unwrap();
        assert_eq!(
            t,
            UserToken {
                access_token: "2YotnFZFEjr1zCsicMWpAA".into(),
                refresh_token: Some("tGzv3JOkF0XG5Qx2TlKWIA".into()),
                expires_in: Some(7200),
                refresh_expires_in: Some(2_592_000),
                token_type: Some("Bearer".into()),
                scopes: vec![],
            }
        );
        assert_eq!(
            parse_introspect(200, &fixture("introspect.json")).unwrap(),
            Introspection {
                active: true,
                client_id: Some(String::new()),
                exp: Some(1_771_046_347),
                scopes: vec![Scope::UserRead, Scope::ChannelRead],
                token_type: Some("user".into()),
            }
        );
        assert_eq!(
            parse_introspect(200, &fixture("introspect-spec.json")).unwrap(),
            Introspection {
                active: true,
                client_id: Some("0144Y845NBDSST5H2G1DVY2T25".into()),
                exp: Some(1_719_619_200),
                scopes: vec![Scope::UserRead, Scope::ChatWrite, Scope::ModerationBan],
                token_type: Some("user".into()),
            }
        );
        assert_eq!(
            parse_introspect(200, &fixture("introspect-inactive.json")).unwrap(),
            Introspection { active: false, client_id: None, exp: None, scopes: vec![], token_type: None }
        );
    }
}
