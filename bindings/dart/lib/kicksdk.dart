/// Kick (kick.com) without the I/O: requests, parsers, the realtime chat
/// protocol and webhook verification, from kicksdk's Rust core.
///
/// For a ready chat connection with `dart:io`, see `package:kicksdk/io.dart`.
library;

export 'src/api.dart';
export 'src/native.dart' show KickException;
