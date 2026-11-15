# Webhook bodies from Kick's docs

**Not recordings.** These are the example bodies of Kick's documentation
("Webhook Payloads", `events/event-types.md` in
github.com/KickEngineering/KickDevDocs), transcribed on 2026-11-15 for the
three webhook types no receiver of ours has subscribed to, so none has been
recorded:

| File | `Kick-Event-Type` | `Kick-Event-Version` |
|---|---|---|
| `chat.message.sent.json` | `chat.message.sent` | 1 |
| `moderation.banned.json` | `moderation.banned` | 1 |
| `channel.reward.redemption.updated.json` | `channel.reward.redemption.updated` | 1 |

CONTRIBUTING.md asks for a recording before a parser relies on a shape; these
are the documented exception, and `docs/KICK.md` §4.1 marks what they say as
*(docs)*. Replace each with an anonymized recording once one is received, and
correct the parser to the recording where they differ.

Changed from the docs, only to make them JSON: the `//` comments (`identity`
"no identity … at the moment"; `expires_at` "null for permanent bans") and
the trailing commas are removed. Every value is as documented; the names,
ids and URLs are Kick's placeholders, not real people's.
