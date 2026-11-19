#!/usr/bin/env bash
# ============================================================================
# Write a per-release accessibility audit record (RT-49 / RC-49) in the
# AUDIT_SCHEMA.md format. The native audit jobs (apple/android/tauri/gtk) call
# this AFTER their platform tool runs, passing the measured result — the failed
# affordances (empty ⇒ all pass). The web family writes its record directly from
# axe-audit.mjs; this is the shared writer for the tool-driven native audits.
#
# Usage:
#   record-audit.sh <shell> <release> <runner> <device> <tool> <tool_version> \
#                   <report> [failed_affordance ...]
# ============================================================================
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2

shell="${1:?shell}"; release="${2:?release}"; runner="${3:?runner}"; device="${4:?device}"
tool="${5:?tool}"; tool_version="${6:?tool_version}"; report="${7:?report}"; shift 7 || true
failed=(" $* ")   # space-padded for membership test

acc="clients/shells/$shell/accessibility.toml"
[ -f "$acc" ] || { echo "record-audit: no accessibility.toml for shell '$shell'"; exit 2; }

out_dir="clients/shells/$shell/audits"
out="$out_dir/$release.toml"
mkdir -p "$out_dir"

# The affordances the shell declares conformant — the exact set the gate checks.
declared=$(grep -oE '^[a-z_]+[[:space:]]*=[[:space:]]*"conformant"' "$acc" | sed -E 's/[[:space:]]*=.*//')

overall="pass"; issues=0
{
    echo "# Accessibility audit record — $shell — release $release"
    echo "# WRITTEN BY CI ($tool) on a real runner/device. Do not hand-edit."
    echo "[audit]"
    echo "shell        = \"$shell\""
    echo "release      = \"$release\""
    echo "commit       = \"${MIDGARD_CI_COMMIT:-${GITHUB_SHA:-${CI_COMMIT_SHA:-unknown}}}\""
    echo "runner       = \"$runner\""
    echo "device       = \"$device\""
    echo "tool         = \"$tool\""
    echo "tool_version = \"$tool_version\""
    echo "timestamp    = \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\""
    printf 'overall      = "%s"\n\n' "PLACEHOLDER_OVERALL"
    echo "[results]"
    for aff in $declared; do
        if [[ "${failed[*]}" == *" $aff "* ]]; then
            printf '%-20s = "fail"\n' "$aff"; overall="fail"; issues=$((issues+1))
        else
            printf '%-20s = "pass"\n' "$aff"
        fi
    done
    echo
    echo "[evidence]"
    echo "report = \"$report\""
    echo "issues = $issues"
} > "$out"

# Second pass to stamp the real overall (known only after scanning affordances).
# Portable across GNU and BSD sed (the Apple audit runs on macOS): rewrite, then move.
# A record that cannot be stamped is removed, never left half-written.
if ! { sed "s/PLACEHOLDER_OVERALL/$overall/" "$out" > "$out.tmp" && mv "$out.tmp" "$out"; }; then
    rm -f "$out" "$out.tmp"
    echo "record-audit: could not stamp $out; no record written"
    exit 2
fi

echo "record-audit: wrote $out (overall=$overall, $issues issue(s))"
[ "$overall" = "pass" ]
