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

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all workflow tests passed"
