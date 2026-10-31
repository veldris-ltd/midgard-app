#!/usr/bin/env bash
# Tests for deploy/kind/bootstrap-secrets.sh, the kind cluster's secret provisioning,
# with a stand-in kubectl that records how it was called. [RC-35] [NFR-SEC-1]
# [VLD-ANB-MIDGARD-0018] [AC-29.1] [AC-29.2]
#
#   bash tools/ci/tests/bootstrap-secrets.test.sh
#
# The claims under test: the script refuses any context that is not a kind cluster
# before it calls kubectl to create anything, including when the kubeconfig's current
# context is a kind cluster but the kubectl it is given names another; and a seed
# carries keys, trust anchors and configuration (and the gateway's governance ledger,
# which it cannot start without), never the identity directory, the object store,
# admin state or any other journal.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../../../deploy/kind/bootstrap-secrets.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# A stand-in kubectl: records every call, reports $STUB_CONTEXT as the current context.
mkdir -p "$work/bin"
cat > "$work/bin/kubectl" <<STUB
#!/bin/sh
echo "\$*" >> "$work/calls"
if [ "\$1 \$2" = "config current-context" ]; then echo "\$STUB_CONTEXT"; exit 0; fi
exit 1
STUB
chmod +x "$work/bin/kubectl"
# docker must never be reached either.
cat > "$work/bin/docker" <<STUB
#!/bin/sh
echo "docker \$*" >> "$work/calls"
exit 1
STUB
chmod +x "$work/bin/docker"

run() { # run CONTEXT KUBECTL
    rm -f "$work/calls"
    STUB_CONTEXT="$1" KUBECTL="$2" PATH="$work/bin:$PATH" bash "$script" > "$work/out" 2>&1
}

# bootstrap_secrets_refuses_non_kind_context
run prod-cluster "$work/bin/kubectl"
status=$?
[ "$status" -eq 1 ] && pass "a prod-cluster context exits 1" \
    || fail "a prod-cluster context must exit 1 (got $status): $(cat "$work/out")"
if grep -q "create\|docker" "$work/calls" 2>/dev/null; then
    fail "a prod-cluster context still reached: $(grep "create\|docker" "$work/calls")"
else
    pass "a prod-cluster context creates no secret and runs no container"
fi
grep -q "for kind only" "$work/out" && grep -q "deploy/helm/midgard/README.md" "$work/out" \
    && pass "the refusal names the key custody path" \
    || fail "the refusal must point at deploy/helm/midgard/README.md: $(cat "$work/out")"

run kind-midgard "$work/bin/kubectl --context prod-cluster"
status=$?
if [ "$status" -eq 1 ] && ! grep -q "create\|docker" "$work/calls" 2>/dev/null; then
    pass "a --context naming another cluster is refused whatever the kubeconfig says"
else
    fail "--context prod-cluster must be refused before any write (got $status)"
fi

run "" "$work/bin/kubectl"
[ "$?" -eq 1 ] && pass "no current context is refused" || fail "no current context must be refused"

# bootstrap_seed_excludes_journals
state="$work/state"
mkdir -p "$state/keys" "$state/trust" "$state/admin/privilege"
for f in keys/idp.keystore keys/audit.keystore trust/idp.pub trust/audit.pub policy.json \
         ontology.json governance.ledger governance.ledger.checkpoint identity.directory \
         objects.store admin/passkeys.journal admin/roles.journal admin/pending.journal \
         admin/privilege/audit.ledger mimir.ledger mimir.ledger.checkpoint datasets.journal \
         midgard.lock; do
    echo fixture > "$state/$f"
done
( . "$script" && seed_tar "$state" "$work/seed.tar" ) > "$work/out" 2>&1
status=$?
listing="$(tar -tf "$work/seed.tar" 2>/dev/null)"
[ "$status" -eq 0 ] && [ -n "$listing" ] && pass "the seed tar is built from a fixture" \
    || fail "seed_tar failed ($status): $(cat "$work/out")"
leaked="$(grep -E '(^|/)(objects\.store|identity\.directory|admin/|[^/]*\.journal|mimir\.ledger|midgard\.lock)' <<<"$listing")"
[ -z "$leaked" ] && pass "the seed lists no directory, object store, admin state or journal" \
    || fail "the seed carries: $leaked"
for want in keys/idp.keystore keys/audit.keystore trust/audit.pub policy.json ontology.json \
            governance.ledger governance.ledger.checkpoint; do
    grep -qx "$want" <<<"$listing" || missing+=" $want"
done
[ -z "${missing:-}" ] && pass "the seed carries keys/, trust/, policy, ontology and the governance ledger" \
    || fail "the seed is missing:$missing"

service="$work/service-state"
mkdir -p "$service"
echo fixture > "$service/policy.json"
echo fixture > "$service/feed.ledger"
( . "$script" && seed_tar "$service" "$work/service.tar" ) >/dev/null 2>&1
listing="$(tar -tf "$work/service.tar" 2>/dev/null)"
[ "$listing" = "policy.json" ] && pass "a service seed carries its policy and not its ledger" \
    || fail "a service seed lists: $listing"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "bootstrap-secrets: all checks pass"
