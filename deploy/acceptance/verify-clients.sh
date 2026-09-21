#!/usr/bin/env bash
# ============================================================================
# Client Platforms — acceptance verification (Phase 8, complete).
#
# Prompts 8.b–8.d land the shared core, tiered inference, auth, the shells, the
# minimal-privilege extension broker, and signed delivery. This script maps the
# FULL Phase-8 Definition of Done (AC-C-1..13) to the tests that exercise it and
# reports PASS/FAIL with real output. Fails closed. SAD_06 §17, BUILD_PLAYBOOK
# §8.4/§8.5.
#
#   AC-C-1  one shared core: crypto/sync/storage from a single linked crate  (8.b)
#   AC-C-2  local data AES-256-GCM under a non-exportable keystore key        (8.b)
#   AC-C-3  client-to-enclosure transport is hybrid PQ-mTLS, fails closed     (8.b)
#   AC-C-4  offline CRDT edits merge conflict-free with no data loss          (8.b)
#   AC-C-13 above-clearance data is never cached or rendered client-side      (8.b)
#   AC-C-5  capable device offline runs Tier-1; weak device disables cleanly  (8.c)
#   AC-C-6  connected client delegates to Mimir, grounded/cited/audited       (8.c)
#   AC-C-9  biometric/WebAuthn auth; no secret leaves the secure boundary     (8.c)
#   AC-C-10 on-device model PQ-signature verified on load, fail closed        (8.c)
#   AC-C-11 shells declare complete HIG/accessibility conformance (static)    (8.c)
#   AC-C-7  artefacts PQ-signed via Bifrost; verified before install/update   (8.d)
#   AC-C-8  extension broker holds no keys; brokers privileged ops            (8.d)
#   AC-C-12 air-gapped install from a signed offline bundle, no store/net     (8.d)
#   Evals   EV-AC1-*, EV-AC2-*, EV-AC3-*, EV-AC-parity                        (8.b–d)
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates the air-gap posture: the whole
# core/inference/broker/delivery path completes with no outbound network.
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

# --- AC-C-1: one shared core (crypto + sync + storage through one crate) ---
run_ac "AC-C-1" "one shared core links crypto/sync/storage with no per-shell reimplementation" \
    cargo test --offline -p midgard-client-core $FIPS --test one_core

# --- AC-C-2: at-rest seal under a non-exportable, ML-KEM-1024-wrapped keystore key ---
ac_c_2() {
    cargo test --offline -p midgard-client-store $FIPS --test at_rest_and_clearance -- \
        round_trips_and_seals_at_rest brokered_custody_holds_no_local_key &&
        cargo test --offline -p midgard-client-store $FIPS --lib &&
        cargo test --offline -p midgard-client-keystore $FIPS --lib
}
run_ac "AC-C-2" "local data is AES-256-GCM under a non-exportable keystore key (or brokered)" ac_c_2

# --- AC-C-3: hybrid PQ-mTLS transport, fails closed ---
run_ac "AC-C-3" "client-to-enclosure transport is hybrid PQ-mTLS and fails closed on PQ failure" \
    cargo test --offline -p midgard-client-enclosure --features test-backend --test connect

# --- AC-C-4: offline CRDT sync, conflict-free, no data loss ---
run_ac "AC-C-4" "offline edits are CRDT ops that merge conflict-free with no data loss" \
    cargo test --offline -p midgard-client-sync --test convergence

# --- AC-C-13: above-clearance data never cached or rendered ---
ac_c_13() {
    cargo test --offline -p midgard-client-store $FIPS --test at_rest_and_clearance -- \
        above_clearance_is_never_cached_or_rendered missing_compartment_is_above_clearance &&
        cargo test --offline -p midgard-client-core $FIPS --test one_core -- \
        above_clearance_edit_is_refused_by_the_core
}
run_ac "AC-C-13" "data above the user's clearance is neither displayed nor cached client-side" ac_c_13

# --- AC-C-5: tiered inference (capable-offline Tier 1 / graceful disable) ---
run_ac "AC-C-5" "capable device offline runs Tier-1; a weak device disables gracefully" \
    cargo test --offline -p midgard-client-inference --features test-backend --test tiering -- \
    capable_device_offline_runs_tier_1 weak_device_offline_disables_gracefully

# --- AC-C-6: remote delegation grounded/cited/audited ---
ac_c_6() {
    cargo test --offline -p midgard-client-inference --features test-backend --test tiering -- \
        connected_heavy_task_delegates_remote_grounded_and_cited &&
        cargo test --offline -p midgard-client-inference --features test-backend --test model_and_audit -- \
        a_served_request_seals_a_verifiable_audit_entry
}
run_ac "AC-C-6" "connected client delegates to enclosure Mimir, grounded/cited/audited" ac_c_6

# --- AC-C-9: biometric/WebAuthn auth, no secret leaves the boundary ---
run_ac "AC-C-9" "biometric/WebAuthn auth; no secret leaves the secure boundary" \
    cargo test --offline -p midgard-client-keystore --features backend-fips --test auth

# --- AC-C-10: on-device model PQ-signature verified on load, fail closed ---
run_ac "AC-C-10" "on-device model PQ-signature verified on load, fail closed" \
    cargo test --offline -p midgard-client-inference --features test-backend --test model_and_audit -- \
    a_correctly_signed_model_loads a_tampered_model_fails_closed a_model_signed_by_the_wrong_key_fails_closed

# --- AC-C-11: shells declare complete HIG/accessibility conformance (static) ---
run_ac "AC-C-11" "all six shells declare complete HIG/accessibility conformance and bind the core" \
    bash deploy/acceptance/verify-shells-accessibility.sh

# --- AC-C-7: artefacts PQ-signed via Bifrost; verified before install/update ---
run_ac "AC-C-7" "client artefacts are PQ-signed and verified before install/update (fail closed)" \
    cargo test --offline -p midgard-client-delivery --features backend-fips --test install -- \
    a_signed_client_artefact_installs a_tampered_artefact_fails_closed_before_install \
    an_artefact_signed_by_an_untrusted_key_fails_closed an_update_must_target_the_installed_artefact

# --- AC-C-8: extension broker holds no keys; brokers privileged ops ---
run_ac "AC-C-8" "extension broker holds no raw keys, reads only shared selection, brokers privileged ops" \
    cargo test --offline -p midgard-client-broker --test broker

# --- AC-C-12: air-gapped install from a signed offline bundle, no store/net ---
run_ac "AC-C-12" "air-gapped install from a signed offline bundle with no store/internet dependency" \
    cargo test --offline -p midgard-client-delivery --features backend-fips --test install -- \
    air_gapped_install_from_a_signed_bundle_offline a_tampered_bundle_fails_closed_offline

# --- Evals: the full client-prompt suite (EV-AC1-*, EV-AC2-*, EV-AC3-*, parity) ---
evals() {
    cargo test --offline -p midgard-client-inference --features test-backend --test tiering -- \
        ev_ac1_offline_and_clearance ev_ac2_privacy_keeps_sensitive_on_device \
        ev_ac2_toobig_online_remote_offline_disable ev_ac2_policy_never_egresses_under_no_egress \
        ev_ac_parity_tier1_matches_tier2 &&
        cargo test --offline -p midgard-client-broker --test broker -- \
        a_plain_task_uses_only_the_shared_selection a_privileged_task_is_brokered_not_performed \
        noscrape_eval_catches_a_leak nosecret_eval_catches_a_leak
}
run_ac "EV-AC*" "EV-AC1-*, EV-AC2-*, EV-AC3-*, and EV-AC-parity all pass" evals

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Client Platforms Phase 8 (AC-C-1..13 + Evals)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> PHASE 8 (CLIENT PLATFORMS) DEFINITION OF DONE: MET"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
