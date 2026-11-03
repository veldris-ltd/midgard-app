#!/usr/bin/env bash
# Render deploy/k8s/enclosure.yaml — the Helm chart for a development cluster, for
# `kubectl apply` without Helm — or check that the committed file is exactly what the
# chart renders, so the two cannot drift. [NFR-SEC-1] [RC-18]
#
# The release values use the kubernetes secrets provider, which the chart refuses to
# render without secrets.acknowledgeEtcdEncryption; this file is rendered with it set,
# and its header says what that commits whoever applies it to. [VLD-ANB-MIDGARD-0018]
#
#   bash deploy/k8s/render.sh            # rewrite deploy/k8s/enclosure.yaml
#   bash deploy/k8s/render.sh --check    # exit 1 if it differs from the chart
#
# Needs helm on PATH (or HELM set).
set -euo pipefail

cd "$(dirname "$0")/../.."
HELM="${HELM:-helm}"
OUT=deploy/k8s/enclosure.yaml

render() {
    cat <<'EOF'
# Midgard enclosure — the Helm chart deploy/helm/midgard rendered with its default
# values and image.allowTag=true: a default-deny, egress-denied namespace
# (NFR-PRV-1), a resource quota, and one hardened, verify-before-run StatefulSet per
# service — non-root, read-only rootfs, dropped capabilities, probes, resource limits,
# its own volume — with per-port ingress and egress NetworkPolicies. [NFR-SEC-1]
# [AC-B-1] [RC-18]
#
# This is a development rendering: images are referenced by tag with pullPolicy
# Always because no release has produced digests yet; the signed release rendering
# is target/release-out/chart/enclosure.yaml, pinned by digest.
# [VLD-ANB-MIDGARD-0012]
#
# Every service expects Secrets "<service>-identity" and
# "<service>-keystore-passphrase" (and "<service>-seed" where it seeds state) and the
# forge-jobs ConfigMap to exist; provision them from your key custody. Rendered with
# secrets.acknowledgeEtcdEncryption=true: apply it only to a cluster whose etcd
# encrypts Secrets at rest (EncryptionConfiguration with a KMS v2 provider), or render
# the chart with the csi or externalSecrets provider instead
# (deploy/helm/midgard/README.md). [VLD-ANB-MIDGARD-0018]
# Apply with:  kubectl apply -f deploy/k8s/enclosure.yaml
#
# GENERATED — do not edit. Regenerate with:  bash deploy/k8s/render.sh
# (CI fails if this file is not what the chart renders.)
EOF
    $HELM template midgard deploy/helm/midgard --namespace default \
        --set image.allowTag=true --set secrets.acknowledgeEtcdEncryption=true | tr -d '\r'
}

if [ "${1:-}" = "--check" ]; then
    if ! diff -u <(tr -d '\r' < "$OUT") <(render); then
        echo "render: $OUT is not what the chart renders; run: bash deploy/k8s/render.sh" >&2
        exit 1
    fi
    echo "render: $OUT matches the chart"
else
    render > "$OUT"
    echo "render: wrote $OUT"
fi
