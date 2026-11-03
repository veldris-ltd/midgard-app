#!/usr/bin/env bash
# Render deploy/k8s/enclosure.yaml — the Helm chart for a development cluster, for
# `kubectl apply` without Helm — or check that the committed file is exactly what the
# chart renders, so the two cannot drift. [NFR-SEC-1] [RC-18]
#
# The committed file is rendered with secrets.provider=csi, so it acknowledges nothing
# about the cluster's etcd: the chart's default, kubernetes, refuses to render without
# secrets.acknowledgeEtcdEncryption, a promise only the cluster's operator can make.
# The csi provider needs no further values; whoever applies the file supplies the
# SecretProviderClass objects. [VLD-ANB-MIDGARD-0018]
#
# image.allowTag stays until a release has published the services' digests: without a
# digest per service the chart renders only by tag. [VLD-ANB-MIDGARD-0012]
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
# values, image.allowTag=true and secrets.provider=csi: a default-deny, egress-denied namespace
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
# Every service mounts its identity, keystore passphrase (and seed where it seeds
# state) through the Secrets Store CSI driver, from SecretProviderClass objects
# "<service>-identity", "<service>-keystore-passphrase" and "<service>-seed" that you
# supply from your key custody, with the forge-jobs ConfigMap; nothing of them is
# written to etcd. To use Kubernetes Secrets instead, render the chart yourself with
# secrets.acknowledgeEtcdEncryption=true, and only for a cluster whose etcd encrypts
# Secrets at rest (deploy/helm/midgard/README.md). [VLD-ANB-MIDGARD-0018]
# Apply with:  kubectl apply -f deploy/k8s/enclosure.yaml
#
# GENERATED — do not edit. Regenerate with:  bash deploy/k8s/render.sh
# (CI fails if this file is not what the chart renders.)
EOF
    $HELM template midgard deploy/helm/midgard --namespace default \
        --set image.allowTag=true --set secrets.provider=csi | tr -d '\r'
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
