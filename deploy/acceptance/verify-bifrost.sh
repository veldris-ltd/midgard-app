#!/usr/bin/env bash
# ============================================================================
# Bifrost — acceptance verification (Phase 5).
#
# The Phase-5 Definition of Done: AC-B-1..6 and the A.7 Eval suite EV-A7-*.
#   AC-B-1  unsigned/tampered artefact rejected before deploy (fail closed)   (Prompt 5.b)
#   AC-B-4  published release has a verifiable CycloneDX SBOM + ML-DSA sig     (Prompt 5.b)
#   AC-B-2  ring health-gate failure → automatic rollback, no downtime        (Prompt 5.b)
#   AC-B-3  air-gapped site verifies a signed bundle offline before deploy    (Prompt 5.c)
#   AC-B-5  Mimir present: a risk score is produced and can gate the rollout  (Prompt 5.d)
#   AC-B-6  Mimir absent: rule-based gate still enforces safe rollout          (Prompt 5.d)
#   EV-A7-* release-risk: critical-CVE flagged, clean not over-flagged, evidence-backed (Prompt 5.d)
#
# Closes forward links (verified at the Bifrost boundary):
#   AC-MS-3  Mimir model bundle transits the signed-bundle workflow, rejected on bad sig (Prompt 5.c)
#   AC-C-7   client artefacts transit the same workflow                        (Prompt 5.c)
#
# AC-MS-3 is a criterion in its own right, so it gets a verdict of its own here rather
# than a remark under AC-B-3's: the gate record (tools/ci/gate-record.sh) accounts for
# every criterion by its verdict line, and refuses a run in which one has none.
#
# For each criterion this runs the test(s) that exercise it and reports PASS/FAIL
# with real output. Fails closed. SAD_03 §17, BUILD_PLAYBOOK §5.4/§5.5.
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates the air-gap posture: signing,
# verification, rollout, and offline bundle import all complete with no network.
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

# --- AC-B-1 + AC-B-4: signed artefacts, fail-closed verify, SBOM attestation ---
run_ac "AC-B-1/4" "unsigned/tampered artefact rejected; published release has verifiable SBOM + ML-DSA signature" \
    cargo test --offline -p bifrost-artefact $FIPS --test registry

# --- AC-B-2: ring health-gate failure → automatic zero-downtime rollback ---
run_ac "AC-B-2" "ring health-gate failure triggers automatic rollback with no downtime" \
    cargo test --offline -p bifrost-rollout --test rollout

# --- AC-B-3 (+AC-MS-3/AC-C-7): offline signed-bundle import verifies before deploy ---
run_ac "AC-B-3" "air-gapped site verifies a signed bundle offline before deploy (model+client links)" \
    cargo test --offline -p bifrost-bundle $FIPS --test ac_b_3

# --- AC-MS-3: a Mimir model bundle transits the same signed-bundle workflow ---
# One named test. A filter that matched nothing would exit 0 having run nothing, so the
# step also requires that exactly one test ran and passed.
ac_ms_3() {
    local out
    out="$(cargo test --offline -p bifrost-bundle $FIPS --test ac_b_3 -- --exact \
        ac_ms_3_and_c_7_model_and_client_bundles_transit_the_same_workflow 2>&1)" || {
        echo "$out"
        return 1
    }
    echo "$out"
    echo "$out" | grep -Eq '^test result: ok\. 1 passed; 0 failed'
}
run_ac "AC-MS-3" "a Mimir model bundle transits the signed-bundle workflow and is rejected on a bad signature" \
    ac_ms_3

# --- AC-B-5 + AC-B-6 + EV-A7: advisory risk score + deterministic gate ---
run_ac "AC-B-5/6" "Mimir-present risk score can gate; Mimir-absent rule-based gate still enforces safe rollout (A.7, EV-A7-*)" \
    cargo test --offline -p bifrost-risk --test ac_b_5_6

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Bifrost Phase 5 (AC-B-1..6, AC-MS-3, EV-A7-*)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> PHASE 5 (BIFROST) DEFINITION OF DONE: MET"
    echo "  >>> Forward link AC-C-7 (client artefacts) is verified with the client platforms, in verify-clients.sh"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
