#!/usr/bin/env bash
# Tests for publishing a release with no forge, and for the offline release's refusals.
# [VLD-ANB-MIDGARD-0035] [AC-RF-SC-5a]
#
#   bash tools/ci/tests/release-publish.test.sh
#
# The claim under test: outside a pipeline, `run.sh release-publish <tag>` needs no
# forge. It copies the finished release (signatures, attestations, SBOMs, bundles, the
# manifest, the artefacts themselves) and the gate run it rests on into a directory of
# its own with a SHA256SUMS that verifies, and it never writes over a release already
# published there. A name that is not a release tag is refused. `run.sh release-offline`
# refuses to start when there is no tag, or when the tag is not the commit checked out.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v sha256sum >/dev/null 2>&1 || { echo "skip  release-publish tests (sha256sum is not on PATH)"; exit 0; }

# A finished release, as release.sh leaves one: test stand-ins, not real artefacts.
out="$work/release-out"
mkdir -p "$out/bin" "$out/chart"
echo "mg-verify 9.9.9 tools bin/mg-verify" > "$out/artefacts.txt"
echo "test stand-in binary" > "$out/bin/mg-verify"
echo "test stand-in signature" > "$out/bin/mg-verify.sig"
echo "test stand-in attestation" > "$out/bin/mg-verify.att"
echo "mg-verify mg-verify tools -" > "$out/release-manifest.txt"
echo "image: test" > "$out/chart/values.yaml"

# publish [VAR=VALUE...] TAG: run the job as a developer's shell would, with no forge.
publish() {
    local vars=()
    while [ "$#" -gt 0 ] && [[ "$1" == *=* ]]; do vars+=("$1"); shift; done
    env -u GITHUB_ACTIONS -u GITLAB_CI -u MIDGARD_PUBLISH_TAG MIDGARD_RELEASE_OUT="$out" \
        MIDGARD_RELEASE_DIR="$work/published" "${vars[@]}" bash "$root/tools/ci/run.sh" release-publish "$@" 2>&1
}

if out_text="$(publish v9.9.9)"; then
    dest="$work/published/v9.9.9"
    [ -f "$dest/SHA256SUMS" ] && (cd "$dest" && sha256sum --quiet -c SHA256SUMS) &&
        pass "with no forge, the release is published to a directory whose SHA256SUMS verifies" ||
        fail "with no forge, the release is published to a directory whose SHA256SUMS verifies: $out_text"
    for f in mg-verify mg-verify.sig mg-verify.att release-manifest.txt values.yaml NUMBERS.md; do
        [ -f "$dest/$f" ] || fail "the published release holds $f"
    done
    [ -f "$dest/mg-verify" ] && [ -f "$dest/NUMBERS.md" ] &&
        pass "it holds the artefacts, their signatures and the gate run they rest on"
    [ ! -e "$dest.partial" ] && pass "nothing half-written is left beside it" || fail "nothing half-written is left beside it"
else
    fail "with no forge, the release is published: $out_text"
fi

if out_text="$(publish v9.9.9)"; then
    fail "a release already published is never written over"
else
    grep -q 'already exists; a published release is never replaced' <<<"$out_text" &&
        pass "a release already published is never written over" ||
        fail "a release already published is never written over, saying why: $out_text"
fi

if out_text="$(publish 'not-a-tag')"; then
    fail "a name that is not a release tag is refused"
else
    grep -q 'is not a release tag' <<<"$out_text" && pass "a name that is not a release tag is refused" ||
        fail "a name that is not a release tag is refused: $out_text"
fi

# release-offline stops before any step when there is nothing to release.
if out_text="$(env -u GITHUB_ACTIONS -u GITLAB_CI -u MIDGARD_RELEASE_TAG bash "$root/tools/ci/run.sh" release-offline v0.0.0-not-made 2>&1)"; then
    fail "release-offline refuses a tag that is not the commit checked out"
else
    grep -q 'check out v0.0.0-not-made first' <<<"$out_text" &&
        pass "release-offline refuses a tag that is not the commit checked out" ||
        fail "release-offline refuses a tag that is not the commit checked out: $out_text"
fi

if [ "$failures" -gt 0 ]; then
    echo "release-publish tests: $failures failure(s)"
    exit 1
fi
echo "release-publish tests: all passed"
