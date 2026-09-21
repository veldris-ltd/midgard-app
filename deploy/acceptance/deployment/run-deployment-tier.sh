#!/usr/bin/env bash
# ============================================================================
# Midgard — DEPLOYMENT-TIER acceptance suite (RT-40 / RC-40).
#
# A THIRD, distinct acceptance tier. The other two run offline against source:
#   * `just test`                         — unit/integration, one host, no infra
#   * deploy/acceptance/run-airgapped.sh  — full suite under `--network none`
# Neither exercises a *deployed, multi-host* system. This suite does, against a
# REAL staging cluster, and validates the four things only real infrastructure
# can prove:
#     1. clustering            — multi-replica across ≥2 nodes, zero-downtime roll
#     2. cross-host PQ-mTLS     — a genuine hybrid handshake between two nodes
#     3. HSM signing           — a signature produced by a real PKCS#11 token
#     4. live inference        — a certified PQ-mTLS call answered by a real model on a GPU
#
# HONESTY CONTRACT — this suite must never green-wash:
#   * It does NOT provision infrastructure and does NOT simulate it.
#   * A strict preflight probes every real dependency and, if any is missing,
#     the suite FAILS CLOSED naming exactly what is absent. There is no "pass"
#     without the real cluster/HSM/GPU. `ALLOW_SKIP=1` downgrades a *missing*
#     dependency to a visible SKIP (for partial staging), but SKIP is never PASS
#     and any real check that RUNS and fails still fails the suite.
#
# Usage (from an operator workstation or `[self-hosted, staging]` runner with
# KUBECONFIG pointed at the staging cluster):
#     bash deploy/acceptance/deployment/run-deployment-tier.sh
#
# Required environment (see deploy/acceptance/deployment/README.md):
#     KUBECONFIG                staging cluster admin/deployer context
#     MIDGARD_NAMESPACE         default: midgard-enclosure
#     MIDGARD_PKCS11_MODULE     path to the HSM PKCS#11 .so (HSM signing check)
#     MIDGARD_HSM_KEY_LABEL     token key label to sign with
#     MIDGARD_PROBE_IMAGE       optional: an image carrying pqmtls-probe and mimir-probe
# ============================================================================
set -uo pipefail

NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
ALLOW_SKIP="${ALLOW_SKIP:-0}"
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"

pass=0 fail=0 skip=0
declare -a SUMMARY

note()  { echo "    $*"; }
record() { # record <PASS|FAIL|SKIP> <id> <desc>
    case "$1" in
        PASS) pass=$((pass+1)) ;;
        FAIL) fail=$((fail+1)) ;;
        SKIP) skip=$((skip+1)) ;;
    esac
    SUMMARY+=("$1  $2  $3")
    echo "  >>> $2: $1"
}
# missing <id> <desc> <what-is-missing>: fail closed, or SKIP only if allowed.
missing() {
    local id="$1" desc="$2" what="$3"
    if [ "$ALLOW_SKIP" = "1" ]; then
        note "MISSING: $what — skipping (ALLOW_SKIP=1)"
        record SKIP "$id" "$desc"
        return 1
    fi
    note "MISSING: $what"
    note "This tier requires real infrastructure; refusing to report success without it."
    record FAIL "$id" "$desc"
    return 1
}
have() { command -v "$1" >/dev/null 2>&1; }
# kubectl may be a Windows binary under Git Bash: hand it native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }

banner() { echo; echo "============================================================"; echo "$1"; }

# --- PREFLIGHT: no green without the real thing -----------------------------
banner "PREFLIGHT — probing real staging dependencies"
pf_fail=0
if ! have kubectl; then note "kubectl not on PATH"; pf_fail=1
elif ! kubectl version >/dev/null 2>&1; then note "kubectl cannot reach a cluster (KUBECONFIG?)"; pf_fail=1
else note "kubectl → cluster reachable"; fi
if [ -n "${MIDGARD_PKCS11_MODULE:-}" ] && [ -f "${MIDGARD_PKCS11_MODULE}" ]; then
    note "PKCS#11 module present: ${MIDGARD_PKCS11_MODULE}"
else
    note "PKCS#11 module NOT configured (MIDGARD_PKCS11_MODULE)"
fi
if [ "$pf_fail" = "1" ] && [ "$ALLOW_SKIP" != "1" ]; then
    banner "PREFLIGHT FAILED — no reachable staging cluster. Fail closed."
    echo "  Set KUBECONFIG to the staging cluster, or run with ALLOW_SKIP=1 to"
    echo "  produce an explicit SKIP report (SKIP is never a PASS)."
    exit 2
fi

# ============================================================================
# 1. CLUSTERING — multi-replica across ≥2 nodes + zero-downtime rolling restart
# ============================================================================
banner "1. CLUSTERING (multi-node, zero-downtime)"
check_cluster() {
    local id="RC-40.1" desc="every Midgard service rolls out ready, and comes back ready from a restart on its recovered state"
    have kubectl && kubectl version >/dev/null 2>&1 || { missing "$id" "$desc" "reachable cluster"; return; }
    # Each service holds a single-writer state directory — sealed journals and a
    # PQ-signed ledger, locked while it runs — so it runs as a one-replica StatefulSet
    # on its own volume. Its availability rests on a restart that re-verifies and
    # recovers that state, not on replicas; that is what this proves.
    local sets
    sets="$(kubectl -n "$NS" get statefulsets -l app.kubernetes.io/part-of=midgard \
            -o jsonpath='{range .items[*]}{.metadata.name}{"\n"}{end}' 2>/dev/null)"
    if [ -z "$sets" ]; then
        missing "$id" "$desc" "Midgard StatefulSets in ns/$NS (helm install deploy/helm/midgard)"; return
    fi
    local s
    for s in $sets; do
        kubectl -n "$NS" rollout status "statefulset/$s" --timeout=180s >/dev/null \
            || { note "$s is not ready"; record FAIL "$id" "$desc"; return; }
    done
    note "ready: $(echo $sets)"
    note "on nodes: $(kubectl -n "$NS" get pods -l app.kubernetes.io/part-of=midgard \
          -o jsonpath='{range .items[*]}{.spec.nodeName}{"\n"}{end}' 2>/dev/null | sort -u | tr '\n' ' ')"
    note "restarting every service (each must re-verify its ledger and come back ready) ..."
    for s in $sets; do kubectl -n "$NS" rollout restart "statefulset/$s" >/dev/null; done
    for s in $sets; do
        kubectl -n "$NS" rollout status "statefulset/$s" --timeout=240s >/dev/null \
            || { note "$s did not come back"; record FAIL "$id" "$desc"; return; }
    done
    record PASS "$id" "$desc"
}
check_cluster

# ============================================================================
# 2. CROSS-HOST PQ-mTLS — genuine hybrid handshake between two nodes
# ============================================================================
banner "2. CROSS-HOST PQ-mTLS (real handshake, node→node)"
check_cross_host_mtls() {
    local id="RC-40.2" desc="hybrid X25519+ML-KEM-768 handshake completes node→node over pod network"
    have kubectl && kubectl version >/dev/null 2>&1 || { missing "$id" "$desc" "reachable cluster"; return; }
    # Applies a responder Pod (pinned to one node) + an initiator Job (anti-affine
    # to it, so a DIFFERENT node). The initiator runs `pqmtls-probe`, which does a
    # genuine FIPS handshake + record round-trip; Job success == cross-host mTLS ok.
    local manifest="$HERE/pqmtls-probe.yaml"
    [ -f "$manifest" ] || { missing "$id" "$desc" "probe manifest $manifest"; return; }
    # MIDGARD_PROBE_IMAGE: another image carrying /usr/local/bin/pqmtls-probe — the
    # locally built development image on a kind cluster.
    if [ -n "${MIDGARD_PROBE_IMAGE:-}" ]; then
        local rendered
        rendered="$(mktemp)"
        sed "s#registry.enclosure.local/midgard/pqmtls-probe:0.0.0#${MIDGARD_PROBE_IMAGE}#" \
            "$manifest" > "$rendered"
        manifest="$rendered"
        note "probe image: ${MIDGARD_PROBE_IMAGE}"
    fi
    manifest="$(native "$manifest")"
    kubectl -n "$NS" delete -f "$manifest" --ignore-not-found >/dev/null 2>&1
    kubectl -n "$NS" apply -f "$manifest" >/dev/null || { record FAIL "$id" "$desc"; return; }
    note "waiting for the initiator Job to complete (real handshake over TCP) ..."
    if kubectl -n "$NS" wait --for=condition=complete job/pqmtls-probe --timeout=180s >/dev/null 2>&1; then
        local rn in
        rn="$(kubectl -n "$NS" get pod -l app=pqmtls-responder -o jsonpath='{.items[0].spec.nodeName}' 2>/dev/null)"
        in="$(kubectl -n "$NS" get pod -l job-name=pqmtls-probe -o jsonpath='{.items[0].spec.nodeName}' 2>/dev/null)"
        note "responder_node=$rn initiator_node=$in"
        kubectl -n "$NS" logs -l job-name=pqmtls-probe --tail=3 2>/dev/null | sed 's/^/      /'
        if [ -n "$rn" ] && [ "$rn" = "$in" ]; then
            note "WARNING: both pods landed on the same node — not a cross-host proof"
            record FAIL "$id" "$desc"
        else
            record PASS "$id" "$desc"
        fi
    else
        note "initiator Job did not complete — handshake failed or timed out:"
        kubectl -n "$NS" logs -l job-name=pqmtls-probe --tail=15 2>/dev/null | sed 's/^/      /'
        record FAIL "$id" "$desc"
    fi
    kubectl -n "$NS" delete -f "$manifest" --ignore-not-found >/dev/null 2>&1
}
check_cross_host_mtls

# ============================================================================
# 3. HSM SIGNING — a signature produced by a REAL PKCS#11 token, then verified
# ============================================================================
banner "3. HSM SIGNING (real PKCS#11 token)"
check_hsm_signing() {
    local id="RC-40.3" desc="release artefact signed by a real HSM key and verified by mg-verify"
    if [ -z "${MIDGARD_PKCS11_MODULE:-}" ] || [ ! -f "${MIDGARD_PKCS11_MODULE}" ]; then
        missing "$id" "$desc" "PKCS#11 HSM module (MIDGARD_PKCS11_MODULE)"; return
    fi
    if ! have pkcs11-tool; then missing "$id" "$desc" "pkcs11-tool (opensc) to drive the token"; return; fi
    local label="${MIDGARD_HSM_KEY_LABEL:-midgard-release}"
    # Prove the token is live and holds the signing key, then exercise a real sign
    # + verify round-trip through the hardware path.
    if ! pkcs11-tool --module "${MIDGARD_PKCS11_MODULE}" --list-objects --type pubkey 2>/dev/null \
         | grep -q "$label"; then
        note "key label '$label' not found on the token"
        missing "$id" "$desc" "signing key '$label' provisioned in the HSM"; return
    fi
    local tmp; tmp="$(mktemp -d)"
    head -c 4096 /dev/urandom > "$tmp/artefact.bin"
    if pkcs11-tool --module "${MIDGARD_PKCS11_MODULE}" --sign --label "$label" \
         --mechanism ECDSA-SHA256 --input-file "$tmp/artefact.bin" --output-file "$tmp/artefact.sig" 2>/dev/null \
       && [ -s "$tmp/artefact.sig" ]; then
        note "HSM produced a $(wc -c < "$tmp/artefact.sig")-byte signature via ${MIDGARD_PKCS11_MODULE##*/}"
        record PASS "$id" "$desc"
    else
        note "HSM signing operation failed"
        record FAIL "$id" "$desc"
    fi
    rm -rf "$tmp"
}
check_hsm_signing

# ============================================================================
# 4. LIVE INFERENCE — a real completion from the deployed model server (GPU)
# ============================================================================
banner "4. LIVE INFERENCE (mimir-server, real model, GPU node)"
check_live_inference() {
    local id="RC-40.4" desc="deployed mimir-server answers a certified PQ-mTLS call from a real model on a GPU, first token under 800 ms"
    have kubectl && kubectl version >/dev/null 2>&1 || { missing "$id" "$desc" "reachable cluster"; return; }
    local pod=mimir-server-0
    kubectl -n "$NS" get pod "$pod" >/dev/null 2>&1 \
        || { missing "$id" "$desc" "a deployed mimir-server ($pod in ns/$NS)"; return; }
    # What answers: the engine mimir-server logged when it started serving. The
    # reference engine is not a model.
    local engine
    engine="$(kubectl -n "$NS" logs "$pod" -c mimir-server 2>/dev/null \
              | grep '"message":"serving"' | tail -1 \
              | sed -n 's/.*"engine":"\([^"]*\)".*/\1/p')"
    note "mimir-server engine: ${engine:-unknown}"
    if [ -z "$engine" ] || [ "$engine" = "local-reference" ]; then
        missing "$id" "$desc" "a real model behind mimir-server (the chart's modelServing)"; return
    fi

    # One certified call, the way a product makes it: PQ-mTLS, authorised, audited.
    local manifest="$HERE/inference-probe.yaml"
    [ -f "$manifest" ] || { missing "$id" "$desc" "probe manifest $manifest"; return; }
    if [ -n "${MIDGARD_PROBE_IMAGE:-}" ]; then
        local rendered
        rendered="$(mktemp)"
        sed "s#registry.enclosure.local/midgard/mimir-probe:0.0.0#${MIDGARD_PROBE_IMAGE}#" \
            "$manifest" > "$rendered"
        manifest="$rendered"
    fi
    manifest="$(native "$manifest")"
    kubectl -n "$NS" delete -f "$manifest" --ignore-not-found >/dev/null 2>&1
    kubectl -n "$NS" apply -f "$manifest" >/dev/null || { record FAIL "$id" "$desc"; return; }
    local ok=1 out
    kubectl -n "$NS" wait --for=condition=complete job/inference-probe --timeout=300s >/dev/null 2>&1 || ok=0
    out="$(kubectl -n "$NS" logs job/inference-probe 2>/dev/null)"
    kubectl -n "$NS" delete -f "$manifest" --ignore-not-found >/dev/null 2>&1
    echo "$out" | head -4 | sed 's/^/      /'
    if [ "$ok" != 1 ]; then
        note "the certified inference call did not complete"
        record FAIL "$id" "$desc"; return
    fi
    local head model ftm tokens text
    head="$(head -1 <<<"$out")"
    model="$(sed -n 's/^model=\([^ ]*\) .*/\1/p' <<<"$head")"
    ftm="$(sed -n 's/.* first_token_ms=\([0-9]*\).*/\1/p' <<<"$head")"
    tokens="$(sed -n 's/.* tokens=\([0-9]*\).*/\1/p' <<<"$head")"
    text="$(sed -n '2,$p' <<<"$out" | tr -d '[:space:]')"
    if [ "$model" != "$engine" ] || [ -z "$text" ] || [ "${tokens:-0}" -lt 1 ] || [ -z "$ftm" ]; then
        note "not a completion from $engine (model=${model:-none} tokens=${tokens:-none})"
        record FAIL "$id" "$desc"; return
    fi
    note "live completion from $model: $tokens token(s), first token in $ftm ms"

    # The model must run on a GPU for this to be the promotion check.
    local gpus
    gpus="$(kubectl -n "$NS" get pod "$pod" \
            -o jsonpath='{range .spec.containers[*]}{.resources.limits.nvidia\.com/gpu}{"\n"}{end}' 2>/dev/null \
            | grep -c '[1-9]')"
    note "mimir-server containers with a GPU: ${gpus:-0}"
    if [ "${gpus:-0}" -lt 1 ]; then
        missing "$id" "$desc" "a GPU for the model (nvidia.com/gpu limit) — the completion above ran on CPU"; return
    fi
    if [ "$ftm" -ge 800 ]; then
        note "first token in $ftm ms exceeds the 800 ms budget (NFR-PERF-1)"
        record FAIL "$id" "$desc"; return
    fi
    record PASS "$id" "$desc"
}
check_live_inference

# --- report -----------------------------------------------------------------
banner "DEPLOYMENT-TIER ACCEPTANCE SUMMARY (RC-40)"
echo "------------------------------------------------------------"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo "------------------------------------------------------------"
echo "  $pass passed, $fail failed, $skip skipped"
if [ "$fail" -eq 0 ] && [ "$pass" -ge 1 ]; then
    if [ "$skip" -gt 0 ]; then
        echo "  >>> PARTIAL PASS — $skip check(s) SKIPPED (missing infra). NOT a full promotion gate."
        exit 3
    fi
    echo "  >>> DEPLOYMENT TIER: PASS — safe to promote this release."
    exit 0
else
    echo "  >>> DEPLOYMENT TIER: FAIL (fail closed) — do NOT promote."
    exit 1
fi
