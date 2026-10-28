#!/usr/bin/env bash
# The key ceremony's published files can be committed. [RT-35] [AC-RF-CA-6a]
#
#   bash tools/ci/tests/ceremony.test.sh
#
# The ceremony itself is the Dvalin app (veldris-ltd/dvalin-app),
# whose contract test runs this repository's release checks against what it
# publishes. The claim under test here is this repository's half: the two trust
# anchors the ceremony writes are not ignored by git, so its commit step works, while
# every other .pub file stays ignored. (tools/ci/tests/verify-tag.test.sh covers the
# allowed_signers file.)
set -uo pipefail

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

if ! git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
    echo "skip  ceremony tests (not a git checkout)"
    exit 0
fi
for anchor in deploy/trust/release-trust.pub deploy/trust/bifrost-trust.pub; do
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
