#!/usr/bin/env bash
# Midgard on a local kind cluster: create the cluster if it is not there, build the
# image from this checkout and load it, install the chart, provision the secrets, and
# wait for every service to be ready. Safe to run again. [RC-33] [NFR-SEC-1]
#
# Needs kind, helm, kubectl and docker on PATH (or KIND, HELM, KUBECTL set), and the
# acceptance toolchain image: `docker build -t midgard-acceptance:1.82.0 deploy/acceptance`.
set -euo pipefail

cd "$(dirname "$0")/../.."
CLUSTER="${MIDGARD_KIND_CLUSTER:-midgard}"
NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
KIND="${KIND:-kind}"
HELM="${HELM:-helm}"
export KUBECTL="${KUBECTL:-kubectl} --context kind-$CLUSTER"

if ! $KIND get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
    $KIND create cluster --name "$CLUSTER" --config deploy/kind/cluster.yaml --wait 180s
fi

docker compose -f deploy/compose/compose.yaml build init
$KIND load docker-image midgard/dev:local --name "$CLUSTER"

# MIDGARD_KIND_MODEL=granite keeps a model that deploy/kind/model.sh loaded being served.
overlay=()
[ "${MIDGARD_KIND_MODEL:-}" = granite ] && overlay=(-f deploy/kind/values-granite.yaml)
$HELM upgrade --install midgard deploy/helm/midgard \
    --kube-context "kind-$CLUSTER" --namespace default \
    -f deploy/kind/values.yaml "${overlay[@]}"
bash deploy/kind/bootstrap-secrets.sh

for service in http-gateway mimir-server bifrost-fleet heimdall-feed forge-worker; do
    $KUBECTL -n "$NS" rollout status "statefulset/$service" --timeout=300s
done
$KUBECTL -n "$NS" get pods -o wide
