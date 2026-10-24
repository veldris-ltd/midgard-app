#!/usr/bin/env bash
# ============================================================================
# Mimir — acceptance verification (Phase 2, growing per prompt).
#
# The Phase-2 Definition of Done: AC-MI-1..8, FR-MI-4 (builders), and the full
# prompt-level Eval suites EV-A1..A5 (per id, EV-A3/A4/A5 also per model family).
#   AC-MI-1  local response, no hosted-model dependency          (Prompt 2.b)
#   AC-MI-8  consumer calls inference over PQ-mTLS, audited       (Prompt 2.b)
#   AC-MI-2  answers cite object-fabric evidence (OAG grounding)  (Prompt 2.c)
#   AC-MI-3  mutations Evals-checked + human-gated (governed loop, INV-1..8, EV-A1-*) (Prompt 2.d)
#   AC-MI-6  prompt-injection blocked before any tool executes (self-defending A.3, EV-A3-*) (Prompt 2.e)
#   AC-MI-5  failed/degraded execution retried/rerouted/rolled back (self-healing A.4, EV-A4-*) (Prompt 2.e)
#   AC-MI-4  governed feedback -> retrieval/prompt/policy updates, never weights (self-learning A.5, EV-A5-*) (Prompt 2.e)
#   AC-MI-7  model-artefact verify-on-load: signature+licence+hardware, fail closed (Prompt 2.f)
#   FR-MI-4  no-code (Logic) + pro-code builders over the same agent contract    (Prompt 2.f)
#
# Phase 3 (Model Supply, SAD_02b) is added here as it lands, prompt by prompt:
#   AC-MS-1  staged model: native provenance verified where available, CycloneDX
#            SBOM generated, licence captured, re-signed under Veldris ML-DSA (Prompt 3.a)
#   EV-A9-*  intake licence/risk assessor: restriction capture, unknown-marking,
#            no verdict (advisory)                                              (Prompt 3.a)
#   AC-MS-2  registry verify-on-load: ML-DSA sig verified, fail closed (Prompt 3.b)
#   AC-MS-5  hardware eligibility: offer largest eligible variant       (Prompt 3.b)
#   AC-MS-6  licence-policy-unmet refused exactly as revoked            (Prompt 3.b)
#   AC-MS-7  revoked fails closed regardless of signature validity      (Prompt 3.b)
#   AC-MS-4  4 families behind the unchanged API; router picks eligible  (Prompt 3.c)
#   AC-MS-8  no outbound connectivity: served locally from the registry  (Prompt 3.c)
#   AC-MS-9  cross-family swap hot-swappable, no contract/data change    (Prompt 3.c)
#
# For each criterion this runs the test(s) that exercise it and reports PASS/FAIL
# with real output. Fails closed. SAD_02 §17, BUILD_PLAYBOOK §2.4/§2.5.
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates AC-MI-1 concretely: the local
# inference path completes with no outbound connectivity.
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

# --- AC-MI-1: local response, no hosted dependency (also proves serving) ---
run_ac "AC-MI-1" "Mistral/local engine responds locally; no hosted-model dependency" \
    cargo test --offline -p model-server $FIPS --test serving -- \
    ac_mi_1_serves_locally_no_hosted_dependency autonomous_selection_by_hardware \
    first_token_latency_under_budget

# --- AC-MI-8: consumer over PQ-mTLS, audited every call ---
run_ac "AC-MI-8" "consumer calls inference over PQ-mTLS and every call is audited" \
    cargo test --offline -p model-server $FIPS --test inference_pqmtls

# --- AC-MI-2: OAG-grounded answers cite object-fabric evidence ---
ac_mi_2() {
    cargo test --offline -p oag-retriever $FIPS --test grounding &&
        cargo test --offline -p agent-runtime $FIPS --test ac_mi_2
}
run_ac "AC-MI-2" "OAG answers cite fabric evidence; ungroundable tasks refuse" ac_mi_2

# --- AC-MI-3: governed loop — mutations Evals-checked + human-gated ---
ac_mi_3() {
    cargo test --offline -p agent-runtime $FIPS --test ac_mi_3 &&
        cargo test --offline -p agent-runtime $FIPS --test ev_a1
}
run_ac "AC-MI-3" "mutations proposed, Evals-checked, human-gated unless pre-authorised (EV-A1-*)" ac_mi_3

# --- AC-MI-6: self-defending — injection blocked before any tool executes ---
run_ac "AC-MI-6" "prompt-injection detected + blocked before any tool executes (A.3, EV-A3-*)" \
    cargo test --offline -p agent-runtime $FIPS --test self_defending

# --- AC-MI-5: self-healing — retry/reroute/rollback within budget ---
run_ac "AC-MI-5" "failed/degraded execution auto-recovers within budget (A.4, EV-A4-*)" \
    cargo test --offline -p agent-runtime $FIPS --test self_healing

# --- AC-MI-4: self-learning — governed feedback, never weights ---
run_ac "AC-MI-4" "governed feedback -> retrieval/prompt/policy updates w/ provenance, never weights (A.5, EV-A5-*)" \
    cargo test --offline -p agent-runtime $FIPS --test self_learning

# --- AC-MI-7: model-artefact verify-on-load, fail closed ---
run_ac "AC-MI-7" "model weights load only if ML-DSA sig + licence + hardware verify; fail closed" \
    cargo test --offline -p model-server $FIPS --test artifact

# --- FR-MI-4: no-code Logic + pro-code builders over the same contract ---
run_ac "FR-MI-4" "no-code Logic + pro-code builders yield the same agent contract/loop" \
    cargo test --offline -p agent-runtime $FIPS --test builders

# --- Per-model-family certification of the self-* prompt evals ---
run_ac "EV-per-family" "EV-A3/A4/A5 certified for Mistral, Nemotron, Gemma and Granite" \
    cargo test --offline -p agent-runtime $FIPS --test per_family

# --- Phase 3 (Model Supply, SAD_02b) — grows per prompt -------------------

# --- AC-MS-1: staging intake — provenance verified, SBOM, licence, re-sign ---
run_ac "AC-MS-1" "staged model: provenance verified where available, CycloneDX SBOM, licence captured, re-signed under Veldris ML-DSA (Prompt 3.a)" \
    cargo test --offline -p staging $FIPS --test ac_ms_1

# --- EV-A9-*: intake licence/risk assessor (advisory, never approves) ---
run_ac "EV-A9" "intake assessor: custom-terms capture, unknown-marking, no approve/reject verdict (A.9)" \
    cargo test --offline -p staging $FIPS --test ev_a9

# --- AC-MS-2/5/6/7: Model Registry — verify-on-load, hardware eligibility, ---
# --- revocation & licence fail-closed (system of record) ---
run_ac "AC-MS-2/5/6/7" "Model Registry: every load verifies ML-DSA sig+licence+hardware, offers largest eligible variant, fails closed on revoked/licence-unmet (Prompt 3.b)" \
    cargo test --offline -p model-server $FIPS --test model_registry

# --- AC-MS-4/8/9: Model Abstraction Layer — adapters + router, unchanged API --
# Runs offline; under `docker --network none` this also demonstrates AC-MS-8's
# no-outbound-dependency claim concretely (served from the local registry).
run_ac "AC-MS-4/8/9" "4 families behind the unchanged contract; router selects by workload+hardware+policy; local-only serving; cross-family hot-swap (Prompt 3.c)" \
    cargo test --offline -p model-server $FIPS --test router

# --- EV-A1..A5: prompt-level Evals, reported per id ---
echo
echo "============================================================"
echo "EV-A1..A5 — prompt-level Evals (per id)"
echo "  \$ cargo test -p agent-runtime --test consolidated -- --nocapture"
ev_out="$(cargo test --offline -p agent-runtime $FIPS --test consolidated -- --nocapture 2>&1)"
echo "$ev_out" | grep -E 'EV-A[0-9]|evals passed' | sed 's/^/  /'
if echo "$ev_out" | grep -q 'test result: ok'; then
    echo "  >>> EV-A1..A5: PASS"
    pass=$((pass + 1))
    SUMMARY+=("EV-A1..A5  PASS  prompt-level Eval suites (21 evals)")
else
    echo "  >>> EV-A1..A5: FAIL"
    echo "$ev_out" | tail -20 | sed 's/^/    /'
    fail=$((fail + 1))
    SUMMARY+=("EV-A1..A5  FAIL  prompt-level Eval suites")
fi

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Mimir Phase 2 (AC-MI-1..8, FR-MI-4, EV-A1..A5) + Phase 3 (AC-MS-1/2/4/5/6/7/8/9, EV-A9)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> PHASE 2 (MIMIR) DEFINITION OF DONE: MET"
    echo "  >>> PHASE 3 (MODEL SUPPLY) DEFINITION OF DONE: MET (AC-MS-3 air-gap transit deferred to Phase 5 / Bifrost)"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
