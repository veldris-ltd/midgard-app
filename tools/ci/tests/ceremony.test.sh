#!/usr/bin/env bash
# The key ceremony's published files can be committed. [RT-35] [AC-RF-CA-6a]
#
#   bash tools/ci/tests/ceremony.test.sh
#
# The ceremony itself is the Dvalin app (veldris-ltd/dvalin-app),
# whose contract test runs this repository's release checks against what it
# publishes. The claims under test here are this repository's half: the three trust
# anchors the ceremony writes are not ignored by git, so its commit step works, while
# every other .pub file stays ignored (tools/ci/tests/verify-tag.test.sh covers the
# allowed_signers file); and the release preflight refuses a release while the
# model-signing anchor is missing. [VLD-ANB-MIDGARD-0028]
set -uo pipefail

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# The preflight's anchor checks, on a copy of it in a tree of its own (it works from the
# checkout root it lives in) holding the release and Bifrost anchors: without
# deploy/trust/model-signing.pub the release is refused, naming it; with it, that check
# passes. (The rest of the preflight fails here, unprovisioned; only the anchor lines
# are looked at.)
tree="$(mktemp -d)"
trap 'rm -rf "$tree"' EXIT
mkdir -p "$tree/tools/ci" "$tree/deploy/trust"
cp "$repo_root/tools/ci/release-preflight.sh" "$repo_root/tools/ci/lib.sh" "$tree/tools/ci/"
hex="$(printf '%064d' 0)"
echo "ML-DSA-87 midgard-release-test $hex" > "$tree/deploy/trust/release-trust.pub"
echo "ML-DSA-65 midgard-bifrost-test $hex" > "$tree/deploy/trust/bifrost-trust.pub"
out="$(PATH="/usr/bin:/bin" bash "$tree/tools/ci/release-preflight.sh" 2>&1)"
status=$?
if [ "$status" -ne 0 ] && grep -q 'deploy/trust/model-signing.pub is not committed' <<<"$out"; then
    pass "the preflight refuses a release without deploy/trust/model-signing.pub"
else
    fail "the preflight did not refuse a missing model-signing anchor (exit $status)"
fi
echo "ML-DSA-65 model-signing-test $hex" > "$tree/deploy/trust/model-signing.pub"
out="$(PATH="/usr/bin:/bin" bash "$tree/tools/ci/release-preflight.sh" 2>&1)"
grep -q 'ok — deploy/trust/model-signing.pub pins an ML-DSA-65 key' <<<"$out" \
    && pass "the preflight accepts a committed ML-DSA-65 model-signing anchor" \
    || fail "the preflight did not accept a committed model-signing anchor: $out"

if ! git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
    echo "skip  ceremony tests (not a git checkout)"
    [ "$failures" -eq 0 ] || { echo "ceremony tests: $failures failure(s)"; exit 1; }
    exit 0
fi
for anchor in deploy/trust/release-trust.pub deploy/trust/bifrost-trust.pub \
    deploy/trust/model-signing.pub; do
    if git -C "$repo_root" check-ignore -q "$anchor"; then
        fail "$anchor is ignored by git, so the ceremony could not commit it"
    else
        pass "$anchor can be committed"
    fi
done
git -C "$repo_root" check-ignore -q deploy/trust/some-other.pub \
    && pass "other .pub files stay ignored" || fail "other .pub files stay ignored"

if [ "$failures" -gt 0 ]; then
    echo "ceremony tests: $failures failure(s)"
    exit 1
fi
echo "ceremony tests: all passed"
