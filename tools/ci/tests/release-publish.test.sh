#!/usr/bin/env bash
# Tests for the release-publish job (tools/ci/run.sh): it publishes the release the tag's
# own run made, as that run made it, and never builds or signs again. [RC-07]
# [AC-RF-SC-5e]
#
#   bash tools/ci/tests/release-publish.test.sh
#
# The claim under test: on GitHub, release-publish runs only on the tag it publishes;
# it takes the output of the tag's latest successful push run of release.yml, refuses
# it unless every artefact the manifest names is there with its .sig and .att and
# hashes to the recorded digest, and only then drafts the release. A stub `gh` on PATH
# serves a fixture release and records every call.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}
sha() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }

# A release as the release job leaves it: two artefacts, their attestations, the images
# lock and the chart.
fixture="$work/fixture"
mkdir -p "$fixture/images" "$fixture/chart"
for id in alpha beta; do
    echo "the $id binary" > "$fixture/$id"
    for ext in sig att sbom.cdx.json; do echo "$id $ext" > "$fixture/$id.$ext"; done
done
echo "registry/alpha:9.9.9 sha256:00" > "$fixture/images/images.lock"
echo "image: pinned" > "$fixture/chart/values.yaml"
printf 'alpha 9.9.9 service alpha\nbeta 9.9.9 tool beta\n' > "$fixture/artefacts.txt"
{
    echo "# Midgard 9.9.9 — gated and admitted. id version component sha256"
    echo "alpha 9.9.9 service $(sha "$fixture/alpha")"
    echo "beta 9.9.9 tool $(sha "$fixture/beta")"
} > "$fixture/release-manifest.txt"

mkdir -p "$work/bin"
cat > "$work/bin/gh" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$GH_LOG"
case "$1 $2" in
    "run list") [ -z "${STUB_RUN:-}" ] || echo "$STUB_RUN" ;;
    "run download")
        while [ "$#" -gt 0 ]; do [ "$1" = --dir ] && dir="$2"; shift; done
        mkdir -p "$dir" && cp -R "$STUB_RELEASE"/. "$dir"/ ;;
    "release create") ;;
esac
STUB
chmod +x "$work/bin/gh"
export GH_LOG="$work/gh.log"

# publish [VAR=VALUE...]: the job on a GitHub tag run for v9.9.9, with the stub gh.
publish() {
    rm -rf "$work/out"
    env GITHUB_ACTIONS=true GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v9.9.9 GITHUB_SHA=abc123 \
        GITHUB_REPOSITORY=example/midgard MIDGARD_PUBLISH_TAG=v9.9.9 \
        MIDGARD_RELEASE_OUT="$work/out" STUB_RUN=77 STUB_RELEASE="$fixture" \
        PATH="$work/bin:$PATH" "$@" bash "$repo/tools/ci/run.sh" release-publish 2>&1
}
created() { grep -q '^release create ' "$GH_LOG" 2>/dev/null; }

rm -f "$GH_LOG"
out="$(publish)"
rc=$?
if [ "$rc" = 0 ] && grep -q '^run list --repo example/midgard --workflow release.yml --event push --commit abc123 --status success ' "$GH_LOG" &&
    grep -q '^run download 77 --repo example/midgard --name release --dir ' "$GH_LOG"; then
    pass "it takes the output of the tag's own successful push run"
else
    fail "fetching the tag's run: rc $rc, gh: $(cat "$GH_LOG" 2>/dev/null), output: $out"
fi
line="$(grep '^release create ' "$GH_LOG" 2>/dev/null)"
if grep -q -- '^release create v9.9.9 --draft --prerelease --verify-tag ' <<<"$line" &&
    grep -q "/out/alpha " <<<"$line" && grep -q "/out/beta.att" <<<"$line" &&
    grep -q '/out/images/images.lock' <<<"$line" && grep -q 'docs/compliance/gate-runs/' <<<"$line"; then
    pass "it drafts the release with the artefacts, their attestations and the gate record"
else
    fail "the draft: $line"
fi
grep -q 'release-publish: 2 artefacts, each as the release recorded it' <<<"$out" &&
    pass "it says every artefact is as the release recorded it" ||
    fail "the count: $out"

rm -f "$GH_LOG"
out="$(publish GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main)"
if [ $? -ne 0 ] && grep -q 'gh workflow run release.yml --ref v9.9.9 -f mode=release -f publish=v9.9.9' <<<"$out" &&
    ! grep -q '^run ' "$GH_LOG" 2>/dev/null && ! created; then
    pass "off the tag it is refused, and says how to run it on the tag"
else
    fail "off the tag: $out"
fi

rm -f "$GH_LOG"
out="$(publish STUB_RUN=)"
if [ $? -ne 0 ] && grep -q 'has no successful release run to publish' <<<"$out" && ! created; then
    pass "with no successful release run of the tag, nothing is published"
else
    fail "no run: $out"
fi

# refused NAME EDIT MESSAGE: a fixture changed by EDIT is refused with MESSAGE, and
# nothing is published.
refused() {
    rm -rf "$work/bad" && cp -R "$fixture" "$work/bad" && (cd "$work/bad" && eval "$2")
    rm -f "$GH_LOG"
    out="$(publish STUB_RELEASE="$work/bad")"
    if [ $? -ne 0 ] && grep -q "$3" <<<"$out" && ! created; then
        pass "$1 is refused, and nothing is published"
    else
        fail "$1: $out"
    fi
}
refused "an artefact that is not the one released" "echo tampered >> beta" "does not hash to the digest the release recorded"
refused "an artefact with no signature" "rm alpha.sig" "is missing"
refused "a release of another version" "sed -i.bak 's/ 9\.9\.9 / 9.9.8 /' release-manifest.txt" "not v9.9.9"
refused "an empty manifest" ": > release-manifest.txt" "holds no completed release"

if [ "$failures" -eq 0 ]; then
    echo "release-publish tests: all passed"
else
    echo "release-publish tests: $failures failed"
    exit 1
fi
