#!/usr/bin/env bash
# Provision a Midgard namespace's secrets for a local cluster, once. [RC-35] [RC-09]
#
# Runs the compose image's `midgard-init` — `mg-admin init` plus `mg-admin mesh issue`
# for every workload — into a throwaway Docker volume, then turns what it wrote into
# Kubernetes Secrets:
#   <service>-identity   the service's identity and audit keystores, certificate and
#                        the mesh trust anchor
#   <service>-seed       seed.tar: the state its init container unpacks on first start
#                        (for http-gateway, the enclosure's keys, policy, ontology and
#                        ledger)
#   <client>-identity    the identities of the probe, agent, source and analyst Jobs
# and the forge-jobs ConfigMap from deploy/compose/forge. Existing secrets are left
# alone: the enclosure's keys are minted once.
#
# The keystores are secrets. On a real cluster provision them from your key custody,
# not from this script.
set -euo pipefail
# Git Bash would rewrite container paths such as /vol into Windows paths.
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
IMAGE="${MIDGARD_IMAGE:-midgard/dev:local}"
KUBECTL="${KUBECTL:-kubectl --context kind-midgard}"
VOLUME=midgard-kind-bootstrap

# kubectl may be a Windows binary under Git Bash: hand it native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }

if $KUBECTL -n "$NS" get secret http-gateway-identity >/dev/null 2>&1; then
    echo "bootstrap: secrets already provisioned in $NS"
    exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"; docker volume rm -f "$VOLUME" >/dev/null 2>&1 || true' EXIT

docker volume rm -f "$VOLUME" >/dev/null 2>&1 || true
docker volume create "$VOLUME" >/dev/null
docker run --rm --network none -v "$VOLUME:/vol" --entrypoint midgard-init "$IMAGE"
docker run --rm --network none -v "$VOLUME:/vol" --entrypoint tar "$IMAGE" -C /vol -cf - . \
    | tar -xf - -C "$work"

# identity <secret> <dir> <service...>: a service's (or several clients') identity files.
identity() {
    local name="$1" dir="$work/$2"
    shift 2
    local args=(--from-file="$(native "$dir/mesh.anchor")")
    for svc in "$@"; do
        for f in "$svc.keystore" "$svc.audit.keystore" "$svc.cert"; do
            args+=(--from-file="$(native "$dir/$f")")
        done
    done
    $KUBECTL -n "$NS" create secret generic "$name" "${args[@]}"
}

# seed <secret> <dir>: a state directory's contents as seed.tar. The entries are named
# individually, with no entry for the directory itself: the volume they are unpacked
# into belongs to the node, and a non-root init container may not change its mode.
seed() {
    local entries
    mapfile -t entries < <(ls -A "$work/$2")
    tar -C "$work/$2" -cf "$work/$1.tar" -- "${entries[@]}"
    $KUBECTL -n "$NS" create secret generic "$1" --from-file=seed.tar="$(native "$work/$1.tar")"
}

identity http-gateway-identity gateway-identity http-gateway
seed http-gateway-seed core-state
identity mimir-server-identity mimir-identity mimir-server
seed mimir-server-seed mimir-state
identity bifrost-fleet-identity fleet-identity bifrost-fleet
seed bifrost-fleet-seed fleet-state
identity heimdall-feed-identity heimdall-identity heimdall-feed
seed heimdall-feed-seed heimdall-state
identity forge-worker-identity forge-identity forge-worker

identity mimir-probe-identity probe-identity mimir-probe
identity fleet-clients-identity fleet-clients fleet-agent-1 fleet-operator
identity heimdall-clients-identity heimdall-clients feed-source-1 picture-analyst

$KUBECTL -n "$NS" create configmap forge-jobs --from-file="$(native deploy/compose/forge)"
echo "bootstrap: provisioned identities, seeds and forge-jobs in $NS"
