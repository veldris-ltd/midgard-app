#!/usr/bin/env bash
# One-shot preparation of a compose enclosure, run before any service starts and
# safe to run again: it creates the enclosure's state directory with fresh issuer
# and audit keys, issues each mesh service its own identity from that issuer, and
# gives mimir-server its policy. Anything that already exists is left alone —
# `mg-admin` never overwrites keys — apart from bringing certificates, trust anchors
# and revocation lists written by an earlier release up to date. Running it again after
# an upgrade is the upgrade. [RC-35] [RC-09] [NFR-SEC-1] [MG-SEC-02]
#
# Every keystore's custody root is wrapped under a passphrase (keystore format
# version 3). Each keystore holder gets its own random passphrase, in a volume of its
# own, /vol/<holder>-passphrase, never beside its keystores; a passphrase that exists
# is kept. Keystores an earlier release wrote, version 1 with the root in the clear or
# version 2 with its header not authenticated, are migrated in place under their
# holder's passphrase before anything opens them.
# [VLD-ANB-MIDGARD-0007]
set -euo pipefail

CORE=/vol/core-state

# pass <holder>: the passphrase file for one keystore holder, made on first use: 32
# random bytes as hex, owner-only, written under a temporary name and renamed.
pass() {
    local dir="/vol/$1-passphrase" file="/vol/$1-passphrase/passphrase"
    if [ ! -s "$file" ]; then
        (
            umask 077
            mkdir -p "$dir"
            head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n' >"$dir/.passphrase.tmp"
            mv "$dir/.passphrase.tmp" "$file"
        )
        echo "init: generated the keystore passphrase for $1" >&2
    fi
    echo "$file"
}

# migrate <holder> <keystore>...: convert any version 1 or version 2 keystore to
# version 3 under the holder's passphrase. A version 3 keystore is checked and left
# alone, so this changes nothing on a current enclosure; one that does not unlock stops
# init.
migrate() {
    local passphrase
    passphrase="$(pass "$1")"
    shift
    for keystore in "$@"; do
        [ -f "$keystore" ] || continue
        mg-admin keystore migrate --keystore "$keystore" --keystore-passphrase-file "$passphrase"
    done
}

# The enclosure's own keystores and the gateway's mesh identity use the enclosure's
# passphrase (the gateway is given one); every other service has its own.
export MIDGARD_KEYSTORE_PASSPHRASE_FILE
MIDGARD_KEYSTORE_PASSPHRASE_FILE="$(pass core)"
migrate core "$CORE/keys/idp.keystore" "$CORE/keys/audit.keystore" \
    /vol/gateway-identity/http-gateway.keystore /vol/gateway-identity/http-gateway.audit.keystore
migrate mimir /vol/mimir-identity/mimir-server.keystore /vol/mimir-identity/mimir-server.audit.keystore
migrate probe /vol/probe-identity/mimir-probe.keystore /vol/probe-identity/mimir-probe.audit.keystore
migrate fleet /vol/fleet-identity/bifrost-fleet.keystore /vol/fleet-identity/bifrost-fleet.audit.keystore
migrate fleet-clients /vol/fleet-clients/*.keystore
migrate heimdall /vol/heimdall-identity/heimdall-feed.keystore \
    /vol/heimdall-identity/heimdall-feed.audit.keystore
migrate heimdall-clients /vol/heimdall-clients/*.keystore
migrate forge /vol/forge-identity/forge-worker.keystore /vol/forge-identity/forge-worker.audit.keystore

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

# adopt <identity-dir> <service> <ledger> <ledger-id> <holder>: a mesh service's own
# ledger, under its audit keystore, unlocked with the holder's passphrase.
adopt() {
    mg-admin audit checkpoint --identity-dir "$1" --service "$2" --ledger "$3" --ledger-id "$4" \
        --keystore-passphrase-file "$(pass "$5")"
}

adopt /vol/mimir-identity mimir-server /vol/mimir-state/mimir.ledger mimir-inference mimir
adopt /vol/fleet-identity bifrost-fleet /vol/fleet-state/fleet.ledger bifrost-fleet-audit fleet
adopt /vol/heimdall-identity heimdall-feed /vol/heimdall-state/feed.ledger heimdall-feed-audit \
    heimdall
adopt /vol/forge-identity forge-worker /vol/forge-state/worker.ledger forge-worker-audit forge

# issue <service> <roles> <out-dir> <holder>: a new service gets its identity, its
# keystores wrapped under the holder's passphrase; one that already has keys is brought
# up to date in place. `mesh upgrade` keeps both keystores (the
# audit keystore signs the service's ledger and seals its state), reissues a
# certificate this release cannot read for the key it already holds, and writes the
# trust anchor and the signed revocation list (mesh.crl) every service now needs. On
# a service that is already current it changes nothing. [MG-SEC-02] [MG-SEC-03]
issue() {
    local command=issue
    [ -f "$3/$1.keystore" ] && command=upgrade
    mg-admin mesh "$command" --state-dir "$CORE" --service "$1" --roles "$2" --out "$3" \
        --identity-passphrase-file "$(pass "$4")"
}

issue mimir-server inference-server /vol/mimir-identity mimir
issue mimir-probe inference-consumer /vol/probe-identity probe
issue bifrost-fleet fleet-manager /vol/fleet-identity fleet
issue fleet-agent-1 fleet-agent /vol/fleet-clients fleet-clients
issue fleet-operator fleet-operator /vol/fleet-clients fleet-clients
issue heimdall-feed feed-gateway /vol/heimdall-identity heimdall
issue feed-source-1 feed-source /vol/heimdall-clients heimdall-clients
issue picture-analyst picture-reader /vol/heimdall-clients heimdall-clients
issue http-gateway fabric-gateway /vol/gateway-identity core
issue forge-worker forge-worker /vol/forge-identity forge

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
