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
    tr -d '' < "$workflows/release.yml" | awk -v job="$job" '
        $0 ~ "^  " job ":$" { on = 1; next }
        on && /^  [a-z]/ { on = 0 }
        on && /^    environment: release$/ { found = 1 }
        END { exit !found }' &&
        pass "GitHub: the $job job runs in the release environment" ||
        fail "GitHub: the $job job runs in the release environment"
done

# The release goes by the shell-audit gate's verdict, not by the audit jobs' results: a
# waived pre-release has audits that failed, or that GitHub cancelled before they
# started, and its release must still run once the gate passes; with no verdict of
# "pass" it must not. [RC-49] [AC-C-11]
# job_block FILE JOB: the job's lines, up to the next job.
job_block() {
    tr -d '\r' < "$1" | awk -v job="$2" '
        $0 ~ "^  " job ":$" { on = 1; next }
        on && /^  [a-z]/ { on = 0 }
        on'
}
release_if="$(job_block "$workflows/release.yml" release | sed -n 's/^    if: *//p')"
for part in '!cancelled()' "needs.preflight.result == 'success'" "needs.shell-audits.outputs.verdict == 'pass'"; do
    grep -qF -- "$part" <<<"$release_if" &&
        pass "GitHub: the release job requires $part" ||
        fail "GitHub: the release job requires $part: if: $release_if"
done
grep -qE 'always\(\)|failure\(\)|\|\|' <<<"$release_if" &&
    fail "GitHub: the release job's condition only narrows, never widens: if: $release_if" ||
    pass "GitHub: the release job's condition only narrows, never widens"
audit="$workflows/shell-audit.yml"
tr -d '\r' < "$audit" | grep -q '^        value: \${{ jobs.gate.outputs.verdict }}$' &&
    pass "GitHub: shell-audit.yml outputs the gate job's verdict" ||
    fail "GitHub: shell-audit.yml outputs the gate job's verdict"
# In the gate job the verdict is written by a step with no condition of its own, right
# after the gate step, so it is reached only when the gate passed.
gate_steps="$(job_block "$audit" gate | sed -n '/^    steps:$/,$p' | grep -E '^      - |^        (run|if|id):')"
tail -n 3 <<<"$gate_steps" | tr '\n' '|' |
    grep -qF -- '      - run: bash tools/ci/run.sh shell-audit-gate|      - id: verdict|        run: echo "verdict=pass" >> "$GITHUB_OUTPUT"|' &&
    pass "GitHub: the verdict is written only after the gate step passes" ||
    fail "GitHub: the verdict is written only after the gate step passes:"$'\n'"$gate_steps"
job_block "$audit" gate | grep -q '^      verdict: \${{ steps.verdict.outputs.verdict }}$' &&
    pass "GitHub: the gate job exposes the verdict step's output" ||
    fail "GitHub: the gate job exposes the verdict step's output"

# Publishing a tag never releases it again: the Bifrost registry refuses a second
# release of a version it holds, and what is published must be what was released. So a
# publish run starts no preflight, audit or release job, and its publish job depends on
# none: it fetches what the tag's own run made (tools/ci/run.sh, fetch_tag_release).
# [RC-07] [AC-RF-SC-5e]
for job in preflight shell-audits; do
    job_block "$workflows/release.yml" "$job" | grep -q "^    if: .*inputs.publish == ''" &&
        pass "GitHub: a publish run does not start the $job job" ||
        fail "GitHub: a publish run does not start the $job job"
done
publish_job="$(job_block "$workflows/release.yml" publish)"
grep -q '^    needs:' <<<"$publish_job" &&
    fail "GitHub: the publish job waits on no release in its own run" ||
    pass "GitHub: the publish job waits on no release in its own run"
grep -q '^      actions: read$' <<<"$publish_job" && grep -q '^      contents: write$' <<<"$publish_job" &&
    pass "GitHub: the publish job may read the tag's run and write the draft" ||
    fail "GitHub: the publish job may read the tag's run and write the draft"
grep -q 'download-artifact' <<<"$publish_job" &&
    fail "GitHub: the publish job takes no artefact from its own run" ||
    pass "GitHub: the publish job takes no artefact from its own run"
# gitlab_block NAME: a top-level key of .gitlab-ci.yml and its lines, up to the next.
gitlab_block() { tr -d '\r' < "$gitlab" | awk -v j="$1:" '$0 == j { on = 1; next } on && /^[^ #]/ { on = 0 } on'; }
gitlab_block release-publish | grep -q '^  needs: \[\]$' &&
    pass "GitLab: release-publish waits on no release in its own pipeline" ||
    fail "GitLab: release-publish waits on no release in its own pipeline"
# Every job rule in the file that starts a job on a v* tag or in release mode excludes
# a publish run, release-publish's own rule apart. (The pipeline-level `workflow:` rules
# decide only whether there is a pipeline.)
rule_lines="$(tr -d '\r' < "$gitlab" | awk '/^workflow:/ { skip = 1; next } /^[^ #]/ { skip = 0 } !skip' |
    grep -E -- '- if: .*(CI_COMMIT_TAG =~|MIDGARD_RELEASE_MODE == "release")' |
    grep -v 'MIDGARD_PUBLISH_TAG != ""')"
[ "$(wc -l <<<"$rule_lines")" -ge 5 ] && ! grep -v 'MIDGARD_PUBLISH_TAG == ""' <<<"$rule_lines" | grep -q . &&
    pass "GitLab: every rule for a tag or a release excludes a publish run" ||
    fail "GitLab: every rule for a tag or a release excludes a publish run:"$'\n'"$(grep -v 'MIDGARD_PUBLISH_TAG == ""' <<<"$rule_lines")"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all workflow tests passed"
