# kicksdk-js

kicksdk for JavaScript. (On npm the name is `kicksdk-js`: npm treats
`kicksdk` as too close to the unrelated `kick-sdk`.)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, compiled from the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core to WebAssembly,
plus `connectChat`, which drives Kick's chat feed with `fetch` and `WebSocket`.

```js
import * as kick from 'kicksdk-js';
await kick.init();            // in Node: kick.init(fs.readFileSync(wasmPath))

const chat = kick.connectChat({
  chatroomId,
  channelId,
  onEvent: ({ event }) => {
    if (event.type === 'chat_message') console.log(`${event.sender.username}: ${event.content}`);
  },
  onState: ({ connected, subscribed }) => {},
  onDrop: (reason, { code, terminal, waitMs }) => {
    if (terminal) console.warn(`Kick refused this client (${reason}); back in ${waitMs / 60_000} min`);
  },
});
chat.close();
```

`connectChat` keeps the chat up until `close()`. Whenever a connection ends
(Kick went quiet, the socket closed, a request or the handshake took longer
than `timeoutMs`, 10 s by default), `onDrop` says why and how long it waits
before connecting again (`waitMs`): a jittered backoff from 0.5 s to 30 s.
After one of Kick's terminal closes (a refused token, a ban, a connection
limit: `terminal: true`) it never gives up, but waits long - 5, 10, 20, then
30 minutes for terminal closes in a row, each plus up to a quarter - and
comes back as a new guest client (a new client id, so a new token). The chat
feed subscribing again ends the streak. `docs/KICK.md` §6.1 has the rules.
A callback that throws is reported to `onError` (or `console.error`) without
disturbing the connection; none fires after `close()`, and what throws after
it goes to `console.error`. A panic in the WebAssembly is reported too, and
ends that connection; the next one starts on a new session.

A mistake in the options throws from `connectChat` at once: a bad id or
endpoint, a `timeoutMs` that is not a number of ms above 0 and at most
2147483647 (setTimeout's limit), an option that should be a function and is
not. The backoff resets when the first of `channels` (the chat feed, by
default) subscribes, or, with `channels: []`, when Kick accepts the
connection. A `now` clock that returns anything but a number of ms from 0
to `Number.MAX_SAFE_INTEGER` is reported to `onError` once, and
`performance.now()` takes over.

Node 22 and later have a `WebSocket`. On older Node, pass one:
`connectChat({ ..., WebSocket })` with the `ws` package's class. Without
one, `connectChat` throws.

**Where the feed works.** Kick's realtime server only accepts a WebSocket whose
Origin is `https://kick.com`. Node: works (the driver sets the header). Browser:
only on kick.com - a page there or an extension's content script. Elsewhere,
relay through a server.

Everything else - `channelRequest` / `parseChannel`, `historyRequest`,
`livestreamsRequests`, `verifyWebhook`, `parseEvent`, `parseContent`, … -
returns plain request objects for your `fetch` and parses what comes back.
Parsers throw an `Error` with a `status` when Kick answered with an error,
and an `excerpt`: the start of Kick's answer, which often says why
(`CHANNEL_IS_NOT_LIVE`). The message ends with it too.
Every number must be a JS number - a numeric string or a boolean throws a
`TypeError` rather than being converted - and ids, statuses, times and
counts whole ones in range (ids at most `Number.MAX_SAFE_INTEGER`), or a
`RangeError` is thrown rather than the number being rounded into another
id. The same holds for results: one that would carry an integer above
`Number.MAX_SAFE_INTEGER` (an id Kick sent that large) throws rather than
coming back rounded.

**User tokens.** To act as a Kick user (send chat, moderate, edit their
channel, manage rewards), get a user token with OAuth's authorization code
flow and PKCE. The core draws no random numbers: `newPkce()` and `newState()`
take them from `crypto.getRandomValues`.

```js
const pkce = kick.newPkce();                  // { verifier, challenge }
const state = kick.newState();
// keep pkce.verifier and state (a session) until the user comes back
location = kick.oauthAuthorizeUrl(clientId, redirectUri, ['user:read', 'chat:write'], state, pkce.verifier);

// on the redirect: check ?state= is yours, then
const token = kick.oauthParseUserToken(...await call(
  kick.oauthCodeRequest(clientId, clientSecret, redirectUri, code, pkce.verifier)));
// later: oauthRefreshRequest(clientId, clientSecret, token.refresh_token) -> a new pair
```
`oauthRevokeRequest(token, 'access_token' | 'refresh_token')` /
`oauthParseRevoke`, `oauthIntrospectRequest(token)` / `oauthParseIntrospect`
(`{active, client_id, exp, scopes, token_type}`), and `oauthScopes()`, the 13
scopes Kick documents, complete the set; `oauthPkce(bytes)` and
`oauthPkceFromVerifier(verifier)` make a pair from your own 32 to 96 random
bytes or a kept verifier. **The verifier, the client secret and both tokens
are secrets: never log them**, nor a request carrying them
(`oauthRevokeRequest` puts the token in its URL).

With the token (`call` being your `fetch` that returns `[status, text]`):

```js
const sent = kick.chatParseSend(...await call(kick.chatSendRequest(token, { user: broadcasterUserId }, 'hi')));
kick.parseDone(...await call(kick.moderationTimeoutRequest(token, broadcasterUserId, userId, 10, 'spam')));
```
- chat: `chatSendRequest(token, {user} | 'bot', content, replyToMessageId?)` /
  `chatParseSend`, `chatDeleteRequest(token, messageId)`;
- moderation: `moderationBanRequest(token, broadcasterUserId, userId, reason?)`,
  `moderationTimeoutRequest(…, minutes, reason?)`, `moderationUnbanRequest`;
- the token's channel: `channelsUpdateRequest(token, categoryId?, customTags?, streamTitle?)`;
- rewards: `rewardsListRequest` / `rewardsParseRewards`,
  `rewardsCreateRequest(token, fields)` and `rewardsUpdateRequest(token, id, fields)` /
  `rewardsParseReward` (`fields` as Kick names them: `{title, cost, is_enabled, …}`),
  `rewardsDeleteRequest`, `rewardsRedemptionsRequest(token, rewardId?, status?, cursor?)`
  and `rewardsRedemptionsByIdsRequest(token, ids, cursor?)` / `rewardsParseRedemptions`,
  `rewardsAcceptRequest` and `rewardsRejectRequest(token, ids)` / `rewardsParseFailures`;
- ads: `adsAdBreakRequest(token, crypto.randomUUID(), seconds)` / `adsParseAdBreak`,
  `adsStatusRequest` / `adsParseStatus`, `adsEnrollRequest`;
- drops (an app token): `dropsClaimsRequest(token, {campaign_id, user_id, limit, cursor, …})` /
  `dropsParseClaims`, `dropsUpdateClaimsRequest(token, [{claim_id, external_status}])`;
- reads, an app or a user token: `usersRequest(token, userIds?)` / `parseUsers`,
  `categoriesRequest(token, names?, tags?, ids?, limit?, cursor?)` / `parseCategories`,
  `livestreamStatsRequest` / `parseLivestreamStats`, `kicksLeaderboardRequest(token, top?)` /
  `parseKicksLeaderboard`.

Calls whose answer carries nothing are read with `parseDone(status, body)`,
which throws with the `status` unless it is a 2xx. Option objects (`fields`,
the drops filter, the claim updates, `{user}`) refuse a key they do not know.
`index.d.ts` types every result (`UserToken`, `Reward`, `Page<Category>`, …).

**Webhooks.** Read Kick's key once and check each delivery against the raw
body (text or bytes), its headers (pairs, a fetch `Headers`, or an object),
and the clock: the timestamp must be within 5 minutes of `now` (Unix
seconds; this machine's clock by default).

```js
const key = new kick.WebhookPublicKey(pem);   // from publicKeyRequest / parsePublicKey
const { headers, event } = key.verify(req.headers, rawBody);
// or: kick.verifyWebhook(pem, headers, rawBody, { now, toleranceSecs })
// a stored delivery, checked again later: { skipFreshness: true }
```

A valid signature means Kick sent this body to *some* app (every app shares
the key): deduplicate on `headers.message_id`. See `docs/KICK.md` §4.3.

See the repository's `docs/KICK.md` for everything known about Kick, with
sources. MIT OR Apache-2.0. Not affiliated with Kick.

## Live API coverage

The shared core was checked against Kick on 2026-11-15: OAuth code exchange
with PKCE, refresh/revocation/introspection, app and user reads, chat
send/reply/delete, timeout/ban/unban and reward create/update/delete.
Bot send returned 404; channel editing was skipped because no original title
was available to restore. Ad breaks, enrollment, drops and redemption
accept/reject remain documentation-based. See the repository `docs/KICK.md`
and `fixtures/{public_api,id}/README.md` for evidence and limits.

Check granted OAuth scopes before protected calls: Kick can grant fewer than
requested. Never log tokens, client secrets, authorization codes, PKCE
verifiers or requests carrying them.
