# Fixtures from Kick's docs, not recordings

Everything under `fixtures/docs/` was **transcribed from Kick's docs and spec
on 2026-11-15** (`github.com/KickEngineering/KickDevDocs` and the Swagger
spec at `https://api.kick.com/swagger/doc.json`), not recorded: there was no
token, or no subscription, to record them with. They show the shape Kick
says it sends. A recording that disagrees wins and replaces the file
(`CONTRIBUTING.md`), and `docs/KICK.md` marks what they say *(docs)*.

Each directory's README says where its files came from. The `actions-*`
files in `public_api/` are the user-token (and drops) endpoints: each holds
the documented request (`json`, `params`) and an answer, in the recorder's
format, and `crates/kicksdk/tests/public_api_actions.rs` checks that the
builders make that request and the parsers read that answer.

For those, the request bodies and answers are the spec's definitions filled
with the spec's own `example` values (the drops ones are the drops guide's
examples, verbatim: its claim is a type sketch, `"user_id": "integer"`).
Filled in, and not Kick's:

- `"message": "OK"` in a success answer (what every recorded answer carries);
- `"data": {}` for a ban's answer and `"data": null` for enrolling in ads
  (the spec defines no fields for them);
- `pagination.next_cursor`, which reads `[no example in the spec]`;
- the 204 answers' empty body, which the spec states.

| File | Endpoint | Spec definitions |
|---|---|---|
| `actions-chat-send.json` | `POST /public/v1/chat` | `PostChatParams`, `ChatResp` |
| `actions-chat-delete.json` | `DELETE /public/v1/chat/{message_id}` | 204 |
| `actions-moderation-timeout.json` | `POST /public/v1/moderation/bans` | `PostModerationBansParams`, `PostModerationBansResp` |
| `actions-moderation-unban.json` | `DELETE /public/v1/moderation/bans` | `DeleteModerationBansParams`, `DeleteModerationBansResp` |
| `actions-channels-update.json` | `PATCH /public/v1/channels` | `PatchChannelsParams`, 204 |
| `actions-rewards-list.json` | `GET /public/v1/channels/rewards` | `ChannelReward` |
| `actions-rewards-create.json` | `POST /public/v1/channels/rewards` | `PostChannelRewardsBody`, `ChannelReward` |
| `actions-rewards-update.json` | `PATCH /public/v1/channels/rewards/{id}` | `PatchChannelRewardsBody`, `ChannelReward` |
| `actions-rewards-delete.json` | `DELETE /public/v1/channels/rewards/{id}` | 204 |
| `actions-rewards-redemptions.json` | `GET /public/v1/channels/rewards/redemptions` | `PaginatedResponse-endpoints_RedemptionsByReward` |
| `actions-rewards-accept.json` | `POST /public/v1/channels/rewards/redemptions/accept` | `AcceptChannelRewardRedemptionsParams`, `FailedRedemption` |
| `actions-rewards-reject.json` | `POST /public/v1/channels/rewards/redemptions/reject` | `RejectChannelRewardRedemptionsParams`, `FailedRedemption` |
| `actions-ads-ad-break.json` | `POST /public/v1/ads/ad-break` | `PostAdBreakBody`, `AdBreak` (202) |
| `actions-ads-ad-break-refused.json` | `POST /public/v1/ads/ad-break` | `AdBreakErrorResponse` (400) |
| `actions-ads-status.json` | `GET /public/v1/ads/ad-break-status` | `AdBreakStatus` |
| `actions-ads-enroll.json` | `POST /public/v1/ads/enroll` | `httpx.Response` |
| `actions-drops-claims.json` | `GET /public/v1/drops/claims` | drops guide, "Retrieve Claims" |
| `actions-drops-update.json` | `PATCH /public/v1/drops/claims` | drops guide, "Update Claims" (204) |
| `actions-drops-unauthorized.json` | `GET /public/v1/drops/claims` | drops guide, 401 |

The names, ids and texts are Kick's placeholders, not real people's.

Live recordings from 2026-11-15 now supplement these examples in
`fixtures/public_api/` and `fixtures/id/`; see their READMEs for exact
coverage and untested operations. These files remain documentation-derived,
including optional shapes not observed in the recordings.
