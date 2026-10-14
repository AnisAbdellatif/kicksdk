// The build hook's decisions: which Rust target, download or build, which
// download failures fall back to a source build (and that a checksum
// mismatch never does), where cargo is, how Android links.
import 'dart:async';
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';
import 'package:test/test.dart';

import '../hook/build.dart' as hook;

/// Serves `body` (or a status) at any path, or never answers.
class Mirror {
  late final HttpServer server;
  List<int> body = const [];
  int status = 200;
  bool silent = false;
  var requests = 0;

  String get url => 'http://127.0.0.1:${server.port}/mirror';

  static Future<Mirror> start() async {
    final m = Mirror();
    m.server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    m.server.listen((req) async {
      m.requests++;
      if (m.silent) return;
      req.response.statusCode = m.status;
      if (m.status == 200) req.response.add(m.body);
      await req.response.close();
    });
    return m;
  }
}

String sha(List<int> bytes) => sha256.convert(bytes).toString();

void main() {
  late Directory tmp;
  late Mirror mirror;
  setUp(() async {
    tmp = await Directory.systemTemp.createTemp('kicksdk_hook');
    mirror = await Mirror.start();
  });
  tearDown(() async {
    await mirror.server.close(force: true);
    await tmp.delete(recursive: true);
  });

  test('every target Rust has gets a triple; musl and simulators are told apart', () {
    expect(hook.rustTriple(OS.linux, Architecture.x64), 'x86_64-unknown-linux-gnu');
    expect(hook.rustTriple(OS.linux, Architecture.x64, musl: true), 'x86_64-unknown-linux-musl');
    expect(hook.rustTriple(OS.linux, Architecture.arm), 'armv7-unknown-linux-gnueabihf');
    expect(hook.rustTriple(OS.linux, Architecture.arm, musl: true), 'armv7-unknown-linux-musleabihf');
    expect(hook.rustTriple(OS.linux, Architecture.riscv64), 'riscv64gc-unknown-linux-gnu');
    expect(hook.rustTriple(OS.linux, Architecture.ia32), 'i686-unknown-linux-gnu');
    expect(hook.rustTriple(OS.windows, Architecture.ia32), 'i686-pc-windows-msvc');
    expect(hook.rustTriple(OS.android, Architecture.ia32), 'i686-linux-android');
    expect(hook.rustTriple(OS.iOS, Architecture.arm64, iosSimulator: true), 'aarch64-apple-ios-sim');
    expect(hook.rustTriple(OS.fuchsia, Architecture.x64), isNull);
    expect(hook.rustTriple(OS.linux, Architecture.riscv32), isNull);
    expect(hook.hostIsMusl('7f00-7f01 r-xp 0 08:01 1 /lib/ld-musl-x86_64.so.1'), isTrue);
    expect(hook.hostIsMusl('7f00-7f01 r-xp 0 08:01 1 /usr/lib/x86_64-linux-gnu/libc.so.6'), isFalse);
  });

  group('download or build', () {
    const release = 'kicksdk_ffi-v0.1.1-x86_64-unknown-linux-gnu.so';
    hook.Plan plan({bool forced = false, Map<String, String>? checksums, Object? baseUrl}) => hook.planDownload(
      triple: 'x86_64-unknown-linux-gnu',
      release: release,
      forced: forced,
      checksums: checksums ?? {release: 'abc'},
      baseUrl: baseUrl,
    );
    String? why(hook.Plan p) => p is hook.FromSource ? p.reason : null;

    test('downloads from the release, or a mirror', () {
      final p = plan() as hook.Download;
      expect(p.url.toString(), '${hook.releases}/v${hook.version}/$release');
      expect(p.sha256, 'abc');
      expect((plan(baseUrl: 'http://mirror.test/x/') as hook.Download).url.toString(), 'http://mirror.test/x/$release');
      expect(
        (plan(baseUrl: 'https://mirror.test/x') as hook.Download).url.toString(),
        'https://mirror.test/x/$release',
      );
    });

    test('builds when asked, from a checkout, for a target with no binary, or for a bad mirror URL', () {
      expect(why(plan(forced: true)), contains('build_from_source'));
      expect(why(plan(checksums: const {})), contains('checksums'));
      expect(why(plan(checksums: const {'other': 'x'})), contains('no precompiled library'));
      for (final bad in ['ftp://mirror.test/x', 'file:///tmp/x', 'mirror.test/x', 'http:///x', 'https://', 42, '::']) {
        expect(why(plan(baseUrl: bad)), contains('not an http(s) URL'), reason: '$bad');
      }
      expect(hook.isSet(true), isTrue);
      expect(hook.isSet('true'), isTrue);
      expect(hook.isSet(null), isFalse);
    });

    test('network, TLS and timeouts fall back; anything else does not', () {
      expect(hook.fallsBack(const SocketException('x')), isTrue);
      expect(hook.fallsBack(const HttpException('x')), isTrue);
      expect(hook.fallsBack(const HandshakeException('x')), isTrue);
      expect(hook.fallsBack(TimeoutException('x')), isTrue);
      expect(hook.fallsBack(hook.ChecksumMismatch(Uri(), 'a', 'b')), isFalse);
      expect(hook.fallsBack(StateError('x')), isFalse);
    });
  });

  group('download', () {
    Uri dest() => tmp.uri.resolve('lib.so');

    test('a matching file is written', () async {
      mirror.body = [1, 2, 3];
      expect(await hook.download(Uri.parse(mirror.url), dest(), sha(mirror.body)), isNull);
      expect(File.fromUri(dest()).readAsBytesSync(), [1, 2, 3]);
    });

    test('a file that does not match is refused, and is a failure', () async {
      mirror.body = [6, 6, 6];
      await expectLater(
        hook.download(Uri.parse(mirror.url), dest(), sha([1, 2, 3])),
        throwsA(isA<hook.ChecksumMismatch>()),
      );
      expect(File.fromUri(dest()).existsSync(), isFalse);
    });

    test('not found, no answer, a TLS failure and no server all fall back', () async {
      mirror.status = 404;
      expect(await hook.download(Uri.parse(mirror.url), dest(), 'x'), 'answered 404');

      mirror.silent = true;
      final slow = await hook.download(Uri.parse(mirror.url), dest(), 'x', timeout: const Duration(milliseconds: 200));
      expect(slow, contains('TimeoutException'));

      // https to a server that speaks plain HTTP: the handshake fails.
      mirror.silent = false;
      final tls = await hook.download(Uri.parse('https://127.0.0.1:${mirror.server.port}/x'), dest(), 'x');
      expect(tls, contains('Handshake'));

      final closed = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = closed.port;
      await closed.close();
      expect(await hook.download(Uri.parse('http://127.0.0.1:$port/x'), dest(), 'x'), contains('SocketException'));
      expect(File.fromUri(dest()).existsSync(), isFalse);
    });
  });

  group('cargo', () {
    bool only(Set<String> files, String p) => files.contains(p);

    test('a named cargo wins, and its directory goes first on PATH', () {
      final c = hook.locateCargo(
        env: {'PATH': '/usr/bin', 'HOME': '/home/u'},
        windows: false,
        cargoPath: '/opt/rust/bin/cargo',
        rustupHome: '/opt/rustup',
        exists: (p) => only({'/usr/bin/cargo'}, p),
      );
      expect(c.executable, '/opt/rust/bin/cargo');
      expect(c.environment, {'RUSTUP_HOME': '/opt/rustup', 'PATH': '/opt/rust/bin:/usr/bin'});
    });

    test('then CARGO_HOME, then PATH, then ~/.cargo/bin', () {
      expect(
        hook
            .locateCargo(
              env: {'PATH': '/usr/bin', 'HOME': '/home/u', 'CARGO_HOME': '/opt/cargo'},
              windows: false,
              exists: (p) => only({'/opt/cargo/bin/cargo', '/usr/bin/cargo'}, p),
            )
            .executable,
        '/opt/cargo/bin/cargo',
      );
      final onPath = hook.locateCargo(
        env: {'PATH': '/usr/local/bin:/usr/bin', 'HOME': '/home/u'},
        windows: false,
        exists: (p) => only({'/usr/bin/cargo', '/home/u/.cargo/bin/cargo'}, p),
      );
      expect(onPath.executable, 'cargo');
      expect(onPath.environment, isEmpty);
      final home = hook.locateCargo(
        env: {'PATH': '/usr/bin', 'HOME': '/home/u'},
        windows: false,
        exists: (p) => only({'/home/u/.cargo/bin/cargo'}, p),
      );
      expect(home.executable, '/home/u/.cargo/bin/cargo');
      expect(home.environment['PATH'], '/home/u/.cargo/bin:/usr/bin');
      expect(hook.locateCargo(env: const {}, windows: false, exists: (_) => false).executable, 'cargo');
    });

    test('on Windows, cargo.exe under %USERPROFILE%', () {
      final c = hook.locateCargo(
        env: {'Path': r'C:\Windows', 'USERPROFILE': r'C:\Users\u'},
        windows: true,
        exists: (p) => p == r'C:\Users\u\.cargo\bin\cargo.exe',
      );
      expect(c.executable, r'C:\Users\u\.cargo\bin\cargo.exe');
      expect(c.environment['PATH'], r'C:\Users\u\.cargo\bin;C:\Windows');
    });
  });

  test('Android links with clang aimed at the target and its API level', () {
    final env = hook.androidEnvironment(
      triple: 'armv7-linux-androideabi',
      api: 24,
      compiler: Uri.file('/ndk/bin/clang'),
      archiver: Uri.file('/ndk/bin/llvm-ar'),
    );
    expect(env, {
      'CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER': '/ndk/bin/clang',
      'CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS': '-C link-arg=--target=armv7a-linux-androideabi24',
      'CC_armv7_linux_androideabi': '/ndk/bin/clang',
      'CFLAGS_armv7_linux_androideabi': '--target=armv7a-linux-androideabi24',
      'AR_armv7_linux_androideabi': '/ndk/bin/llvm-ar',
    });
    final arm64 = hook.androidEnvironment(
      triple: 'aarch64-linux-android',
      api: 21,
      compiler: Uri.file('/ndk/bin/clang'),
      archiver: Uri.file('/ndk/bin/llvm-ar'),
    );
    expect(arm64['CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS'], '-C link-arg=--target=aarch64-linux-android21');
    expect(hook.muslEnvironment('x86_64-unknown-linux-musl'), {
      'CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS': '-C target-feature=-crt-static',
    });
  });

  group('the hook, end to end', () {
    final triple = hook.rustTriple(OS.current, Architecture.current, musl: Platform.isLinux && hook.hostIsMusl())!;
    final fileName = OS.current.dylibFileName('kicksdk_ffi');
    final release = 'kicksdk_ffi-v${hook.version}-$triple.${fileName.split('.').last}';

    Future<List<int>> run(Map<String, Object?> defines, Map<String, String> checksums) async {
      late List<int> built;
      await testCodeBuildHook(
        mainMethod: (args) => build(args, (input, output) => hook.provide(input, output, checksums: checksums)),
        userDefines: PackageUserDefines(
          workspacePubspec: PackageUserDefinesSource(defines: defines, basePath: Directory.current.uri),
        ),
        check: (input, output) {
          built = File.fromUri(output.assets.code.single.file!).readAsBytesSync();
        },
      );
      return built;
    }

    test('takes the library from a mirror when its checksum matches', () async {
      mirror.body = 'not really a library'.codeUnits;
      final got = await run({'binary_base_url': mirror.url}, {release: sha(mirror.body)});
      expect(got, mirror.body);
      expect(mirror.requests, 1);
    });

    test('fails, and does not build, when the checksum does not match', () async {
      mirror.body = 'tampered'.codeUnits;
      await expectLater(
        run({'binary_base_url': mirror.url}, {release: sha('the real one'.codeUnits)}),
        throwsA(isA<hook.ChecksumMismatch>()),
      );
    });

    test('builds from source when the mirror does not have it', () async {
      mirror.status = 404;
      final got = await run({'binary_base_url': mirror.url}, {release: 'x'});
      expect(mirror.requests, 1);
      if (Platform.isLinux) expect(got.take(4), [0x7f, 0x45, 0x4c, 0x46]); // ELF
      expect(got.length, greaterThan(100000));
    }, timeout: const Timeout(Duration(minutes: 5)));
  });
}
