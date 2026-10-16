#!/usr/bin/env bash
# Tests for tools/ci/numbers.sh, against throwaway repositories.
# [AC-RF-CA-2a] [AC-RF-CA-2b] [AC-RF-CA-2c] [AC-RF-CA-2d]
#
#   bash tools/ci/tests/numbers.test.sh
#
# The claims under test: the sheet is a function of the tree and reproduces byte for
# byte; a document that quotes a different figure fails the check, named by file and
# line; a run's figures come only from a gate record that covers the tree, and are
# otherwise "not run at this commit".
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../numbers.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

SPEC=docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md
g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
numbers() { NUMBERS_ROOT="$repo" bash "$script" "$@"; }

new_repo() {
    repo="$work/$1"
    mkdir -p "$repo/tools/ci" "$repo/docs/dev/markdown" "$repo/docs/dev/diagrams" "$repo/docs/dev/fixes" \
        "$repo/docs/compliance" "$repo/docs/releases" "$repo/docs/launch/pipeline" "$repo/a/src" "$repo/b/src" "$repo/vendor/x/src"
    git init -q "$repo"
    git -C "$repo" config core.autocrlf false
    printf '[workspace]\nresolver = "2"\nmembers = [\n    "a",\n    "b",\n]\n\n[workspace.package]\nversion = "9.9.9"\n' > "$repo/Cargo.toml"
    printf '//! a\n\nfn one() {}\n// note\n#[test]\nfn t1() {}\n    #[test]\n    #[ignore]\n    fn t2() {}\n' > "$repo/a/src/lib.rs"
    printf 'fn two() {}\n' > "$repo/b/src/lib.rs"
    printf '#[test]\nfn vendored() {}\n' > "$repo/vendor/x/src/lib.rs"
    printf '# Spec\n## PART C — matrix\n| AC-X-1 |\n| AC-X-2 |\n## PART D\n' > "$repo/$SPEC"
    printf 'test:\n    cargo test -p a\n' > "$repo/justfile"
    for s in SAD_00_A SAD_01_B SAD_05_Master_Prompt_and_Acceptance_Spec; do [ -f "$repo/docs/dev/markdown/$s.md" ] || echo "# $s" > "$repo/docs/dev/markdown/$s.md"; done
    for d in one two three; do echo "graph" > "$repo/docs/dev/diagrams/$d.mmd"; echo "x" > "$repo/docs/dev/diagrams/$d.png"; done
    printf '#!/usr/bin/env bash\nprintf "a a service a\\na b-tool tool -\\nb b service b\\n"\n' > "$repo/tools/ci/release-artefacts.sh"
    printf '#!/usr/bin/env bash\nprintf "ui-bundle c ui/c c.ui.tar @x/c\\nnpm-package s sdk s.tgz @x/s\\n"\n' > "$repo/tools/ci/js-artefacts.sh"
    printf '# Readme\n\nA 2-crate workspace with 2 acceptance criteria.\n' > "$repo/README.md"
    printf '# Getting started\n' > "$repo/GETTING_STARTED.md"
    printf '# Changelog\n' > "$repo/CHANGELOG.md"
    printf '# GA\n\n<!-- gate-record:begin -->\n<!-- gate-record:end -->\n' > "$repo/docs/compliance/GA_READINESS.md"
    printf '# rc\n' > "$repo/docs/releases/v9.md"
    printf '// deck\n' > "$repo/docs/launch/pipeline/build-deck.js"
    printf '# Plan\n## 0. Intro\n## 1. Executive summary\nA 2-crate tree.\n## 1a. Later\nThe old tree had 56 crates.\n' > "$repo/docs/dev/fixes/remedial-fixes.md"
    g add -A
    g commit -q -m base
}

# --- counting and reproduction (AC-RF-CA-2a) -----------------------------------------------
new_repo count
numbers --figures > "$work/figs" 2>&1
want="$(printf 'version=9.9.9\ncrates=2\nrust_files=2\nrust_lines=10\nrust_code_lines=7\ntest_functions=2\ntest_functions_ignored=1\ncriteria_defined=2\nbinaries=3\nservices=2\nimages=2\nconsoles=1\nnpm_packages=1\nsad_documents=3\ndiagrams=3\ngate_recorded=no')"
if [ "$(head -n 16 "$work/figs")" = "$want" ]; then
    pass "each figure is counted by its rule, vendor/ aside"
else
    fail "each figure is counted by its rule: $(diff <(echo "$want") <(head -n 16 "$work/figs"))"
fi

numbers --write >/dev/null 2>&1
numbers > "$work/printed" 2>/dev/null
cmp -s "$work/printed" "$repo/docs/compliance/NUMBERS.md" && pass "the sheet printed equals the sheet written, byte for byte" \
    || fail "the sheet printed equals the sheet written"
g add -A && g commit -q -m sheet
rm -rf "$work/clone" && git -c core.autocrlf=false clone -q "$repo" "$work/clone"
NUMBERS_ROOT="$work/clone" bash "$script" > "$work/cloned" 2>/dev/null
cmp -s "$work/cloned" "$work/clone/docs/compliance/NUMBERS.md" && pass "a clean clone reproduces the committed sheet" \
    || fail "a clean clone reproduces the committed sheet: $(diff "$work/clone/docs/compliance/NUMBERS.md" "$work/cloned" | head -5)"
numbers --check >/dev/null 2>&1 && pass "the committed sheet checks" || fail "the committed sheet checks: $(numbers --check 2>&1)"
grep -q '| Tests passed, `just test` | not run at this commit |' "$repo/docs/compliance/NUMBERS.md" \
    && pass "with no gate record, a run's figure is \"not run at this commit\"" || fail "with no gate record, a run's figure is \"not run at this commit\""
grep -q 'Test functions | \*\*2\*\* |.*A count of functions written, not of tests run' "$repo/docs/compliance/NUMBERS.md" \
    && pass "test functions are labelled as functions, not tests run" || fail "test functions are labelled as functions, not tests run"

printf '#[test]\nfn t3() {}\n' >> "$repo/b/src/lib.rs"
if out="$(numbers --check 2>&1)"; then
    fail "a sheet the tree has moved past fails the check"
else
    grep -q 'not what the tree gives' <<<"$out" && pass "a sheet the tree has moved past fails the check" || fail "it says why: $out"
fi
g checkout -q -- .

# --- drift fails (AC-RF-CA-2b) -------------------------------------------------------------------
sed -i 's/A 2-crate workspace/A 3-crate workspace/' "$repo/README.md"
if out="$(numbers --check 2>&1)"; then
    fail "a document quoting a different crate count fails"
else
    grep -q 'README.md:3: "3-crate" — the sheet counts 2 workspace crates' <<<"$out" && pass "a document quoting a different crate count fails, naming file and line" \
        || fail "the file and line are named: $out"
fi
g checkout -q -- .

check_line() { # check_line NAME FILE LINE EXPECT(pass|fail)
    printf '%s\n' "$3" >> "$repo/$2"
    if numbers --check >/dev/null 2>&1; then got=pass; else got=fail; fi
    [ "$got" = "$4" ] && pass "$1" || fail "$1: $(numbers --check 2>&1 | tail -3)"
    g checkout -q -- .
}
check_line "a line count that is neither raw nor code fails" CHANGELOG.md "About 30,000 lines of Rust." fail
check_line "the code line count passes" CHANGELOG.md "7 lines of Rust, excluding blanks and comments." pass
check_line "a test function count that differs fails" GETTING_STARTED.md "It has 5 test functions." fail
check_line "a criteria count that differs fails" docs/releases/v9.md "All 3 acceptance criteria." fail
check_line "a pass ratio with no recorded run fails" docs/releases/v9.md "2 / 2 acceptance criteria pass." fail
check_line "a tests-passed figure with no recorded run fails" GETTING_STARTED.md "870 tests passed." fail
check_line "a historical figure that names its commit passes" CHANGELOG.md 'At `60091a9`: 72 / 72 pass and 248 tests passed, on 56 crates.' pass
check_line "a figure of a tree with no public commit passes with the marker" CHANGELOG.md "The first audit saw 56 crates. <!-- numbers:historical -->" pass
check_line "the launch deck source is swept" docs/launch/pipeline/build-deck.js "// 44 crates" fail
check_line "the remedial plan is swept in its executive summary" docs/dev/fixes/remedial-fixes.md "## 1. Again
Now 9 crates." fail
grep -q '56 crates' "$repo/docs/dev/fixes/remedial-fixes.md" && numbers --check >/dev/null 2>&1 \
    && pass "and not in the items after it" || fail "and not in the items after it"
printf '<!-- gate-record:begin -->\n| Tests | 870 tests passed |\n<!-- gate-record:end -->\n' >> "$repo/README.md"
numbers --check >/dev/null 2>&1 && pass "a gate-record block is left to gate-record.sh" || fail "a gate-record block is left to gate-record.sh"
g checkout -q -- .
g mv CHANGELOG.md HISTORY.md
numbers --check >/dev/null 2>&1 && fail "a swept document that has gone fails, rather than going unchecked" \
    || pass "a swept document that has gone fails, rather than going unchecked"
g reset -q --hard

# --- a run's figures come from the gate record (AC-RF-CA-2c) ------------------------------------------
gated="$(g rev-parse HEAD)"
run="$work/run"
mkdir -p "$run"
printf '\n============\nAC-X-1 — one\n    test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n  >>> AC-X-1/2: PASS\n\n>>> FULL SUITE PASSED, AIR-GAPPED (--network none).\n' > "$run/acceptance.log"
printf 'cargo test -p a\ntest result: ok. 123 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n' > "$run/test.log"
for part in acceptance test; do
    printf 'part=%s\ncommit=%s\ntree=clean\nimage=t\nimage_id=\nbase_image=b\ndockerfile_sha256=d\nrustc=rustc 1.82.0\ncargo=cargo 1.82.0\nnetwork=none\nstarted=2026-10-14T00:00:00Z\nfinished=2026-10-14T00:01:00Z\nseconds=60\nexit=0\nplatform=local\nrun_url=\n' \
        "$part" "$gated" > "$run/$part.meta"
done
printf 'x\n<!-- gate-record:begin -->\n<!-- gate-record:end -->\n' >> "$repo/README.md"
g commit -q -am "status block"
gated="$(g rev-parse HEAD)"
sed -i "s/^commit=.*/commit=$gated/" "$run/acceptance.meta" "$run/test.meta"
GATE_RECORD_ROOT="$repo" bash "$here/../gate-record.sh" --from "$run" >/dev/null 2>&1 || fail "set-up: the gate record is written"
numbers --write >/dev/null 2>&1
if grep -q '| Tests passed, `just test` | \*\*123\*\* |' "$repo/docs/compliance/NUMBERS.md" &&
   grep -q "gate was run at commit \[\`$gated\`\]" "$repo/docs/compliance/NUMBERS.md" &&
   grep -q '| Acceptance criteria passed | \*\*2 / 2\*\* |' "$repo/docs/compliance/NUMBERS.md"; then
    pass "with a record that covers the tree, the run's figures are on the sheet with its commit"
else
    fail "with a record that covers the tree, the run's figures are on the sheet: $(sed -n '/Shown by a run/,$p' "$repo/docs/compliance/NUMBERS.md")"
fi
echo "123 tests passed in the recorded run." >> "$repo/CHANGELOG.md"
g add -A && g commit -q -m "record and sheet"
numbers --check >/dev/null 2>&1 && pass "a document quoting the recorded run's figure passes" || fail "a document quoting the recorded run's figure passes: $(numbers --check 2>&1)"

echo "fn changed() {}" >> "$repo/b/src/lib.rs"
numbers --write >/dev/null 2>&1
if grep -q '| Tests passed, `just test` | not run at this commit | at `'"$gated"'`: 123 |' "$repo/docs/compliance/NUMBERS.md"; then
    pass "once the gated sources change, the run's figures are \"not run at this commit\", the last run named with its commit"
else
    fail "once the gated sources change, the run's figures are \"not run at this commit\": $(sed -n '/Shown by a run/,$p' "$repo/docs/compliance/NUMBERS.md")"
fi
if out="$(numbers --check 2>&1)"; then
    fail "and a document still quoting it as this tree's fails"
else
    grep -q 'CHANGELOG.md:2: "123 tests passed"' <<<"$out" && pass "and a document still quoting it as this tree's fails" || fail "the document is named: $out"
fi

# --- this repository ---------------------------------------------------------------------------------
if out="$(bash "$script" --check 2>&1)"; then
    pass "this repository's sheet and documents check"
else
    fail "this repository's sheet and documents check: $out"
fi

if [ "$failures" -gt 0 ]; then
    echo "numbers tests: $failures failure(s)"
    exit 1
fi
echo "numbers tests: all passed"
