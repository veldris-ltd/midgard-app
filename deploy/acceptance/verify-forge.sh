#!/usr/bin/env bash
# ============================================================================
# Forge — acceptance verification (Phase 4).
#
# The Phase-4 Definition of Done: AC-F-1..7 and the A.6 Eval suite EV-A6-*.
#   AC-F-1  connector sync -> raw dataset encrypted with a PQ-wrapped key  (Prompt 4.b)
#   AC-F-2  a pipeline built twice from the same inputs is content-identical (Prompt 4.b)
#   AC-F-6  Mimir absent: all non-AI pipeline features remain functional   (Prompt 4.b)
#   AC-F-7  no outbound connectivity: all core data operations succeed      (Prompt 4.b)
#   AC-F-3  failing health expectations block promotion and are reported    (Prompt 4.c)
#   AC-F-4  curated dataset projects to objects/links with markings propagated (Prompt 4.c)
#   AC-F-5  AI-suggested transform is a preview diff, not committed w/o approval (Prompt 4.d)
#   EV-A6-* NL-to-transform: valid transform, invented-column caught, always a preview (Prompt 4.d)
#
# For each criterion this runs the test(s) that exercise it and reports PASS/FAIL
# with real output. Fails closed. SAD_01 §17, BUILD_PLAYBOOK §4.4/§4.5.
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates AC-F-7 concretely: the whole
# ingest -> version -> build -> project path completes with no outbound network.
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

# --- AC-F-1: connector sync -> PQ-sealed raw dataset ---
run_ac "AC-F-1" "connector sync creates a raw dataset encrypted with a PQ-wrapped key" \
    cargo test --offline -p forge-pipeline $FIPS --test ac_forge_4b -- \
    ac_f_1_connector_sync_creates_a_pq_sealed_raw_dataset

# --- AC-F-2: reproducible rebuild (content-identical) ---
ac_f_2() {
    cargo test --offline -p forge-pipeline $FIPS --test ac_forge_4b -- \
        ac_f_2_pipeline_rebuild_is_content_identical &&
        cargo test --offline -p forge-dataset-store $FIPS --test versioning
}
run_ac "AC-F-2" "a pipeline built twice from the same inputs is content-identical" ac_f_2

# --- AC-F-3: health gate blocks promotion on failure ---
run_ac "AC-F-3" "failing health expectations block promotion and are reported" \
    cargo test --offline -p forge-health $FIPS --test ac_f_3

# --- AC-F-4: projection to objects/links with markings propagated ---
run_ac "AC-F-4" "curated dataset projects into object/link types with markings propagated" \
    cargo test --offline -p forge-projection $FIPS --test ac_f_4

# --- AC-F-5 + EV-A6-*: AI-assist preview-before-commit ---
run_ac "AC-F-5" "AI-suggested transform is a preview diff, not committed without approval (A.6, EV-A6-*)" \
    cargo test --offline -p forge-ai-assist --test ac_f_5

# --- AC-F-6 + AC-F-7: standalone (no Mimir) and offline ---
# The full ingest->version->build->checkout flow with no Mimir crate in the graph;
# under `docker --network none` this same run also demonstrates AC-F-7.
run_ac "AC-F-6/7" "all core data ops run standalone (no Mimir) and with no outbound connectivity" \
    cargo test --offline -p forge-pipeline $FIPS --test ac_forge_4b -- \
    ac_f_6_and_7_full_flow_standalone_and_offline

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Forge Phase 4 (AC-F-1..7, EV-A6-*)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> PHASE 4 (FORGE) DEFINITION OF DONE: MET"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
