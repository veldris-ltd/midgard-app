#!/usr/bin/env bash
# ============================================================================
# Client shells — per-release ON-DEVICE accessibility audit gate (RT-49 / RC-49).
#
# The sibling `verify-shells-accessibility.sh` checks the STATIC declaration
# (AC-C-11 completeness). This gate checks the other half RC-49 requires: that a
# real, on-device/on-runner audit actually RAN for this release and was RECORDED,
# for every shell. Records are written by the on-device jobs in
# .github/workflows/shell-audit.yml; see clients/shells/AUDIT_SCHEMA.md.
#
# Fails closed if any shell lacks a record for <release>, if a record carries a
# placeholder runner/device/tool, if it is not `overall = "pass"`, or if any
# affordance the shell DECLARES is not MEASURED `pass` in the record.
#
# A PRE-RELEASE only (a version with a `-` suffix, e.g. 0.1.0-rc.1) may be waived by
# a line in clients/shells/audit-waivers.txt naming that exact version, who approved
# it and why. A waived run still reports every problem, and says WAIVED; a waiver
# listed for a final release is refused. See clients/shells/AUDIT_SCHEMA.md.
#
# Usage:  bash deploy/acceptance/verify-shell-audits.sh <release>
#   e.g.  bash deploy/acceptance/verify-shell-audits.sh 1.4.0
# ============================================================================
set -uo pipefail
cd "${SHELL_AUDIT_ROOT:-$(dirname "$0")/../..}" || exit 2

release="${1:-}"
if [ -z "$release" ]; then
    echo "usage: verify-shell-audits.sh <release>   (e.g. 1.4.0 or the release tag)"
    exit 2
fi

SHELLS=(apple android linux-gtk desktop-tauri web extension)
WAIVERS="clients/shells/audit-waivers.txt"

# The waiver line for a release, without its version: empty when there is none.
waiver_for() {
    [ -f "$WAIVERS" ] || return 0
    tr -d '\r' < "$WAIVERS" | grep -vE '^[[:space:]]*(#|$)' |
        awk -v r="$1" '$1 == r { $1 = ""; sub(/^[[:space:]]+/, ""); print; exit }'
}

# A field's value from a flat `key = "value"` TOML line.
field() { grep -oE "^$2[[:space:]]*=[[:space:]]*\"[^\"]*\"" "$1" | head -1 | sed -E 's/^[^"]*"([^"]*)".*/\1/'; }

# A placeholder / unset value we must reject (so a stubbed record cannot pass).
is_placeholder() {
    case "$1" in
        ""|"TODO"|"todo"|"TBD"|"tbd"|"placeholder"|"none"|"n/a"|"N/A"|"<"*">") return 0 ;;
        *) return 1 ;;
    esac
}

fail=0
for shell in "${SHELLS[@]}"; do
    rec="clients/shells/$shell/audits/$release.toml"
    acc="clients/shells/$shell/accessibility.toml"

    if [ ! -f "$rec" ]; then
        echo "  MISSING: $shell has no on-device audit record for release $release ($rec)"
        fail=$((fail + 1)); continue
    fi
    if [ ! -f "$acc" ]; then
        echo "  $shell: accessibility.toml missing — cannot cross-check declared affordances"
        fail=$((fail + 1)); continue
    fi

    # The audit must name a REAL runner, device, and tool (no placeholders), and the
    # release must match the file it is recorded under.
    runner="$(field "$rec" runner)"; device="$(field "$rec" device)"
    tool="$(field "$rec" tool)";     rel="$(field "$rec" release)"
    overall="$(field "$rec" overall)"
    for pair in "runner:$runner" "device:$device" "tool:$tool"; do
        if is_placeholder "${pair#*:}"; then
            echo "  $shell: audit '${pair%%:*}' is empty/placeholder — not a real on-device run"
            fail=$((fail + 1))
        fi
    done
    [ "$rel" = "$release" ] || { echo "  $shell: record's release '$rel' != $release"; fail=$((fail + 1)); }
    [ "$overall" = "pass" ] || { echo "  $shell: overall = '$overall' (must be 'pass')"; fail=$((fail + 1)); }

    # Every affordance the shell DECLARES conformant must be MEASURED `pass` here.
    declared=$(grep -oE '^[a-z_]+[[:space:]]*=[[:space:]]*"conformant"' "$acc" | sed -E 's/[[:space:]]*=.*//')
    for aff in $declared; do
        measured="$(field "$rec" "$aff")"
        if [ "$measured" != "pass" ]; then
            echo "  $shell: affordance '$aff' declared conformant but audit measured '${measured:-<absent>}'"
            fail=$((fail + 1))
        fi
    done

    [ "$fail" -eq 0 ] && echo "  OK: $shell — audited on '$runner' / '$device' via $tool; all affordances pass"
done

echo
if [ "$fail" -eq 0 ]; then
    echo "  >>> all ${#SHELLS[@]} shells have a passing on-device accessibility audit recorded for $release"
    exit 0
fi

waiver="$(waiver_for "$release")"
if [ -n "$waiver" ]; then
    case "$release" in
        *-*)
            if grep -q 'approved-by:' <<<"$waiver" && grep -q 'reason:' <<<"$waiver"; then
                echo "  >>> shell audit gate WAIVED for pre-release $release ($fail problem(s) above stand): $waiver"
                exit 0
            fi
            echo "  the waiver for $release names no approver or no reason ('approved-by:' and 'reason:' are required)"
            ;;
        *)
            echo "  a waiver is listed for $release, but only a pre-release (a version with a '-' suffix) can be waived"
            ;;
    esac
fi
echo "  >>> shell audit gate FAILED for $release ($fail problem(s)) — release blocked (fail closed)"
exit 1
