# Releasing

A release is a tag: bump the version everywhere `scripts/release-check.sh`
looks, commit, `git tag v<version> && git push origin v<version>`. The Release
workflow creates the GitHub release, uploads every precompiled binary to it,
and publishes each package.

## One-time setup, per registry

Each registry is independent: one that is not set up fails its own job and
the rest still publish.

| Registry | Package | Setup |
|---|---|---|
| crates.io | `kicksdk`, `kicksdk-ffi` | First release: an API token (crates.io → Account Settings → API Tokens, scope `publish-new` and `publish-update`) as the repository secret `CARGO_REGISTRY_TOKEN`. After it, optionally switch to trusted publishing (each crate → Settings → Trusted Publishing: repository `AnisAbdellatif/kicksdk`, workflow `release.yml`) and delete the secret. |
| PyPI | `kicksdk-py` | A pending trusted publisher (pypi.org → Your projects → Publishing → Add a new pending publisher): project `kicksdk-py`, owner `AnisAbdellatif`, repository `kicksdk`, workflow `release.yml`, environment `pypi`. No secret. |
| npm | `kicksdk-js` | First release: an automation token (npmjs.com → Access Tokens → Generate New Token, granular, publish rights for all packages) as the secret `NPM_TOKEN`. After it, optionally switch to trusted publishing (package → Settings → Trusted publishing: GitHub Actions, `AnisAbdellatif/kicksdk`, `release.yml`) and delete the secret. |
| hex | `kicksdk` | `mix hex.user key generate --key-name kicksdk-ci --permission api:write`, as the secret `HEX_API_KEY`. |
| pub.dev | `kicksdk` | The first version by hand, after the Release workflow has uploaded the libraries: `scripts/publish-dart.sh <version>`. Then pub.dev → kicksdk → Admin → Automated publishing: enable GitHub Actions, repository `AnisAbdellatif/kicksdk`, tag pattern `v{{version}}`. |

Secrets go under the repository's Settings → Secrets and variables → Actions,
or: `gh secret set CARGO_REGISTRY_TOKEN --repo AnisAbdellatif/kicksdk`.

The PyPI environment `pypi` is created on first use; to require approval
before each PyPI publish, add a protection rule to it.
