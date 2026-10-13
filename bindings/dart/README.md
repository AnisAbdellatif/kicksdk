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
);
// later: chat.close();
```

`package:kicksdk/kicksdk.dart` has the rest, without `dart:io`:
`Website.channelRequest` / `parseChannel`, `PublicApi.livestreamsRequests`,
`Realtime` and `Session`, `Events.parse`, `Webhooks.verify`, … Requests are
`KickRequest`s for any HTTP client; errors are `KickException` with `kind`
and `status`.

**The native library.** The package's build hook downloads it, precompiled,
from the GitHub release matching the package version, for Linux, macOS,
Windows (x64, arm64), Android (arm64, arm, x64) and iOS (device and
simulator), and checks it against the SHA-256 published with the package. On
any other target, if the download fails, or when asked to, it builds it from
the Rust sources shipped in the package, which needs a Rust toolchain
(`rustup target add <triple>`):

```yaml
# your app's pubspec.yaml
hooks:
  user_defines:
    kicksdk:
      build_from_source: true          # always build
      # binary_base_url: https://…     # a mirror of the release assets
```

Kick's realtime server only accepts `Origin: https://kick.com`; `connectChat`
sends it. MIT OR Apache-2.0. Not affiliated with Kick.
