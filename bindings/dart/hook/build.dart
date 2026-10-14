// Provides kicksdk's native library (crates/kicksdk-ffi) to Dart and Flutter.
//
// 1. Precompiled: the library for this target is downloaded from the
//    GitHub release matching this package's version and checked against the
//    SHA-256 recorded in hook/checksums.json when the package was published.
//    A file that does not match is refused - never loaded, never fallen back
//    from: the build fails.
// 2. From source, when there is no precompiled library for the target
//    (Linux arm/riscv64/ia32, musl, Windows ia32, ...), the download fails
//    (offline, not found, TLS, too slow, a binary_base_url that is not an
//    http(s) URL), there are no checksums (a checkout of the repository), or
//    the root package asks for it. Each fallback says why on stderr.
//
// User defines, in the root package's pubspec.yaml:
//
//     hooks:
//       user_defines:
//         kicksdk:
//           build_from_source: true       # never download
//           binary_base_url: https://...  # a mirror of the release assets
//           cargo_path: /opt/rust/bin/cargo
//           cargo_home: /opt/cargo        # if not ~/.cargo
//           rustup_home: /opt/rustup      # if not ~/.rustup
//
// The checksums apply to a mirror too. The hooks runner gives a hook only
// some environment variables (PATH and HOME, not CARGO_HOME or
// RUSTUP_HOME), so a toolchain that is not on PATH, or not in ~/.cargo and
// ~/.rustup, has to be named here. cargo is looked for at `cargo_path`,
// then in `cargo_home`/bin, then on PATH.
//
// Building needs a Rust toolchain with the target installed (`rustup target
// add <triple>`); for Android, the NDK's clang (which Flutter passes in) is
// the linker, aimed at the app's minimum API level. The sources are the
// package's native/ directory, or the repository itself when the package is
// used from a checkout. Unless native/vendor/ holds the crates they depend
// on (`cargo vendor`), in which case the build is offline, a source build
// downloads those from crates.io the first time (or needs them in Cargo's
// cache already, or a vendored source set up in Cargo's config).

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';

/// This package's version: the release the precompiled libraries come from.
/// scripts/release-check.sh keeps it equal to pubspec.yaml's.
const version = '0.2.0';
const releases = 'https://github.com/AnisAbdellatif/kicksdk/releases/download';
const assetName = 'kicksdk_ffi';

void main(List<String> args) async {
  await build(args, (input, output) => provide(input, output));
}

/// The hook. `checksums` replaces hook/checksums.json, for tests.
Future<void> provide(BuildInput input, BuildOutputBuilder output, {Map<String, String>? checksums}) async {
  if (!input.config.buildCodeAssets) return;
  final code = input.config.code;
  final os = code.targetOS;
  final arch = code.targetArchitecture;
  final musl = os == OS.linux && Platform.isLinux && hostIsMusl();
  final triple = targetTriple(
    os,
    arch,
    iosSimulator: os == OS.iOS && code.iOS.targetSdk == IOSSdk.iPhoneSimulator,
    musl: musl,
  );
  if (triple == null) {
    throw UnsupportedError('kicksdk: no Rust target for $os/$arch, so no library to download or build');
  }
  final fileName = os.dylibFileName('kicksdk_ffi');
  final dest = input.outputDirectory.resolve(fileName);
  final release = '$assetName-v$version-$triple.${fileName.split('.').last}';

  final plan = planDownload(
    triple: triple,
    release: release,
    forced: isSet(input.userDefines['build_from_source']),
    checksums: checksums ?? await readChecksums(input.packageRoot.resolve('hook/checksums.json')),
    baseUrl: input.userDefines['binary_base_url'],
  );
  var ready = false;
  switch (plan) {
    case Download(:final url, :final sha256):
      final failed = await download(url, dest, sha256);
      if (failed == null) {
        ready = true;
      } else {
        log('could not download $url ($failed); building from source');
      }
    case FromSource(:final reason):
      log('$reason; building from source');
  }
  if (!ready) {
    final root = sourceRoot(input.packageRoot);
    if (root == null) {
      throw StateError('kicksdk: no precompiled library for $triple, and no sources to build one from');
    }
    final built = await buildFromSource(input, code, root, triple, fileName);
    await File.fromUri(built).copy(dest.toFilePath());
    output.dependencies.add(root.resolve('crates/'));
  }

  output.assets.code.add(
    CodeAsset(package: input.packageName, name: assetName, linkMode: DynamicLoadingBundled(), file: dest),
  );
}

void log(String message) => stderr.writeln('kicksdk: $message');

/// A user define that is on: `true`, or "true" from the command line.
bool isSet(Object? define) => define == true || define == 'true';

/// Rust's name for a target with a precompiled library: the release
/// builds exactly these. scripts/ffi-targets.sh reads the list from this
/// function's string literals, so each triple stays a literal here, and a
/// target that is only built from source goes in [sourceOnlyTriple].
String rustTriple(OS os, Architecture arch, {bool iosSimulator = false}) {
  String? t;
  if (os == OS.linux) {
    t = {Architecture.x64: 'x86_64-unknown-linux-gnu', Architecture.arm64: 'aarch64-unknown-linux-gnu'}[arch];
  } else if (os == OS.macOS) {
    t = {Architecture.x64: 'x86_64-apple-darwin', Architecture.arm64: 'aarch64-apple-darwin'}[arch];
  } else if (os == OS.windows) {
    t = {Architecture.x64: 'x86_64-pc-windows-msvc', Architecture.arm64: 'aarch64-pc-windows-msvc'}[arch];
  } else if (os == OS.android) {
    t = {
      Architecture.arm64: 'aarch64-linux-android',
      Architecture.arm: 'armv7-linux-androideabi',
      Architecture.x64: 'x86_64-linux-android',
    }[arch];
  } else if (os == OS.iOS) {
    t = {
      Architecture.arm64: iosSimulator ? 'aarch64-apple-ios-sim' : 'aarch64-apple-ios',
      Architecture.x64: 'x86_64-apple-ios',
    }[arch];
  }
  if (t == null) throw UnsupportedError('kicksdk: no precompiled library for $os/$arch');
  return t;
}

/// Rust's name for a target the release has no library for, which is
/// built from source: other Linux, Windows and Android architectures, and
/// Linux on musl. Null if Rust has no target for it.
String? sourceOnlyTriple(OS os, Architecture arch, {bool musl = false}) {
  if (os == OS.linux && musl) {
    return {
      Architecture.x64: 'x86_64-unknown-linux-musl',
      Architecture.arm64: 'aarch64-unknown-linux-musl',
      Architecture.arm: 'armv7-unknown-linux-musleabihf',
      Architecture.ia32: 'i686-unknown-linux-musl',
      Architecture.riscv64: 'riscv64gc-unknown-linux-musl',
    }[arch];
  }
  if (os == OS.linux) {
    return {
      Architecture.arm: 'armv7-unknown-linux-gnueabihf',
      Architecture.ia32: 'i686-unknown-linux-gnu',
      Architecture.riscv64: 'riscv64gc-unknown-linux-gnu',
    }[arch];
  }
  if (os == OS.windows) return {Architecture.ia32: 'i686-pc-windows-msvc'}[arch];
  if (os == OS.android) {
    return {Architecture.ia32: 'i686-linux-android', Architecture.riscv64: 'riscv64-linux-android'}[arch];
  }
  return null;
}

/// The Rust target to download or build for: [rustTriple], or else
/// [sourceOnlyTriple] (always, on musl, where a glibc library would not
/// load). Null if there is none.
String? targetTriple(OS os, Architecture arch, {bool iosSimulator = false, bool musl = false}) {
  if (!musl) {
    try {
      return rustTriple(os, arch, iosSimulator: iosSimulator);
    } on UnsupportedError {
      // Not precompiled; maybe buildable.
    }
  }
  return sourceOnlyTriple(os, arch, musl: musl);
}

/// Whether this process runs on musl (Alpine, ...), where a library built
/// for glibc does not load: the C library mapped into it says. Only a mapped
/// file's own name counts, so a directory such as `build-musl-ci/` on a
/// glibc system does not.
bool hostIsMusl([String? maps]) {
  try {
    maps ??= File('/proc/self/maps').readAsStringSync();
  } on FileSystemException {
    return false;
  }
  for (final line in const LineSplitter().convert(maps)) {
    // The path is the last field; a line without one (`[heap]`, anonymous
    // memory) has no '/', and its start is an address, which matches nothing.
    final name = line.substring(line.lastIndexOf('/') + 1);
    if (name.startsWith('ld-musl-') || name.startsWith('libc.musl-')) return true;
  }
  return false;
}

Future<Map<String, String>> readChecksums(Uri file) async {
  final f = File.fromUri(file);
  if (!await f.exists()) return const {};
  return (jsonDecode(await f.readAsString()) as Map<String, dynamic>).cast<String, String>();
}

/// What to do for the library: download it, or build it, and why.
sealed class Plan {}

final class Download extends Plan {
  final Uri url;
  final String sha256;
  Download(this.url, this.sha256);
}

final class FromSource extends Plan {
  final String reason;
  FromSource(this.reason);
}

Plan planDownload({
  required String triple,
  required String release,
  required bool forced,
  required Map<String, String> checksums,
  Object? baseUrl,
}) {
  if (forced) return FromSource('build_from_source is set');
  if (checksums.isEmpty) return FromSource('no hook/checksums.json (a checkout of the repository)');
  final expected = checksums[release];
  if (expected == null) return FromSource('no precompiled library for $triple');
  final base = baseUrl ?? '$releases/v$version';
  final parsed = base is String ? Uri.tryParse(base.endsWith('/') ? base : '$base/') : null;
  if (parsed == null || !(parsed.isScheme('http') || parsed.isScheme('https')) || parsed.host.isEmpty) {
    return FromSource('binary_base_url `$base` is not an http(s) URL with a host');
  }
  return Download(parsed.resolve(release), expected);
}

/// A download whose SHA-256 is not the one published with the package.
class ChecksumMismatch implements Exception {
  final Uri url;
  final String actual;
  final String expected;
  ChecksumMismatch(this.url, this.actual, this.expected);

  @override
  String toString() =>
      'kicksdk: $url has SHA-256 $actual, expected $expected. Refusing it, and not building from source '
      'instead: a library that does not match may have been tampered with. Set build_from_source: true '
      'to build it yourself.';
}

/// Whether a failed download is a reason to build from source: the network,
/// TLS or the server failed, or took too long. Anything else (a full disk,
/// a bug) is not, and a [ChecksumMismatch] never is.
bool fallsBack(Object error) =>
    error is SocketException ||
    error is HttpException ||
    error is TlsException ||
    error is TimeoutException ||
    error is OSError;

/// Downloads `url` to `dest` if its SHA-256 is `expected`. Returns null when
/// done, or why it could not be had (offline, not found, TLS, no answer in
/// time); throws [ChecksumMismatch] when it was had and is wrong.
Future<String?> download(
  Uri url,
  Uri dest,
  String expected, {
  Duration connectTimeout = const Duration(seconds: 30),
  Duration timeout = const Duration(minutes: 5),
}) async {
  final client = HttpClient()..connectionTimeout = connectTimeout;
  HttpClientRequest? request;
  Future<(int, Uint8List)> fetch() async {
    final req = request = await client.getUrl(url);
    final res = await req.close();
    if (res.statusCode != 200) {
      await res.drain<void>();
      return (res.statusCode, Uint8List(0));
    }
    final bytes = await res.fold(BytesBuilder(copy: false), (all, chunk) => all..add(chunk));
    return (200, bytes.takeBytes());
  }

  try {
    final (status, bytes) = await fetch().timeout(
      timeout,
      onTimeout: () {
        request?.abort();
        throw TimeoutException('no complete answer', timeout);
      },
    );
    if (status != 200) return 'answered $status';
    final actual = sha256.convert(bytes).toString();
    if (actual != expected) throw ChecksumMismatch(url, actual, expected);
    await File.fromUri(dest).writeAsBytes(bytes);
    return null;
  } catch (e) {
    if (e is ChecksumMismatch || !fallsBack(e)) rethrow;
    return '$e';
  } finally {
    client.close(force: true);
  }
}

/// The Cargo workspace to build from: native/ in a published package, the
/// repository root in a checkout.
Uri? sourceRoot(Uri packageRoot) {
  for (final candidate in [packageRoot.resolve('native/'), packageRoot.resolve('../../')]) {
    if (File.fromUri(candidate.resolve('crates/kicksdk-ffi/Cargo.toml')).existsSync()) return candidate;
  }
  return null;
}

/// Where cargo is, and what its environment needs on top of the hook's.
typedef Cargo = ({String executable, Map<String, String> environment});

/// Finds cargo: `cargoPath`, then `cargoHome`/bin (`CARGO_HOME` when the
/// runner passed it), then PATH. Not ~/.cargo/bin when it is not on PATH:
/// a build with no Rust on PATH must not find one there (the release checks
/// the precompiled path that way). A cargo found by path gets its directory
/// first on PATH, so its rustc is the one used, and `CARGO_HOME` and
/// `RUSTUP_HOME` are passed on when known: the hooks runner drops them from
/// the environment.
Cargo locateCargo({
  required Map<String, String> env,
  required bool windows,
  String? cargoPath,
  String? cargoHome,
  String? rustupHome,
  bool Function(String path)? exists,
}) {
  final has = exists ?? (p) => File(p).existsSync();
  final sep = windows ? r'\' : '/';
  final exe = windows ? 'cargo.exe' : 'cargo';
  final environment = <String, String>{};
  cargoHome ??= env['CARGO_HOME'];
  rustupHome ??= env['RUSTUP_HOME'];
  if (cargoHome != null) environment['CARGO_HOME'] = cargoHome;
  if (rustupHome != null) environment['RUSTUP_HOME'] = rustupHome;
  if (env['RUSTUP_TOOLCHAIN'] case final toolchain?) environment['RUSTUP_TOOLCHAIN'] = toolchain;

  String? inHome() {
    if (cargoHome == null) return null;
    final bin = cargoHome.endsWith(sep) ? '${cargoHome}bin' : '$cargoHome${sep}bin';
    return has('$bin$sep$exe') ? '$bin$sep$exe' : null;
  }

  final cargo = cargoPath ?? inHome();
  if (cargo == null) return (executable: 'cargo', environment: environment);
  if (cargo.contains(sep)) {
    final path = env['PATH'] ?? env['Path'] ?? '';
    final dir = cargo.substring(0, cargo.lastIndexOf(sep));
    environment['PATH'] = path.isEmpty ? dir : '$dir${windows ? ';' : ':'}$path';
  }
  return (executable: cargo, environment: environment);
}

/// The environment that makes cargo link for Android with the NDK's clang:
/// clang is told the target and the minimum API level, as the NDK's
/// `<triple><api>-clang` wrappers would (a bare clang builds for the host).
Map<String, String> androidEnvironment({
  required String triple,
  required int api,
  required Uri compiler,
  required Uri archiver,
}) {
  // Rust's armv7 target is armv7a for clang.
  final clangTarget = '${triple == 'armv7-linux-androideabi' ? 'armv7a-linux-androideabi' : triple}$api';
  final key = triple.toUpperCase().replaceAll('-', '_');
  final low = triple.replaceAll('-', '_');
  return {
    'CARGO_TARGET_${key}_LINKER': compiler.toFilePath(),
    'CARGO_TARGET_${key}_RUSTFLAGS': '-C link-arg=--target=$clangTarget',
    'CC_$low': compiler.toFilePath(),
    'CFLAGS_$low': '--target=$clangTarget',
    'AR_$low': archiver.toFilePath(),
  };
}

/// For a musl target: a cdylib needs the C runtime linked dynamically,
/// which is not Rust's default there.
Map<String, String> muslEnvironment(String triple) => {
  'CARGO_TARGET_${triple.toUpperCase().replaceAll('-', '_')}_RUSTFLAGS': '-C target-feature=-crt-static',
};

/// cargo's arguments. With the dependencies' sources vendored in the
/// workspace's vendor/ (`cargo vendor`), cargo is pointed at them and kept
/// offline; their checksums are in vendor/ and Cargo.lock.
List<String> cargoArgs(String triple, {required bool vendored}) => [
  'build',
  '--release',
  '--locked',
  if (vendored) ...[
    '--offline',
    '--config',
    'source.crates-io.replace-with="vendored-sources"',
    '--config',
    'source.vendored-sources.directory="vendor"',
  ],
  '-p',
  'kicksdk-ffi',
  '--target',
  triple,
];

Future<Uri> buildFromSource(BuildInput input, CodeConfig code, Uri root, String triple, String fileName) async {
  final targetDir = input.outputDirectoryShared.resolve('cargo/');
  final cargo = locateCargo(
    env: Platform.environment,
    windows: Platform.isWindows,
    cargoPath: input.userDefines.path('cargo_path')?.toFilePath(),
    cargoHome: input.userDefines.path('cargo_home')?.toFilePath(),
    rustupHome: input.userDefines.path('rustup_home')?.toFilePath(),
  );
  final env = <String, String>{'CARGO_TARGET_DIR': targetDir.toFilePath(), ...cargo.environment};
  final cc = code.cCompiler;
  if (code.targetOS == OS.android && cc != null) {
    env.addAll(
      androidEnvironment(triple: triple, api: code.android.targetNdkApi, compiler: cc.compiler, archiver: cc.archiver),
    );
  }
  if (triple.contains('-musl')) env.addAll(muslEnvironment(triple));
  final args = cargoArgs(triple, vendored: Directory.fromUri(root.resolve('vendor/')).existsSync());
  log('${cargo.executable} ${args.join(' ')} (in ${root.toFilePath()})');
  final help =
      'It needs Rust (https://rustup.rs) and `rustup target add $triple`. A toolchain not on PATH, or outside '
      '~/.cargo and ~/.rustup, is named with the cargo_path, cargo_home and rustup_home user defines (see '
      'hook/build.dart). The first build downloads crates from crates.io.';
  final ProcessResult run;
  try {
    run = await Process.run(
      cargo.executable,
      args,
      workingDirectory: root.toFilePath(),
      environment: env,
      runInShell: Platform.isWindows,
    );
  } on ProcessException catch (e) {
    throw StateError('kicksdk: could not run cargo to build from source ($e). $help');
  }
  if (run.exitCode != 0) {
    throw StateError('kicksdk: building from source failed. $help\n${run.stderr}');
  }
  return targetDir.resolve('$triple/release/$fileName');
}
