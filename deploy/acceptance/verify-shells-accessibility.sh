#!/usr/bin/env bash
# ============================================================================
# Client shells — accessibility & HIG conformance completeness (AC-C-11).
#
# The six shells cannot be compiled/rendered in the reproducible build
# environment (they need native toolchains). This checks the part that IS
# machine-verifiable: every shell declares the full required accessibility
# affordance set for its OS guideline set, and binds the single shared core
# (KDD-1). The on-device HIG/interaction audits (XCUITest / Espresso / Playwright
# / Tauri harness) run per platform, off this path.
#
# Fails closed if any shell is missing or under-declares an affordance.
# ============================================================================
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 2

SHELLS=(apple android linux-gtk desktop-tauri web extension)
AFFORDANCES=(
    dynamic_type
    screen_reader_labels
    contrast_aa
    focus_order
    reduced_motion
    keyboard_navigation
    hit_target_size
)

fail=0
for shell in "${SHELLS[@]}"; do
    dir="clients/shells/$shell"
    man="$dir/manifest.toml"
    acc="$dir/accessibility.toml"

    if [ ! -f "$man" ] || [ ! -f "$acc" ]; then
        echo "  MISSING: $shell (manifest.toml / accessibility.toml)"
        fail=$((fail + 1))
        continue
    fi

    # KDD-1: the shell must bind the shared core, not re-implement crypto/sync.
    if ! grep -q 'links_core *= *"midgard-client-core"' "$man"; then
        echo "  $shell: manifest does not bind midgard-client-core (KDD-1)"
        fail=$((fail + 1))
    fi

    # Every required affordance must be declared conformant.
    for aff in "${AFFORDANCES[@]}"; do
        if ! grep -qE "^${aff} *= *\"conformant\"" "$acc"; then
            echo "  $shell: affordance '$aff' not declared conformant"
            fail=$((fail + 1))
        fi
    done

    if [ "$fail" -eq 0 ]; then
        echo "  OK: $shell — binds core, all ${#AFFORDANCES[@]} affordances conformant"
    fi
done

if [ "$fail" -eq 0 ]; then
    echo "  >>> all ${#SHELLS[@]} shells declare complete accessibility conformance"
    exit 0
else
    echo "  >>> $fail accessibility/shell defect(s) (fail closed)"
    exit 1
fi
