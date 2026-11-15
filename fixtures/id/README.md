# OAuth recordings

The `20261115T*` fixtures were recorded against Kick on 2026-11-15 using
`tools/record-api.mjs`: client credentials, authorization code with S256 PKCE,
user-token introspection, refresh, access-token revoke, refresh-token revoke,
and introspection after revocation. Credentials are redacted; raw exchanges
remain outside the repository.

Both revokes returned 200. Introspection after revocation returned 401,
not an `active:false` response. The initial authorization granted four scopes;
a subsequent authorization granted twelve. Applications must inspect granted
scopes rather than assuming all requested scopes were granted.

Every successful response parsed with the SDK during recording. Error
responses were rejected with their HTTP status. The browser redirect used
`http://localhost:4000/auth/kick/callback`; the documented 127.0.0.1 workaround
was not tested live.
