# Kick: what we know

Everything learnt about Kick as a data source, across KickTracker, BetterChat,
KickPlus and Viewmux: the official API and webhooks, Kick's own website
endpoints, and the realtime chat feed. kicksdk is built on it, and this is its
canonical copy: **when Kick behaves differently from what is written here,
correct it here first**, then the fixtures, then the code (see
[CONTRIBUTING.md](../CONTRIBUTING.md)).

Each fact says where it comes from: Kick's **docs**; a **recording**
(anonymized into `fixtures/`, by KickTracker's recorder or
`tools/record-realtime.mjs`); a **probe** (one request per endpoint); KickTracker's
**sandbox** in real-Kick mode; **production** (what KickTracker's collector has
received since 2026-09-24); or a **client** (what BetterChat and KickPlus do
against the live site, and what Kick's own frontend bundle does).

"We" and "our" in §1–4 are KickTracker, where most of those facts were
learnt; its use is kept beside each one because the reason for a choice is
part of the fact (polling every 60 s because the count changes once a minute).

Last updated 2026-10-13.

---

## 1. The sources at a glance

| Source | Status | Auth | What only it gives | Our use |
|---|---|---|---|---|
| Public API `api.kick.com/public/v1` | Official, documented | App token | Viewer counts, channel state, subscriber totals | Polled: `/livestreams` every 60s, `/channels` every 5 min |
| Webhooks | Official, documented | App token subscribes; Kick signs deliveries | Follows, subs, gifts, Kicks as they happen; stream start/end, metadata | 7 event types per channel, through our ingress |
| v2 website API `kick.com/api/v2` | Undocumented, Kick's own frontend | None | **Follower total**, **chatroom id** | One channel per request: every 15 min live, daily offline |
| Other website endpoints | Undocumented | None (mostly) | Past streams, gift leaderboards, clips | Probed only; nothing uses them |
| Realtime feed (Centrifugo, `*.platform.kick.com`) | Unofficial (what kick.com's chat uses) | A guest token, no account | **Chat**, **hosts (raids)**, moderation events, Kicks, live/offline | One socket per chat; Origin must be kick.com (§6) |

Nothing gives: viewer history before we started tracking, a livestream id in
the official API, unfollows, audience demographics, or personal facts.

## 2. Authorization

- **App access token** via the OAuth client-credentials grant:
  `POST https://id.kick.com/oauth/token` with `grant_type=client_credentials`,
  `client_id`, `client_secret` (form-encoded). **Lasts 60 days**
  (`expires_in` 5 184 000). *(recording)*
- The app token reads **public data of any channel** and **subscribes to
  webhooks for any channel** by `broadcaster_user_id`, no permission from
  the channel owner. The docs say app tokens "have full permission"; a user
  token needs the `events:subscribe` scope and only covers its own channel.
  *(docs; confirmed in production for every event type we use)*
- A **user token** (authorization code + PKCE) is only for acting as someone
  (chat, moderation, editing a channel) or private data. Not used.
- Webhook subscriptions **belong to the Kick app**: two deployments sharing
  one app remove each other's subscriptions (our sync deletes what it
  doesn't track). The sandbox therefore has its own app.

## 3. Public API (`https://api.kick.com/public/v1`)

### Endpoints we use

| Endpoint | Gives | Batching |
|---|---|---|
| `GET /livestreams?broadcaster_user_id=…` | `broadcaster_user_id`, `channel_id`, `slug`, `viewer_count`, `started_at`, `stream_title`, `category` {id, name, thumbnail}, `language`, `has_mature_content`, `thumbnail`, `profile_picture` (the docs also list tags; none in the recorded answers) | **50 ids per request** (the parameter repeated) |
| `GET /channels?broadcaster_user_id=…` or `?slug=…` | slug, broadcaster id, title, category, `stream` (is_live, viewer_count, start, language, tags), description, banner, **`active_subscribers_count`**, **`active_gifted_subscribers_count`**, **`canceled_subscribers_count`** | **50 per request**, by id or by slug, not mixed |
| `GET /public-key` | The PEM key webhooks are signed with | – |
| `GET/POST/DELETE /events/subscriptions` | Webhook subscriptions (§4.2) | – |

`/livestreams` also filters by `category_id` and `language`, sorts by
`viewer_count` and takes `limit` up to 100: usable for category rankings.
*(docs)*

### Behaviour observed

- **`viewer_count` refreshes about once a minute**: median 61s between
  changes, shortest 46s, over 40 polls at 15s. Polling faster sees the same
  value repeated, so we poll every 60s. *(recording)*
- **Subscriber counts are filled for channels that never authorized us**
  (real non-zero values with the app token). *(recording, production)*
- **No follower count** anywhere in the public API. *(docs, recording)*
- **An unknown slug fails the whole request**: 400 `Invalid request`, not an
  empty result. Slugs are checked one at a time; batches use ids.
  *(recording)*
- **Offline** `/livestreams` answers 200 `{"data": [], "message": "OK"}`.
- **No rate-limit headers**, so the limits are unknown. We stay batched,
  back off on 429 and never probe for them. *(recording)*
- **The API lags Kick's own state**: a stream was still listed 3s after
  `ended_at` and gone 19s after; a start webhook can arrive before
  `/livestreams` lists the stream. Hence the 90s grace before a poll counts
  a channel offline. *(recording)*
- `stream.key` and `stream.url` are empty strings for an app token.
- **No livestream id** in `/livestreams` (nor in the status webhook). A
  stream is identified by `(channel, started_at)`; `started_at` is
  **string-identical** between `/livestreams` and both status webhooks
  (`YYYY-MM-DDTHH:MM:SSZ`, second precision). A short disconnect Kick
  treats as the same stream keeps its `started_at`. *(recording)*
- The 5-minute `/channels` answer is our source of truth for **renames**:
  it reports each broadcaster id's current slug. A renamed or banned
  channel can vanish from the answer. *(production)*
- In 24 hours of production, three `/channels` requests failed for the
  whole batch at once, each an isolated moment; cause not investigated.

## 4. Webhooks

### 4.1 Event types

The ten types Kick documents, with what production has received
(2026-09-24 → 26, 47 channels):

| Event | Version | Received | Body (fields seen) | Our use |
|---|---|---|---|---|
| `livestream.status.updated` | 1 | 110 | `broadcaster`, `is_live`, `title`, `started_at`, `ended_at` | Stream start and end |
| `livestream.metadata.updated` | 1 | 139 | `broadcaster`, `metadata` {title, language, `has_mature_content`, `category` **and** `Category`} | Title/category changes |
| `channel.followed` | 1 | 3 457 | `broadcaster`, `follower` | Gross follows |
| `channel.subscription.new` | 1 | 31 | `broadcaster`, `subscriber`, `duration`, `created_at`, `expires_at` | New subs |
| `channel.subscription.renewal` | 1 | 14 | same as new | Resubs |
| `channel.subscription.gifts` | 1 | 222 | `broadcaster`, `gifter`, `giftees[]`, `created_at`, `expires_at` | Gifted subs |
| `kicks.gifted` | 1 | 367 | `broadcaster`, `sender`, `gift` {amount, name, type, tier, message, pinned_time_seconds}, `created_at` | Kicks |
| `moderation.banned` | – | not subscribed | bans and timeouts | Chat feed has richer moderation (§6.4) |
| `channel.reward.redemption.updated` | – | not subscribed | channel-point redemptions | – |
| `chat.message.sent` | – | not subscribed | every chat message | Chat comes from Pusher |

Every type we subscribe to has now been delivered with the app token for
channels that never authorized us. Only status, metadata and follows are
recorded in `fixtures/`; the sub, gift and Kicks shapes above come from
production's stored bodies (key names only) and should be recorded and
anonymized into fixtures before a parser change relies on a new field.

**A person** in any body (`broadcaster`, `follower`, `subscriber`,
`gifter`, `giftees[]`, `sender`) is `{user_id, username, channel_slug,
is_verified, is_anonymous, profile_picture, identity}` (`identity` null so
far; `kicks.gifted`'s broadcaster has neither `is_anonymous` nor
`identity`).

Details worth knowing:

- **Start and end are one event type.** Start: `is_live: true, ended_at:
  null`. End: `is_live: false` with `ended_at`. Both carry the same
  `started_at`. The status event's `title` is the title *at that moment*,
  so it is not a source of title changes. *(recording)*
- **`livestream.metadata.updated` is a full snapshot**, sent when any field
  changes; which one changed is found by comparing with the previous one.
  The category is sent twice, as `category` and `Category`, same value; we
  read the lowercase one. *(recording)*
- **Metadata and follow bodies carry no time**: when it happened is the
  `Kick-Event-Message-Timestamp` header. *(recording)*
- **Anonymous gifts**: `gifter.is_anonymous: true`, with `user_id`,
  `username`, `channel_slug`, `profile_picture` null (3 of 222). One gift
  event covered **1 to 25 giftees**. *(production)*
- **`duration` is in months** (1 for a new sub; 3 to 20 seen on renewals).
  On a renewal, `created_at` is **when the subscription first started**
  (dates back to 2025), not when this renewal happened; use the header
  time. *(production)*
- `created_at` / `expires_at` have **varying fractional precision** (6 or 9
  digits, `Z`); parse any.
- **Kicks**: `gift.type` / `gift.tier` seen as `BASIC`/`BASIC` (amounts 1,
  10, 50, 100) and `LEVEL_UP`/`MID` (500, 2 000). `gift.name` is the gift's
  display name, `pinned_time_seconds` 0 for basic, 600 and 2 400 for the
  larger ones. `gift.message` is free text from the sender: we never read
  it. *(production)*

### 4.2 Subscriptions

- `POST /public/v1/events/subscriptions` with
  `{"broadcaster_user_id": <id>, "events": [{"name": "<type>", "version": 1}, …], "method": "webhook"}`.
  One subscription per channel per event type. **No URL in the request**:
  the delivery URL is a setting of the Kick app.
- `GET /public/v1/events/subscriptions` lists all of the app's
  subscriptions.
- `DELETE /public/v1/events/subscriptions?id=…&id=…` removes them.
  **A request removing ~330 ids got 400 `Invalid request` every time**; we
  remove 50 per request (Kick's limit elsewhere; the real limit is
  undocumented). *(sandbox)*
- Limits *(docs)*: 10 000 subscriptions per event type per app;
  `chat.message.sent` capped at 1 000 for apps Kick hasn't verified.
- "If an app's webhook continually fails to process an event for over a
  day, Kick automatically unsubscribes the app from that event." *(docs)*
  Our sync (on boot and every 15 minutes) restores anything missing.

### 4.3 Delivery

- Headers: `Kick-Event-Message-Id` (ULID), `Kick-Event-Subscription-Id`,
  `Kick-Event-Signature` (base64), `Kick-Event-Message-Timestamp` (RFC 3339,
  `…Z`, second precision), `Kick-Event-Type`, `Kick-Event-Version`.
- **Signature**: RSA, SHA-256, PKCS#1 v1.5 over
  `<message id>.<timestamp>.<raw body>`, verified with the key from
  `/public/v1/public-key`. **Type, subscription id and version headers are
  not signed**: the receiver checks them against the envelope schema.
  Every recorded delivery verified. *(recording)*
- **The same event can arrive twice**: key on the message id.
- **What a valid signature proves**: that Kick sent *this body*, at *that
  timestamp*, under *that message id*, to **some** app. Every app verifies
  with the same public key, so another app's operator (or anyone holding a
  delivery Kick sent elsewhere) can replay it to your receiver, and the
  type, version and subscription-id headers can be changed at will. A
  receiver should therefore:
  - refuse a timestamp far from its clock (kicksdk: 5 minutes either way
    by default; deliveries arrive within a second or two);
  - keep the message ids it has seen for at least that window and drop
    repeats (needed for Kick's own duplicates anyway);
  - read the body by its own shape, not only the type header (kicksdk
    refuses a body that lacks the claimed type's own field - `is_live`,
    `metadata` with a title/language/category, `follower`, `subscriber`,
    `giftees`, `gift` - which no other type's body has; fields it does not
    know are ignored, so a field Kick adds does not break deliveries;
    `.new` and `.renewal` subscriptions share one shape and cannot be told
    apart);
  - expect the message id to be a ULID and the timestamp RFC 3339: neither
    can then contain the `.` that separates the signed parts.
- **Fast**: 0.6 to 0.9s on average from Kick's timestamp to our receipt, for
  every type; an end event arrived 5s after its `ended_at`. *(production,
  recording)*
- **Retries** are implied by the docs ("for over a day") but the policy is
  not documented, and **not seen**: on 2026-09-28 our receivers were
  unreachable for 26 minutes (the main VPS lost its network); of the
  webhooks Kick sent then, only the two that got through at the time
  exist, and none arrived afterwards (checked two hours later).
  *(production, once)* Treat a failed delivery as lost; the ingress
  Worker retries on the backup receiver itself (KickTracker's project.md §15.2).
  **If Kick does retry**, the retry most likely carries the original message
  id and timestamp (they are what is signed), so a freshness check refuses
  one that comes later than its tolerance (kicksdk: `Stale`). 5 minutes
  suits a receiver that is up; one that would rather take late retries
  than lose them can widen the tolerance (to a day, say) and must then
  remember message ids for as long, since that window is also how long a
  captured delivery can be replayed. *(inference, unverified)*
- **Missed webhooks are lost.** Stream state and metadata can be recovered
  by polling; follows, subs, gifts and Kicks cannot.
- No viewer-count event and **no raid/host event** among webhooks.

## 5. Kick's website endpoints (undocumented)

Kick's own frontend API. Can change or be blocked at any time and is a grey
area under Kick's terms: everything using it is isolated, optional, and a
failure is a gap, never a zero.

### 5.1 v2 channel: `GET https://kick.com/api/v2/channels/<slug>`

- The **only source of the follower total** (`followers_count`) and of the
  **chatroom id** (`chatroom.id`), which chat needs and no official
  endpoint gives. We read those two fields and drop the rest there.
- `followers_count` came as a **number in one recording and a string in
  another**: parse both. *(recording)*
- The channel id is repeated as `chatroom.chatable_id`. *(recording)*
- The response carries a **`playback_url` with a signed token**: the
  stream's HLS master playlist (Viewmux plays it). kicksdk passes it on for
  players; treat it as a credential, never log or store it, and redact it in
  recordings.
- **Answers from a datacenter IP**: production has read followers from the
  VPS every 15 minutes since 2026-09-24 with no failed reading. *(production)*
- By slug, so a rename makes it 404 until the new slug is known.

### 5.2 Other endpoints probed (2026-09-24, from a home machine)

| Endpoint | Result | Possible use |
|---|---|---|
| `api.kick.com/private/v1/channels/{slug}` | 200, no auth | Fallback follower source: its count was **0.06% higher than v2's** at almost the same moment, and it uses **opaque string ids** (`channel_…`, `user_…`). A channel's follower history must come from one source only |
| `api.kick.com/private/v1/livestreams` | 200: all live streams by viewers, 20 per page, cursor | Rankings (the official `/livestreams` is preferred) |
| `kick.com/current-viewers?ids[]=` | 200: `[{livestream_id, viewers, show_view_count}]` | Several streams' viewers in one call |
| `kick.com/api/v2/channels/{slug}/videos` | 200: the last ~27 days of streams; `duration` in **milliseconds**; `viewer_count` **0 for every finished stream** | Airtime and category history from before tracking |
| `kick.com/api/v1/channels/{slug}` | 200 (51 KB): the same past streams, `followersCount` equal to v2's | Alternative to the above |
| `kick.com/api/v2/channels/{slug}/leaderboards` | 200: top gifters all time (10), month (10), week (5) | Partial gift history; a cross-check |
| `kick.com/api/v2/channels/{slug}/clips` | 200: clips with `view_count`, `likes_count`, cursor | A later clips feature |
| `kick.com/api/v2/channels/{slug}/livestream` | 200: live stream details, `viewers` | – |
| `kick.com/api/v2/channels/{slug}/chatroom` | 200: chat settings (followers-only, slow mode…) | Context for chat activity |
| `kick.com/api/v2/channels/{slug}/subscribers/last` | 401 (needs a login) | – |
| `api.kick.com/channels/:id/followers-count`, `api.kick.com/private/v0/channels/:id/viewer-count`, `…/videos/latest`, `private/v1/channels/{slug}/clips` | 404 | Gone |

Source for candidates: the community list fb-sean/kick-website-endpoints.

### 5.3 Reading and acting from a client

What BetterChat and KickPlus use against the live site. *(client, 2026-10)*

| Endpoint | Auth | Gives / does |
|---|---|---|
| `GET kick.com/api/v2/channels/<channel id>/messages` | None | Recent chat (`data.messages`, newest first) and `data.pinned_message` (the pin event's shape plus `finish_at`). Note: by **channel id**, not slug |
| `GET kick.com/api/v2/channels/<slug>/users/<username>` | None | A user as the channel sees them: `following_since`, `subscribed_for`, `is_moderator`, `banned`, badges. **404** when the channel does not know the name (BetterChat uses it to tell `@user` from `@word`) |
| `GET kick.com/api/v2/channels/<slug>/me` | Session | `is_moderator`, `is_broadcaster`, `is_super_admin`; 401/403 signed out |
| `POST kick.com/api/v2/channels/<slug>/bans` | Session | Ban `{permanent: true, banned_username, reason: null}`, timeout `{permanent: false, banned_username, reason: null, duration: <minutes>}` |
| `DELETE kick.com/api/v2/channels/<slug>/bans/<username>` | Session | Lift a ban or timeout |
| `DELETE kick.com/api/v2/chatrooms/<chatroom id>/messages/<message id>` | Session | Delete a message (needs the **chatroom** id) |
| `POST kick.com/api/v2/channels/<slug>/pinned-message` | Session | Pin `{message: <the whole message as the feed sent it>, duration: <minutes>}`: Kick hands the message back to every viewer as the banner |
| `DELETE kick.com/api/v2/channels/<slug>/pinned-message` | Session | Unpin (there is only ever one) |
| `POST kick.com/api/v2/messages/send/<chatroom id>` | Session | A reply: `{content, type: "reply", message_ref, metadata: {original_message: {id, content}, original_sender: {id, username}}, thread_parent_id}`. `message_ref` is any string (Kick's page sends the clock); the feed echoes it back |

- **Session**: kick.com keeps the viewer's session in the `session_token`
  cookie and sends it as `Authorization: Bearer <it>`, with
  `x-app-platform: web`. Readable from the page today; never log or store it.
- **CORS**: `kick.com/api/v2` answers cross-origin reads from other sites
  (BetterChat on betterchat.tech reads channels, history and users this way).
- **A full browser User-Agent from a non-browser is refused**: Node's `fetch`
  with a Chrome UA got 403 `{"error": "Request blocked by security policy."}`
  on `kick.com/api/v2/channels/<slug>`, while `User-Agent: Mozilla/5.0` (or
  curl's own) passed. A client that is not a browser should not pretend to be
  one. *(2026-10-13)*

## 6. Realtime chat feed

### 6.1 Centrifugo (since October 2026)

Kick moved its realtime feed from Pusher to [Centrifugo](https://centrifugal.dev).
The Pusher app key (`32cbd69e4b950bf97679`, cluster `us2`) stopped working:
every cluster answers `{"event":"pusher:error","data":{"code":4001,"message":"App
key 32cbd69e4b950bf97679 not in this cluster…"}}` and closes. Noticed
2026-10-13 when BetterChat, KickPlus and KickTracker all lost chat at once.
*(client)*

**Finding the server** *(client: Kick's frontend bundle, then live)*

- Kick's page asks `POST https://web.kick.com/api/v1/realtime/channels/<channel id>/chat/connection`
  (a channel's chat) or `POST …/realtime/connection` (general), body
  `{"client": {"id": "<uuid>", "type": "web"}, "capabilities": {"accepted_providers": [{"provider": "pusher"}, {"provider": "centrifugo"}]}}`.
- Answer: `{"data": {"connections": [{"provider": "centrifugo", "credentials": {"url": "wss://realtime.<region>.platform.kick.com/connection/websocket"}}], "mode": "websocket"}, "message": "success"}`.
  Only Centrifugo is offered now; Kick's client would still accept
  `{"provider": "pusher", "credentials": {"app_key", "cluster"}}`.
- **Each channel's chat has a region**: one channel answered `us-west-2`,
  another `us-east-1`, while the general endpoint answered `us-east-1`. Ask
  per channel.

**The token**

- `POST https://web.kick.com/api/v1/realtime/auth/connection` with
  `{"client_id": "<the same uuid>"}` → `{"data": {"token": "<JWT>"}}`. No
  session needed: the JWT's subject is `guest:<client id>`. **Lives 45
  minutes** (`exp − iat` = 2700). A signed-in page gets a user token from the
  same call with its session.
- Feeds named `private-…` take a per-feed token from
  `POST …/realtime/auth/channel`; a guest is refused them (below).

**Origin and CORS** *(client)*

- The WebSocket handshake is **refused (403) unless `Origin` is
  `https://kick.com`** (or `https://www.kick.com`). `https://betterchat.tech`,
  `http://localhost:8000` and others: 403.
- The two `web.kick.com` endpoints answer CORS for `https://kick.com` only.
- So a browser page can only use the feed **on kick.com** (a page there, or an
  extension's content script, whose requests carry the page's origin).
  Elsewhere it has to go through a server that sets the header - BetterChat's
  `/realtime` relay does that. Servers (Node, Python, Elixir) set it directly.
- No other Centrifugo transport is exposed: `/connection/http_stream`, `sse`,
  `uni_*` are all 404.

**Protocol** (Centrifugo JSON protocol; server `"version": "6.9.7 PRO"`)

- Client: `{"id":1,"connect":{"token":"<JWT>","name":"js"}}`.
- Reply: `{"id":1,"connect":{"client":"<uuid>","version":"6.9.7 PRO","expires":true,"ttl":2700,"ping":25,"pong":true}}`.
- Subscribe: `{"id":N,"subscribe":{"channel":"<feed>"}}` → `{"id":N,"subscribe":{}}`.
  Several commands can go in one frame, a line each, and several replies
  come back in one frame, a line each.
- **Ping**: the server sends an empty frame `{}` every `ping` (25) seconds;
  the client answers `{}` (`pong: true`).
- **Refresh**: `{"id":N,"refresh":{"token":"<new JWT for the same client id>"}}` →
  `{"id":N,"refresh":{"expires":true,"ttl":2700}}`. Worked live; a whole
  45-minute expiry has not been watched yet.
- **Publications**: `{"push":{"channel":"<feed>","pub":{"data":{"event":"<Kick event name>","data":"<JSON string>"},"tags":{"event":"<Kick event name>"}}}}`.
  The `data` inside is exactly what Pusher carried, so §6.2–6.4 still hold.
- **Refusals**: subscribing to `private-chatroom_<id>` as a guest:
  `{"id":N,"error":{"code":103,"message":"permission denied"}}`.
- **Disconnect and unsubscribe codes** follow Centrifugo's conventions
  *(Centrifugo docs; Kick's server is Centrifugo 6)*: a disconnect (a
  `push.disconnect`, or the WebSocket close frame's code) of 3000-3499 or
  4000-4499 is worth a reconnect, **3500-3999 and 4500-4999 are terminal**
  (reconnecting as before would be refused again). A refused connect or
  refresh is worth retrying only for 109 (token expired), an error marked
  `temporary`, or a transport error. An `unsub` push with a code from
  **2500 to 2999 asks the client to subscribe again**; kicksdk's session
  does so itself. None of these has been seen from Kick yet.
- **No `ping` in the connect reply** means the server will not ping (a
  Centrifugo server can be set up so); the 25 s silence rule then does not
  apply, and kicksdk does not close a quiet feed. Kick has always sent
  `ping: 25` so far.
- **No validation of ids**: subscribing to `chatrooms.999999999.v2` (no such
  chatroom) succeeds, and nothing ever arrives.

**The feeds for one chat** (Kick's page joins all five) *(client: Kick's bundle,
BetterChat; recording)*

| Feed | Carries |
|---|---|
| `chatrooms.<chatroom id>.v2` | Chat, deletions, bans, subs, clears, pins, polls, **hosts arriving** (`StreamHostEvent`) |
| `chatrooms.<chatroom id>` | The same hosts again, as `StreamHostedEvent` (another shape: `message {numberOfViewers, optionalMessage}`, a whole `user`). Counting both counts every host twice |
| `chatroom_<chatroom id>` | Gifted subs (`GiftedSubscriptionsEvent`, **unprefixed**), reward redemptions (`RewardRedeemedEvent`, unprefixed) |
| `channel.<channel id>` | Live / offline (`StreamerIsLive`, `StopStreamBroadcast`), **hosts leaving** (`ChatMoveToSupportedChannelEvent`) |
| `channel_<channel id>` | Kicks (`KicksGifted`, unprefixed) |

Subscribing to `.v2` alone was why gifted subs never reached BetterChat for a
while: the handler was right, and nothing was ever delivered to it.

**Limits**: subscriptions per connection and connections per IP are unknown.
BetterChat's relay holds one connection per chatroom from one VPS.

### 6.1a Pusher (until October 2026, kept for the record)

- `wss://ws-us2.pusher.com/app/32cbd69e4b950bf97679?protocol=7&client=js&version=8.4.0&flash=false`
  (the app key kick.com shipped to every visitor), any Origin accepted.
- Subscribe with `{"event": "pusher:subscribe", "data": {"auth": "", "channel": "<name>"}}`:
  no auth, no account. *(recording)*
- The server's first frame (`pusher:connection_established`, with
  `activity_timeout` 120) often arrived in the same read as the HTTP 101
  upgrade: a client had to decode those bytes or it never subscribed.
- `pusher:ping` / `pusher:pong` keepalive.
- A frame's `data` was a JSON string inside the JSON (occasionally already an
  object): decode twice. The same holds for the Kick envelope inside a
  Centrifugo publication.
- Worked from a datacenter IP: KickTracker kept 47 channels' sockets up from
  its VPS from 2026-09-24. Recordings: `fixtures/pusher-legacy/`.

### 6.2 Chat messages: `App\Events\ChatMessageEvent`

Fields: `id` (UUID), `chatroom_id`, `content`, `type` (`message` or
`reply`), `created_at`, `sender` {`id`, `username`, `slug`, `identity`
{`color`, `badges`, `badges_v2`}}, `metadata` {`message_ref`, and on a
reply `original_message` {id, content}, `original_sender` {id, username}},
`thread_parent_id` on replies. *(recording)*

- `badges`: `{type, text, count?}` (`moderator`, `vip`, `og`, `broadcaster`,
  `founder`, `verified`, `staff`, `bot`, `subscriber` with months as
  `count`, `sub_gifter`). `badges_v2`: `{name, badge_type, image_url,
  metadata {level}, selected, sort_order}`; **`selected: false` means the user
  has the badge and chose to hide it**. *(recording, client)*

- Times end in **`+00:00`**, not `Z`.
- **Emotes** are inline tokens in `content`: `[emote:<id>:<name>]`. The
  image is `https://files.kick.com/emotes/<id>/fullsize`.

### 6.3 Hosts (Kick's raids)

Kick calls raids hosts. One host shows on both sides, **under a second
apart** when both channels are tracked; **neither event carries a time**.
*(production, sandbox; fixtures:
`fixtures/pusher/20260926T203429Z-pusher__*.jsonl`)*

| Event | Feed | Fields |
|---|---|---|
| `App\Events\StreamHostEvent` | the **receiving** channel's `chatrooms.<id>.v2` | `chatroom_id`, `host_username`, `number_viewers`, `optional_message` (free text, may be null or `""`) |
| `App\Events\ChatMoveToSupportedChannelEvent` | the **hosting** channel's `channel.<id>` | `slug` (the hosted channel), `hosted` {id (a channel id), slug, username, `viewers_count`, `is_live`, category, `profile_pic`, `preview_thumbnail`}, and the whole hosting `channel` (with its `playback_url` and `current_livestream`) |

- The receiving side names the host **only by username** (no id, no
  slug), and its **case can differ from the slug's**: match
  case-insensitively.
- Both viewer counts were the **hosting channel's own viewer count** at the
  time (the viewers taken along).
- The hosting side carries the hoster's playback URL: we store only
  `slug` and `hosted` {slug, username, viewers_count}. kicksdk's realtime
  session removes every `playback_url` from an event's data before handing
  it on: it is a signed URL for the stream, and nothing read from the feed
  needs it.

### 6.4 Moderation and other events

Seen on logged channels' chatrooms in production (one day, two channels),
and by BetterChat on the live feed:

| Event | Count | Fields |
|---|---|---|
| `App\Events\UserBannedEvent` | 665 | `id`, `user` {id, slug, username}, `banned_by` {id, slug, username}, `permanent`, `duration` (timeouts only, minutes: 1 to 5 seen), `expires_at` |
| `App\Events\UserUnbannedEvent` | 3 | `id`, `user`, `unbanned_by`, `permanent` |
| `App\Events\MessageDeletedEvent` | 19 | `id`, `message` {id}, `aiModerated`, `violatedRules[]` (17 of 19 were AI-moderated, each with rules listed) |
| `App\Events\PinnedMessageCreatedEvent` | 10 | `message` (a full chat message), `pinnedBy`, `duration` (a string, `"1200"` every time: seconds, presumably) |
| `App\Events\PinnedMessageDeletedEvent` | 2 | `data` is an empty array |
| `App\Events\StopStreamBroadcast` | 1 | `livestream` {id, channel} |

| `App\Events\ChatroomClearEvent` | – | (nothing needed) |
| `App\Events\SubscriptionEvent` | – | `username`, `months` |
| `GiftedSubscriptionsEvent` (on `chatroom_<id>`; `App\Events\…` also accepted) | – | `gifter_username`, `gifted_usernames[]` |
| `KicksGifted` (on `channel_<id>`) | – | `gift_transaction_id`, `message` (**beside** `gift`, not inside it; only the larger gifts carry one), `sender` {id, username, `username_color`, profile_picture}, `gift` {gift_id, name, amount, type, tier, character_limit, pinned_time}, `created_at` |
| `RewardRedeemedEvent` (on `chatroom_<id>`) | – | Shape has varied: title as `reward_title` or `reward.title`, typed input as `user_input`; read both |
| `App\Events\StreamerIsLive` (on `channel.<id>`) | – | `livestream` {id, `session_title`, …} |
| `App\Events\PollUpdateEvent` / `PollDeleteEvent` | – | `poll` {…, `remaining`, `result_display_duration`} |

Permanent bans far outnumber timeouts (659 against 6). Deleted messages
name only the message id: the text has to come from the chat log.
*(the rows without counts: client, BetterChat)*

## 7. Identities, names and images

- **Ids differ per object**: a channel has a **user id**
  (`broadcaster_user_id`, what the public API and webhooks use), a
  **channel id** (`/livestreams`' `channel_id`, the `channel.<id>` feed,
  `hosted.id`), and a **chatroom id** (v2 only, the chat feed). The website
  API's private/v1 uses opaque strings instead.
- **Slug vs username**: a slug is the lowercase form of the username in the
  cases seen; usernames keep their case. Kick allows renames: the slug of
  a broadcaster id can change, and endpoints taking a slug then 404.
- **No livestream id** in the official API or webhooks; v2 and the website
  have one, never relied on.
- **Pictures**: a channel's picture is `profile_picture` in `/livestreams`
  and in webhook people (`broadcaster.profile_picture`), a URL on
  `files.kick.com`. We copy it (PNG/JPEG/GIF/WebP only, ≤ 1 MB) so
  visitors' browsers never contact Kick. Emotes come from
  `files.kick.com/emotes/<id>/fullsize` (admin pages only).

## 8. Open questions

- **Webhook retry policy**: whether Kick ever redelivers after a failed
  delivery. Seen once not to within two hours (§4); a deliberate test
  (stop the ingress, trigger an event, watch for a day) would settle it.
- **Subscription removal limit**: 50 per `DELETE` works, ~330 doesn't; the
  real limit is unknown.
- **Public API rate limits**: unknown (no headers); not to be probed.
- **Realtime limits**: subscriptions per connection, connections per IP,
  and whether Kick rate-limits the token endpoint.
- **A full token lifetime**: refresh works, but no connection has yet been
  watched through a 45-minute expiry, nor what Kick sends when a token
  lapses unrefreshed (Centrifugo's usual is a disconnect with code 3005).
- **Not recorded yet** (shapes known only from production's stored bodies
  or the docs): `channel.subscription.*`, `kicks.gifted`,
  `moderation.banned`, `channel.reward.redemption.updated`,
  `chat.message.sent`, and the moderation chat-feed events. Record and
  anonymize before relying on new fields.
- **Units inferred, not confirmed**: a timeout's `duration` (minutes, from
  values 1–5). A pin's `duration`: the feed sends `"1200"` every time, and
  **the two readings disagree** - KickTracker took it for seconds (20
  minutes), BetterChat for minutes (20 hours), and Kick's pin call is sent
  minutes (§5.3). The history endpoint's absolute `finish_at` settles any one
  pin; kicksdk passes the number through without a unit.
- **Sending a plain message**: only replies have been sent (§5.3); a plain
  message is presumably the same call with `type: "message"` and no
  `metadata`, unverified.

## 9. Where this lives in kicksdk

| What | Where |
|---|---|
| Ids kept apart (user, channel, chatroom) | `crates/kicksdk/src/ids.rs` |
| Public API, app token, public key, subscriptions | `crates/kicksdk/src/public_api.rs` |
| Website API (§5) | `crates/kicksdk/src/website.rs` |
| Realtime endpoints and the Centrifugo session (§6.1) | `crates/kicksdk/src/realtime/` |
| Chat-feed events (§6.2–6.4) | `crates/kicksdk/src/events.rs` |
| Emotes in message text | `crates/kicksdk/src/content.rs` |
| Webhook headers, signature, bodies (§4) | `crates/kicksdk/src/webhooks.rs` |
| Recorded, anonymized payloads | `fixtures/` (realtime ones by `tools/record-realtime.mjs`) |

KickTracker's own copy of this file, its recorder and its simulator are in
that repository (`KICK.md`, `sim/`).
