#!/usr/bin/env bash
# The gate record: one run of the acceptance gate, written down so it can be checked.
# [AC-RF-CA-5a] [AC-RF-CA-5b] [AC-RF-CA-5c] [AC-RF-CA-5d]
#
#   bash tools/ci/gate-record.sh [--from DIR]     record the run whose logs are in DIR
#   bash tools/ci/gate-record.sh --check          the committed record still holds
#   bash tools/ci/gate-record.sh --check --release   ... and covers the commit being released
#   bash tools/ci/gate-record.sh --compare DIR    a fresh run agrees with the record
#   bash tools/ci/gate-record.sh --status         rewrite the status tables from the record
#   bash tools/ci/gate-record.sh --print DIR      print the record for DIR, write nothing
#   bash tools/ci/gate-record.sh --figures        the record's figures as key=value lines
#
# DIR (default target/gate) is what tools/ci/gate-run.sh leaves: acceptance.log,
# test.log, and a .meta beside each. A record is
#
#   docs/compliance/gate-runs/<commit>.md         the record
#   docs/compliance/gate-runs/<commit>/           the logs and metadata it was made from
#   docs/compliance/gate-runs/CURRENT             the commit of the record now in force
#
# The record is a function of those logs, that metadata, and two files at the gated
# commit: the acceptance spec, which names the criteria, and the justfile, which names
# the test invocations. Nothing in it is typed by hand, so --check can make it again
# from the committed logs and require the same bytes.
#
# A record is REFUSED, and nothing is written, when
#   - a criterion the spec defines has no verdict in the log, or the log gives a
#     verdict for a criterion the spec does not define;
#   - the log is cut short, or its verdicts disagree with the exit status;
#   - the tree was not clean, the two parts are of different commits or images, or the
#     commit is not in this repository's history.
# A FAILING run is not refused. It is recorded as failing, and the status tables name
# the criteria that failed: a failure is fixed or reported, never left out.
#
# The record names the commit that was gated, so it cannot be part of that commit. It
# is committed after it, and --check --release requires that nothing but the record and
# the prose generated from it separates the two.
set -uo pipefail

ROOT="${GATE_RECORD_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
SPEC="docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md"
RECORDS="docs/compliance/gate-runs"
BEGIN_MARK='<!-- gate-record:begin -->'
END_MARK='<!-- gate-record:end -->'
# Files that carry a status table generated from the record: path, then the prefix that
# takes a link in that file to the repository root.
STATUS_FILES=("README.md|" "docs/compliance/GA_READINESS.md|../../")
# What may differ between the gated commit and the commit released: the record, the
# prose and ledgers generated from it or citing it, and documents the gate never reads
# (the design system and marketing site, the launch material, the guides). Nothing the
# gate builds, runs or reads: the acceptance spec under docs/dev/ stays gated.
UNGATED=("$RECORDS/" "README.md" "CHANGELOG.md" "docs/compliance/GA_READINESS.md"
    "docs/compliance/NUMBERS.md" "docs/compliance/CLAIMS.md" "docs/compliance/claims.toml"
    "docs/releases/" "docs/design/" "docs/launch/" "docs/guides/")

ESC=$'\033'
TAB=$'\t'
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

die() { echo "gate-record: $*" >&2; exit 1; }
refuse() {
    echo "gate-record: REFUSED — $1" >&2
    shift
    for line in "$@"; do echo "  $line" >&2; done
    exit 1
}
sha256_of() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }
bytes_of() { wc -c < "$1" | tr -d ' '; }
meta() { sed -n "s/^$2=//p" "$1" | head -n 1; }
# Colour codes and carriage returns are how a terminal was addressed, not what was said.
clean() { sed -e "s/${ESC}\[[0-9;]*[A-Za-z]//g" -e 's/\r$//' "$1"; }
gitr() { git -C "$ROOT" "$@"; }

# --- parsing ---------------------------------------------------------------------------

# The criteria, from Part C of the acceptance spec as it stood at COMMIT.
spec_ids() {
    gitr show "$1:$SPEC" 2>/dev/null |
        awk '/^## PART C/ { on = 1; next } /^## PART D/ { on = 0 } on' |
        grep -oE 'AC-[A-Z]+-[0-9]+' | sort -t- -k2,2 -k3,3n -u
}

# The `cargo test` lines of the justfile's `test` recipe at COMMIT.
expected_invocations() {
    gitr show "$1:justfile" 2>/dev/null |
        awk '/^test:/ { on = 1; next } on && /^[^ \t#]/ { on = 0 } on && /^[ \t]+cargo test / { sub(/^[ \t]+/, ""); print }'
}

# The acceptance log. Each step is a ruled header `<id> — <description>`, the test
# result lines of what it ran, and a verdict `  >>> <id>: PASS|FAIL`. An id may name
# several criteria that one step verifies together: AC-B-1/4 is AC-B-1 and AC-B-4.
#   V <criterion> <result> <verdict id> <description>
#   O <other step> <result>
#   T <test binaries> <passed> <failed> <ignored>
#   D <definitions of done met> <not met>
#   S <PASSED|FAILED|>            the closing line of the suite
parse_acceptance() {
    clean "$1" | awk -v OFS="$TAB" '
        function count(line, key,   a, n, i) {
            n = split(line, a, /[ ;]+/)
            for (i = 2; i <= n; i++) if (a[i] == key) return a[i - 1] + 0
            return 0
        }
        /^=+$/ { header = 1; next }
        header {
            header = 0; instep = 1; desc = ""
            p = index($0, " — ")
            if (p) desc = substr($0, p + length(" — "))
            next
        }
        # Only up to the verdict: a failing step prints the tail of its output after
        # it, and that tail repeats result lines already counted.
        instep && /^    test result: (ok|FAILED)\. [0-9]+ passed; / {
            bins++; passed += count($0, "passed"); failed += count($0, "failed"); ignored += count($0, "ignored")
            next
        }
        /^  >>> [^ ]+: (PASS|FAIL)/ {
            instep = 0
            id = $2; sub(/:$/, "", id); res = $3
            if (id ~ /^AC-[A-Z]+-[0-9]+(\/[0-9]+)*$/) {
                n = split(id, part, "/")
                stem = part[1]; sub(/[0-9]+$/, "", stem)
                print "V", part[1], res, id, desc
                for (i = 2; i <= n; i++) print "V", stem part[i], res, id, desc
            } else {
                print "O", id, res
            }
            next
        }
        /DEFINITION OF DONE: MET/ { met++ }
        /DEFINITION OF DONE: NOT MET/ { notmet++ }
        /^>>> FULL SUITE PASSED/ { suite = "PASSED" }
        /^>>> FULL SUITE FAILED/ { suite = "FAILED" }
        END {
            print "T", bins + 0, passed + 0, failed + 0, ignored + 0
            print "D", met + 0, notmet + 0
            print "S", suite
        }'
}

# The `just test` log. `just` echoes each recipe line before running it, so every
# `cargo test ...` line opens an invocation, and each test binary or doc-test run in
# it closes with one `test result:` line.
#   I <n> <binaries> <passed> <failed> <ignored> <backend> <command>
#   X <result lines before the first command>
parse_test() {
    clean "$1" | awk -v OFS="$TAB" '
        function count(line, key,   a, n, i) {
            n = split(line, a, /[ ;]+/)
            for (i = 2; i <= n; i++) if (a[i] == key) return a[i - 1] + 0
            return 0
        }
        /^cargo test / { n++; cmd[n] = $0; next }
        /^test result: (ok|FAILED)\. [0-9]+ passed; / {
            if (n == 0) { orphan++; next }
            bins[n]++; p[n] += count($0, "passed"); f[n] += count($0, "failed"); g[n] += count($0, "ignored")
        }
        END {
            for (i = 1; i <= n; i++) {
                backend = "none selected"
                if (cmd[i] ~ /--features[ =]test-backend/) backend = "test-backend"
                if (cmd[i] ~ /--features[ =]backend-fips/) backend = "backend-fips"
                print "I", i, bins[i] + 0, p[i] + 0, f[i] + 0, g[i] + 0, backend, cmd[i]
            }
            print "X", orphan + 0
        }'
}

# compute DIR — parse the run in DIR into $work/<name> files and the variables below,
# and refuse it if it cannot be a record. Sets:
#   commit started image image_id base_image dockerfile_sha rustc cargo run_url platform
#   acc_exit acc_seconds test_exit test_seconds
#   crit_total crit_pass failing        acc_bins acc_passed acc_failed acc_ignored
#   dod_met dod_total suite             inv_ran inv_expected inv_empty
#   t_bins t_passed t_failed t_ignored  backends   result
compute() {
    local dir="$1" f
    for f in acceptance.log acceptance.meta test.log test.meta; do
        [ -s "$dir/$f" ] || refuse "$dir/$f is missing or empty" \
            "A record needs both parts of the gate: bash tools/ci/gate-run.sh"
    done
    local am="$dir/acceptance.meta" tm="$dir/test.meta"

    commit="$(meta "$am" commit)"
    [ -n "$commit" ] || refuse "$am names no commit"
    [ "$commit" = "$(meta "$tm" commit)" ] || refuse "the two parts are of different commits" \
        "acceptance: $commit" "test:       $(meta "$tm" commit)"
    [ "$(meta "$am" tree)" = clean ] && [ "$(meta "$tm" tree)" = clean ] ||
        refuse "the tree was not clean when the gate ran" \
            "A record names a commit, so the run must be of exactly that commit."
    for f in base_image dockerfile_sha256 rustc; do
        [ -n "$(meta "$am" "$f")" ] || refuse "$am does not record $f"
        [ "$(meta "$am" "$f")" = "$(meta "$tm" "$f")" ] ||
            refuse "the two parts ran in different toolchains ($f differs)"
    done
    [ "$(meta "$am" network)" = none ] && [ "$(meta "$tm" network)" = none ] ||
        refuse "the run was not made under --network none"
    gitr cat-file -e "$commit^{commit}" 2>/dev/null ||
        refuse "commit $commit is not in this repository" \
            "A pull request's merge commit is in no history; record a run of a pushed commit."

    started="$(meta "$am" started)"
    image="$(meta "$am" image)"
    image_id="$(meta "$am" image_id)"
    base_image="$(meta "$am" base_image)"
    dockerfile_sha="$(meta "$am" dockerfile_sha256)"
    rustc="$(meta "$am" rustc)"
    cargo="$(meta "$am" cargo)"
    run_url="$(meta "$am" run_url)"
    platform="$(meta "$am" platform)"
    acc_exit="$(meta "$am" exit)"
    acc_seconds="$(meta "$am" seconds)"
    test_exit="$(meta "$tm" exit)"
    test_seconds="$(meta "$tm" seconds)"

    # -- criteria ----------------------------------------------------------------------
    spec_ids "$commit" > "$work/ids"
    crit_total="$(wc -l < "$work/ids" | tr -d ' ')"
    [ "$crit_total" -gt 0 ] || refuse "no criteria found in Part C of $SPEC at $commit"

    parse_acceptance "$dir/acceptance.log" > "$work/acc"
    suite="$(awk -F"$TAB" '$1 == "S" { print $2 }' "$work/acc")"
    [ -n "$suite" ] || refuse "the acceptance log has no closing '>>> FULL SUITE ...' line" \
        "The log is cut short, or is not a log of deploy/acceptance/run-airgapped.sh."

    # One row per criterion: FAIL if any verdict that covers it failed.
    awk -F"$TAB" -v OFS="$TAB" '
        NR == FNR { want[$1] = 1; order[++n] = $1; next }
        $1 == "V" {
            if (!($2 in want)) { unknown[$2] = 1; next }
            if (!($2 in res) || $3 == "FAIL") res[$2] = $3
            if (!($2 in src)) src[$2] = $4; else if (index(src[$2], $4) == 0) src[$2] = src[$2] ", " $4
            if (!($2 in desc)) desc[$2] = $5
        }
        END {
            for (i = 1; i <= n; i++) {
                id = order[i]
                if (id in res) print "V", id, res[id], src[id], desc[id]; else print "M", id
            }
            for (id in unknown) print "U", id
        }' "$work/ids" "$work/acc" > "$work/criteria"

    local missing unknown
    missing="$(awk -F"$TAB" '$1 == "M" { print $2 }' "$work/criteria" | tr '\n' ' ')"
    unknown="$(awk -F"$TAB" '$1 == "U" { print $2 }' "$work/criteria" | sort | tr '\n' ' ')"
    [ -z "$missing" ] || refuse "the log has no verdict for: ${missing% }" \
        "The spec defines $crit_total criteria; every one must have a '>>> <id>: PASS|FAIL' line." \
        "A criterion with no verdict was not shown to pass. Give it a step in deploy/acceptance/."
    [ -z "$unknown" ] || refuse "the log gives a verdict for a criterion the spec does not define: ${unknown% }"

    crit_pass="$(awk -F"$TAB" '$1 == "V" && $3 == "PASS"' "$work/criteria" | wc -l | tr -d ' ')"
    failing="$(awk -F"$TAB" '$1 == "V" && $3 != "PASS" { print $2 }' "$work/criteria" | tr '\n' ' ')"
    failing="${failing% }"
    read -r acc_bins acc_passed acc_failed acc_ignored < <(awk -F"$TAB" '$1 == "T" { print $2, $3, $4, $5 }' "$work/acc")
    read -r dod_met dod_notmet < <(awk -F"$TAB" '$1 == "D" { print $2, $3 }' "$work/acc")
    dod_total=$((dod_met + dod_notmet))

    # The log must agree with itself and with the exit status it ran to.
    if [ "$suite" = PASSED ]; then
        [ "$acc_exit" = 0 ] && [ -z "$failing" ] ||
            refuse "the acceptance log says the suite passed, but the exit status is $acc_exit and failing criteria are '${failing:-none}'"
    else
        [ "$acc_exit" != 0 ] || refuse "the acceptance log says the suite failed, but the exit status is 0"
    fi

    # -- tests -------------------------------------------------------------------------
    parse_test "$dir/test.log" > "$work/test"
    [ "$(awk -F"$TAB" '$1 == "X" { print $2 }' "$work/test")" = 0 ] ||
        refuse "the test log has results before the first 'cargo test' line" \
            "It is not a log of 'just test', or its start is missing."
    inv_ran="$(awk -F"$TAB" '$1 == "I"' "$work/test" | wc -l | tr -d ' ')"
    [ "$inv_ran" -gt 0 ] || refuse "the test log has no 'cargo test' invocation"
    expected_invocations "$commit" > "$work/expected"
    inv_expected="$(wc -l < "$work/expected" | tr -d ' ')"
    [ "$inv_expected" -gt 0 ] || refuse "no 'cargo test' lines found in the justfile's test recipe at $commit"
    # What ran must be the recipe, in order, as far as it got.
    awk -F"$TAB" '$1 == "I" { print $8 }' "$work/test" > "$work/ran"
    head -n "$inv_ran" "$work/expected" | cmp -s - "$work/ran" ||
        refuse "the test log's invocations are not the justfile's test recipe at $commit" \
            "$(diff <(head -n "$inv_ran" "$work/expected") "$work/ran" | head -n 6)"
    inv_empty="$(awk -F"$TAB" '$1 == "I" && $3 == 0' "$work/test" | wc -l | tr -d ' ')"
    read -r t_bins t_passed t_failed t_ignored < <(awk -F"$TAB" '
        $1 == "I" { b += $3; p += $4; f += $5; g += $6 } END { print b + 0, p + 0, f + 0, g + 0 }' "$work/test")
    backends="$(awk -F"$TAB" '$1 == "I" && $7 != "none selected" && $3 > 0 { print $7 }' "$work/test" | sort -u | tr '\n' ' ')"
    backends="${backends% }"; backends="${backends// /, }"
    if [ "$test_exit" = 0 ]; then
        [ "$t_failed" = 0 ] && [ "$inv_ran" = "$inv_expected" ] && [ "$inv_empty" = 0 ] ||
            refuse "'just test' exited 0, but the log shows $t_failed failed test(s), $inv_ran of $inv_expected invocations, $inv_empty with no result"
    fi

    if [ "$acc_exit" = 0 ] && [ "$test_exit" = 0 ] && [ -z "$failing" ]; then result=PASS; else result=FAIL; fi
}

# --- rendering -------------------------------------------------------------------------

duration() { printf '%dm %02ds' "$(($1 / 60))" "$(($1 % 60))"; }

# render_record DIR — the record, on stdout. compute DIR must have run.
render_record() {
    local dir="$1" f
    cat <<EOF
# Gate run — \`$commit\`

<!-- Generated by tools/ci/gate-record.sh from the logs in $commit/. Do not edit:
     \`bash tools/ci/gate-record.sh --check\` makes this file again and compares. -->

**Result: $result.** $crit_pass of $crit_total acceptance criteria pass; \`just test\` ran $t_passed tests with $t_failed failed.
EOF
    [ -z "$failing" ] || printf '\n**Failing criteria:** %s.\n' "${failing// /, }"
    cat <<EOF

| | |
|---|---|
| Commit | \`$commit\` |
| Run started | $started (UTC) |
| Network | none — every step under \`docker run --network none\` |
| Toolchain | \`$rustc\` · \`$cargo\` |
| Base image | \`$base_image\` |
| Toolchain image | \`deploy/acceptance/Dockerfile\`, SHA-256 \`$dockerfile_sha\` |
| Built image id | \`${image_id:-not recorded}\` (\`$image\`; local to the machine that built it) |
| Run by | ${platform:-local}${run_url:+ — <$run_url>} |
| Acceptance suite | exit \`$acc_exit\` — \`>>> FULL SUITE $suite\` — $(duration "$acc_seconds") |
| \`just test\` | exit \`$test_exit\` — $(duration "$test_seconds") |
| Wall time | $(duration $((acc_seconds + test_seconds))) |

## Acceptance criteria

$crit_pass of $crit_total pass. The criteria are those of Part C of [the acceptance spec](../../dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md) at this commit. Each result is the verdict line the suite printed for it. Where one step verifies several criteria together its verdict names them all — \`AC-B-1/4\` is AC-B-1 and AC-B-4 — and each is given that step's result.

| Criterion | Result | Verdict line | What the step shows |
|---|---|---|---|
EOF
    awk -F"$TAB" '$1 == "V" { gsub(/\|/, "\\|", $5); printf "| %s | %s | `%s` | %s |\n", $2, $3, $4, $5 }' "$work/criteria"
    cat <<EOF

Phase definitions of done: $dod_met of $dod_total met.

Steps the suite also ran that are not acceptance criteria:

| Step | Result |
|---|---|
EOF
    awk -F"$TAB" '$1 == "O" { printf "| `%s` | %s |\n", $2, $3 }' "$work/acc"
    cat <<EOF

Tests the suite ran to reach those verdicts: **$acc_passed passed · $acc_failed failed · $acc_ignored ignored**, in $acc_bins test binaries. This counts each \`test result:\` line printed between a step's header and its verdict. Most steps run a subset of a crate's tests and several run the same binary, so the figure is neither a subset of nor comparable with the \`just test\` total below.

## \`just test\`

**$t_passed passed · $t_failed failed · $t_ignored ignored**, in $t_bins test binaries and doc-test runs across $inv_ran of $inv_expected invocations. Crypto backends exercised: ${backends:-none}. The two backends cannot be compiled together, so each is its own invocation.

A count here is the sum of the \`test result:\` lines an invocation printed: tests that ran and passed, not test functions in the source. An invocation is one \`cargo test\` line of the justfile's \`test\` recipe.

| # | Invocation | Backend | Binaries | Passed | Failed | Ignored |
|---|---|---|---|---|---|---|
EOF
    awk -F"$TAB" '$1 == "I" { printf "| %d | `%s` | %s | %d | %d | %d | %d |\n", $2, $8, $7, $3, $4, $5, $6 }' "$work/test"
    printf '| | **Total** | | **%d** | **%d** | **%d** | **%d** |\n' "$t_bins" "$t_passed" "$t_failed" "$t_ignored"
    if [ "$inv_ran" != "$inv_expected" ]; then
        printf '\nThe run stopped early. Not run:\n\n'
        tail -n +"$((inv_ran + 1))" "$work/expected" | sed 's/^/- `/; s/$/`/'
    fi
    cat <<EOF

## Logs

The complete output of both parts, as it was printed, and the metadata the record was made with.

| File | SHA-256 | Bytes |
|---|---|---|
EOF
    for f in acceptance.log test.log acceptance.meta test.meta; do
        printf '| [`%s`](%s/%s) | `%s` | %s |\n' "$f" "$commit" "$f" "$(sha256_of "$dir/$f")" "$(bytes_of "$dir/$f")"
    done
    cat <<EOF

## Reproduce

\`\`\`bash
git checkout $commit
bash tools/ci/gate-run.sh
bash tools/ci/gate-record.sh --compare target/gate
\`\`\`

The first command needs the network once, to build the toolchain image from the base image pinned above; everything after it runs with none. \`--compare\` checks the fresh run's result for each criterion, and its test totals, against this record. Timings and the built image's id will differ; nothing else should.
EOF
}

# status_block PREFIX — the table for a README, linking with PREFIX to the root.
status_block() {
    local prefix="$1" current
    echo "$BEGIN_MARK"
    echo "<!-- Generated by tools/ci/gate-record.sh --status from the gate record. Do not edit. -->"
    current="$(current_commit)"
    if [ -z "$current" ]; then
        cat <<EOF
**No gate run is recorded yet.** A record is made from a run of the acceptance suite and of \`just test\`, in the pinned toolchain image under \`docker run --network none\`, at a commit in this repository's history. Until one is committed under [\`$RECORDS/\`](${prefix}$RECORDS/), no pass count is claimed here.
EOF
    else
        cat <<EOF
The gate was last recorded at commit [\`${commit:0:12}\`](${prefix}$RECORDS/$commit.md), in a run started $started (UTC) in the pinned toolchain image under \`docker run --network none\`. The record lists every criterion's result and every test invocation, and carries the complete logs. Commits after \`${commit:0:12}\` are not covered by it.

| | |
|---|---|
| Gate result | **$result** |
| Acceptance criteria | **$crit_pass / $crit_total pass** |
EOF
        [ -z "$failing" ] || echo "| Failing criteria | **${failing// /, }** |"
        cat <<EOF
| Tests run by the acceptance suite | $acc_passed passed · $acc_failed failed · $acc_ignored ignored |
| Tests run by \`just test\` | **$t_passed passed · $t_failed failed · $t_ignored ignored**, in $inv_ran of $inv_expected invocations ($t_bins test binaries) |
| Crypto backends | ${backends:-none}, in separate invocations |
| Phase definitions of done | $dod_met / $dod_total met |
| Toolchain | \`$rustc\` |
| Network during the run | none (\`--network none\`) |
| Exit | acceptance \`$acc_exit\` (\`>>> FULL SUITE $suite\`) · \`just test\` \`$test_exit\` |
EOF
    fi
    echo "$END_MARK"
}

# splice FILE BLOCK — FILE with what lies between the markers replaced by BLOCK, on
# stdout with LF line ends whatever FILE has.
splice() {
    tr -d '\r' < "$1" | awk -v begin="$BEGIN_MARK" -v end="$END_MARK" -v block="$2" '
        $0 == begin { while ((getline line < block) > 0) print line; skip = 1; found = 1; next }
        $0 == end { skip = 0; next }
        !skip { print }
        END { if (!found || skip) exit 3 }'
}

current_commit() {
    [ -f "$ROOT/$RECORDS/CURRENT" ] && tr -d ' \r\n' < "$ROOT/$RECORDS/CURRENT"
    return 0
}

# status MODE — write or check the status tables. compute must have run if a record exists.
status() {
    local mode="$1" entry file prefix bad=0
    for entry in "${STATUS_FILES[@]}"; do
        file="${entry%%|*}"
        prefix="${entry#*|}"
        [ -f "$ROOT/$file" ] || continue
        status_block "$prefix" > "$work/block"
        if ! splice "$ROOT/$file" "$work/block" > "$work/spliced"; then
            echo "gate-record: $file has no '$BEGIN_MARK' ... '$END_MARK' block" >&2
            bad=1
            continue
        fi
        # Compared without regard to line ends, and written back with the file's own: a
        # checkout may give these files CRLF, and that is not a difference in content.
        tr -d '\r' < "$ROOT/$file" > "$work/have"
        if [ "$mode" = write ]; then
            if ! cmp -s "$work/spliced" "$work/have"; then
                if [ "$(bytes_of "$ROOT/$file")" != "$(bytes_of "$work/have")" ]; then
                    sed $'s/$/\r/' "$work/spliced" > "$ROOT/$file"
                else
                    cat "$work/spliced" > "$ROOT/$file"
                fi
                echo "gate-record: updated $file"
            fi
        elif ! cmp -s "$work/spliced" "$work/have"; then
            echo "gate-record: the status table in $file is not the one the record gives" >&2
            diff "$work/have" "$work/spliced" | head -n 12 >&2
            echo "  Rewrite it: bash tools/ci/gate-record.sh --status" >&2
            bad=1
        fi
    done
    return "$bad"
}

# stray_paths COMMIT [HEAD] — files the gate covers that differ between COMMIT and HEAD,
# or, with no second argument, between COMMIT and the working tree.
stray_paths() {
    local path allowed ok
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        ok=0
        for allowed in "${UNGATED[@]}"; do
            case "$allowed" in
                */) [ "${path#"$allowed"}" != "$path" ] && ok=1 ;;
                *) [ "$path" = "$allowed" ] && ok=1 ;;
            esac
        done
        [ "$ok" = 1 ] || echo "$path"
    done < <(gitr diff --name-only "$@")
}

# --- modes -----------------------------------------------------------------------------

mode=write
from="$ROOT/target/gate"
release=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --from) from="${2:?--from needs a directory}"; shift 2 ;;
        --check) mode=check; shift ;;
        --release) release=1; shift ;;
        --status) mode=status; shift ;;
        --compare) mode=compare; from="${2:?--compare needs a directory}"; shift 2 ;;
        --print) mode=print; from="${2:?--print needs a directory}"; shift 2 ;;
        --figures) mode=figures; shift ;;
        -h | --help)
            [ "$#" -eq 1 ] || die "--help takes no other argument"
            sed -n '2,31p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) die "unknown argument '$1'" ;;
    esac
done

case "$mode" in
    print)
        compute "$from"
        render_record "$from"
        ;;

    write)
        compute "$from"
        gitr merge-base --is-ancestor "$commit" HEAD 2>/dev/null ||
            refuse "commit $commit is not in the history of the branch checked out" \
                "The record is committed on top of the commit it names."
        dest="$ROOT/$RECORDS/$commit"
        mkdir -p "$dest"
        for f in acceptance.log test.log acceptance.meta test.meta; do
            [ "$from/$f" -ef "$dest/$f" ] || cp "$from/$f" "$dest/$f"
        done
        render_record "$dest" > "$ROOT/$RECORDS/$commit.md"
        echo "$commit" > "$ROOT/$RECORDS/CURRENT"
        echo "gate-record: wrote $RECORDS/$commit.md — $result, $crit_pass of $crit_total criteria, $t_passed tests passed"
        status write || exit 1
        [ "$result" = PASS ] || echo "gate-record: the gate FAILED at this commit; the record and the status tables say so." >&2
        ;;

    status)
        current="$(current_commit)"
        [ -z "$current" ] || compute "$ROOT/$RECORDS/$current"
        status write
        ;;

    check)
        current="$(current_commit)"
        if [ -z "$current" ]; then
            [ "$release" = 0 ] || die "no gate run is recorded; a release needs one for its commit"
            status check || exit 1
            echo "gate-record: no gate run is recorded yet, and the status tables say so."
            exit 0
        fi
        dir="$ROOT/$RECORDS/$current"
        [ -d "$dir" ] || die "$RECORDS/CURRENT names $current, but $RECORDS/$current/ does not exist"
        compute "$dir"
        [ "$commit" = "$current" ] || die "$RECORDS/CURRENT names $current, but its logs are of $commit"
        gitr merge-base --is-ancestor "$commit" HEAD 2>/dev/null ||
            die "the recorded commit $commit is not in the history of the commit checked out"
        render_record "$dir" > "$work/record"
        if ! cmp -s "$work/record" "$ROOT/$RECORDS/$commit.md"; then
            echo "gate-record: $RECORDS/$commit.md is not what its logs give" >&2
            diff "$ROOT/$RECORDS/$commit.md" "$work/record" | head -n 20 >&2
            exit 1
        fi
        status check || exit 1
        if [ "$release" = 1 ]; then
            [ "$result" = PASS ] || die "the recorded gate run FAILED (${failing:-see the record}); it cannot back a release"
            stray="$(stray_paths "$commit" HEAD)"
            if [ -n "$stray" ]; then
                echo "gate-record: the record is of ${commit:0:12}, and the commit being released differs from it in files the gate covers:" >&2
                head -n 20 <<<"$stray" | sed 's/^/  /' >&2
                echo "  Run the gate at the release commit and record it." >&2
                exit 1
            fi
        fi
        echo "gate-record: $RECORDS/$commit.md matches its logs — $result, $crit_pass of $crit_total criteria, $t_passed tests passed."
        ;;

    figures)
        # For tools/ci/numbers.sh. covers_tree says whether the tree as it stands is the
        # gated commit, apart from the record and the prose generated from it: only then
        # are the record's figures figures for this tree.
        current="$(current_commit)"
        if [ -z "$current" ]; then
            echo "recorded=no"
            exit 0
        fi
        compute "$ROOT/$RECORDS/$current"
        if [ -z "$(stray_paths "$commit")" ]; then covers=yes; else covers=no; fi
        cat <<EOF
recorded=yes
commit=$commit
covers_tree=$covers
result=$result
criteria_pass=$crit_pass
criteria_total=$crit_total
failing=$failing
suite_tests_passed=$acc_passed
suite_tests_failed=$acc_failed
suite_tests_ignored=$acc_ignored
tests_passed=$t_passed
tests_failed=$t_failed
tests_ignored=$t_ignored
test_binaries=$t_bins
invocations=$inv_ran
invocations_expected=$inv_expected
EOF
        ;;

    compare)
        current="$(current_commit)"
        [ -n "$current" ] || die "no gate run is recorded to compare with"
        compute "$ROOT/$RECORDS/$current"
        awk -F"$TAB" '$1 == "V" { print $2, $3 }' "$work/criteria" > "$work/recorded.criteria"
        echo "$t_passed passed, $t_failed failed, $t_ignored ignored, $inv_ran invocations" > "$work/recorded.tests"
        recorded="$commit"
        compute "$from"
        [ "$commit" = "$recorded" ] || die "the run in $from is of $commit; the record is of $recorded"
        awk -F"$TAB" '$1 == "V" { print $2, $3 }' "$work/criteria" > "$work/fresh.criteria"
        echo "$t_passed passed, $t_failed failed, $t_ignored ignored, $inv_ran invocations" > "$work/fresh.tests"
        bad=0
        if ! diff "$work/recorded.criteria" "$work/fresh.criteria" > "$work/diff"; then
            echo "gate-record: this run's criteria differ from the record's (< recorded, > this run):" >&2
            cat "$work/diff" >&2
            bad=1
        fi
        if ! cmp -s "$work/recorded.tests" "$work/fresh.tests"; then
            echo "gate-record: test totals differ — recorded: $(cat "$work/recorded.tests"); this run: $(cat "$work/fresh.tests")" >&2
            bad=1
        fi
        [ "$bad" = 0 ] || exit 1
        echo "gate-record: the run in $from reproduces the record of ${commit:0:12}: $crit_pass of $crit_total criteria, $(cat "$work/fresh.tests")."
        ;;
esac
