#!/usr/bin/env bash
# The numbers sheet: every headline figure, counted from the tree. [AC-RF-CA-2a]
# [AC-RF-CA-2b] [AC-RF-CA-2c] [AC-RF-CA-2d]
#
#   bash tools/ci/numbers.sh             print the sheet
#   bash tools/ci/numbers.sh --write     write it to docs/compliance/NUMBERS.md
#   bash tools/ci/numbers.sh --check     the committed sheet is the tree's, and no swept
#                                        document quotes a figure that differs from it
#   bash tools/ci/numbers.sh --figures   the figures as key=value lines
#
# A figure quoted in a README goes stale the day after it is typed. So nothing is typed:
# the sheet is generated, each figure beside the rule that counts it and a command that
# reproduces it, and --check holds the documents to the sheet.
#
# Two kinds of figure are kept apart. What is IN the tree — crates, lines, test
# functions, criteria defined — is counted from the files as they stand. What a RUN
# showed — tests passed, criteria passed — is read from the gate record
# (tools/ci/gate-record.sh) and never from anywhere else. When the tree is not the
# commit that record gated, the sheet prints "not run at this commit" and names the
# last recorded run beside it, with its commit: an old result is not this tree's result.
#
# --check sweeps the documents in SWEPT for crate, Rust line, test and criteria counts.
# A figure passes when it equals the sheet's. A figure that is deliberately of another
# tree passes when its line names that tree's commit (a 7-to-40 digit hex id in
# backticks), or — for a figure of a tree with no public commit — carries the comment
# <!-- numbers:historical -->. Text between the gate-record markers is generated from the
# record, names its commit, and is checked by gate-record.sh; it is skipped here.
set -uo pipefail

ROOT="${NUMBERS_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
SHEET="docs/compliance/NUMBERS.md"
SPEC="docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md"
tools="$(cd "$(dirname "$0")" && pwd)"
# path, or path|FROM|TO to sweep only from the first line matching FROM up to the first
# after it matching TO. The completion plan is swept for its summary only: its
# items quote superseded figures as the evidence for each fix.
SWEPT=(
    "README.md"
    "GETTING_STARTED.md"
    "CHANGELOG.md"
    "docs/compliance/GA_READINESS.md"
    "docs/releases/*.md"
    "docs/launch/pipeline/build-deck.js"
    "docs/dev/markdown/SAD_09_Completion_Plan.md|^## 1[.] |^## 2[.] "
)

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
die() { echo "numbers: $*" >&2; exit 1; }
gitr() { git -C "$ROOT" "$@"; }
# 97041 -> 97,041
group() { sed -e ':a' -e 's/^\([0-9]\{1,\}\)\([0-9]\{3\}\)/\1,\2/' -e 'ta' <<<"$1"; }
field() { sed -n "s/^$2=//p" "$1" | head -n 1; }

# --- counting --------------------------------------------------------------------------
# Each count is of tracked files, so a build's leftovers and an editor's scratch files
# are not in it, and of the working tree, so the sheet describes what is checked out.

count() {
    # Members of [workspace] in the root Cargo.toml.
    crates="$(tr -d '\r' < "$ROOT/Cargo.toml" |
        awk '/^members *= *\[/ { on = 1; next } on && /^\]/ { exit } on && /^ *"/ { n++ } END { print n + 0 }')"

    # Tracked *.rs outside vendor/: files, lines, and lines that are neither blank nor a
    # //-comment; #[test] attributes, and #[ignore] attributes.
    read -r rust_files rust_lines rust_code test_functions test_ignored < <(
        cd "$ROOT" && git ls-files -z -- '*.rs' ':(exclude)vendor' | xargs -0 awk '
            FNR == 1 { files++ }
            {
                lines++
                line = $0; sub(/\r$/, "", line); sub(/^[ \t]+/, "", line)
                if (line != "" && line !~ /^\/\//) code++
                if (line ~ /^#\[test\]/) tests++
                if (line ~ /^#\[ignore/) ignored++
            }
            END { print files + 0, lines + 0, code + 0, tests + 0, ignored + 0 }'
    )

    # Criteria in Part C of the acceptance spec.
    criteria_defined="$(tr -d '\r' < "$ROOT/$SPEC" |
        awk '/^## PART C/ { on = 1; next } /^## PART D/ { on = 0 } on' |
        grep -oE 'AC-[A-Z]+-[0-9]+' | sort -u | wc -l | tr -d ' ')"

    # What ships: the release matrix derived from the workspace, and the JS workspace.
    (cd "$ROOT" && bash tools/ci/release-artefacts.sh --rows) > "$work/rows" 2>/dev/null ||
        die "tools/ci/release-artefacts.sh --rows failed"
    binaries="$(wc -l < "$work/rows" | tr -d ' ')"
    services="$(awk '$3 == "service"' "$work/rows" | wc -l | tr -d ' ')"
    images="$(awk '$4 != "-" { print $4 }' "$work/rows" | sort -u | wc -l | tr -d ' ')"
    (cd "$ROOT" && bash tools/ci/js-artefacts.sh --rows) > "$work/js" 2>/dev/null ||
        die "tools/ci/js-artefacts.sh --rows failed"
    consoles="$(awk '$1 == "ui-bundle"' "$work/js" | wc -l | tr -d ' ')"
    npm_packages="$(awk '$1 == "npm-package"' "$work/js" | wc -l | tr -d ' ')"

    sad_documents="$(gitr ls-files -- 'docs/dev/markdown/SAD_*.md' | wc -l | tr -d ' ')"
    diagrams="$(gitr ls-files -- 'docs/dev/diagrams/*.mmd' | wc -l | tr -d ' ')"

    version="$(tr -d '\r' < "$ROOT/Cargo.toml" |
        awk '/^\[workspace\.package\]/ { on = 1; next } on && /^\[/ { exit } on && /^version *=/ { gsub(/[" ]/, ""); sub(/^version=/, ""); print; exit }')"

    # The gate record's figures, and whether this tree is the commit it gated.
    GATE_RECORD_ROOT="$ROOT" bash "$tools/gate-record.sh" --figures > "$work/gate" ||
        die "the gate record does not check; see: bash tools/ci/gate-record.sh --check"
    recorded="$(field "$work/gate" recorded)"
    gate_commit="$(field "$work/gate" commit)"
    covers="$(field "$work/gate" covers_tree)"
    for key in result criteria_pass criteria_total failing suite_tests_passed suite_tests_failed \
        suite_tests_ignored tests_passed tests_failed tests_ignored test_binaries invocations invocations_expected; do
        printf -v "g_$key" '%s' "$(field "$work/gate" "$key")"
    done
}

# --- the sheet -------------------------------------------------------------------------

sheet() {
    local run last
    cat <<EOF
# Numbers

<!-- Generated by tools/ci/numbers.sh --write. Do not edit: CI makes this file again
     and compares, and holds the documents that quote a figure to it. -->

Every headline figure about this repository, each with the rule that counts it and a command that reproduces it. This sheet is the single source for these figures: a document that quotes one quotes it from here, and \`bash tools/ci/numbers.sh --check\` fails if it differs.

The figures are of the tree this file is committed with, version \`$version\`. A sheet cannot contain the id of the commit that contains it, so it does not name one; check it against any checkout with \`bash tools/ci/numbers.sh --check\`.

## In the tree

Counted from tracked files. \`vendor/\` is other people's code and is not counted.

| Figure | Value | Counting rule | Reproduce |
|---|---|---|---|
| Workspace crates | **$(group "$crates")** | members of \`[workspace]\` in the root \`Cargo.toml\` | \`cargo metadata --no-deps --offline --format-version 1\` lists the same packages |
| Rust source files | **$(group "$rust_files")** | tracked \`*.rs\` outside \`vendor/\` | \`git ls-files '*.rs' ':(exclude)vendor' \\| wc -l\` |
| Rust lines, raw | **$(group "$rust_lines")** | every line of those files | \`git ls-files -z '*.rs' ':(exclude)vendor' \\| xargs -0 cat \\| wc -l\` |
| Rust lines, code | **$(group "$rust_code")** | lines that are neither blank nor begin with \`//\` (so doc comments are excluded and block comments are not) | \`git ls-files -z '*.rs' ':(exclude)vendor' \\| xargs -0 cat \\| grep -vcE '^\\s*(//.*)?\\r?\$'\` |
| Test functions | **$(group "$test_functions")** | \`#[test]\` attributes in those files. A count of functions written, not of tests run | \`git ls-files -z '*.rs' ':(exclude)vendor' \\| xargs -0 cat \\| grep -cE '^\\s*#\\[test\\]'\` |
| …of which ignored | **$(group "$test_ignored")** | \`#[ignore]\` attributes in those files | \`git ls-files -z '*.rs' ':(exclude)vendor' \\| xargs -0 cat \\| grep -cE '^\\s*#\\[ignore'\` |
| Acceptance criteria defined | **$(group "$criteria_defined")** | distinct \`AC-*\` ids in Part C of \`$SPEC\` | \`bash tools/ci/numbers.sh --figures \\| grep criteria_defined\` |
| Shipped binaries | **$(group "$binaries")** | binaries the workspace builds, each of which the release gates | \`bash tools/ci/release-artefacts.sh --rows \\| wc -l\` |
| …of which services | **$(group "$services")** | those \`supply-chain/release-artefacts.txt\` marks \`service\`; the rest ship as tools | \`bash tools/ci/release-artefacts.sh --rows \\| awk '\$3 == "service"' \\| wc -l\` |
| Service images | **$(group "$images")** | verify-before-run images those binaries are packed into | \`bash tools/ci/release-artefacts.sh --images \\| wc -l\` |
| Console UI bundles | **$(group "$consoles")** | pnpm workspace packages that build a UI bundle | \`bash tools/ci/js-artefacts.sh --rows \\| grep -c '^ui-bundle'\` |
| npm packages | **$(group "$npm_packages")** | pnpm workspace packages published as a package | \`bash tools/ci/js-artefacts.sh --rows \\| grep -c '^npm-package'\` |
| SAD documents | **$(group "$sad_documents")** | \`docs/dev/markdown/SAD_*.md\` | \`git ls-files 'docs/dev/markdown/SAD_*.md' \\| wc -l\` |
| Diagrams | **$(group "$diagrams")** | Mermaid sources in \`docs/dev/diagrams/\`; each is also kept as draw.io and PNG | \`git ls-files 'docs/dev/diagrams/*.mmd' \\| wc -l\` |

## Shown by a run

Read from the gate record and from nowhere else. A test count here is of tests that ran and passed in that run. It is a different measure from the count of test functions above, and the two are never quoted as one figure.

EOF
    if [ "$recorded" != yes ]; then
        cat <<EOF
No gate run is recorded.

| Figure | Value |
|---|---|
| Acceptance criteria passed | not run at this commit |
| Tests passed, \`just test\` | not run at this commit |
| Tests passed, acceptance suite | not run at this commit |
EOF
        return
    fi
    last="at \`$gate_commit\`"
    if [ "$covers" = yes ]; then
        cat <<EOF
The gate was run at commit [\`$gate_commit\`](gate-runs/$gate_commit.md). This tree is that commit, apart from the record itself and the prose generated from it, so its results are this tree's.

| Figure | Value | Counting rule |
|---|---|---|
| Gate result | **$g_result** | the acceptance suite and \`just test\` both exited 0 and no criterion failed |
| Acceptance criteria passed | **$g_criteria_pass / $g_criteria_total** | criteria of Part C with a PASS verdict line in the acceptance log |
EOF
        [ -z "$g_failing" ] || echo "| Failing criteria | **${g_failing// /, }** | criteria with a FAIL verdict line |"
        cat <<EOF
| Tests passed, \`just test\` | **$(group "$g_tests_passed")** | sum of the \`test result:\` lines of the \`just test\` log; $(group "$g_tests_failed") failed, $(group "$g_tests_ignored") ignored, in $g_test_binaries test binaries across $g_invocations of $g_invocations_expected invocations |
| Tests passed, acceptance suite | **$(group "$g_suite_tests_passed")** | sum of the \`test result:\` lines between each step's header and its verdict; $(group "$g_suite_tests_failed") failed, $(group "$g_suite_tests_ignored") ignored. The suite runs selected tests, some more than once, so this is not comparable with the figure above |

Reproduce: \`bash tools/ci/gate-record.sh --check\` makes these from the logs under [\`gate-runs/$gate_commit/\`](gate-runs/$gate_commit/); [the record](gate-runs/$gate_commit.md) says how to make the run again.
EOF
    else
        cat <<EOF
The gate has not been run at this commit. Files it covers have changed since the last recorded run, so that run's results are not this tree's and are shown only as what they are: the results of another commit.

| Figure | Value | Last recorded run |
|---|---|---|
| Acceptance criteria passed | not run at this commit | $last: $g_criteria_pass / $g_criteria_total |
| Tests passed, \`just test\` | not run at this commit | $last: $(group "$g_tests_passed") |
| Tests passed, acceptance suite | not run at this commit | $last: $(group "$g_suite_tests_passed") |

The record of that run is [\`gate-runs/$gate_commit.md\`](gate-runs/$gate_commit.md).
EOF
    fi
}

figures() {
    cat <<EOF
version=$version
crates=$crates
rust_files=$rust_files
rust_lines=$rust_lines
rust_code_lines=$rust_code
test_functions=$test_functions
test_functions_ignored=$test_ignored
criteria_defined=$criteria_defined
binaries=$binaries
services=$services
images=$images
consoles=$consoles
npm_packages=$npm_packages
sad_documents=$sad_documents
diagrams=$diagrams
gate_recorded=$recorded
gate_commit=$gate_commit
gate_covers_tree=${covers:-no}
EOF
    if [ "$recorded" = yes ] && [ "$covers" = yes ]; then
        cat <<EOF
criteria_passed=$g_criteria_pass
tests_passed=$g_tests_passed
suite_tests_passed=$g_suite_tests_passed
EOF
    fi
}

# --- the sweep -------------------------------------------------------------------------

# sweep_file PATH [FROM TO] — print "path:line: message" for each figure that differs.
sweep_file() {
    local path="$1" from="${2:-}" to="${3:-}" run=no
    [ "$recorded" = yes ] && [ "$covers" = yes ] && run=yes
    tr -d '\r' < "$ROOT/$path" | awk -v path="$path" -v from="$from" -v to="$to" -v run="$run" \
        -v crates="$crates" -v lines="$rust_lines" -v code="$rust_code" -v fns="$test_functions" \
        -v defined="$criteria_defined" -v cpass="${g_criteria_pass:-}" \
        -v tpass="${g_tests_passed:-}" -v spass="${g_suite_tests_passed:-}" '
        function num(s) { gsub(/[^0-9]/, "", s); return s + 0 }
        function say(text, want) { printf "%s:%d: \"%s\" — %s\n", path, NR, text, want; bad = 1 }
        # each(REGEX, KIND): every match of REGEX on the line, judged as KIND.
        function each(re, kind,   rest, m, n, a, b) {
            rest = $0
            while (match(rest, re)) {
                m = substr(rest, RSTART, RLENGTH)
                rest = substr(rest, RSTART + RLENGTH)
                if (kind == "crates") {
                    if (num(m) != crates) say(m, "the sheet counts " crates " workspace crates")
                } else if (kind == "lines") {
                    n = m; sub(/ .*/, "", n); n = num(n)
                    if (n != lines && n != code) say(m, "the sheet counts " lines " raw and " code " code lines of Rust")
                } else if (kind == "fns") {
                    if (num(m) != fns) say(m, "the sheet counts " fns " test functions")
                } else if (kind == "passed") {
                    n = num(m)
                    if (n < 100) continue      # a headline figure, not "3 passed" in a log excerpt
                    if (run != "yes") say(m, "no gate run is recorded for this tree; the sheet says \"not run at this commit\"")
                    else if (n != tpass && n != spass) say(m, "the recorded run passed " tpass " tests under just test and " spass " in the acceptance suite")
                } else if (kind == "tests") {
                    n = num(m)
                    if (n < 100) continue
                    if (n == fns) continue
                    if (run == "yes" && (n == tpass || n == spass)) continue
                    say(m, "the sheet counts " fns " test functions" (run == "yes" ? ", and the recorded run passed " tpass : ", and records no run for this tree"))
                } else if (kind == "ratio") {
                    a = m; sub(/ *\/.*/, "", a); a = num(a)
                    b = m; sub(/^[^\/]*\/ */, "", b); b = num(b)
                    if (b != defined) say(m, "the spec defines " defined " acceptance criteria")
                    else if (run != "yes") say(m, "no gate run is recorded for this tree; the sheet says \"not run at this commit\"")
                    else if (a != cpass) say(m, "the recorded run passed " cpass " of " defined)
                } else if (kind == "criteria") {
                    if (num(m) != defined) say(m, "the spec defines " defined " acceptance criteria")
                }
            }
        }
        BEGIN { on = (from == "") }
        from != "" && !on && $0 ~ from { on = 1 }
        from != "" && on && $0 ~ to && $0 !~ from { on = 0 }
        !on { next }
        /<!-- gate-record:begin -->/ { skip = 1 }
        /<!-- gate-record:end -->/ { skip = 0; next }
        skip { next }
        /`[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*`/ { next }
        /<!-- numbers:historical -->/ { next }
        {
            each("[0-9][0-9,]*[- ](workspace[- ]|Rust[- ])?crates?", "crates")
            each("[0-9][0-9,]* (raw |source )?(lines of Rust|Rust lines|Rust source lines)", "lines")
            each("[0-9][0-9,]* (`#\\[test\\]` |test )functions", "fns")
            each("[0-9][0-9,]* ?/ ?[0-9][0-9,]* (acceptance criteria|criteria|pass)", "ratio")
            each("[0-9][0-9,]* acceptance criteria", "criteria")
            each("[0-9][0-9,]* (tests? )?passed", "passed")
            each("[0-9][0-9,]* tests", "tests")
        }
        END { exit bad }'
}

sweep() {
    local entry glob from to path bad=0 any
    for entry in "${SWEPT[@]}"; do
        glob="${entry%%|*}"
        from="" to=""
        if [ "$glob" != "$entry" ]; then
            from="${entry#*|}"; to="${from#*|}"; from="${from%%|*}"
        fi
        any=0
        for path in $(cd "$ROOT" && ls -d $glob 2>/dev/null); do
            any=1
            sweep_file "$path" "$from" "$to" || bad=1
        done
        # A document that was swept and has gone is a renamed document nobody is checking.
        [ "$any" = 1 ] || { echo "numbers: nothing to sweep at $glob; update SWEPT in tools/ci/numbers.sh"; bad=1; }
    done
    return "$bad"
}

# --- modes -----------------------------------------------------------------------------

mode="${1:-print}"
[ "$#" -le 1 ] || die "one mode at a time"
case "$mode" in
    print | --print) count; sheet ;;
    --figures) count; figures ;;
    --write)
        count
        sheet > "$ROOT/$SHEET"
        echo "numbers: wrote $SHEET"
        ;;
    --check)
        count
        sheet > "$work/sheet"
        bad=0
        if [ ! -f "$ROOT/$SHEET" ]; then
            echo "numbers: $SHEET does not exist; write it: bash tools/ci/numbers.sh --write" >&2
            bad=1
        elif ! tr -d '\r' < "$ROOT/$SHEET" | cmp -s - "$work/sheet"; then
            echo "numbers: $SHEET is not what the tree gives:" >&2
            diff <(tr -d '\r' < "$ROOT/$SHEET") "$work/sheet" | grep '^[<>]' | head -n 12 | cut -c1-160 >&2
            echo "  Write it again: bash tools/ci/numbers.sh --write" >&2
            bad=1
        fi
        if ! sweep > "$work/sweep"; then
            echo "numbers: figures that differ from the sheet:" >&2
            sed 's/^/  /' "$work/sweep" >&2
            echo "  Quote the sheet's figure; or, for a figure of another tree, name that tree's commit on the line." >&2
            bad=1
        fi
        [ "$bad" = 0 ] || exit 1
        echo "numbers: $SHEET is the tree's, and every swept document agrees with it."
        ;;
    -h | --help) sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//' ;;
    *) die "unknown mode '$mode'" ;;
esac
