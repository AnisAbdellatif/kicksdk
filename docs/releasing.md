# Releasing

A release is a tag. Every package is published from it by the Release
workflow (`.github/workflows/release.yml`), with the workflow, the scripts
and the sources all taken from the tagged commit.

1. Bump the version everywhere `scripts/release-check.sh` looks: the
   workspace version and the `kicksdk` requirement in `Cargo.toml`'s
   `[workspace.dependencies]`, `bindings/js/package.json`,
   `bindings/elixir/mix.exs`, `bindings/python/pyproject.toml`,
   `bindings/dart/pubspec.yaml`, `bindings/dart/hook/build.dart`; and add a
   `## <version>` heading to `CHANGELOG.md`.
2. Dry-run it. A pull request that touches the release's files gets the
   Release dry run workflow by itself; for any other change, run the Release
   workflow on the branch: `gh workflow run release.yml --ref <branch>`. A run
   on a branch builds and checks everything a release does and uploads and
   publishes nothing.
3. Merge, then tag the merge commit: `git tag v<version> && git push origin
   v<version>`.

## What the workflow does

1. **plan** (`scripts/release-plan.sh`) checks the tag is `v<semver>` and is
   the version of every package, and decides which jobs run.
2. **Builds** (`.github/workflows/build.yml`, read-only): the Elixir NIFs, the
   Dart/C library per target, the Python wheels and the npm package, as
   artifacts of the run; and the vendored `native/` workspaces the hex and
   pub packages ship, built `--locked --offline`, with each package built
   from its own. Every Linux glibc library is checked against its target's
   glibc floor (below). Every wheel is installed and tested on a machine it
   is for.
3. **The GitHub release** is created, and the NIF tarballs and the FFI
   libraries are uploaded to it afterwards, by other jobs (which is why
   GitHub's immutable releases must stay off: see the setup below). The
   wheels and the npm package go to their registries only.
4. **Publishing.** crates.io, PyPI and npm publish as soon as their own
   builds are done. hex and pub wait for every one of their binaries to be
   on the release (and for the vendored-source check), record those files'
   SHA-256s in the package (`checksum-Elixir.Kicksdk.Native.exs`,
   `hook/checksums.json`), test the package as a user gets it (the binary
   downloaded from the release and checked, with no Rust on `PATH` to fall
   back to), check that the package as the registry will serve it builds
   from source (`scripts/check-dart-archive.sh` on the archive pub uploads,
   `scripts/check-hex-package.sh` on hex's), then publish. CI and every dry
   run make the same check on the pub archive, so a file pub leaves out of
   the package fails a pull request rather than a release.

A registry that is not set up fails its own job and leaves the others alone.

## Release files are never replaced

The NIF tarballs and FFI libraries on a release are what the hex and pub
packages' checksums name, and neither registry takes a version twice.
Replacing one of those files breaks every install of that version, for good.
So the workflow never replaces a release file (`scripts/upload-release-assets.sh`):
one that is there is kept, and if a re-run built different bytes, the run
says so in a warning and the release's file stays. The packages take their
checksums from the release, never from the build, so they always match it.

NIF tarballs are packed reproducibly (`scripts/package-nif.sh`: one entry,
owner 0, fixed mode, the commit's time, `gzip -n`), so the same library
always gives the same tarball.

If a file on a release is wrong, delete it by hand only while neither hex
nor pub has published that version; then re-run (below). Once either has, the
fix is a new version.

## Re-running part of a release

Registries never take a version twice, and every publish job skips a version
its registry already has (PyPI and crates.io per file and per crate). A
re-run therefore does only what is left.

- **Re-run failed jobs** on the tag's own run, in the Actions tab (within 30
  days). Same commit, same event: this is the first choice, and the only
  automated one for pub.dev, which takes publishes from tag pushes only.
- **Run the workflow on the tag**: Actions → Release → Run workflow → Use
  workflow from: *the tag*, or

  ```sh
  gh workflow run release.yml --ref v0.2.0 -f only=hex,crates
  ```

  This runs the tag's own `release.yml` and scripts. A fix to the workflow
  therefore does not reach an existing tag: release the fix as a new
  version. (Tags up to v0.1.1 carry the old workflow, whose dispatch takes a
  `tag` input and runs main's workflow against the tag's scripts.)
  `only` takes, comma separated:

  | Value | Runs |
  |---|---|
  | `release` | creates the GitHub release if it is missing |
  | `nif` | builds the NIFs and uploads those the release lacks |
  | `ffi` | builds the FFI libraries and uploads those the release lacks |
  | `wheels` | builds and tests the wheels and the sdist |
  | `crates` | publishes `kicksdk`, then `kicksdk-ffi`, each unless crates.io has it |
  | `pypi` | builds the wheels and publishes them (skipping files PyPI has) |
  | `npm` | builds, tests and publishes `kicksdk-js` unless npm has the version |
  | `hex` | publishes from the release's NIFs, which must all be there |
  | `pub` | refused on a dispatched run: pub.dev rejects it (below) |

  Empty means all of them. An unknown value fails the run. `dry_run` builds
  and checks without uploading or publishing.

  On a dispatched run, pub.dev refuses the workflow's token: it takes
  automated publishes from tag pushes only. A dispatched run therefore
  leaves pub out (with a warning), and fails if `pub` is asked for by name.
  Publish such a version by hand: `git checkout v<version> &&
  scripts/publish-dart.sh <version>`.
- **Hazards**: hex and pub need every binary on the release; with `only=hex`
  or `only=pub` they take what the release has and fail if a target is
  missing. Two runs of one tag never overlap: the second waits for the
  first. GitHub keeps only one waiting run per tag, though: a third run
  started while the second waits cancels the second, so start one re-run
  at a time, or start the cancelled one again once the others are done.

## Publishing the Dart package by hand

`scripts/publish-dart.sh <version>` does what the pub job does, from your
machine: for the first version (before automated publishing can be switched
on) and for a version whose run was not a tag push. It must run from a clean
checkout of the version's tag (`git checkout v<version>`), and only after the
release has every library: it refuses otherwise, so what reaches pub.dev is
the tag's code with the checksums of the files the release serves.

## Platforms

| Binary | Floor |
|---|---|
| Linux glibc, x86_64 (NIF, FFI library) | glibc 2.17 (built in cross's CentOS 7 image) |
| Linux glibc, arm hard-float (NIF only) | glibc 2.17 (cross's crosstool image) |
| Linux glibc, aarch64 | glibc 2.17 (NIF), 2.18 (FFI library) |
| Linux glibc, riscv64 (NIF only) | glibc 2.27, the port's first |
| Linux glibc, Python wheels | the wheel's manylinux tag (maturin's default) |
| Linux musl (NIFs, wheels) | the NIF needs `libgcc_s` at run time: on Alpine, `apk add libgcc` |
| macOS, iOS | install name `@rpath/<file>`, not the build machine's path |
| Windows MSVC | C runtime linked in: no VC++ redistributable needed |

The release profile strips symbols (`strip = "symbols"`); the exported
functions (`nif_init`, `kicksdk_*`, `PyInit__native`) are in the dynamic
symbol table and stay.

## One-time setup, per registry

Keep GitHub's immutable releases off (Settings → General → Releases →
release immutability): the workflow creates the release first and uploads
the binaries to it afterwards, from other jobs, which an immutable release
refuses once it is published. The workflow never replaces a release file
anyway (above), and the protected tags below keep a version's code fixed.

Each registry is independent: one that is not set up fails its own job and
the rest still publish. Each publish job runs in a GitHub environment
(`crates-io`, `pypi`, `npm`, `hex`, `pub-dev`), created on first use; give
them protection rules (Settings → Environments → the environment →
required reviewers; deployment branches and tags: tag `v*` only) and keep
each secret in its environment rather than in the repository.

| Registry | Package | Setup |
|---|---|---|
| crates.io | `kicksdk`, `kicksdk-ffi` | Trusted publishing (each crate → Settings → Trusted Publishing: repository `AnisAbdellatif/kicksdk`, workflow `release.yml`, environment `crates-io`), then delete the `CARGO_REGISTRY_TOKEN` secret. Until then, an API token (scopes `publish-new`, `publish-update`) as `CARGO_REGISTRY_TOKEN` in the `crates-io` environment. |
| PyPI | `kicksdk-py` | A trusted publisher (pypi.org → the project → Publishing): owner `AnisAbdellatif`, repository `kicksdk`, workflow `release.yml`, environment `pypi`. No secret. |
| npm | `kicksdk-js` | Trusted publishing (npmjs.com → kicksdk-js → Settings → Trusted publisher → GitHub Actions: `AnisAbdellatif`, `kicksdk`, workflow `release.yml`, environment `npm`), then delete the `NPM_TOKEN` secret; the workflow uses the job's OIDC token whenever npm accepts it, with provenance. Until then, a granular access token (npmjs.com → Access Tokens → Generate New Token → Granular: read and write on `kicksdk-js`, bypass 2FA) as `NPM_TOKEN` in the `npm` environment; such tokens expire, which is one more reason to switch. |
| hex | `kicksdk` | `mix hex.user key generate --key-name kicksdk-ci --permission api:write`, as `HEX_API_KEY` in the `hex` environment. |
| pub.dev | `kicksdk` | pub.dev → kicksdk → Admin → Automated publishing: enable publishing from GitHub Actions, repository `AnisAbdellatif/kicksdk`, tag pattern `v{{version}}`, and require the environment `pub-dev`. The first version of a package goes up by hand (`scripts/publish-dart.sh`). |

Secrets go under Settings → Secrets and variables → Actions, or Settings →
Environments → the environment → Environment secrets; or
`gh secret set HEX_API_KEY --repo AnisAbdellatif/kicksdk --env hex`.

Protect the tags too (Settings → Rules → Rulesets → New tag ruleset: target
`v*`, restrict creations, updates and deletions to the maintainers): a
re-run uses the tag's own workflow, and a moved tag would publish other code
under a version that is already out.
