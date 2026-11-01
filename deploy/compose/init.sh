#!/usr/bin/env bash
# One-shot preparation of a compose enclosure, run before any service starts and
# safe to run again: it creates the enclosure's state directory with fresh issuer
# and audit keys, issues each mesh service its own identity from that issuer, and
# gives mimir-server its policy. Anything that already exists is left alone —
# `mg-admin` never overwrites keys — apart from bringing certificates, trust anchors
# and revocation lists written by an earlier release up to date. Running it again after
# an upgrade is the upgrade. [RC-35] [RC-09] [NFR-SEC-1] [MG-SEC-02]
set -euo pipefail

CORE=/vol/core-state

if [ -f "$CORE/keys/idp.keystore" ]; then
    echo "init: the enclosure state directory already exists"
else
    mg-admin init --state-dir "$CORE"
    # A fresh enclosure gets the demonstration schema the Forge worker syncs into, and
    # a policy that lets it; an existing enclosure keeps whatever it has.
    cp /etc/midgard/core-ontology.json "$CORE/ontology.json"
    cp /etc/midgard/core-policy.json "$CORE/policy.json"
    echo "init: wrote the demonstration ontology and policy"
fi

# Audit ledgers written by an earlier release have no checkpoint, and nothing opens a
# ledger without one. Adopt each: verify its whole chain, then record its first signed
# checkpoint. A ledger that already has one, or does not exist yet, is left alone, so
# this changes nothing on a current enclosure. It comes before the identities: issuing
# or upgrading one is recorded in the gateway's ledger, which must open first. Adopting
# a service's ledger needs only its audit keystore, which an upgrade keeps. [MG-SEC-05]
#
# The gateway's ledgers (governance and dual-control), under the audit key pinned in
# the state directory. A deployment that keeps the pin outside it passes that copy.
mg-admin audit checkpoint --state-dir "$CORE" --audit-pin "$CORE/trust/audit.pub"

# adopt <identity-dir> <service> <ledger> <ledger-id>: a mesh service's own ledger,
# under its audit keystore.
adopt() {
    mg-admin audit checkpoint --identity-dir "$1" --service "$2" --ledger "$3" --ledger-id "$4"
}

adopt /vol/mimir-identity mimir-server /vol/mimir-state/mimir.ledger mimir-inference
adopt /vol/fleet-identity bifrost-fleet /vol/fleet-state/fleet.ledger bifrost-fleet-audit
adopt /vol/heimdall-identity heimdall-feed /vol/heimdall-state/feed.ledger heimdall-feed-audit
adopt /vol/forge-identity forge-worker /vol/forge-state/worker.ledger forge-worker-audit

# issue <service> <roles> <out-dir>: a new service gets its identity; one that already
# has keys is brought up to date in place. `mesh upgrade` keeps both keystores (the
# audit keystore signs the service's ledger and seals its state), reissues a
# certificate this release cannot read for the key it already holds, and writes the
# trust anchor and the signed revocation list (mesh.crl) every service now needs. On
# a service that is already current it changes nothing. [MG-SEC-02] [MG-SEC-03]
issue() {
    if [ -f "$3/$1.keystore" ]; then
        mg-admin mesh upgrade --state-dir "$CORE" --service "$1" --roles "$2" --out "$3"
    else
        mg-admin mesh issue --state-dir "$CORE" --service "$1" --roles "$2" --out "$3"
    fi
}

issue mimir-server inference-server /vol/mimir-identity
issue mimir-probe inference-consumer /vol/probe-identity
issue bifrost-fleet fleet-manager /vol/fleet-identity
issue fleet-agent-1 fleet-agent /vol/fleet-clients
issue fleet-operator fleet-operator /vol/fleet-clients
issue heimdall-feed feed-gateway /vol/heimdall-identity
issue feed-source-1 feed-source /vol/heimdall-clients
issue picture-analyst picture-reader /vol/heimdall-clients
issue http-gateway fabric-gateway /vol/gateway-identity
issue forge-worker forge-worker /vol/forge-identity

# place <source> <destination>
place() {
    if [ -f "$2" ]; then
        echo "init: $2 already exists"
    else
        cp "$1" "$2"
        echo "init: wrote $2"
    fi
}

place /etc/midgard/mimir-policy.json /vol/mimir-state/policy.json
place /etc/midgard/fleet-policy.json /vol/fleet-state/policy.json
place /etc/midgard/fleet-channels.json /vol/fleet-state/channels.json
place /etc/midgard/heimdall-policy.json /vol/heimdall-state/policy.json
