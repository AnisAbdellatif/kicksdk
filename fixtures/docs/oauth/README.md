# OAuth examples from Kick's docs and spec

**These are not recordings.** They were transcribed on 2026-11-15 from Kick's
developer docs (github.com/KickEngineering/KickDevDocs@main,
`getting-started/generating-tokens-oauth2-flow.md`) and API spec
(`https://api.kick.com/swagger/doc.json`), because no user-token flow has
been recorded yet (it needs a Kick account to approve an app). Replace or
join them with recordings when there are some (see CONTRIBUTING.md).

| File | Where it comes from |
|---|---|
| `token.json` | The docs' token-endpoint answer (authorization code and refresh), as shown: every value an empty string. |
| `token-spec.json` | The example in the spec's `/oauth/token` description. Its tokens are RFC 6749's sample values. It has `refresh_expires_in` and no `scope`. |
| `error.json` | The docs' 400 answer of `/oauth/authorize`, `/oauth/token` and `/oauth/revoke`. |
| `revoke.txt` | The docs' 200 answer of `/oauth/revoke`. |
| `introspect.json` | The docs' `/oauth/token/introspect` example, without its `// "app" or "user"` comment (not JSON). |
| `introspect-spec.json` | The spec's `endpoints.PostTokenIntrospectResponse` examples, in its `httpx.Response` envelope (`data`, `message`). |
| `introspect-inactive.json` | Made from the spec's words "When active=false there is no additional information added in the response". |

The parsers in `crates/kicksdk/src/oauth.rs` read every one of them.
