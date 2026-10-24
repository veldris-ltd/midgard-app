#!/usr/bin/env bash
# The claims ledger: render it, and check it against the tree. [AC-RF-CA-1b]
# [AC-RF-CA-1c] [AC-RF-CA-13b] [AC-RF-CA-13d]
#
#   bash tools/ci/claims.sh --render          write docs/compliance/CLAIMS.md
#   bash tools/ci/claims.sh --check           exit 1 if the ledger and the tree disagree
#   bash tools/ci/claims.sh --check --strict  also fail while any wording is still listed
#                                             as stronger than its row's status
#
# docs/compliance/claims.toml has one [[claim]] per capability the repository states.
# CLAIMS.md is what it renders, and nothing else is edited by hand. The check holds a
# row to what it cites:
#
#   - every evidence path exists, and a line anchor is inside its file;
#   - every named test is a #[test] function in the file it names;
#   - every acceptance criterion id is one the specification or the gate knows;
#   - a row at `operable` or `production` names a service test: a step of a script that
#     drives the running services, or a test that starts a service and talks to it;
#   - a row below `operable` carries the wording to use until then, and a milestone;
#   - an `external` row cites nothing, and its name occurs nowhere in the workspace's
#     code, manifests, lockfiles or deployment files;
#   - CLAIMS.md is exactly what claims.toml renders.
#
# The data file is a small subset of TOML, read here with awk: `[[claim]]` opens a row,
# and a row holds `key = "text"` and `key = ["a", "b"]` on one line each. Text holds no
# double quote.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/lib.sh"
cd "${MIDGARD_CLAIMS_ROOT:-$here/../..}"

LEDGER=docs/compliance/claims.toml
RENDERED=docs/compliance/CLAIMS.md
STATUSES='spec reference operable production external'
TEXT_KEYS='id area claim status limit milestone fallback'
LIST_KEYS='criteria evidence tests service_tests stated_in overstated_in'
# Where an acceptance criterion id has to be known.
CRITERIA_ROOTS='deploy/acceptance docs/dev/markdown BUILD_PLAYBOOK.md'
# Where the name of an external tool may not occur: everything that builds, ships or
# deploys. Documents may name it; that is how the boundary gets stated.
CODE_ROOTS='Cargo.toml Cargo.lock package.json pnpm-lock.yaml pnpm-workspace.yaml justfile
    admin bifrost clients crypto forge heimdall mimir services sdk ui products deploy
    supply-chain contracts/proto contracts/schema contracts/fixtures tools'

# ---- reading ---------------------------------------------------------------------------

# The ledger as lines of `row<TAB>key<TAB>value`, one per value; a list gives one line
# per element. A line it cannot read becomes `row<TAB>!<TAB>the line`.
records() {
    tr -d '\r' < "$LEDGER" | awk '
        function emit(key, value) { printf "%d\t%s\t%s\n", row, key, value }
        /^[ \t]*(#.*)?$/ { next }
        /^\[\[claim\]\][ \t]*$/ { row++; next }
        row == 0 { printf "0\t!\t%s\n", $0; next }
        match($0, /^[a-z_]+ = "/) {
            key = substr($0, 1, RLENGTH - 4)
            value = substr($0, RLENGTH + 1)
            if (value !~ /"[ \t]*$/ || index(substr(value, 1, length(value) - 1), "\"")) { emit("!", $0); next }
            sub(/"[ \t]*$/, "", value)
            emit(key, value)
            next
        }
        match($0, /^[a-z_]+ = \[/) {
            key = substr($0, 1, RLENGTH - 4)
            value = substr($0, RLENGTH + 1)
            if (value !~ /\][ \t]*$/) { emit("!", $0); next }
            sub(/\][ \t]*$/, "", value)
            if (value ~ /^[ \t]*$/) next
            if (value !~ /^"[^"]*"(, "[^"]*")*$/) { emit("!", $0); next }
            count = split(substr(value, 2, length(value) - 2), items, /", "/)
            for (i = 1; i <= count; i++) emit(key, items[i])
            next
        }
        { emit("!", $0) }'
}

declare -A TEXT LIST
ROWS=0
load() {
    local row key value
    [ -f "$LEDGER" ] || { ci_error "claims: $LEDGER is missing"; exit 1; }
    while IFS=$'\t' read -r row key value; do
        [ "$row" -gt "$ROWS" ] && ROWS="$row"
        case " $TEXT_KEYS " in *" $key "*)
            if [ -n "${TEXT[$row.$key]:-}" ]; then
                LIST[$row.!]+="\"$key\" is given twice"$'\n'
            fi
            TEXT[$row.$key]="$value"
            continue ;;
        esac
        case " $LIST_KEYS " in *" $key "*) LIST[$row.$key]+="$value"$'\n'; continue ;; esac
        if [ "$key" = "!" ]; then
            LIST[$row.!]+="cannot read: $value"$'\n'
        else
            LIST[$row.!]+="unknown key \"$key\""$'\n'
        fi
    done < <(records)
}

# list ROW KEY: the values of a list, one per line.
list() { printf '%s' "${LIST[$1.$2]:-}"; }
count() { list "$1" "$2" | grep -c . || true; }

# ---- checking --------------------------------------------------------------------------

status=0
problem() {
    ci_error "claims: $1"
    status=1
}

lines_in() { wc -l < "$1" | tr -d ' '; }

# anchored WHO WHAT VALUE: VALUE is `path` or `path:line`; the path exists and the line is
# inside it.
anchored() {
    local who="$1" what="$2" value="$3" path line
    path="${value%%:*}"
    line=""
    [ "$path" = "$value" ] || line="${value#*:}"
    if [ ! -f "$path" ]; then
        problem "$who: $what $value names a file that does not exist"
        return
    fi
    [ -z "$line" ] && return
    case "$line" in
        ''|*[!0-9]*|0) problem "$who: $what $value has a line anchor that is not a line number" ;;
        *) [ "$line" -le "$(lines_in "$path")" ] \
            || problem "$who: $what $value points past the end of the file ($(lines_in "$path") lines)" ;;
    esac
}

# a_test WHO WHAT VALUE: VALUE is `path::name`, a #[test] function in that file.
a_test() {
    local who="$1" what="$2" value="$3" path name
    path="${value%%::*}"
    name="${value#*::}"
    if [ "$path" = "$value" ] || [ -z "$name" ]; then
        problem "$who: $what $value is not written path::test_name"
        return
    fi
    if [ ! -f "$path" ]; then
        problem "$who: $what $value names a file that does not exist"
        return
    fi
    tr -d '\r' < "$path" | awk -v name="$name" '
        /^[ \t]*#\[test\]/ { armed = 1; next }
        armed && $0 ~ ("^[ \t]*(pub )?(async )?fn " name "[ \t]*\\(") { found = 1 }
        armed && !/^[ \t]*(#\[|\/\/)/ { armed = 0 }
        END { exit !found }' \
        || problem "$who: $what $value is not a #[test] function in that file"
}

check() {
    local row who id st value ids="" seen
    for row in $(seq 1 "$ROWS"); do
        id="${TEXT[$row.id]:-}"
        who="claim $row${id:+ ($id)}"
        while IFS= read -r value; do
            [ -n "$value" ] && problem "$who: $value"
        done < <(list "$row" '!')

        case "$id" in
            '') problem "$who: missing \"id\"" ;;
            *[!a-z0-9-]*) problem "$who: \"id\" may hold only lower-case letters, digits and -" ;;
        esac
        case $'\n'"$ids" in *$'\n'"$id"$'\n'*) problem "$who: the id is used twice" ;; esac
        ids+="$id"$'\n'
        [ -n "${TEXT[$row.claim]:-}" ] || problem "$who: missing \"claim\""
        [ -n "${TEXT[$row.area]:-}" ] || problem "$who: missing \"area\""

        st="${TEXT[$row.status]:-}"
        case " $STATUSES " in
            *" $st "*) ;;
            *) problem "$who: \"status\" must be one of: $STATUSES"; continue ;;
        esac

        while IFS= read -r value; do [ -n "$value" ] && anchored "$who" evidence "$value"; done < <(list "$row" evidence)
        while IFS= read -r value; do [ -n "$value" ] && a_test "$who" test "$value"; done < <(list "$row" tests)
        while IFS= read -r value; do [ -n "$value" ] && anchored "$who" "stated_in" "$value"; done < <(list "$row" stated_in)
        while IFS= read -r value; do [ -n "$value" ] && anchored "$who" "overstated_in" "$value"; done < <(list "$row" overstated_in)
        while IFS= read -r value; do
            [ -n "$value" ] || continue
            case "$value" in
                *::*) a_test "$who" "service test" "$value" ;;
                *.sh:*) anchored "$who" "service test" "$value" ;;
                *) problem "$who: service test $value is neither a script step (path.sh:line) nor a test (path::name)" ;;
            esac
        done < <(list "$row" service_tests)
        while IFS= read -r value; do
            [ -n "$value" ] || continue
            # shellcheck disable=SC2086
            grep -rqF -- "$value" $CRITERIA_ROOTS 2>/dev/null \
                || problem "$who: criterion $value is not an id the specification or the gate uses"
        done < <(list "$row" criteria)

        case "$st" in
            operable|production)
                [ "$(count "$row" service_tests)" -gt 0 ] \
                    || problem "$who: a row at $st must name a service test, not only a library test"
                ;;
            reference)
                [ "$(count "$row" tests)" -gt 0 ] \
                    || problem "$who: a row at reference must name the test that exercises the library"
                ;;
            external)
                for seen in evidence tests service_tests criteria; do
                    [ "$(count "$row" "$seen")" -eq 0 ] \
                        || problem "$who: an external row cites nothing, but it has \"$seen\""
                done
                # shellcheck disable=SC2086
                seen="$(grep -rliF --exclude-dir=node_modules --exclude-dir=target --exclude-dir=dist \
                    --exclude='claims*.sh' -- "$id" $CODE_ROOTS 2>/dev/null | head -5 | tr '\n' ' ')"
                [ -z "$seen" ] || problem "$who: the name occurs in the workspace: $seen"
                ;;
        esac
        case "$st" in
            spec|reference)
                [ -n "${TEXT[$row.fallback]:-}" ] || problem "$who: a row below operable needs \"fallback\", the wording to use until then"
                [ -n "${TEXT[$row.milestone]:-}" ] || problem "$who: a row below operable needs \"milestone\""
                ;;
        esac
    done
    [ "$ROWS" -gt 0 ] || problem "$LEDGER holds no claims"
}

# ---- rendering -------------------------------------------------------------------------

linked() {
    local value path
    while IFS= read -r value; do
        [ -n "$value" ] || continue
        path="${value%%:*}"
        printf '  - [`%s`](../../%s)\n' "$value" "$path"
    done
}

render() {
    local row st area last_area="" n total_over=0
    declare -A tally
    for row in $(seq 1 "$ROWS"); do
        st="${TEXT[$row.status]:-}"
        tally[$st]=$(( ${tally[$st]:-0} + 1 ))
        total_over=$(( total_over + $(count "$row" overstated_in) ))
    done
    cat <<'HEAD'
# Claims ledger

Generated from [`claims.toml`](claims.toml) by `bash tools/ci/claims.sh --render`. Do not
edit this file; edit the data and render again. `bash tools/ci/claims.sh --check` fails
if the two differ, or if a row cites a file, line or test that does not exist.

One row per capability this repository states about Midgard. A row gives the claim, its
status, and what supports it. It is the only place a capability's status is decided:
the README, the release notes and the launch material say what their row says.

## Status vocabulary

| Status | Meaning | How it may be worded |
|---|---|---|
| `production` | Operable, and assured on the hardware and at the scale it is claimed for. | Present tense. |
| `operable` | Runs in a service this repository builds, and a test drives that running service. | Present tense: "built", "delivers", "supports". |
| `reference` | Implemented and tested as a library. No running service uses it yet. | "Implemented and tested as a library (test); not yet in a running service." |
| `spec` | Specified. Not implemented, or implemented only as a scaffold. | "Specified in (document); not implemented." |
| `external` | Exists outside this repository, which makes no checkable claim about it. | Named as separate. Never counted among Midgard's capabilities or figures. |

A service test is a step of a script that drives the running services
(`deploy/compose/smoke.sh`, `deploy/kind/smoke.sh`) or a test that starts a service and
talks to it over its own listener.

HEAD
    printf '## Summary\n\n| Status | Rows |\n|---|---|\n'
    for st in production operable reference spec external; do
        printf '| `%s` | %d |\n' "$st" "${tally[$st]:-0}"
    done
    printf '| **Total** | **%d** |\n\n' "$ROWS"
    if [ "$total_over" -gt 0 ]; then
        printf '%d place(s) in the repository still word a capability more strongly than its row allows. Each is listed under its row as "Worded above this status at". `--check --strict` fails until that list is empty.\n\n' "$total_over"
    else
        printf 'No document is listed as wording a capability above its status.\n\n'
    fi

    for row in $(seq 1 "$ROWS"); do
        area="${TEXT[$row.area]:-}"
        if [ "$area" != "$last_area" ]; then
            printf '## %s\n\n' "$area"
            last_area="$area"
        fi
        st="${TEXT[$row.status]:-}"
        printf '### `%s` (%s)\n\n%s\n\n' "${TEXT[$row.id]:-}" "$st" "${TEXT[$row.claim]:-}"
        [ -z "${TEXT[$row.limit]:-}" ] || printf '**Limit.** %s\n\n' "${TEXT[$row.limit]}"
        if [ -n "${TEXT[$row.fallback]:-}" ]; then
            printf '**Until it is operable, say:** "%s"\n\n' "${TEXT[$row.fallback]}"
            printf '**Milestone.** %s\n\n' "${TEXT[$row.milestone]:-}"
        fi
        n="$(list "$row" criteria | paste -sd, - | sed 's/,/, /g')"
        [ -z "$n" ] || printf -- '- Criteria: %s\n' "$n"
        if [ "$(count "$row" evidence)" -gt 0 ]; then printf -- '- Source:\n'; list "$row" evidence | linked; fi
        if [ "$(count "$row" tests)" -gt 0 ]; then printf -- '- Tests:\n'; list "$row" tests | linked; fi
        if [ "$(count "$row" service_tests)" -gt 0 ]; then printf -- '- Service tests:\n'; list "$row" service_tests | linked; fi
        if [ "$(count "$row" stated_in)" -gt 0 ]; then printf -- '- Stated at:\n'; list "$row" stated_in | linked; fi
        if [ "$(count "$row" overstated_in)" -gt 0 ]; then printf -- '- Worded above this status at:\n'; list "$row" overstated_in | linked; fi
        printf '\n'
    done
}

# ---- entry -----------------------------------------------------------------------------

mode="" strict=0
for arg in "$@"; do
    case "$arg" in
        --render|--check) mode="$arg" ;;
        --strict) strict=1 ;;
        *) echo "usage: $0 --render | --check [--strict]" >&2; exit 2 ;;
    esac
done
[ -n "$mode" ] || { echo "usage: $0 --render | --check [--strict]" >&2; exit 2; }

load
case "$mode" in
    --render)
        render > "$RENDERED"
        echo "claims: wrote $RENDERED ($ROWS claims)"
        ;;
    --check)
        check
        if [ ! -f "$RENDERED" ]; then
            problem "$RENDERED is missing; run: bash tools/ci/claims.sh --render"
        elif ! diff -q <(render) <(tr -d '\r' < "$RENDERED") >/dev/null; then
            problem "$RENDERED is not what $LEDGER renders; run: bash tools/ci/claims.sh --render"
        fi
        over=0
        for row in $(seq 1 "$ROWS"); do over=$(( over + $(count "$row" overstated_in) )); done
        if [ "$over" -gt 0 ]; then
            if [ "$strict" -eq 1 ]; then
                problem "$over place(s) still word a capability above its ledger status (see $RENDERED)"
            else
                ci_warning "claims: $over place(s) still word a capability above its ledger status (see $RENDERED)"
            fi
        fi
        [ "$status" -eq 0 ] && echo "claims: $ROWS claims; every cited file, line, test and criterion exists, and $RENDERED is current."
        exit "$status"
        ;;
esac
