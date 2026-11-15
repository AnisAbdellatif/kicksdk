# Public API recordings

The `20261115T*` fixtures were recorded against Kick on 2026-11-15 with
`tools/record-api.mjs`. Ids, names, free text, asset URLs, credentials and
cursors are anonymized. Raw exchanges remain outside the repository.

Recorded: app-token reads, v2 livestream pagination and filters, users,
categories, stats, user-token reads, leaderboard, rewards/redemptions, ads
status, chat send/reply/delete, timeout/ban/unban, reward create/update/delete.
All successful responses parsed with the SDK during recording.

Expected errors include an invalid cursor (400), insufficient scopes (401),
and deleting an unknown message (404). A bot send returned 404; this does not
establish that bot sends work. Channel title editing was skipped because no
original title was available to restore. All performed action cleanups
succeeded. No ad breaks, enrollment, drops updates or real redemption
accept/reject actions were performed.

These recordings verify observed response shapes, not every documented
limit or optional field. `fixtures/docs/` remains documentation-derived.
