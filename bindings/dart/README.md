# kicksdk (Dart and Flutter)

Kick (kick.com) without the I/O: requests, response parsers, the realtime chat
protocol and webhook verification, from the
[kicksdk](https://github.com/AnisAbdellatif/kicksdk) Rust core, through
`dart:ffi`.

```dart
import 'package:kicksdk/io.dart';

final chat = connectChat(
  chatroomId: chatroomId,
  channelId: channelId,
  onEvent: (e) {
    final event = e['event'];
    if (event['type'] == 'chat_message') print('${event['sender']['username']}: ${event['content']}');
  },
  onDrop: (reason) => print('dropped: $reason'), // a reconnect always follows
  onError: (error, stack) => print('error: $error'), // e.g. onEvent threw
);
// later: chat.close();  `chat.done` completes then, and only then.
```

`connectChat` never gives up by itself (docs/KICK.md §6.1). An ordinary
drop reconnects after a jittered 1 s, doubling to 30 s. A terminal close
(Kick refused the guest token or the connection, or closed with a code that
means it would refuse the same client again: 3500-3999, 4500-4999) waits 5
minutes, then 10, 20 and 30 for more in a row, and reconnects as a new
client; the chat feed subscribing starts the waits over. `onDropInfo` gets
each drop as a `ChatDrop`: `reason`, `code`, `terminal` and `retryIn`, the
wait before the next attempt. Pass your own `Backoff` to change the waits;
`Backoff.nextDelayFor(retryAfterMs, random)` is the rule, for a driver of
your own around `Session`.

Each request and the WebSocket handshake time out after `timeout` (10 s)
and are retried like any other failure. An exception thrown by `onEvent` or
`onState` goes to `onError` (stderr without it); the frames after it are
still handled.

`package:kicksdk/kicksdk.dart` has the rest, without `dart:io`:
`Website.channelRequest` / `parseChannel`, `PublicApi.livestreamsRequests`,
`Realtime` and `Session`, `Events.parse`, `Webhooks.verify` (the raw body as
bytes or text; it checks the delivery's timestamp against `now`, 5 minutes
either way, unless `skipFreshness`), … Requests are
`KickRequest`s for any HTTP client; errors are `KickException` with `kind`
and `status`.

**The native library.** The package's build hook downloads it, precompiled,
from the GitHub release matching the package version, for Linux (glibc),
macOS, Windows (x64, arm64), Android (arm64, arm, x64) and iOS (device and
simulator), and checks it against the SHA-256 published with the package. A
download that does not match fails the build: it is never loaded, and never
quietly replaced by a source build. Otherwise the hook builds the library
from the Rust sources shipped in the package, and says why on stderr: on any
other target (Linux arm, riscv64 or musl, Windows ia32, ...), when the
download fails (offline, not found, TLS, too slow), or when asked to. That
needs a Rust toolchain with the target (`rustup target add <triple>`); for
Android, the NDK's clang that Flutter passes in links it, for the app's
minimum API level.

```yaml
# your app's pubspec.yaml
hooks:
  user_defines:
    kicksdk:
      build_from_source: true          # always build
      # binary_base_url: https://…     # a mirror of the release assets
      # Hooks only see PATH and HOME, not CARGO_HOME or RUSTUP_HOME. A
      # toolchain not on PATH, or outside ~/.cargo and ~/.rustup, is named
      # here:
      # cargo_path: /opt/rust/bin/cargo
      # cargo_home: /opt/cargo
      # rustup_home: /opt/rustup
```

A source build fetches the crates it depends on from crates.io the first
time; the package carries kicksdk's own sources but not theirs. To build
offline, fetch them beforehand (`cargo fetch` in the package's `native/`
directory, with the same `CARGO_HOME`), or point Cargo at a vendored copy
(`cargo vendor`, and the `[source]` replacement it prints, in
`$CARGO_HOME/config.toml`).

Kick's realtime server only accepts `Origin: https://kick.com`; `connectChat`
sends it. MIT OR Apache-2.0. Not affiliated with Kick.
