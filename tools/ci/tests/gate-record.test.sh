#!/usr/bin/env bash
# Tests for tools/ci/gate-record.sh, against throwaway repositories.
# [AC-RF-CA-5a] [AC-RF-CA-5b] [AC-RF-CA-5c] [AC-RF-CA-5d]
#
#   bash tools/ci/tests/gate-record.test.sh
#
# The claims under test: a record is made only from a run that gives every criterion of
# the spec a verdict; a failing run is recorded as failing, not left out; the committed
# record is exactly what its logs give; and a fresh run can be held against it.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../gate-record.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

SPEC=docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md
RECORDS=docs/compliance/gate-runs

g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
record() { GATE_RECORD_ROOT="$repo" bash "$script" "$@"; }

# A repository whose spec defines three criteria and whose test recipe is two invocations.
new_repo() {
    repo="$work/$1"
    mkdir -p "$repo/docs/dev/markdown" "$repo/docs/compliance" "$repo/src"
    git init -q "$repo"
    # A checkout that rewrites line ends would change the logs under the record.
    git -C "$repo" config core.autocrlf false
    cat > "$repo/$SPEC" <<'EOF'
# Spec
## PART B — prompts
Mentions AC-Z-9, which is not a criterion of Part C.
## PART C — Consolidated Acceptance-Criteria Matrix
| AC-X-1 | first |
| AC-X-2 | second |
| AC-Y-1 | third |
## PART D — other
AC-Q-1 is mentioned here only.
EOF
    printf 'test:\n    cargo test -p alpha --features test-backend\n    cargo test -p beta --features backend-fips\n\nother:\n    cargo test -p never\n' > "$repo/justfile"
    printf 'Status\n\n<!-- gate-record:begin -->\nstale\n<!-- gate-record:end -->\n\nAfter.\n' > "$repo/README.md"
    printf 'Sweep\n\n<!-- gate-record:begin -->\n<!-- gate-record:end -->\n' > "$repo/docs/compliance/GA_READINESS.md"
    echo "fn main() {}" > "$repo/src/main.rs"
    g add -A
    g commit -q -m base
    commit="$(g rev-parse HEAD)"
}

# step ID DESCRIPTION RESULT [PASSED FAILED]
step() {
    printf '\n============================================================\n%s — %s\n  $ cargo test\n' "$1" "$2"
    printf '    running %d tests\n    test result: %s. %d passed; %d failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n' \
        "$((${4:-2} + ${5:-0}))" "$([ "${5:-0}" = 0 ] && echo ok || echo FAILED)" "${4:-2}" "${5:-0}"
    if [ "$3" = PASS ]; then echo "  >>> $1: PASS"; else echo "  >>> $1: FAIL (exit 101)"; fi
}

# meta DIR PART EXIT [KEY=VALUE...] — later lines win, as `meta` reads the first match,
# so overrides are written first.
meta() {
    local dir="$1" part="$2" rc="$3"
    shift 3
    {
        for kv in "$@"; do echo "$kv"; done
        cat <<EOF
part=$part
commit=$commit
tree=clean
image=toolchain:1
image_id=sha256:1111
base_image=rust:1.82@sha256:2222
dockerfile_sha256=3333
rustc=rustc 1.82.0 (f6e511eec 2024-10-15)
cargo=cargo 1.82.0 (8f40fc59f 2024-08-21)
network=none
started=2026-10-14T10:00:00Z
finished=2026-10-14T10:02:05Z
seconds=125
exit=$rc
platform=local
run_url=
EOF
    } > "$dir/$part.meta"
}

test_log() {
    printf '\033[1mcargo test -p alpha --features test-backend\033[0m\r\n'
    printf '     Running unittests src/lib.rs\n\ntest result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
    printf '   Doc-tests alpha\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
    printf 'cargo test -p beta --features backend-fips\n'
    printf 'test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
}

# good_run DIR — a complete passing run of $commit.
good_run() {
    local dir="$1"
    mkdir -p "$dir"
    {
        step AC-X-1/2 "one step, two criteria" PASS 5
        step EV-A1 "an eval step, not a criterion" PASS 1
        step AC-Y-1 "third" PASS 2
        echo "  >>> PHASE 1 DEFINITION OF DONE: MET"
        echo
        echo ">>> FULL SUITE PASSED, AIR-GAPPED (--network none)."
    } > "$dir/acceptance.log"
    test_log > "$dir/test.log"
    meta "$dir" acceptance 0
    meta "$dir" test 0
}

refused() { # refused NAME PATTERN DIR
    if out="$(record --from "$3" 2>&1)"; then
        fail "$1: a record was written"
    elif ! grep -q "$2" <<<"$out"; then
        fail "$1: refused, but not for the reason: $out"
    elif [ -e "$repo/$RECORDS/CURRENT" ] && [ "$(cat "$repo/$RECORDS/CURRENT")" != "${kept:-}" ]; then
        fail "$1: refused, but something was written"
    else
        pass "$1"
    fi
}

# --- a complete, passing run -------------------------------------------------------------
new_repo good
run="$work/run-good"
good_run "$run"
if out="$(record --from "$run" 2>&1)"; then
    pass "a complete run is recorded"
else
    fail "a complete run is recorded: $out"
fi
rec="$repo/$RECORDS/$commit.md"
[ "$(cat "$repo/$RECORDS/CURRENT" 2>/dev/null)" = "$commit" ] && pass "CURRENT names the recorded commit" || fail "CURRENT names the recorded commit"
grep -q '^\*\*Result: PASS.\*\* 3 of 3 acceptance criteria pass' "$rec" && pass "all three criteria are counted" || fail "all three criteria are counted: $(sed -n 6p "$rec")"
if grep -q '^| AC-X-1 | PASS | `AC-X-1/2` |' "$rec" && grep -q '^| AC-X-2 | PASS | `AC-X-1/2` |' "$rec"; then
    pass "a grouped verdict gives each of its criteria a row, naming the verdict line"
else
    fail "a grouped verdict gives each of its criteria a row, naming the verdict line"
fi
grep -q 'AC-Z-9\|AC-Q-1' "$rec" && fail "only Part C defines criteria" || pass "only Part C defines criteria"
grep -q '^| `EV-A1` | PASS |' "$rec" && pass "a step that is not a criterion is listed apart" || fail "a step that is not a criterion is listed apart"
grep -q '\*\*8 passed · 0 failed · 0 ignored\*\*, in 3 test binaries' "$rec" && pass "the suite's own tests are totalled" || fail "the suite's own tests are totalled"
grep -q '^\*\*7 passed · 0 failed · 1 ignored\*\*, in 3 test binaries and doc-test runs across 2 of 2 invocations. Crypto backends exercised: backend-fips, test-backend' "$rec" \
    && pass "just test is totalled, with both backends" || fail "just test is totalled, with both backends: $(grep -n 'invocations' "$rec")"
grep -q '^| 1 | `cargo test -p alpha --features test-backend` | test-backend | 2 | 3 | 0 | 1 |' "$rec" \
    && pass "each invocation has its own row, colour codes and CRs aside" || fail "each invocation has its own row: $(grep -n 'alpha' "$rec")"
digest="$( (sha256sum "$repo/$RECORDS/$commit/acceptance.log" 2>/dev/null || shasum -a 256 "$repo/$RECORDS/$commit/acceptance.log") | awk '{print $1}')"
grep -q "acceptance.log) | \`$digest\`" "$rec" && pass "the record cites the digest of the log kept beside it" || fail "the record cites the digest of the log kept beside it"
grep -q "Wall time | 4m 10s" "$rec" && pass "wall time is the two parts together" || fail "wall time is the two parts together"
if grep -q "| Acceptance criteria | \*\*3 / 3 pass\*\* |" "$repo/README.md" && ! grep -q stale "$repo/README.md" &&
   grep -q "^After\.$" "$repo/README.md" && grep -q "$RECORDS/$commit.md" "$repo/README.md"; then
    pass "the README table is generated from the record and names its commit"
else
    fail "the README table is generated from the record and names its commit: $(cat "$repo/README.md")"
fi
grep -q "(../../$RECORDS/$commit.md)" "$repo/docs/compliance/GA_READINESS.md" && pass "and so is the GA_READINESS table, with its own relative link" \
    || fail "and so is the GA_READINESS table"

g add -A && g commit -q -m "gate record"
record --check >/dev/null 2>&1 && pass "the committed record checks" || fail "the committed record checks: $(record --check 2>&1)"
record --check --release >/dev/null 2>&1 && pass "a release one records-only commit after the gated commit is covered" \
    || fail "a release one records-only commit after the gated commit is covered: $(record --check --release 2>&1)"

# --- the record matches the log (AC-RF-CA-5b) ---------------------------------------------
sed -i 's/3 of 3 acceptance criteria pass/4 of 4 acceptance criteria pass/' "$rec"
record --check >/dev/null 2>&1 && fail "a record edited by hand fails the check" || pass "a record edited by hand fails the check"
g checkout -q -- .
echo "an extra line" >> "$repo/$RECORDS/$commit/test.log"
record --check >/dev/null 2>&1 && fail "a log that no longer matches its digest fails the check" || pass "a log that no longer matches its digest fails the check"
g checkout -q -- .
sed -i 's|3 / 3 pass|72 / 72 pass|' "$repo/README.md"
if out="$(record --check 2>&1)"; then
    fail "a status table edited by hand fails the check"
else
    grep -q 'README.md is not the one the record gives' <<<"$out" && pass "a status table edited by hand fails the check" || fail "the edit is explained: $out"
fi
record --status >/dev/null 2>&1
record --check >/dev/null 2>&1 && pass "and --status puts it back" || fail "and --status puts it back"
sed 's/$/\r/' "$repo/README.md" > "$work/crlf" && cat "$work/crlf" > "$repo/README.md"
record --check >/dev/null 2>&1 && pass "a CRLF checkout of the README is not a difference" || fail "a CRLF checkout of the README is not a difference"
g checkout -q -- .

# --- the run reproduces (AC-RF-CA-5d) -----------------------------------------------------
fresh="$work/run-fresh"
good_run "$fresh"
meta "$fresh" acceptance 0 seconds=999 image_id=sha256:other
record --compare "$fresh" >/dev/null 2>&1 && pass "a fresh run with the same results reproduces the record, timings aside" \
    || fail "a fresh run with the same results reproduces the record: $(record --compare "$fresh" 2>&1)"
sed -i 's/  >>> AC-Y-1: PASS/  >>> AC-Y-1: FAIL (exit 101)/; s/FULL SUITE PASSED/FULL SUITE FAILED/' "$fresh/acceptance.log"
meta "$fresh" acceptance 1
if out="$(record --compare "$fresh" 2>&1)"; then
    fail "a fresh run in which a criterion differs does not reproduce it"
else
    grep -q '> AC-Y-1 FAIL' <<<"$out" && pass "a fresh run in which a criterion differs does not reproduce it, and the criterion is named" \
        || fail "the difference is named: $out"
fi

# --- the release must be the gated commit ----------------------------------------------------
echo "fn main() { changed(); }" > "$repo/src/main.rs"
g commit -q -am "a change the gate covers"
record --check >/dev/null 2>&1 && pass "an older record still checks on a later commit" || fail "an older record still checks on a later commit: $(record --check 2>&1)"
if out="$(record --check --release 2>&1)"; then
    fail "a release whose sources differ from the gated commit is not covered"
else
    grep -q 'src/main.rs' <<<"$out" && pass "a release whose sources differ from the gated commit is not covered, and the file is named" \
        || fail "the stray file is named: $out"
fi

# --- all criteria are accounted for (AC-RF-CA-5c) ---------------------------------------------
new_repo refuse
kept=""
run="$work/run-missing"
good_run "$run"
sed -i '/^AC-Y-1 — /,/>>> AC-Y-1: PASS/d' "$run/acceptance.log"
refused "a log from which one criterion is absent is refused" 'no verdict for: AC-Y-1' "$run"
[ ! -e "$repo/$RECORDS" ] && pass "and no record, log or pointer is written" || fail "and no record, log or pointer is written: $(ls -R "$repo/$RECORDS")"
grep -q stale "$repo/README.md" && pass "and the status tables are left alone" || fail "and the status tables are left alone"

run="$work/run-unknown"
good_run "$run"
sed -i 's/>>> EV-A1: PASS/>>> AC-X-3: PASS/' "$run/acceptance.log"
refused "a verdict for a criterion the spec does not define is refused" 'does not define: AC-X-3' "$run"

run="$work/run-cut"
good_run "$run"
sed -i '/FULL SUITE/d' "$run/acceptance.log"
refused "a log cut short is refused" "no closing" "$run"

run="$work/run-dirty"
good_run "$run"
meta "$run" test 0 tree=dirty
refused "a run of a tree that was not clean is refused" 'tree was not clean' "$run"

run="$work/run-commits"
good_run "$run"
meta "$run" test 0 commit=0000000000000000000000000000000000000001
refused "two parts of different commits are refused" 'different commits' "$run"

run="$work/run-nowhere"
good_run "$run"
meta "$run" acceptance 0 commit=0000000000000000000000000000000000000002
meta "$run" test 0 commit=0000000000000000000000000000000000000002
refused "a commit that is not in the repository is refused" 'not in this repository' "$run"

run="$work/run-networked"
good_run "$run"
meta "$run" acceptance 0 network=bridge
refused "a run that was not air-gapped is refused" 'network none' "$run"

run="$work/run-lies"
good_run "$run"
meta "$run" acceptance 1
refused "a log that says PASSED beside a non-zero exit is refused" 'exit status is 1' "$run"

run="$work/run-other-recipe"
good_run "$run"
sed -i 's/cargo test -p beta --features backend-fips/cargo test -p gamma/' "$run/test.log"
refused "a test log that is not of the justfile's recipe is refused" "not the justfile's test recipe" "$run"

run="$work/run-short"
good_run "$run"
sed -i '/cargo test -p beta/,$d' "$run/test.log"
refused "a test run that exited 0 without running every invocation is refused" '1 of 2 invocations' "$run"

run="$work/run-half"
good_run "$run"
rm "$run/test.log"
refused "a run with only one part is refused" 'test.log is missing' "$run"

# --- a failing run is recorded, not left out -----------------------------------------------------
new_repo failing
run="$work/run-failing"
good_run "$run"
{
    step AC-X-1/2 "one step, two criteria" PASS 5
    step AC-Y-1 "third" FAIL 1 1
    echo "    test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    echo
    echo ">>> FULL SUITE FAILED (fail closed)."
} > "$run/acceptance.log"
meta "$run" acceptance 1
if record --from "$run" >/dev/null 2>&1; then
    pass "a run in which a criterion fails is still recorded"
else
    fail "a run in which a criterion fails is still recorded: $(record --from "$run" 2>&1)"
fi
rec="$repo/$RECORDS/$commit.md"
if grep -q '^\*\*Result: FAIL.\*\* 2 of 3' "$rec" 2>/dev/null && grep -q '^\*\*Failing criteria:\*\* AC-Y-1\.$' "$rec" &&
   grep -q '^| AC-Y-1 | FAIL |' "$rec"; then
    pass "the record says FAIL and names the criterion"
else
    fail "the record says FAIL and names the criterion"
fi
grep -q '\*\*6 passed · 1 failed · 0 ignored\*\*' "$rec" 2>/dev/null && pass "the output a failing step repeats after its verdict is not counted twice" \
    || fail "the output a failing step repeats after its verdict is not counted twice: $(grep -n 'Tests the suite' "$rec")"
if grep -q '| Failing criteria | \*\*AC-Y-1\*\* |' "$repo/README.md" && grep -q '| Gate result | \*\*FAIL\*\* |' "$repo/README.md"; then
    pass "the README says so, with the id"
else
    fail "the README says so, with the id: $(cat "$repo/README.md")"
fi
g add -A && g commit -q -m "gate record"
record --check >/dev/null 2>&1 && pass "a failing record is a valid record" || fail "a failing record is a valid record: $(record --check 2>&1)"
record --check --release >/dev/null 2>&1 && fail "but it cannot back a release" || pass "but it cannot back a release"

# --- no record yet ------------------------------------------------------------------------------
new_repo empty
record --check >/dev/null 2>&1 && fail "a table that claims something with no record fails the check" || pass "a table that claims something with no record fails the check"
record --status >/dev/null 2>&1
grep -q 'No gate run is recorded yet' "$repo/README.md" && record --check >/dev/null 2>&1 \
    && pass "with no record the tables say so, and that checks" || fail "with no record the tables say so, and that checks"
record --check --release >/dev/null 2>&1 && fail "a release with no record is refused" || pass "a release with no record is refused"

# --- this repository --------------------------------------------------------------------------------
if out="$(bash "$script" --check 2>&1)"; then
    pass "this repository's own record and status tables check"
else
    fail "this repository's own record and status tables check: $out"
fi

if [ "$failures" -gt 0 ]; then
    echo "gate-record tests: $failures failure(s)"
    exit 1
fi
echo "gate-record tests: all passed"
