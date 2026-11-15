# Graph Report - kicksdk  (2026-11-15)

## Corpus Check
- 383 files · ~154,439 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 24 file(s) not represented in the graph (top: (none) 13, .jsonl 4, .toml 3)

## Summary
- 3344 nodes · 6784 edges · 192 communities (154 shown, 38 thin omitted)
- Extraction: 95% EXTRACTED · 5% INFERRED · 0% AMBIGUOUS · INFERRED: 342 edges (avg confidence: 0.89)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `bd74f07c`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- api.dart
- kicksdk-nif/src/lib.rs
- build.dart
- kicksdk-ffi/src/lib.rs
- Endpoints
- Kicksdk.Native
- test_user_token.py
- events.rs
- call
- record-api.mjs
- _
- drops.rs
- io_test.dart
- http.rs
- with
- Kicksdk.PublicApi
- string
- rewards.rs
- JsValue
- ChatFeed
- test_aio.py
- to_endpoints
- website.rs
- with
- native.dart
- user_api_test.dart
- user-token.test.js
- Kicksdk.Website
- Chat
- _call
- Backoff
- type_error
- aio.py
- oauth.py
- accept
- dispatch.rs
- Action
- Request
- call
- consumer.ts
- kicksdk-wasm/src/lib.rs
- index.js
- package.json
- rewards.py
- Kick: what we know
- record-realtime.mjs
- mod.rs
- webhooks.rs
- fixtures.rs
- tests/session.rs
- uses
- oauth.rs
- hook_test.dart
- realtime/session.rs
- Scope
- outputs
- index.d.ts
- webhooks.py
- with
- Session
- connectChat
- public_api_actions.rs
- Releasing
- updates
- hex
- bindings/dart/pubspec.yaml
- _request
- content.rs
- jobs
- build-js
- Kick
- ads.rs
- backoff.rs
- env
- Kicksdk.Realtime.Session
- local-socket.test.js
- __init__.py
- out
- contents
- chat.test.js
- kicksdk
- moderation.rs
- freshness
- run
- Kicksdk.Realtime.Backoff
- realtime.py
- WebhookError
- ffi
- jobs
- jobs
- WebhookEvent
- dry_run
- compilerOptions
- 0.3.0
- Pkce
- wheels-test
- main.rs
- ads.py
- Shape
- How kicksdk is built, and why
- include
- python
- Kicksdk.Native.Build
- Kicksdk.Webhooks
- compilerOptions
- drops.py
- revoke_request
- elixir
- record-api.test.mjs
- release-plan.sh
- kicksdk/chat.py
- moderation.py
- QuickBackoff
- PublicKey
- uint
- with
- plan
- examples/dart/pubspec.yaml
- hex-package
- vendored
- jobs
- npm
- pypi
- Kicksdk.UserApiTest
- Kicksdk.MixProject
- Conn
- Freshness
- part
- js
- lint
- build-ffi
- release
- upload-nif
- kicksdk
- Request
- sdist
- wheels
- consistency
- dart
- vendored
- True
- part
- vendored
- Kicksdk.Moderation
- UserToken
- Phase
- Person
- crates-package
- js
- nif-tarballs
- pub-package
- js
- nif
- packages
- vendored
- wheels
- True
- Other
- Pending
- env
- Kicksdk (Elixir)
- kicksdk.h
- publish-dart.sh
- release-check.sh
- upload-release-assets.sh
- without-rust.sh
- docs/public_api/README.md
- webhook/README.md
- build-docs.sh
- build-js.sh
- check-built.sh
- check-dart-archive.sh
- check-glibc.sh
- check-hex-package.sh
- dart-checksums.sh
- ffi-targets.sh
- install-wasm-tools.sh
- package-files.sh
- package-nif.sh
- vendor-native.sh
- version.sh
- REQUIRED_ORIGIN
- kicksdk-py

## God Nodes (most connected - your core abstractions)
1. `Endpoints` - 83 edges
2. `result()` - 73 edges
3. `Kicksdk.Native` - 70 edges
4. `Request` - 68 edges
5. `term()` - 68 edges
6. `err()` - 63 edges
7. `_request()` - 62 edges
8. `to_endpoints()` - 58 edges
9. `_` - 56 edges
10. `_call()` - 56 edges

## Surprising Connections (you probably didn't know these)
- `Dart (kicksdk)` --references--> `connectChat()`  [INFERRED]
  CHANGELOG.md → bindings/js/index.js
- `Dart (kicksdk) and the C ABI (kicksdk-ffi)` --references--> `connectChat()`  [INFERRED]
  CHANGELOG.md → bindings/js/index.js
- `JavaScript (kicksdk-js)` --references--> `connectChat()`  [INFERRED]
  CHANGELOG.md → bindings/js/index.js
- `No I/O in the core` --references--> `connectChat()`  [INFERRED]
  docs/architecture.md → bindings/js/index.js
- `2. Authorization` --references--> `state()`  [INFERRED]
  docs/KICK.md → bindings/js/index.js

## Import Cycles
- 1-file cycle: `crates/kicksdk/tests/public_api_actions.rs -> crates/kicksdk/tests/public_api_actions.rs`

## Communities (192 total, 38 thin omitted)

### Community 0 - "api.dart"
Cohesion: 0.01
Nodes (134): acceptRequest, _actions, adBreakRequest, Ads, authorizeUrl, Backoff, banRequest, body (+126 more)

### Community 1 - "kicksdk-nif/src/lib.rs"
Cohesion: 0.08
Nodes (96): a_backoff_keeps_its_terminal_count_across_calls(), a_busy_session_is_refused_instead_of_waited_for(), a_negative_clock_is_counted_from_its_first_reading(), a_poisoned_session_is_refused_until_reset(), app_token_request(), argument_error(), backoff(), backoff_new() (+88 more)

### Community 2 - "build.dart"
Cohesion: 0.03
Nodes (67): actual, androidEnvironment, arch, args, assetName, base, build, buildFromSource (+59 more)

### Community 3 - "kicksdk-ffi/src/lib.rs"
Cohesion: 0.06
Nodes (29): args(), call(), caught(), _native(), NativeBackoff, NativeSession, panic_message(), result() (+21 more)

### Community 4 - "Endpoints"
Cohesion: 0.07
Nodes (49): Endpoints, endpoints_by_parts(), AppToken, batch(), BATCH, batch_of(), batches_are_capped(), bounded() (+41 more)

### Community 5 - "Kicksdk.Native"
Cohesion: 0.06
Nodes (63): Kicksdk.Native, app_token_request(), backoff_new(), backoff_next_for(), backoff_reset(), call(), call_dirty(), channel_request() (+55 more)

### Community 6 - "test_user_token.py"
Cohesion: 0.07
Nodes (32): files(), response(), test_a_recorded_session_replays(), test_errors_carry_kicks_status(), test_next_delay_for_refuses_a_bad_wait(), test_packaged_licenses_are_the_repositorys(), test_parsers_read_the_recordings(), test_reconnect_codes_and_backoff() (+24 more)

### Community 7 - "events.rs"
Cohesion: 0.04
Nodes (52): Badge, BadgeImage, CHAT_MESSAGE, CHAT_MOVE_TO_SUPPORTED_CHANNEL, ChatMessage, CHATROOM_CLEAR, GIFTED_SUBSCRIPTIONS, GIFTED_SUBSCRIPTIONS_PLAIN (+44 more)

### Community 8 - "call"
Cohesion: 0.06
Nodes (40): Kicksdk.Ads, ad_break_request(), enroll_request(), parse_ad_break(), parse_status(), status_request(), Kicksdk.Channels, update_request() (+32 more)

### Community 9 - "record-api.mjs"
Cohesion: 0.09
Nodes (47): CATALOG_KEEP, cmdActions(), cmdApp(), cmdLogin(), cmdRevoke(), cmdStatus(), cmdUserReads(), COLOR_KEYS (+39 more)

### Community 10 - "_"
Cohesion: 0.04
Nodes (42): _, b, ChatDrop, clientId, clock, close, closed, code (+34 more)

### Community 11 - "drops.rs"
Cohesion: 0.07
Nodes (29): ad_break_request(), MAX_TAGS, patches_what_is_given(), update_request(), body(), delete_request(), MAX_BYTES, MAX_CHARACTERS (+21 more)

### Community 12 - "io_test.dart"
Cohesion: 0.04
Nodes (43): ChatConnection, asked, backoff, chat_, clientIds, closeOnConnect, connect, connections (+35 more)

### Community 13 - "http.rs"
Cohesion: 0.08
Nodes (28): data(), debug_hides_a_token_in_the_query(), debug_hides_credentials(), encode(), Error, Invalid, Json, Shape (+20 more)

### Community 14 - "with"
Cohesion: 0.05
Nodes (41): runs-on, steps, timeout-minutes, concurrency, cancel-in-progress, group, environment, needs (+33 more)

### Community 15 - "Kicksdk.PublicApi"
Cohesion: 0.08
Nodes (32): Kicksdk, describe(), encode(), encode_endpoints(), endpoint!(), endpoints!(), request(), Kicksdk.PublicApi (+24 more)

### Community 16 - "string"
Cohesion: 0.15
Nodes (25): chat_message(), kicks(), parse(), pin(), pin_from_history(), array(), boolean(), decode_twice() (+17 more)

### Community 17 - "rewards.rs"
Cohesion: 0.11
Nodes (32): accept_request(), answers_are_read_leniently(), body(), create_request(), decide(), delete_request(), FailedRedemption, list_request() (+24 more)

### Community 18 - "JsValue"
Cohesion: 0.18
Nodes (34): ads_parse_ad_break(), ads_parse_status(), chat_parse_send(), drops_parse_claims(), http_status(), kicks_leaderboard_request(), oauth_parse_introspect(), oauth_parse_revoke() (+26 more)

### Community 19 - "ChatFeed"
Cohesion: 0.09
Nodes (11): ChatDrop, ChatFeed, apply(), refresh(), _closed_by_socket(), connect_chat(), _describe(), _now() (+3 more)

### Community 20 - "test_aio.py"
Cohesion: 0.13
Nodes (23): backoff_of(), next_event(), run(), wrapper(), stay(), test_a_dropped_connection_or_a_5xx_is_retried(), test_a_panic_in_the_core_stops_the_feed_with_the_error(), test_a_refreshed_token_is_sent_on_the_same_connection() (+15 more)

### Community 21 - "to_endpoints"
Cohesion: 0.13
Nodes (32): ads_ad_break_request(), ads_enroll_request(), ads_status_request(), app_token_request(), ban_request(), channel_request(), channel_user_request(), channels_by_ids_request() (+24 more)

### Community 22 - "website.rs"
Cohesion: 0.16
Nodes (23): acting(), acting_carries_the_session(), ban_request(), channel_request(), channel_user_request(), ChannelUser, delete_message_request(), followers_come_as_numbers_or_strings() (+15 more)

### Community 23 - "with"
Cohesion: 0.10
Nodes (33): environment, if, name, needs, runs-on, steps, timeout-minutes, steps (+25 more)

### Community 24 - "native.dart"
Cohesion: 0.06
Nodes (25): a, abiVersion, args, _call, _checkAbi, _checked, close, _finalizer (+17 more)

### Community 25 - "user_api_test.dart"
Cohesion: 0.07
Nodes (20): KickException, files, fixtures, main, r, response, answer, docs (+12 more)

### Community 26 - "user-token.test.js"
Cohesion: 0.11
Nodes (11): FIXTURES, here, WASM, FakeSocket, docs(), fixture(), body, dir (+3 more)

### Community 27 - "Kicksdk.Website"
Cohesion: 0.07
Nodes (11): Kicksdk.Events, emote_url(), Kicksdk.Website, me_request(), parse_action(), parse_channel(), parse_me(), Kicksdk.ConventionsTest (+3 more)

### Community 28 - "Chat"
Cohesion: 0.14
Nodes (27): Kicksdk.Realtime, chat_channels(), parse_connection(), parse_token(), required_origin(), retry_after_ms(), Chat, act() (+19 more)

### Community 29 - "_call"
Cohesion: 0.07
Nodes (21): emote_url(), parse(), _call(), parse_categories(), parse_channels(), parse_done(), parse_kicks_leaderboard(), parse_livestream_stats() (+13 more)

### Community 30 - "Backoff"
Cohesion: 0.09
Nodes (13): Backoff, _ms(), 0.2.0, Build, Dart (kicksdk) and the C ABI (kicksdk-ffi), Dispatch, For binding authors, Python (kicksdk-py) (+5 more)

### Community 31 - "type_error"
Cohesion: 0.14
Nodes (22): categories_request(), channels_update_request(), chat_channels(), chat_send_request(), drops_claims_request(), id_list(), livestreams_page_request(), nothing() (+14 more)

### Community 32 - "aio.py"
Cohesion: 0.09
Nodes (6): _by_status(), _failure(), _status_of(), KickError, Python (kicksdk-py), main()

### Community 33 - "oauth.py"
Cohesion: 0.09
Nodes (14): authorize_url(), _b64url(), code_request(), introspect_request(), new_pkce(), new_state(), parse_introspect(), parse_revoke() (+6 more)

### Community 34 - "accept"
Cohesion: 0.11
Nodes (23): accept(), chat_message(), recv_json(), dropped(), second(), test_a_failed_refresh_keeps_the_connection_and_a_slow_one_does_not_block_pings(), script(), test_a_full_buffer_drops_the_oldest_and_says_so() (+15 more)

### Community 35 - "dispatch.rs"
Cohesion: 0.10
Nodes (19): a_session_by_name(), a_session_that_panicked_is_refused_until_closed(), answer(), endpoints_refuse_unknown_keys(), errors_keep_kicks_status(), every_listed_function_is_dispatched(), FUNCTIONS, new_public_reads_by_name() (+11 more)

### Community 36 - "Action"
Cohesion: 0.15
Nodes (11): Action, Close, Connected, Event, RefreshToken, Send, Subscribed, SubscribeFailed (+3 more)

### Community 37 - "Request"
Cohesion: 0.15
Nodes (16): Request, _requests(), categories_request(), channels_by_ids_request(), channels_by_slugs_request(), kicks_leaderboard_request(), livestream_stats_request(), livestreams_page_request() (+8 more)

### Community 38 - "call"
Cohesion: 0.27
Nodes (5): Args, call(), DispatchError, webhook_verify(), whole()

### Community 40 - "kicksdk-wasm/src/lib.rs"
Cohesion: 0.11
Nodes (19): drops_update_claims_request(), emote_url(), error(), fail(), flag(), json_arg(), MAX_RETRY_AFTER_MS, MAX_SAFE (+11 more)

### Community 41 - "index.js"
Cohesion: 0.10
Nodes (12): guestId(), monotonicClock(), newPkce(), newState(), noop(), randomBytes(), REQUIRED_ORIGIN, sessionClock() (+4 more)

### Community 42 - "package.json"
Cohesion: 0.08
Nodes (24): description, engines, node, exports, ./kicksdk_bg.wasm, files, keywords, license (+16 more)

### Community 43 - "rewards.py"
Cohesion: 0.11
Nodes (12): accept_request(), create_request(), delete_request(), list_request(), parse_failures(), parse_redemptions(), parse_reward(), parse_rewards() (+4 more)

### Community 44 - "Kick: what we know"
Cohesion: 0.08
Nodes (24): poll(), 1. The sources at a glance, 2. Authorization, 3. Public API (`https://api.kick.com`, `/public/v1` and `/public/v2`), 4.1 Event types, 4.2 Subscriptions, 4.3 Delivery, 4. Webhooks (+16 more)

### Community 45 - "record-realtime.mjs"
Cohesion: 0.15
Nodes (23): Recording, anon(), anonChannel(), anonClose(), anonData(), anonFrame(), anonHttp(), anonHttpValue() (+15 more)

### Community 46 - "mod.rs"
Cohesion: 0.14
Nodes (16): chat_channels(), chat_connection_request(), client_body(), Connection, Centrifugo, Other, Pusher, connection_request() (+8 more)

### Community 47 - "webhooks.rs"
Cohesion: 0.09
Nodes (14): a_documented_ban_and_a_permanent_one(), a_documented_chat_message(), a_documented_redemption(), a_timestamp_with_fractional_seconds_verifies_and_stays_put(), BODIES, bodies_verify_byte_for_byte_and_fresh(), DEFAULT_TOLERANCE_SECS, DOCS (+6 more)

### Community 48 - "fixtures.rs"
Cohesion: 0.17
Nodes (17): a_missing_token_is_an_error_status(), channels_carry_subscriber_counts(), documented(), documented_livestreams_are_read(), documented_reads_are_read(), fixtures(), legacy_chat_payloads_parse(), livestreams_polls_are_read() (+9 more)

### Community 49 - "tests/session.rs"
Cohesion: 0.13
Nodes (12): a_close_serializes_with_the_wait(), a_disconnect_push_closes_and_closed_reports_every_feed(), a_refused_refresh_closes(), a_repeated_unsub_with_nothing_between_closes_instead_of_looping(), a_short_token_is_refreshed_halfway(), a_subscribe_refused_for_now_closes_a_refused_one_for_good_does_not(), an_unsub_push_is_reported(), close_says_how_long_to_wait() (+4 more)

### Community 50 - "uses"
Cohesion: 0.19
Nodes (23): steps, ABI, CARGO_NET_OFFLINE, GLIBC, MIX_ENV, RUSTFLAGS, TOOL, steps (+15 more)

### Community 51 - "oauth.rs"
Cohesion: 0.13
Nodes (12): debug_hides_the_user_token(), docs_fixtures(), fixture(), parse_revoke(), parse_user_token(), REDACTED, revoke_answers(), RFC_BYTES (+4 more)

### Community 52 - "hook_test.dart"
Cohesion: 0.10
Nodes (12): ChecksumMismatch, body, main, Mirror, requests, server, sha, silent (+4 more)

### Community 53 - "realtime/session.rs"
Cohesion: 0.11
Nodes (11): debug_hides_tokens(), DEFAULT_PING_MS, PING_SECS, playback_urls_are_removed_at_any_depth(), redact_tokens(), retry_after_ms(), retryable(), SessionOptions (+3 more)

### Community 54 - "Scope"
Cohesion: 0.10
Nodes (16): Introspection, Scope, AdsRead, AdsWrite, .ALL, ChannelRead, ChannelRewardsRead, ChannelRewardsWrite (+8 more)

### Community 55 - "outputs"
Cohesion: 0.10
Nodes (21): plan, crates, dry_run, ffi, hex, js, nif, npm (+13 more)

### Community 56 - "index.d.ts"
Cohesion: 0.11
Nodes (9): ChatDrop, ChatEvent, ChatOptions, ChatSocket, ChatSocketConstructor, ChatState, init(), kick (+1 more)

### Community 57 - "webhooks.py"
Cohesion: 0.14
Nodes (8): _body(), parse(), _unix_secs(), verify(), parse_channel(), kicksdk (Python), Live API coverage, Dispatch, and the C ABI and Python sessions

### Community 58 - "with"
Cohesion: 0.10
Nodes (20): with, args, command, elixir-version, if-no-files-found, manylinux, maturin-version, merge-multiple (+12 more)

### Community 59 - "Session"
Cohesion: 0.16
Nodes (5): kicksdk (Dart and Flutter), Live API coverage, Session, The realtime session, Examples

### Community 60 - "connectChat"
Cohesion: 0.27
Nodes (18): connectChat(), apply(), arm(), callback(), connect(), detach(), drop(), end() (+10 more)

### Community 61 - "public_api_actions.rs"
Cohesion: 0.20
Nodes (9): ads(), channel_update(), chat(), drops(), Fixture, fixture(), moderation(), rewards() (+1 more)

### Community 62 - "Releasing"
Cohesion: 0.11
Nodes (13): Checks, Contributing, Releasing, When Kick changes, One-time setup, per registry, Platforms, Publishing the Dart package by hand, Re-running part of a release (+5 more)

### Community 63 - "updates"
Cohesion: 0.11
Nodes (18): patterns, patterns, actions, cargo, mix, pip, pub, patterns (+10 more)

### Community 64 - "hex"
Cohesion: 0.11
Nodes (19): run, defaults, environment, if, name, needs, permissions, runs-on (+11 more)

### Community 65 - "bindings/dart/pubspec.yaml"
Cohesion: 0.11
Nodes (17): dependencies, code_assets, crypto, ffi, hooks, description, dev_dependencies, lints (+9 more)

### Community 66 - "_request"
Cohesion: 0.27
Nodes (13): _request(), ban_request(), channel_request(), channel_user_request(), delete_message_request(), history_request(), me_request(), pin_request() (+5 more)

### Community 67 - "content.rs"
Cohesion: 0.16
Nodes (12): emote(), emote_token(), parse(), Part, Emote, Text, text(), Token (+4 more)

### Community 68 - "jobs"
Cohesion: 0.11
Nodes (17): env, CARGO_TERM_COLOR, CROSS_TARGET_AARCH64_UNKNOWN_LINUX_GNU_IMAGE, CROSS_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_IMAGE, CROSS_TARGET_ARM_UNKNOWN_LINUX_GNUEABIHF_IMAGE, CROSS_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_IMAGE, CROSS_TARGET_X86_64_UNKNOWN_LINUX_GNU_IMAGE, CROSS_TARGET_X86_64_UNKNOWN_LINUX_MUSL_IMAGE (+9 more)

### Community 69 - "build-js"
Cohesion: 0.11
Nodes (18): if, name, needs, permissions, uses, with, if, name (+10 more)

### Community 70 - "Kick"
Cohesion: 0.16
Nodes (5): close_4501(), close_4999_at_once(), connect_then_close_4501(), Kick, ping_then_close_3501()

### Community 71 - "ads.rs"
Cohesion: 0.21
Nodes (13): AdBreak, AdBreakState, AdLimits, AdStatus, builds_the_three(), enroll_request(), MAX_BREAK_SECONDS, MIN_BREAK_SECONDS (+5 more)

### Community 72 - "backoff.rs"
Cohesion: 0.26
Nodes (9): Backoff, bounds_are_sane(), doubles_to_the_cap_and_resets(), jitter_spreads_within_the_delay(), spread(), TERMINAL_CAP, terminal_closes_wait_long_and_longer_until_reset(), terminal_waits_are_spread_upwards() (+1 more)

### Community 73 - "env"
Cohesion: 0.12
Nodes (17): CARGO_REGISTRY_TOKEN, DRY_RUN, EVENT, GH_REPO, GH_TOKEN, HEX_API_KEY, MIX_ENV, NODE_AUTH_TOKEN (+9 more)

### Community 74 - "Kicksdk.Realtime.Session"
Cohesion: 0.17
Nodes (14): Kicksdk.Realtime.Session, closed(), connected?(), deadline(), new(), open(), receive(), refresh() (+6 more)

### Community 75 - "local-socket.test.js"
Cohesion: 0.14
Nodes (5): CONNECTED, localKick(), kick, harness(), RFC-6455

### Community 76 - "__init__.py"
Cohesion: 0.16
Nodes (4): update_request(), parse(), _redact_pairs(), _redact_url()

### Community 77 - "out"
Cohesion: 0.18
Nodes (7): connection_request(), now_ms(), oauth_code_request(), oauth_refresh_request(), out(), public_key_request(), Session

### Community 78 - "contents"
Cohesion: 0.13
Nodes (16): if, name, needs, permissions, uses, permissions, build-wheels, packages (+8 more)

### Community 79 - "chat.test.js"
Cohesion: 0.20
Nodes (11): flush(), harness(), advance(), CHANNEL, CHAT_FEED, chatPush(), CHATROOM, CONNECTED (+3 more)

### Community 80 - "kicksdk"
Cohesion: 0.13
Nodes (14): oauth(), OAuth, Kick API compatibility: implemented in core and every binding (uncommitted), Building and testing, Dart / Flutter, Elixir, JavaScript, kicksdk (+6 more)

### Community 81 - "moderation.rs"
Cohesion: 0.35
Nodes (11): at_most(), ban(), ban_request(), bans(), bans_timeouts_and_unbans(), body(), MAX_REASON, MAX_TIMEOUT_MINUTES (+3 more)

### Community 82 - "freshness"
Cohesion: 0.24
Nodes (9): body_bytes(), freshness(), header_pair(), machine_now(), parse_webhook(), verified(), verify_webhook(), webhook_headers() (+1 more)

### Community 83 - "run"
Cohesion: 0.17
Nodes (15): steps, steps, DEPS, PYTHON, RUSTDOCFLAGS, rust, steps, if (+7 more)

### Community 84 - "Kicksdk.Realtime.Backoff"
Cohesion: 0.26
Nodes (11): backoff_next(), Kicksdk.Realtime.Backoff, delay(), fields(), from(), new(), next_delay(), next_delay_for() (+3 more)

### Community 85 - "realtime.py"
Cohesion: 0.14
Nodes (7): chat_channels(), chat_connection_request(), connection_request(), parse_connection(), retry_after_ms(), token_request(), 6.1 Centrifugo (since October 2026)

### Community 86 - "WebhookError"
Cohesion: 0.19
Nodes (11): headers_in_any_case_and_missing_ones_named(), is_ulid(), rfc3339_secs(), signed_headers_must_be_in_their_formats(), WebhookError, BadKey, MalformedHeader, MalformedSignature (+3 more)

### Community 87 - "ffi"
Cohesion: 0.14
Nodes (14): EXT, TARGET, env, if, name, runs-on, timeout-minutes, ffi (+6 more)

### Community 88 - "jobs"
Cohesion: 0.14
Nodes (13): name, runs-on, timeout-minutes, concurrency, cancel-in-progress, group, env, CARGO_TERM_COLOR (+5 more)

### Community 89 - "jobs"
Cohesion: 0.14
Nodes (13): concurrency, cancel-in-progress, group, jobs, upload-ffi, name, permissions, if (+5 more)

### Community 90 - "WebhookEvent"
Cohesion: 0.15
Nodes (13): Category, Reward, WebhookEvent, Banned, ChatMessage, Followed, KicksGifted, LivestreamMetadata (+5 more)

### Community 91 - "dry_run"
Cohesion: 0.15
Nodes (13): default, description, type, dry_run, only, default, description, required (+5 more)

### Community 92 - "compilerOptions"
Cohesion: 0.17
Nodes (11): compilerOptions, exactOptionalPropertyTypes, lib, module, moduleResolution, noEmit, skipLibCheck, strict (+3 more)

### Community 93 - "0.3.0"
Cohesion: 0.17
Nodes (11): 0.1.0, 0.1.1, 0.3.0, Build, Changelog, Dart (kicksdk), Docs, JavaScript (kicksdk-js) (+3 more)

### Community 94 - "Pkce"
Cohesion: 0.23
Nodes (6): authorize_url(), authorize_url_is_the_documented_get(), debug_hides_the_verifier(), Pkce, pkce_matches_the_rfc(), .METHOD

### Community 95 - "wheels-test"
Cohesion: 0.23
Nodes (12): strategy, wheels-test, strategy, fail-fast, matrix, strategy, strategy, name (+4 more)

### Community 97 - "ads.py"
Cohesion: 0.22
Nodes (5): ad_break_request(), enroll_request(), parse_ad_break(), parse_status(), status_request()

### Community 98 - "Shape"
Cohesion: 0.18
Nodes (10): Shape, Banned, Chat, Followed, Gifts, Kicks, Metadata, Redemption (+2 more)

### Community 99 - "How kicksdk is built, and why"
Cohesion: 0.18
Nodes (10): Bindings, Fixtures are the contract, How kicksdk is built, and why, Lenient parsing, strict types, No I/O in the core, Official API recording, Precompiled, and from source, Requests and parsers (+2 more)

### Community 100 - "include"
Cohesion: 0.18
Nodes (11): crate, cross, ext, glibc, libc, manylinux, ndk, os (+3 more)

### Community 101 - "python"
Cohesion: 0.18
Nodes (11): deps, python, python, include, if, name, runs-on, strategy (+3 more)

### Community 102 - "Kicksdk.Native.Build"
Cohesion: 0.22
Nodes (3): Kicksdk.Native.Build, inputs(), __mix_recompile__?()

### Community 103 - "Kicksdk.Webhooks"
Cohesion: 0.22
Nodes (6): dirty?(), Kicksdk.Webhooks, freshness(), public_key(), verify(), Kicksdk.WebhooksTest

### Community 104 - "compilerOptions"
Cohesion: 0.20
Nodes (9): compilerOptions, lib, module, moduleResolution, noEmit, skipLibCheck, strict, target (+1 more)

### Community 105 - "drops.py"
Cohesion: 0.22
Nodes (3): claims_request(), parse_claims(), update_claims_request()

### Community 106 - "revoke_request"
Cohesion: 0.24
Nodes (5): revoke_is_a_query(), revoke_request(), TokenTypeHint, AccessToken, RefreshToken

### Community 107 - "elixir"
Cohesion: 0.20
Nodes (10): defaults, run, defaults, if, name, runs-on, timeout-minutes, elixir (+2 more)

### Community 108 - "record-api.test.mjs"
Cohesion: 0.20
Nodes (5): before, FIXTURES, REAL, REAL_IDS, TOOL

### Community 109 - "release-plan.sh"
Cohesion: 0.36
Nodes (8): builds_wheels(), dry_packages(), has(), needs_release(), needs_vendored(), publish(), release-plan.sh script, want

### Community 110 - "kicksdk/chat.py"
Cohesion: 0.25
Nodes (3): delete_request(), parse_send(), send_request()

### Community 111 - "moderation.py"
Cohesion: 0.28
Nodes (3): ban_request(), timeout_request(), unban_request()

### Community 112 - "QuickBackoff"
Cohesion: 0.22
Nodes (4): QuickBackoff, banned(), test_the_feed_says_while_it_waits_and_stops_waiting_with_the_block(), next_delay_for()

### Community 114 - "uint"
Cohesion: 0.28
Nodes (4): Backoff, number(), retry_after_ms(), uint()

### Community 115 - "with"
Cohesion: 0.22
Nodes (9): with, components, elixir-version, node-version, otp-version, persist-credentials, targets, tool (+1 more)

### Community 116 - "plan"
Cohesion: 0.22
Nodes (9): plan, name, runs-on, steps, timeout-minutes, run, uses, with (+1 more)

### Community 117 - "examples/dart/pubspec.yaml"
Cohesion: 0.25
Nodes (7): dependencies, kicksdk, environment, sdk, path, name, publish_to

### Community 118 - "hex-package"
Cohesion: 0.25
Nodes (8): run, defaults, if, name, runs-on, timeout-minutes, hex-package, working-directory

### Community 119 - "vendored"
Cohesion: 0.25
Nodes (8): CRATE, PKG, vendored, env, if, name, runs-on, timeout-minutes

### Community 120 - "jobs"
Cohesion: 0.25
Nodes (7): concurrency, cancel-in-progress, group, jobs, name, permissions, contents

### Community 121 - "npm"
Cohesion: 0.25
Nodes (8): npm, environment, if, name, needs, permissions, runs-on, timeout-minutes

### Community 122 - "pypi"
Cohesion: 0.25
Nodes (8): pypi, environment, if, name, needs, permissions, runs-on, timeout-minutes

### Community 123 - "Kicksdk.UserApiTest"
Cohesion: 0.33
Nodes (4): parse_done(), Kicksdk.UserApiTest, answer(), done()

### Community 124 - "Kicksdk.MixProject"
Cohesion: 0.38
Nodes (4): Kicksdk.MixProject, deps(), package(), project()

### Community 126 - "Freshness"
Cohesion: 0.38
Nodes (6): Freshness, Skip, Within, Headers, verify(), verify_pem()

### Community 127 - "part"
Cohesion: 0.29
Nodes (7): part, description, required, type, True, workflow_call, inputs

### Community 128 - "js"
Cohesion: 0.29
Nodes (7): js, if, name, runs-on, steps, timeout-minutes, working-directory

### Community 129 - "lint"
Cohesion: 0.29
Nodes (7): lint, if, name, runs-on, steps, timeout-minutes, name

### Community 130 - "build-ffi"
Cohesion: 0.29
Nodes (7): if, name, needs, permissions, uses, with, build-ffi

### Community 131 - "release"
Cohesion: 0.29
Nodes (7): release, if, name, needs, permissions, runs-on, timeout-minutes

### Community 132 - "upload-nif"
Cohesion: 0.29
Nodes (7): upload-nif, if, name, needs, permissions, runs-on, timeout-minutes

### Community 133 - "kicksdk"
Cohesion: 0.33
Nodes (6): kicksdk-py, kicksdk, kicksdk-example, kicksdk-ffi, kicksdk_nif, kicksdk-wasm

### Community 134 - "Request"
Cohesion: 0.40
Nodes (5): code_and_refresh_are_forms(), code_request(), introspect_is_a_bearer_post(), introspect_request(), refresh_request()

### Community 135 - "sdist"
Cohesion: 0.33
Nodes (6): sdist, if, name, runs-on, steps, timeout-minutes

### Community 136 - "wheels"
Cohesion: 0.33
Nodes (6): wheels, if, name, runs-on, steps, timeout-minutes

### Community 137 - "consistency"
Cohesion: 0.33
Nodes (6): if, name, runs-on, steps, timeout-minutes, consistency

### Community 138 - "dart"
Cohesion: 0.33
Nodes (6): if, name, runs-on, steps, timeout-minutes, dart

### Community 139 - "vendored"
Cohesion: 0.33
Nodes (6): vendored, if, name, uses, with, part

### Community 140 - "True"
Cohesion: 0.33
Nodes (6): branches, cron, True, pull_request, push, schedule

### Community 141 - "part"
Cohesion: 0.33
Nodes (6): name, needs, uses, with, ffi, part

### Community 142 - "vendored"
Cohesion: 0.33
Nodes (6): vendored, if, name, needs, permissions, uses

### Community 145 - "Phase"
Cohesion: 0.40
Nodes (5): Phase, Closing, Connected, Connecting, Idle

### Community 146 - "Person"
Cohesion: 0.50
Nodes (4): Person, person(), ReplyTo, who()

### Community 147 - "crates-package"
Cohesion: 0.40
Nodes (5): if, name, runs-on, timeout-minutes, crates-package

### Community 148 - "js"
Cohesion: 0.40
Nodes (5): js, if, name, runs-on, timeout-minutes

### Community 149 - "nif-tarballs"
Cohesion: 0.40
Nodes (5): nif-tarballs, name, needs, runs-on, timeout-minutes

### Community 150 - "pub-package"
Cohesion: 0.40
Nodes (5): pub-package, if, name, runs-on, timeout-minutes

### Community 151 - "js"
Cohesion: 0.40
Nodes (5): js, name, needs, uses, with

### Community 152 - "nif"
Cohesion: 0.40
Nodes (5): nif, name, needs, uses, with

### Community 153 - "packages"
Cohesion: 0.40
Nodes (5): packages, name, needs, uses, with

### Community 154 - "vendored"
Cohesion: 0.40
Nodes (5): vendored, name, needs, uses, with

### Community 155 - "wheels"
Cohesion: 0.40
Nodes (5): wheels, name, needs, uses, with

### Community 156 - "True"
Cohesion: 0.40
Nodes (5): paths, cron, True, pull_request, schedule

### Community 157 - "Other"
Cohesion: 0.50
Nodes (3): authorize_url_refuses_what_kick_would(), Other, scopes_round_trip()

### Community 158 - "Pending"
Cohesion: 0.50
Nodes (4): Pending, Connect, Refresh, Subscribe

### Community 159 - "env"
Cohesion: 0.50
Nodes (4): EVENT, REF_NAME, REF_TYPE, env

## Knowledge Gaps
- **435 isolated node(s):** `Cargo`, `version`, `releases`, `assetName`, `args` (+430 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 1486 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **38 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Endpoints` connect `Endpoints` to `main.rs`, `kicksdk-nif/src/lib.rs`, `dispatch.rs`, `call`, `Request`, `ads.rs`, `kicksdk-wasm/src/lib.rs`, `revoke_request`, `drops.rs`, `http.rs`, `mod.rs`, `moderation.rs`, `rewards.rs`, `oauth.rs`, `to_endpoints`, `website.rs`, `public_api_actions.rs`, `Pkce`?**
  _High betweenness centrality (0.039) - this node is a cross-community bridge._
- **What connects `Cargo`, `version`, `releases` to the rest of the system?**
  _435 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `api.dart` be split into smaller, more focused modules?**
  _Cohesion score 0.014285714285714285 - nodes in this community are weakly interconnected._
- **Why does `connect_chat()` connect `ChatFeed` to `aio.py`, `QuickBackoff`, `kicksdk`, `test_aio.py`, `webhooks.py`, `0.3.0`?**
  _High betweenness centrality (0.036) - this node is a cross-community bridge._
- **Should `kicksdk-nif/src/lib.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.08281756557618626 - nodes in this community are weakly interconnected._
- **Why does `For binding authors` connect `Backoff` to `webhooks.py`, `realtime.py`, `connectChat`, `_call`?**
  _High betweenness centrality (0.036) - this node is a cross-community bridge._
- **Should `build.dart` be split into smaller, more focused modules?**
  _Cohesion score 0.028560250391236306 - nodes in this community are weakly interconnected._