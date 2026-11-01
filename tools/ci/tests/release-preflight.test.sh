#!/usr/bin/env bash
# Tests for the job-completed hook check in tools/ci/release-preflight.sh. [RC-07]
# [VLD-ANB-MIDGARD-0081] [AC-68.5]
#
#   bash tools/ci/tests/release-preflight.test.sh
#
# The claim under test: on a self-hosted runner with no job-completed cleanup hook the
# preflight fails closed, naming the hook and pointing to docs/ops/PKI.md; with the
# hook, that item passes; a hosted runner is not asked for one. The preflight's other
# items have no inputs here (a stub docker that has no images, no keystores), so the
# run fails on them either way; only the hook item's lines are checked.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../release-preflight.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

mkdir -p "$work/stubs"
printf '#!/usr/bin/env bash\nexit 1\n' > "$work/stubs/docker"
chmod +x "$work/stubs/docker"
printf '#!/usr/bin/env bash\nrm -rf "$GITHUB_WORKSPACE"\n' > "$work/cleanup.sh"

preflight() { # preflight VAR=VALUE...: run with only these runner settings
    env -u RUNNER_ENVIRONMENT -u ACTIONS_RUNNER_HOOK_JOB_COMPLETED -u MIDGARD_SIGNING_KEYSTORE \
        -u MIDGARD_BIFROST_KEYSTORE -u MIDGARD_BIFROST_REGISTRY PATH="$work/stubs:$PATH" \
        "$@" bash "$script" 2>&1
}

if out="$(preflight RUNNER_ENVIRONMENT=self-hosted)"; then
    fail "a self-hosted runner with no hook fails closed"
else
    grep 'ACTIONS_RUNNER_HOOK_JOB_COMPLETED is not set' <<<"$out" | grep -q 'docs/ops/PKI.md, "Signing runner"' &&
        pass "a self-hosted runner with no hook fails closed, pointing to PKI.md" ||
        fail "the missing hook is named, with the pointer: $out"
fi

out="$(preflight RUNNER_ENVIRONMENT=self-hosted ACTIONS_RUNNER_HOOK_JOB_COMPLETED="$work/absent.sh")"
grep -q "names $work/absent.sh, which is not a readable file" <<<"$out" &&
    pass "a hook that names no file is refused" || fail "a hook that names no file is refused: $out"

out="$(preflight RUNNER_ENVIRONMENT=self-hosted ACTIONS_RUNNER_HOOK_JOB_COMPLETED="$work/cleanup.sh")"
grep -q "ok — job-completed hook $work/cleanup.sh" <<<"$out" && ! grep -q 'HOOK_JOB_COMPLETED' <<<"$out" &&
    pass "with the hook installed, the item passes" || fail "with the hook installed, the item passes: $out"

out="$(preflight RUNNER_ENVIRONMENT=github-hosted)"
grep -q 'job-completed\|HOOK_JOB_COMPLETED' <<<"$out" && fail "a hosted runner is not asked for a hook: $out" ||
    pass "a hosted runner is not asked for a hook"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all release-preflight tests passed"
