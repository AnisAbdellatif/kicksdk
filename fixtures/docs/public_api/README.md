# Public API answers from Kick's docs, not recordings

The files here were **transcribed from Kick's docs and spec on 2026-11-15**
(`docs.kick.com`, `github.com/KickEngineering/KickDevDocs` and the Swagger
spec at `https://api.kick.com/swagger/doc.json`). Nobody recorded them: there
was no token to make the requests with. They show the shape Kick says it
sends, not what it does send; a recording that disagrees wins, and replaces
the file (`CONTRIBUTING.md`).

Each body is the spec's response definition filled with the spec's own
`example` values. Two things the spec gives no example for are filled in and
are not Kick's: `"message": "OK"` (what every recorded answer carries) and
`pagination.next_cursor`, which reads `[no example in the spec]`.

| File | Endpoint | Spec definition |
|---|---|---|
| `v1-users-livestreams.json` | `GET /public/v1/users/livestreams` | `endpoints.LivestreamV2` |
| `v2-livestreams.json` | `GET /public/v2/livestreams` | `endpoints.PaginatedResponse-endpoints_LivestreamV2` |
| `v1-livestreams-stats.json` | `GET /public/v1/livestreams/stats` | `endpoints.LivestreamStats` |
| `v1-users.json` | `GET /public/v1/users` | `endpoints.GetUser` |
| `v2-categories.json` | `GET /public/v2/categories` | `endpoints.PaginatedResponse-endpoints_CategoryWithTags` |
| `v1-kicks-leaderboard.json` | `GET /public/v1/kicks/leaderboard` | `endpoints.KicksLeaderboard` |

Live recordings from 2026-11-15 now supplement these examples in
`fixtures/public_api/` and `fixtures/id/`; see their READMEs for exact
coverage and untested operations. These files remain documentation-derived,
including optional shapes not observed in the recordings.
