#!/usr/bin/env bash
# Tests for the pre-release waiver in deploy/acceptance/verify-shell-audits.sh, against
# throwaway trees. [RC-49] [AC-C-11]
#
#   bash tools/ci/tests/shell-audit-waiver.test.sh
#
# The claim under test: with no audit records, the gate fails closed, unless the exact
# pre-release being gated is waived with an approver and a reason. A final release is
# never waived, and a waived run still names what is missing.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../../../deploy/acceptance/verify-shell-audits.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

tree() { # tree NAME [WAIVER-LINE...]: six shells with declarations and no records
    local root="$work/$1"; shift
    local shell
    for shell in apple android linux-gtk desktop-tauri web extension; do
        mkdir -p "$root/clients/shells/$shell/audits"
        echo 'screen_reader = "conformant"' > "$root/clients/shells/$shell/accessibility.toml"
    done
    if [ "$#" -gt 0 ]; then printf '%s\n' "# waivers" "$@" > "$root/clients/shells/audit-waivers.txt"; fi
    echo "$root"
}
gate() { SHELL_AUDIT_ROOT="$1" bash "$script" "$2"; }

line='0.1.0-rc.1  approved-by: Test Owner  date: 2026-10-22  reason: no shells yet'

root="$(tree none)"
gate "$root" 0.1.0-rc.1 >/dev/null 2>&1 && fail "with no records and no waiver, the gate fails" ||
    pass "with no records and no waiver, the gate fails"

root="$(tree waived "$line")"
if out="$(gate "$root" 0.1.0-rc.1 2>&1)"; then
    grep -q 'WAIVED for pre-release 0.1.0-rc.1' <<<"$out" && grep -q 'MISSING: apple' <<<"$out" &&
        pass "a waived pre-release passes, says WAIVED, and still names what is missing" ||
        fail "a waived run reports itself: $out"
else
    fail "a waived pre-release passes: $out"
fi

gate "$root" 0.1.0-rc.2 >/dev/null 2>&1 && fail "a waiver covers only the version it names" ||
    pass "a waiver covers only the version it names"

root="$(tree final '1.0.0  approved-by: Test Owner  date: 2026-10-22  reason: no shells yet')"
if out="$(gate "$root" 1.0.0 2>&1)"; then fail "a final release cannot be waived"; else
    grep -q 'only a pre-release' <<<"$out" && pass "a final release cannot be waived, and why" ||
        fail "the reason is given: $out"
fi

root="$(tree bare '0.1.0-rc.1  date: 2026-10-22')"
gate "$root" 0.1.0-rc.1 >/dev/null 2>&1 && fail "a waiver with no approver or reason is refused" ||
    pass "a waiver with no approver or reason is refused"

root="$(tree commented "# $line")"
gate "$root" 0.1.0-rc.1 >/dev/null 2>&1 && fail "a commented-out waiver waives nothing" ||
    pass "a commented-out waiver waives nothing"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all shell-audit waiver tests passed"
