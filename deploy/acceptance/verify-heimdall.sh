#!/usr/bin/env bash
# ============================================================================
# Heimdall — acceptance verification (Phase 6).
#
# Definition of Done: AC-H-1..7 + the A.8 Eval suite EV-A8-*.
#   AC-H-1  multiple feeds fuse into one picture with per-layer toggles   (Prompt 6.b)
#   AC-H-2  an unverified feed source is rejected from the picture        (Prompt 6.b)
#   AC-H-3  ranked options each cite the evidence used (advisory)         (Prompt 6.c)
#   AC-H-4  consequential actions need human authorisation; fully audited (Prompt 6.c)
#   AC-H-5  the picture replicates across nodes over PQ-mTLS              (Prompt 6.b)
#   AC-H-6  Mimir absent: fusion + rules classification stay functional   (Prompt 6.b)
#   AC-H-7  no outbound connectivity: verify->fuse->classify->project run  (Prompt 6.b)
#   EV-A8-* option generation: cited, grounded, ranked, neutral, insuff, advisory (Prompt 6.c)
#
# For each criterion this runs the test(s) that exercise it and reports PASS/FAIL
# with real output. Fails closed. SAD_04 §17.
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates AC-H-7 concretely: the whole
# feed-verify -> fuse -> classify -> fabric-project path completes with no
# outbound network.
# ============================================================================
set -uo pipefail
export PATH=/usr/local/cargo/bin:${PATH:-}
cd "$(dirname "$0")/../.." || exit 2
[ -f Cargo.toml ] || { echo "must run from the workspace root"; exit 2; }

pass=0
fail=0
declare -a SUMMARY

run_ac() {
    local id="$1" desc="$2"
    shift 2
    echo
    echo "============================================================"
    echo "$id — $desc"
    echo "  \$ $*"
    local out rc
    out="$("$@" 2>&1)"
    rc=$?
    echo "$out" | grep -E 'test result:|running [0-9]+ test' | sed 's/^/    /'
    if [ "$rc" -eq 0 ]; then
        echo "  >>> $id: PASS"
        pass=$((pass + 1))
        SUMMARY+=("$id  PASS  $desc")
    else
        echo "  >>> $id: FAIL (exit $rc)"
        echo "$out" | tail -25 | sed 's/^/    /'
        fail=$((fail + 1))
        SUMMARY+=("$id  FAIL  $desc")
    fi
}

FIPS="--features backend-fips"

# --- AC-H-2: unverified feed rejected (PQ source signature gate) ---
run_ac "AC-H-2" "an unverified feed source is rejected from the picture" \
    cargo test --offline -p heimdall-feed-gateway $FIPS --test ac_h_2

# --- AC-H-1: multi-feed fusion into one layered picture with toggles ---
run_ac "AC-H-1" "multiple feeds fuse into one picture with per-layer toggles" \
    cargo test --offline -p heimdall-fusion $FIPS --test ac_h_1_6_7 -- \
    ac_h_1_multiple_feeds_fuse_into_one_layered_picture_with_toggles

# --- AC-H-6: rules classification with Mimir absent ---
run_ac "AC-H-6" "Mimir absent: fusion + rules-based classification remain functional" \
    cargo test --offline -p heimdall-fusion $FIPS --test ac_h_1_6_7 -- \
    ac_h_6_rules_classification_without_mimir

# --- AC-H-7: full path runs offline and projects into the fabric ---
run_ac "AC-H-7" "no outbound connectivity: verify->fuse->classify->project all succeed" \
    cargo test --offline -p heimdall-fusion $FIPS --test ac_h_1_6_7 -- \
    ac_h_7_full_path_runs_offline_and_projects_to_fabric

# --- AC-H-3 + EV-A8-*: cited, grounded, ranked, neutral, insuff, advisory options ---
run_ac "AC-H-3" "ranked options each cite the evidence used, strictly advisory (A.8, EV-A8-*)" \
    cargo test --offline -p heimdall-decision-support --test ac_h_3

# --- AC-H-4: consequential actions are human-gated and fully audited ---
run_ac "AC-H-4" "a consequential action needs explicit human authorisation and is fully audited" \
    cargo test --offline -p heimdall-workflow $FIPS --test ac_h_4

# --- AC-H-5: multi-node picture replication over PQ-mTLS ---
run_ac "AC-H-5" "the common picture replicates across nodes over PQ-mTLS" \
    cargo test --offline -p heimdall-picture-sync $FIPS --test ac_h_5

# --- Supporting unit suites: canonical codec + rules engine ---
run_ac "UNIT" "canonical picture codec + deterministic rules engine" \
    cargo test --offline -p heimdall-picture -p heimdall-classify

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Heimdall (AC-H-1..7, EV-A8-*)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> HEIMDALL AC-H-1..7 DEFINITION OF DONE: MET"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
