#!/usr/bin/env bash
# Decides what a run of the Release workflow does, as job=true|false lines
# (into $GITHUB_OUTPUT when set). Read from the environment:
#
#   EVENT     push or workflow_dispatch
#   REF_TYPE  tag or branch: what the run was started on
#   REF_NAME  the tag (or branch) name
#   ONLY      comma-separated jobs to run, empty for all:
#             release, nif, ffi, wheels, crates, pypi, npm, hex, pub
#   DRY_RUN   true to build and check everything, upload and publish nothing
#
# A run on a tag releases that tag, with the workflow and scripts of that same
# commit. A run on a branch is always a dry run. An unknown job in ONLY, an
# ONLY that names no job (","), a tag that is not v<semver>, or a tag that is
# not the packages' version fails the plan, and with it the whole run.
set -euo pipefail
cd "$(dirname "$0")/.."

out=${GITHUB_OUTPUT:-/dev/stdout}
event=${EVENT:?}
ref_type=${REF_TYPE:?}
ref_name=${REF_NAME:?}
only=${ONLY:-}
dry=${DRY_RUN:-false}
[ "$dry" = true ] || dry=false

version=$(scripts/version.sh)
if [ "$ref_type" = tag ]; then
  if ! [[ "$ref_name" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    echo "::error::tag '$ref_name' is not v<major>.<minor>.<patch>[-<pre>]"
    exit 1
  fi
  if [ "$ref_name" != "v$version" ]; then
    echo "::error::tag $ref_name, but the packages are at $version"
    exit 1
  fi
else
  if [ "$dry" != true ]; then
    echo "::notice::a run on a branch ($ref_name) is a dry run: nothing is uploaded or published"
  fi
  dry=true
fi
scripts/release-check.sh "$version"
scripts/ffi-targets.sh --check

all="release nif ffi wheels crates pypi npm hex pub"
declare -A want=()
if [ -z "${only// /}" ]; then
  for j in $all; do want[$j]=1; done
else
  IFS=, read -ra asked <<< "$only"
  for j in "${asked[@]}"; do
    j=${j// /}
    [ -n "$j" ] || continue
    if [[ " $all " != *" $j "* ]]; then
      echo "::error::unknown job '$j' in only; the jobs are: ${all// /, }"
      exit 1
    fi
    want[$j]=1
  done
  # "," or " , " is not empty, but names no job: refuse it rather than run
  # a release that does nothing.
  if [ "${#want[@]}" -eq 0 ]; then
    echo "::error::only ('$only') names no job; the jobs are: ${all// /, }, or empty for all"
    exit 1
  fi
fi
has() { [ -n "${want[$1]:-}" ]; }

# pub.dev takes automated publishes from tag pushes only; a dispatched run's
# token is refused.
if has pub && [ "$event" != push ] && [ "$dry" != true ]; then
  if [ -n "${only// /}" ]; then
    echo "::error::pub.dev publishes from tag pushes only: re-run the failed jobs of the tag's own run, or run scripts/publish-dart.sh $version from a clean checkout of the tag"
    exit 1
  fi
  echo "::warning::pub.dev publishes from tag pushes only, so this run leaves pub out: re-run the failed jobs of the tag's own run, or run scripts/publish-dart.sh $version from a clean checkout of the tag"
  unset 'want[pub]'
fi

flag() { if "$@"; then echo true; else echo false; fi; }
publish() { [ "$dry" = false ] && has "$1"; }
# The GitHub release, which every uploaded binary needs.
needs_release() { [ "$dry" = false ] && { has release || has nif || has ffi; }; }
builds_wheels() { has wheels || has pypi; }
# The packages built from this run's binaries: dry runs only (a release
# builds them from the release's own files).
dry_packages() { [ "$dry" = true ] && has nif && has ffi; }
# The source fallback the hex and pub packages ship.
needs_vendored() { has hex || has pub || dry_packages; }
{
  echo "version=$version"
  echo "tag=v$version"
  echo "dry_run=$dry"
  echo "release=$(flag needs_release)"
  echo "nif=$(flag has nif)"
  echo "ffi=$(flag has ffi)"
  echo "wheels=$(flag builds_wheels)"
  echo "js=$(flag has npm)"
  echo "vendored=$(flag needs_vendored)"
  echo "packages=$(flag dry_packages)"
  for j in crates pypi npm hex pub; do echo "$j=$(flag publish "$j")"; done
} >> "$out"
