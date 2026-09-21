#!/usr/bin/env bash
# ============================================================================
# Admin Control Panel — acceptance verification (Phase 7).
#
# Definition of Done: the full AC-AD-1..13 matrix.
#   AC-AD-1   MFA on sign-in; step-up re-auth on entering an admin scope   (Prompt 7.b)
#   AC-AD-2   no role grants all capabilities; Auditor is read-only        (Prompt 7.b)
#   AC-AD-3   high-risk action needs a second, distinct approver           (Prompt 7.b)
#   AC-AD-4   self-approval denied by separation-of-duties                 (Prompt 7.b)
#   AC-AD-5   policy/access change is simulated exactly before commit      (Prompt 7.c)
#   AC-AD-6   every action (incl. denied + simulated) is PQ-signed audited (Prompt 7.c)
#   AC-AD-7   config is versioned with diff + rollback                     (Prompt 7.c)
#   AC-AD-8   break-glass: time-boxed, justified, alerted, forced review   (Prompt 7.c)
#   AC-AD-9   distinct least-privilege service identity per subsystem      (Prompt 7.d)
#   AC-AD-10  data plane authorises without the admin panel               (Prompt 7.c)
#   AC-AD-11  advisory copilot cannot execute; action stays on guarded path(Prompt 7.d)
#   AC-AD-12  break-glass + audit work air-gapped                         (Prompt 7.c)
#   AC-AD-13  panel privilege-logic change needs security-officer control (Prompt 7.b)
#
# For each criterion this runs the test(s) that exercise it and reports PASS/FAIL
# with real output. Fails closed. SAD_07 §17.
#
# Run inside the pinned toolchain (rust:1.82). Running it under
# `docker --network none` additionally demonstrates AC-AD-12 concretely: the whole
# simulate -> version -> audit -> break-glass path completes with no outbound
# network.
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

# --- AC-AD-1: MFA sign-in + step-up on scope entry ---
run_ac "AC-AD-1" "MFA on sign-in; step-up re-auth on entering an admin scope" \
    cargo test --offline -p admin-identity --test ac_ad_1_2 -- ac_ad_1

# --- AC-AD-2: role separation + read-only auditor + scoped/expiring bindings ---
run_ac "AC-AD-2" "no role grants all capabilities; Auditor strictly read-only" \
    cargo test --offline -p admin-identity --test ac_ad_1_2 -- ac_ad_2

# --- AC-AD-3: high-risk action needs a second distinct approver ---
run_ac "AC-AD-3" "a high-risk action needs a second, distinct approver" \
    cargo test --offline -p admin-privilege $FIPS --test ac_ad_3_4_13 -- ac_ad_3

# --- AC-AD-4: separation-of-duties denies self-approval ---
run_ac "AC-AD-4" "a requester cannot approve their own action" \
    cargo test --offline -p admin-privilege $FIPS --test ac_ad_3_4_13 -- ac_ad_4

# --- AC-AD-5: policy simulation shows exact effect before commit ---
run_ac "AC-AD-5" "a policy/access change is simulated exactly before any commit" \
    cargo test --offline -p admin-sim --test ac_ad_5

# --- AC-AD-6: every admin action (incl. denied + simulated) is PQ-signed audited ---
run_ac "AC-AD-6" "every action incl. denied + simulated writes a PQ-signed audit entry" \
    cargo test --offline -p admin-audit $FIPS --test ac_ad_6_10_12 -- ac_ad_6

# --- AC-AD-7: versioned config with diff + rollback ---
run_ac "AC-AD-7" "configuration is versioned with a diff and can be rolled back" \
    cargo test --offline -p admin-config --test ac_ad_7

# --- AC-AD-8: time-boxed, justified, alerted break-glass with forced review ---
run_ac "AC-AD-8" "break-glass is time-boxed, justified, alerted, and forces review" \
    cargo test --offline -p admin-breakglass --test ac_ad_8

# --- AC-AD-9: distinct least-privilege service identity per subsystem ---
run_ac "AC-AD-9" "the panel uses a distinct least-privilege identity per subsystem" \
    cargo test --offline -p admin-service-identity --test ac_ad_9

# --- AC-AD-10: data plane authorises without the admin panel ---
run_ac "AC-AD-10" "the data plane keeps authorising while the panel is absent" \
    cargo test --offline -p admin-audit $FIPS --test ac_ad_6_10_12 -- ac_ad_10

# --- AC-AD-11: advisory copilot cannot execute; action stays on the guarded path ---
run_ac "AC-AD-11" "the copilot proposes but cannot execute; the guarded path still applies" \
    cargo test --offline -p admin-ai-assist --test ac_ad_11

# --- AC-AD-12: break-glass + audit run air-gapped ---
run_ac "AC-AD-12" "break-glass and audit function fully offline" \
    cargo test --offline -p admin-audit $FIPS --test ac_ad_6_10_12 -- ac_ad_12

# --- AC-AD-13: panel privilege-logic change needs security-officer dual-control ---
run_ac "AC-AD-13" "a panel privilege-logic change requires a security-officer approver" \
    cargo test --offline -p admin-privilege $FIPS --test ac_ad_3_4_13 -- ac_ad_13

echo
echo "============================================================"
echo "ACCEPTANCE SUMMARY — Admin Control Panel (AC-AD-1..13)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed"
if [ "$fail" -eq 0 ]; then
    echo "  >>> ADMIN PANEL (Phase 7, AC-AD-1..13) DEFINITION OF DONE: MET"
    exit 0
else
    echo "  >>> DEFINITION OF DONE: NOT MET (fail closed)"
    exit 1
fi
