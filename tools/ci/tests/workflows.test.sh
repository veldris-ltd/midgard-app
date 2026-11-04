#!/usr/bin/env bash
# Every actions/checkout step in .github/workflows sets persist-credentials: false, so
# the workflow token is never written into .git/config for the rest of the job: no job
# pushes, and the release jobs run on self-hosted signing runners whose workspace sits
# beside the signing material. [VLD-ANB-MIDGARD-0081] [AC-68.3]
#
#   bash tools/ci/tests/workflows.test.sh
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
workflows="$(cd "$here/../../../.github/workflows" && pwd)"
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# Each checkout step, step by step: the lines after `uses: actions/checkout@` up to the
# next step or a line indented no deeper than the step's dash.
missing="$(awk '
    { sub(/\r$/, "") }
    function close_step() {
        if (open && !seen) print at ": actions/checkout without persist-credentials: false"
        open = 0
    }
    FNR == 1 { close_step() }
    open {
        match($0, /^ */)
        if ($0 !~ /^ *$/ && RLENGTH <= dash) close_step()
        else if ($0 ~ /^ *persist-credentials: *false *(#.*)?$/) seen = 1
    }
    /^ *- uses: actions\/checkout@/ { match($0, /^ */); dash = RLENGTH; open = 1; seen = 0; at = FILENAME ":" FNR }
    END { close_step() }
' "$workflows"/*.yml)"
[ -z "$missing" ] && pass "every checkout step sets persist-credentials: false" ||
    fail "a checkout keeps the workflow token in .git/config:"$'\n'"$missing"

checkouts="$(cat "$workflows"/*.yml | grep -c 'uses: actions/checkout@')"
unpersisted="$(cat "$workflows"/*.yml | grep -c 'persist-credentials: false')"
[ "$checkouts" -gt 0 ] && [ "$checkouts" = "$unpersisted" ] &&
    pass "$checkouts checkout(s), $unpersisted persist-credentials: false" ||
    fail "the counts agree: $checkouts checkout(s), $unpersisted persist-credentials: false"

# The release gate runs from a checkout of main, not from the tag it gates: in both
# pipelines' preflight job the first two jobs run are release-tag and release-preflight
# from .ci-trusted, and the trusted checkout is of main. [VLD-ANB-MIDGARD-0035] [AC-P2.3]
gitlab="$workflows/../../.gitlab-ci.yml"
want=$'bash .ci-trusted/tools/ci/run.sh release-tag\nbash .ci-trusted/tools/ci/run.sh release-preflight'
# job_runs FILE JOB: the commands the job runs, in order, YAML list markers removed.
job_runs() {
    tr -d '\r' < "$1" | awk -v job="$2" '
        $0 ~ "^ *" job ":$" { match($0, /^ */); indent = RLENGTH; on = 1; next }
        on && /^ *[a-z.][a-z0-9_.-]*:/ { match($0, /^ */); if (RLENGTH <= indent) on = 0 }
        on' | sed -n -e 's/^ *- run: *//p' -e 's/^    - //p'
}
gh_runs="$(job_runs "$workflows/release.yml" preflight | grep 'run\.sh')"
gl_runs="$(job_runs "$gitlab" release-preflight | grep 'run\.sh')"
[ "$(head -n 2 <<<"$gh_runs")" = "$want" ] &&
    pass "GitHub: the release preflight runs the tag gate from .ci-trusted before any tagged-commit script" ||
    fail "GitHub: the release preflight runs the tag gate from .ci-trusted first:"$'\n'"$gh_runs"
[ "$(head -n 2 <<<"$gl_runs")" = "$want" ] &&
    pass "GitLab: the release preflight runs the tag gate from .ci-trusted before any tagged-commit script" ||
    fail "GitLab: the release preflight runs the tag gate from .ci-trusted first:"$'\n'"$gl_runs"
tr -d '\r' < "$workflows/release.yml" | grep -B3 -A3 'path: .ci-trusted' | grep -q 'ref: main' &&
    pass "GitHub: .ci-trusted is a checkout of main" || fail "GitHub: .ci-trusted is a checkout of main"
tr -d '\r' < "$gitlab" | grep -q '^    - git clone --branch main --depth [0-9]* "$CI_REPOSITORY_URL" .ci-trusted$' &&
    pass "GitLab: .ci-trusted is a clone of main" || fail "GitLab: .ci-trusted is a clone of main"

# Both jobs that run on the signing runner are behind the protected `release`
# environment, so only a v* tag reaches them, and only once a second maintainer has
# approved the deployment. [VLD-ANB-MIDGARD-0035]
for job in preflight release; do
    tr -d '\r' < "$workflows/release.yml" | awk -v job="$job" '
        $0 ~ "^  " job ":$" { on = 1; next }
        on && /^  [a-z]/ { on = 0 }
        on && /^    environment: release$/ { found = 1 }
        END { exit !found }' &&
        pass "GitHub: the $job job runs in the release environment" ||
        fail "GitHub: the $job job runs in the release environment"
done

# Every job that runs a target which opens a keystore is handed both passphrase files,
# from the runner secrets docs/ops/PKI.md names; without them the keystores do not open
# and a tagged release stops at its preflight. On GitHub each such job maps them in its
# env:. On GitLab they are CI/CD variables of the names run.sh reads, which reach every
# job as they are, so each such job runs on the signing runner (extends .signing) and
# nothing in .gitlab-ci.yml sets them, which would shadow or empty them.
# [VLD-ANB-MIDGARD-0007]
keystore_targets='release-preflight|release|release-images|release-bundle'
passphrases='MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE'
# keystore_jobs FILE: the top-level jobs that run one of those targets through run.sh.
keystore_jobs() {
    tr -d '\r' < "$1" | awk -v t="$keystore_targets" '
        /^jobs:$/ { gh = 1; next }
        gh && /^  [a-z][a-z0-9_-]*:$/ { job = $1; sub(/:$/, "", job) }
        !gh && /^[a-z][a-z0-9_-]*:$/ { job = $1; sub(/:$/, "", job) }
        job && $0 ~ ("run[.]sh (" t ")( |$)") { print job }' | sort -u
}
# job_block FILE JOB INDENT: the lines of JOB, a key at INDENT spaces.
job_block() {
    tr -d '\r' < "$1" | awk -v job="$2" -v ind="$3" '
        $0 ~ ("^" ind job ":$") { on = 1; next }
        on && $0 ~ ("^" ind "[^ #]") { on = 0 }
        on'
}
gh_jobs="$(keystore_jobs "$workflows/release.yml" | tr '
' ' ')"
[ "$gh_jobs" = "preflight release " ] &&
    pass "GitHub: the jobs that open a keystore are preflight and release" ||
    fail "GitHub: the jobs that open a keystore are preflight and release, found: $gh_jobs"
for job in $gh_jobs; do
    block="$(job_block "$workflows/release.yml" "$job" "  ")"
    for var in $passphrases; do
        grep -qx "      $var: \${{ secrets.$var }}" <<<"$block" &&
            pass "GitHub: the $job job maps $var from its runner secret" ||
            fail "GitHub: the $job job maps $var from its runner secret"
    done
done
gl_jobs="$(keystore_jobs "$gitlab" | tr '
' ' ')"
[ "$gl_jobs" = "release release-preflight " ] &&
    pass "GitLab: the jobs that open a keystore are release-preflight and release" ||
    fail "GitLab: the jobs that open a keystore are release-preflight and release, found: $gl_jobs"
for job in $gl_jobs; do
    job_block "$gitlab" "$job" "" | grep -qE '^  extends: \[?[^]]*\.signing' &&
        pass "GitLab: the $job job runs on the signing runner (extends .signing)" ||
        fail "GitLab: the $job job runs on the signing runner (extends .signing)"
done
for var in $passphrases; do
    tr -d '\r' < "$gitlab" | grep -qE "^ +$var:" &&
        fail "GitLab: nothing sets $var, which would shadow the CI/CD variable" ||
        pass "GitLab: nothing sets $var, so the CI/CD variable reaches the release jobs"
    tr -d '\r' < "$gitlab" | grep -q "^#.*$var" &&
        pass "GitLab: the header lists $var among the release runner's variables" ||
        fail "GitLab: the header lists $var among the release runner's variables"
done

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all workflow tests passed"
