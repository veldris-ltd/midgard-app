#!/usr/bin/env bash
# Every ingress rule in the rendered enclosure names who may reach its port, and the
# chart refuses to render a port open to any pod. [NFR-SEC-1] [RC-18] [AC-32.1]
# [AC-32.2] [VLD-ANB-MIDGARD-0027]
#
#   bash tools/ci/tests/k8s-ingress.test.sh
#
# The claim under test: a NetworkPolicy ingress rule with ports but no `from:` admits
# every pod in the cluster, so deploy/k8s/enclosure.yaml must have none; the check
# itself fails on a from-less rule; and `helm template` with a port's from set to "*"
# fails naming the service and port (skipped, with a notice, when helm is not on PATH,
# as the k8s-render job needs it too).
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
manifest="$root/deploy/k8s/enclosure.yaml"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# open_rules FILE: print "line N: NetworkPolicy NAME" for each ingress rule with no
# from: block before the next rule; print nothing when every rule has one.
open_rules() {
    tr -d '\r' < "$1" | awk '
        function close_rule() {
            if (rule && !from) print "line " rule ": NetworkPolicy " name
            rule = 0; from = 0
        }
        /^---/                 { close_rule(); kind = ""; name = ""; ingress = 0; next }
        /^kind:/               { kind = $2 }
        /^  name:/ && name == "" { name = $2 }
        /^  [a-zA-Z]+:/        { close_rule(); ingress = (kind == "NetworkPolicy" && $1 == "ingress:") }
        ingress && /^    - /   { close_rule(); rule = NR; if ($0 ~ /^    - from:/) from = 1 }
        ingress && /^      from:/ { from = 1 }
        END                    { close_rule() }
    '
}

# The committed manifest: every ingress rule carries a from: block.
if [ ! -f "$manifest" ]; then
    fail "deploy/k8s/enclosure.yaml exists"
elif out="$(open_rules "$manifest")" && [ -z "$out" ]; then
    pass "every ingress rule in deploy/k8s/enclosure.yaml names who may reach it"
else
    fail "ingress rules open to every pod in deploy/k8s/enclosure.yaml:"$'\n'"$out"
fi

# The check catches a from-less rule, and only that rule.
cat > "$work/open.yaml" <<'YAML'
---
kind: NetworkPolicy
metadata:
  name: allow-edge-ingress
spec:
  policyTypes: [Ingress]
  ingress:
    - ports:
        - { protocol: TCP, port: 7720 }
      from:
        - podSelector:
            matchLabels:
              app.kubernetes.io/name: client
    - ports:
        - { protocol: TCP, port: 9090 }
---
kind: NetworkPolicy
metadata:
  name: allow-edge-egress
spec:
  egress:
    - ports:
        - { protocol: TCP, port: 443 }
YAML
out="$(open_rules "$work/open.yaml")"
if [ "$out" = "line 14: NetworkPolicy allow-edge-ingress" ]; then
    pass "a from-less ingress rule is reported, an egress rule is not"
else
    fail "the check on a from-less ingress rule printed: ${out:-nothing}"
fi

# The chart refuses a wildcard.
HELM="${HELM:-helm}"
if command -v "$HELM" >/dev/null 2>&1; then
    if out="$("$HELM" template midgard "$root/deploy/helm/midgard" \
        --set 'services[1].ports[1].from={*}' 2>&1)"; then
        fail "helm template with from=* rendered"
    elif grep -q 'from must name workloads' <<<"$out"; then
        pass "helm template with from=* fails: from must name workloads"
    else
        fail "helm template with from=* failed for another reason: $out"
    fi
else
    echo "skip  helm is not on PATH: the wildcard render check needs it"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "k8s-ingress: all tests passed"
else
    echo "k8s-ingress: $failures test(s) failed"
    exit 1
fi
