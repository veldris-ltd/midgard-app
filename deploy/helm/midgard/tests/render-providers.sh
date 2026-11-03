#!/usr/bin/env bash
# Render the chart with each secrets provider and check what it gives. [NFR-SEC-1]
# [VLD-ANB-MIDGARD-0018] [AC-29.3]
#
#   bash deploy/helm/midgard/tests/render-providers.sh
#
#   csi_renders_csi_volumes                    csi mounts secrets-store.csi.k8s.io volumes
#                                              and no Secret a workload would mount
#   external_secrets_renders_one_per_workload  externalSecrets renders an ExternalSecret
#                                              for each Secret each workload mounts, and
#                                              mounts those Secret names unchanged
#   kubernetes_without_ack_fails               kubernetes outside the kind profile fails
#                                              without the etcd-encryption acknowledgement
#
# Needs helm on PATH (or HELM set).
set -uo pipefail

cd "$(dirname "$0")/../../../.."
HELM="${HELM:-helm}"
CHART=deploy/helm/midgard
command -v "${HELM%% *}" >/dev/null 2>&1 || { echo "render-providers: helm is not on PATH" >&2; exit 1; }
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}
render() { $HELM template midgard "$CHART" --namespace default "$@" 2>&1 | tr -d '\r'; }

# The workloads that hold an identity, and those that also seed state, in the values.
workloads=(http-gateway mimir-server bifrost-fleet heimdall-feed forge-worker)
seeded=(http-gateway mimir-server bifrost-fleet heimdall-feed)

# csi_renders_csi_volumes
out="$(render --set secrets.provider=csi)"
if [ "$(grep -c 'driver: secrets-store.csi.k8s.io' <<<"$out")" -eq $(( ${#workloads[@]} * 2 + ${#seeded[@]} )) ]; then
    pass "csi: every identity, passphrase and seed is a secrets-store.csi.k8s.io volume"
else
    fail "csi: expected $(( ${#workloads[@]} * 2 + ${#seeded[@]} )) CSI volumes: $(grep -c 'secrets-store.csi.k8s.io' <<<"$out")"
fi
for w in "${workloads[@]}"; do
    if grep -qE "secretName: $w-(identity|keystore-passphrase|seed)\b" <<<"$out"; then
        fail "csi: $w still mounts a Kubernetes Secret"
    fi
    grep -q "secretProviderClass: $w-keystore-passphrase" <<<"$out" \
        || fail "csi: $w has no keystore passphrase SecretProviderClass"
done
grep -q "kind: ExternalSecret" <<<"$out" && fail "csi: renders ExternalSecret objects"
pass "csi: no workload Secret is mounted"

# external_secrets_renders_one_per_workload
out="$(render --set secrets.provider=externalSecrets --set secrets.externalSecrets.storeRef.name=custody)"
for w in "${workloads[@]}"; do
    for kind in identity keystore-passphrase; do
        [ "$(grep -A3 '^kind: ExternalSecret' <<<"$out" | grep -c "^  name: $w-$kind$")" -eq 1 ] \
            || fail "externalSecrets: no single ExternalSecret for $w-$kind"
        grep -q "secretName: $w-$kind," <<<"$out" || fail "externalSecrets: $w does not mount $w-$kind"
    done
done
for w in "${seeded[@]}"; do
    [ "$(grep -A3 '^kind: ExternalSecret' <<<"$out" | grep -c "^  name: $w-seed$")" -eq 1 ] \
        || fail "externalSecrets: no single ExternalSecret for $w-seed"
done
total=$(( ${#workloads[@]} * 2 + ${#seeded[@]} ))
[ "$(grep -c '^kind: ExternalSecret' <<<"$out")" -eq "$total" ] \
    && pass "externalSecrets: one ExternalSecret per workload Secret ($total)" \
    || fail "externalSecrets: expected $total ExternalSecret objects, got $(grep -c '^kind: ExternalSecret' <<<"$out")"
grep -q "key: midgard/http-gateway-keystore-passphrase" <<<"$out" \
    && pass "externalSecrets: the passphrase is a remote key of its own" \
    || fail "externalSecrets: the passphrase is not its own remote key"
out="$(render --set secrets.provider=externalSecrets)"
grep -q "needs secrets.externalSecrets.storeRef.name" <<<"$out" \
    && pass "externalSecrets: a missing store fails the render" \
    || fail "externalSecrets: rendered without a store"

# kubernetes_without_ack_fails
out="$(render)"
grep -q "secrets.provider=kubernetes needs etcd encryption at rest" <<<"$out" \
    && pass "kubernetes: the release values fail without the etcd acknowledgement" \
    || fail "kubernetes: rendered without the etcd acknowledgement"
out="$(render --set secrets.acknowledgeEtcdEncryption=true)"
grep -q "secretName: http-gateway-keystore-passphrase, defaultMode: 288" <<<"$out" \
    && pass "kubernetes: with the acknowledgement it renders the passphrase Secret" \
    || fail "kubernetes: with the acknowledgement it does not render: $(head -3 <<<"$out")"
out="$(render -f deploy/kind/values.yaml)"
grep -q "kind: StatefulSet" <<<"$out" && pass "kubernetes: the kind profile renders" \
    || fail "kubernetes: the kind profile does not render: $(head -3 <<<"$out")"
out="$(render --set secrets.provider=vault)"
grep -q "secrets.provider must be kubernetes, csi or externalSecrets" <<<"$out" \
    && pass "an unknown provider fails the render" || fail "an unknown provider rendered"

# Every provider: the service reads its passphrase from memory it owns, never the source.
out="$(render --set secrets.provider=csi)"
[ "$(grep -c 'value: /keystore-passphrase/passphrase' <<<"$out")" -eq "${#workloads[@]}" ] \
    && pass "every workload is pointed at /keystore-passphrase/passphrase" \
    || fail "MIDGARD_KEYSTORE_PASSPHRASE_FILE is not set on every workload"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "render-providers: all checks pass"
