#!/usr/bin/env bash
# Provision a Midgard namespace's secrets for a local kind cluster, once. [RC-35] [RC-09]
#
# For kind only: it refuses any kubectl context whose name does not start with kind-.
# On a real cluster the identities come from your key custody through the chart's csi
# or externalSecrets provider (deploy/helm/midgard/README.md), never from this script.
# [VLD-ANB-MIDGARD-0018]
#
# Runs the compose image's `midgard-init` (`mg-admin init` plus `mg-admin mesh issue`
# for every workload) into a throwaway Docker volume, then turns what it wrote into
# Kubernetes Secrets:
#   <service>-identity             the service's identity and audit keystores,
#                                  certificate, the mesh trust anchor and the signed
#                                  revocation list (mesh.crl)
#   <service>-keystore-passphrase  the passphrase its keystores are wrapped under
#                                  (keystore format version 2), as key `passphrase`, a
#                                  Secret of its own, never beside the keystores
#                                  [VLD-ANB-MIDGARD-0007]
#   <service>-seed                 seed.tar: what its init container unpacks on first
#                                  start. Keys, trust anchors and configuration only
#                                  (keys/, trust/, policy.json, ontology.json,
#                                  channels.json) and, for http-gateway, the governance
#                                  ledger and its checkpoint that `mg-admin init`
#                                  creates: the gateway refuses a missing ledger as
#                                  tampering [MG-SEC-05]. No directory, object store,
#                                  admin state or other journal: the services create
#                                  those empty on first start.
#   <client>-identity              the identities of the probe, agent, source and
#   <client>-keystore-passphrase   analyst Jobs, and their passphrases
# and the forge-jobs ConfigMap from deploy/compose/forge. Existing secrets are left
# alone: the enclosure's keys are minted once. A namespace provisioned by an earlier
# release holds keystores, certificates and an anchor this release refuses: a local
# cluster is upgraded by deleting the namespace and running deploy/kind/up.sh again
# (docs/ops/UPGRADING.md). [MG-SEC-02]
#
# Sourcing this file defines its functions without running it (the CI self-tests do).
set -euo pipefail
# Git Bash would rewrite container paths such as /vol into Windows paths.
export MSYS_NO_PATHCONV=1

NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
IMAGE="${MIDGARD_IMAGE:-midgard/dev:local}"
KUBECTL="${KUBECTL:-kubectl --context kind-midgard}"
VOLUME=midgard-kind-bootstrap

# What a seed may carry, relative to the state directory. Anything else in it (the
# identity directory, the object store, admin/, journals, service ledgers) stays out.
SEED_ALLOW=(keys trust policy.json ontology.json channels.json
            governance.ledger governance.ledger.checkpoint)

# kubectl may be a Windows binary under Git Bash: hand it native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }

# kube_context: the context $KUBECTL writes to: its --context option if it has one
# (kubectl's `config current-context` ignores that option), else the kubeconfig's
# current context.
kube_context() {
    local word take=0
    for word in $KUBECTL; do
        if [ "$take" = 1 ]; then echo "$word"; return; fi
        case "$word" in
            --context=*) echo "${word#--context=}"; return ;;
            --context) take=1 ;;
        esac
    done
    $KUBECTL config current-context
}

# require_kind: refuse, before any kubectl write, unless the context is a kind cluster.
require_kind() {
    local context
    context="$(kube_context 2>/dev/null || true)"
    case "$context" in
        kind-?*) ;;
        *)
            echo "bootstrap-secrets.sh provisions throwaway keys for kind only; on a real" \
                 "cluster provision identities from your key custody, see" \
                 "deploy/helm/midgard/README.md (context: ${context:-none})" >&2
            exit 1
            ;;
    esac
}

# seed_tar <state-dir> <out.tar>: the allowed entries of a state directory as a tar.
# The entries are named individually, with no entry for the directory itself: the
# volume they are unpacked into belongs to the node, and a non-root init container may
# not change its mode.
seed_tar() {
    local dir="$1" out="$2" entry entries=()
    for entry in "${SEED_ALLOW[@]}"; do
        if [ -e "$dir/$entry" ]; then entries+=("$entry"); fi
    done
    if [ "${#entries[@]}" -eq 0 ]; then
        echo "bootstrap: $dir holds nothing to seed" >&2
        return 1
    fi
    tar -C "$dir" -cf "$out" -- "${entries[@]}"
}

main() {
    cd "$(dirname "${BASH_SOURCE[0]}")/../.."
    require_kind

    if $KUBECTL -n "$NS" get secret http-gateway-identity >/dev/null 2>&1; then
        if ! $KUBECTL -n "$NS" get secret http-gateway-keystore-passphrase >/dev/null 2>&1; then
            echo "bootstrap: $NS was provisioned by an earlier release, whose keystores" \
                 "this release cannot open; delete the namespace and run deploy/kind/up.sh" \
                 "again (docs/ops/UPGRADING.md)" >&2
            exit 1
        fi
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

    identity http-gateway-identity gateway-identity http-gateway
    passphrase http-gateway core
    seed http-gateway-seed core-state
    identity mimir-server-identity mimir-identity mimir-server
    passphrase mimir-server mimir
    seed mimir-server-seed mimir-state
    identity bifrost-fleet-identity fleet-identity bifrost-fleet
    passphrase bifrost-fleet fleet
    seed bifrost-fleet-seed fleet-state
    identity heimdall-feed-identity heimdall-identity heimdall-feed
    passphrase heimdall-feed heimdall
    seed heimdall-feed-seed heimdall-state
    identity forge-worker-identity forge-identity forge-worker
    passphrase forge-worker forge

    identity mimir-probe-identity probe-identity mimir-probe
    passphrase mimir-probe probe
    identity fleet-clients-identity fleet-clients fleet-agent-1 fleet-operator
    passphrase fleet-clients fleet-clients
    identity heimdall-clients-identity heimdall-clients feed-source-1 picture-analyst
    passphrase heimdall-clients heimdall-clients

    $KUBECTL -n "$NS" create configmap forge-jobs --from-file="$(native deploy/compose/forge)"
    echo "bootstrap: provisioned identities, passphrases, seeds and forge-jobs in $NS"
}

# identity <secret> <dir> <service...>: a service's (or several clients') identity files.
identity() {
    local name="$1" dir="$work/$2"
    shift 2
    # The trust anchor and the signed revocation list every service loads with its
    # identity. A revocation is a newer mesh.crl in this secret: the kubelet updates the
    # mounted file and the service re-reads it within 30 seconds. [MG-SEC-03]
    local args=(--from-file="$(native "$dir/mesh.anchor")" --from-file="$(native "$dir/mesh.crl")")
    for svc in "$@"; do
        for f in "$svc.keystore" "$svc.audit.keystore" "$svc.cert"; do
            args+=(--from-file="$(native "$dir/$f")")
        done
    done
    $KUBECTL -n "$NS" create secret generic "$name" "${args[@]}"
}

# passphrase <name> <holder>: the passphrase init generated for one keystore holder, as
# Secret "<name>-keystore-passphrase". [VLD-ANB-MIDGARD-0007]
passphrase() {
    $KUBECTL -n "$NS" create secret generic "$1-keystore-passphrase" \
        --from-file=passphrase="$(native "$work/$2-passphrase/passphrase")"
}

# seed <secret> <dir>: a state directory's allowed entries as seed.tar.
seed() {
    seed_tar "$work/$2" "$work/$1.tar"
    $KUBECTL -n "$NS" create secret generic "$1" --from-file=seed.tar="$(native "$work/$1.tar")"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
