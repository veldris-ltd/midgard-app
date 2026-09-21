#!/usr/bin/env bash
# One-shot preparation of a compose enclosure, run before any service starts and
# safe to run again: it creates the enclosure's state directory with fresh issuer
# and audit keys, issues each mesh service its own identity from that issuer, and
# gives mimir-server its policy. Anything that already exists is left alone —
# `mg-admin` never overwrites keys or identities. [RC-35] [RC-09] [NFR-SEC-1]
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

# issue <service> <roles> <out-dir>
issue() {
    if [ -f "$3/$1.cert" ]; then
        echo "init: $1 already has a mesh identity"
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
