// Provides kicksdk's native library (crates/kicksdk-ffi) to Dart and Flutter.
//
// 1. Precompiled: the library for this target is downloaded from the
//    GitHub release matching this package's version and checked against the
//    SHA-256 recorded in hook/checksums.json when the package was published.
//    A file that does not match is refused - never loaded, never fallen back
//    from.
// 2. From source, when there is no precompiled library for the target, the
//    download fails, there are no checksums (a checkout of the repository),
//    or the root package asks for it:
//
//        hooks:
//          user_defines:
//            kicksdk:
//              build_from_source: true
//
//    `binary_base_url` in the same place points the download at a mirror of
//    the release assets (the checksums still apply).
//
//    Building needs a Rust toolchain with the target installed
//    (`rustup target add <triple>`); for Android, Flutter's NDK supplies the
//    linker. The sources are the package's native/ directory, or the
//    repository itself when the package is used from a checkout.

import 'dart:convert';
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';

/// This package's version: the release the precompiled libraries come from.
/// scripts/release-check.sh keeps it equal to pubspec.yaml's.
const version = '0.1.1';
const releases = 'https://github.com/AnisAbdellatif/kicksdk/releases/download';
const assetName = 'kicksdk_ffi';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    final code = input.config.code;
    final triple = rustTriple(code);
    final fileName = code.targetOS.dylibFileName('kicksdk_ffi');
    final dest = input.outputDirectory.resolve(fileName);

    final forced = input.userDefines['build_from_source'] == true;
    final checksums = await readChecksums(input.packageRoot.resolve('hook/checksums.json'));
    final release = '$assetName-v$version-$triple.${fileName.split('.').last}';
    final expected = checksums[release];

    var ready = false;
    if (!forced && expected != null) {
      final base = input.userDefines['binary_base_url'] as String? ?? '$releases/v$version';
      ready = await download('$base/$release', dest, expected);
    }
    if (!ready) {
      final built = await buildFromSource(input, code, triple, fileName);
      await File.fromUri(built).copy(dest.toFilePath());
      output.dependencies.add(sourceRoot(input.packageRoot)!.resolve('crates/'));
    }

    output.assets.code.add(
      CodeAsset(package: input.packageName, name: assetName, linkMode: DynamicLoadingBundled(), file: dest),
    );
  });
}

/// Rust's name for the target being built.
String rustTriple(CodeConfig code) {
  final os = code.targetOS;
  final arch = code.targetArchitecture;
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
    final sim = code.iOS.targetSdk == IOSSdk.iPhoneSimulator;
    t = {
      Architecture.arm64: sim ? 'aarch64-apple-ios-sim' : 'aarch64-apple-ios',
      Architecture.x64: 'x86_64-apple-ios',
    }[arch];
  }
  if (t == null) throw UnsupportedError('kicksdk: no Rust target for $os/$arch');
  return t;
}

Future<Map<String, String>> readChecksums(Uri file) async {
  final f = File.fromUri(file);
  if (!await f.exists()) return const {};
  return (jsonDecode(await f.readAsString()) as Map<String, dynamic>).cast<String, String>();
}

/// Downloads `url` to `dest` if its SHA-256 is `expected`. False when it
/// could not be had (offline, not found); throws when it was had and is wrong.
Future<bool> download(String url, Uri dest, String expected) async {
  final client = HttpClient();
  try {
    final req = await client.getUrl(Uri.parse(url));
    final res = await req.close();
    if (res.statusCode != 200) {
      stderr.writeln('kicksdk: $url answered ${res.statusCode}; building from source');
      await res.drain<void>();
      return false;
    }
    final bytes = await res.fold<List<int>>(<int>[], (all, chunk) => all..addAll(chunk));
    final actual = sha256.convert(bytes).toString();
    if (actual != expected) {
      throw StateError('kicksdk: $url has SHA-256 $actual, expected $expected. Refusing it.');
    }
    await File.fromUri(dest).writeAsBytes(bytes);
    return true;
  } on SocketException catch (e) {
    stderr.writeln('kicksdk: could not download $url ($e); building from source');
    return false;
  } on HttpException catch (e) {
    stderr.writeln('kicksdk: could not download $url ($e); building from source');
    return false;
  } finally {
    client.close();
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

Future<Uri> buildFromSource(BuildInput input, CodeConfig code, String triple, String fileName) async {
  final root = sourceRoot(input.packageRoot);
  if (root == null) {
    throw StateError('kicksdk: no precompiled library for $triple, and no sources to build one from');
  }
  final targetDir = input.outputDirectoryShared.resolve('cargo/');
  final env = <String, String>{'CARGO_TARGET_DIR': targetDir.toFilePath()};
  // Cross-compiling for Android links with the NDK's clang, which Flutter
  // passes in.
  final cc = code.cCompiler;
  if (code.targetOS == OS.android && cc != null) {
    final key = triple.toUpperCase().replaceAll('-', '_');
    env['CARGO_TARGET_${key}_LINKER'] = cc.compiler.toFilePath();
    env['CC_${triple.replaceAll('-', '_')}'] = cc.compiler.toFilePath();
    env['AR_${triple.replaceAll('-', '_')}'] = cc.archiver.toFilePath();
  }
  final args = ['build', '--release', '--locked', '-p', 'kicksdk-ffi', '--target', triple];
  stderr.writeln('kicksdk: cargo ${args.join(' ')} (in ${root.toFilePath()})');
  final run = await Process.run(
    'cargo',
    args,
    workingDirectory: root.toFilePath(),
    environment: env,
    runInShell: Platform.isWindows,
  );
  if (run.exitCode != 0) {
    throw StateError(
      'kicksdk: building from source failed. It needs Rust (https://rustup.rs) and '
      '`rustup target add $triple`.\n${run.stderr}',
    );
  }
  return targetDir.resolve('$triple/release/$fileName');
}
