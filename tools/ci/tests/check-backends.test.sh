#!/usr/bin/env bash
# The check-backends gate is a CI job, and both pipelines run it. [RT-08]
# [VLD-ANB-MIDGARD-0067] [AC-54.4]
#
#   bash tools/ci/tests/check-backends.test.sh
#
# Runs on the host, without cargo. pipelines.sh --check already fails when one
# pipeline calls a job the other does not; this pins that the job exists at all, that
# both pipelines call it, and that the justfile recipe it runs keeps its four steps.
set -uo pipefail

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$repo_root"
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

if bash tools/ci/run.sh list | grep -E '^check-backends +\[toolchain\]' >/dev/null; then
    pass "run.sh list names check-backends as a toolchain job"
else
    fail "run.sh list does not name check-backends"
fi

if cat .github/workflows/*.yml | tr -d '\r' | grep -E '^ *- run: bash tools/ci/run\.sh check-backends *$' >/dev/null; then
    pass "GitHub Actions runs check-backends"
else
    fail "GitHub Actions does not run check-backends"
fi

if tr -d '\r' < .gitlab-ci.yml | grep -E '^ *- bash tools/ci/run\.sh check-backends *$' >/dev/null; then
    pass "GitLab CI runs check-backends"
else
    fail "GitLab CI does not run check-backends"
fi

recipe="$(tr -d '\r' < justfile | awk '/^check-backends:/ { on = 1; next } on && /^[^ #]/ { exit } on')"
if [ -z "$recipe" ]; then
    fail "the justfile has no check-backends recipe"
else
    pass "the justfile has a check-backends recipe"
    for step in 'tools/ci/no-test-backend.sh' "-f '{p} {f}'" 'resolved --workspace' \
                'release-artefacts.sh --rows' 'resolved -p "$pkg"' \
                '--release --offline --locked --features test-backend' 'INSECURE' \
                '--release --offline --locked --features backend-fips'; do
        if grep -qF -- "$step" <<<"$recipe"; then
            pass "check-backends runs: $step"
        else
            fail "check-backends no longer runs: $step"
        fi
    done
fi

[ "$failures" -eq 0 ] || exit 1
