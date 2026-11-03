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

# Both jobs on the signing runner are handed each keystore's passphrase file, from the
# _PATH secret the preflight names, as they are handed the keystores: without them the
# preflight refuses and signing_mounts stops. [VLD-ANB-MIDGARD-0007]
# job_block FILE INDENT JOB: the lines of JOB, a key at INDENT spaces, in FILE.
job_block() {
    tr -d '\r' < "$1" | awk -v pre="$2" -v job="$3" '
        $0 == pre job ":" { on = 1; next }
        on && /^ *[^ #]/ { match($0, /^ */); if (RLENGTH <= length(pre)) on = 0 }
        on'
}
for var in MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE; do
    for job in preflight release; do
        job_block "$workflows/release.yml" "  " "$job" |
            grep -qx "      $var: \${{ secrets.${var}_PATH }}" &&
            pass "GitHub: the $job job maps $var from secrets.${var}_PATH" ||
            fail "GitHub: the $job job maps $var from secrets.${var}_PATH"
    done
    job_block "$gitlab" "" .signing | grep -qx "    $var: \$${var}_PATH" &&
        pass "GitLab: .signing maps $var from \$${var}_PATH" ||
        fail "GitLab: .signing maps $var from \$${var}_PATH"
done

# The advisory refresh's write token reaches only the step that pushes and opens the
# pull request, not the toolchain build or any other step. [AC-P4.5]
# (VLD-ANB-MIDGARD-0076)
token_steps="$(tr -d '\r' < "$workflows/advisory-refresh.yml" | awk '
    /^      - / { step = $0; sub(/^      - (run: *)?/, "", step) }
    /^    steps:$/ { in_steps = 1 }
    /github\.token|secrets\./ { print (in_steps && step != "" ? step : "outside a step") }')"
[ "$token_steps" = "bash tools/ci/run.sh advisory-refresh" ] &&
    pass "GitHub: the advisory refresh's token reaches the advisory-refresh step alone" ||
    fail "GitHub: the advisory refresh's token reaches the advisory-refresh step alone; it reaches:"$'\n'"$token_steps"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all workflow tests passed"
