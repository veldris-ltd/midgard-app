#!/usr/bin/env bash
# Smoke test Midgard on the local kind cluster `deploy/kind/up.sh` created: each mesh
# client runs as a Job with its own certified identity, through the enclosure's
# NetworkPolicies, and must succeed. [RC-33] [RC-09] [FR-MI-8] [FR-B-3] [FR-H-3] [FR-F-4]
#
#   1. inference over PQ-mTLS, audited; mimir-server restarts on its volume and the next
#      call continues the recovered ledger
#   2. an installation checks in with the fleet manager and the operator sees it
#   3. two signed observations fuse into one tracked entity
#   4. the Forge worker has synced its sources into the enclosure's fabric
#   5. a pod with no mesh identity cannot even open a connection to mimir-server
set -euo pipefail
# Git Bash would rewrite container paths such as /identity into Windows paths.
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
CLUSTER="${MIDGARD_KIND_CLUSTER:-midgard}"
NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
IMAGE="${MIDGARD_IMAGE:-midgard/dev:local}"
KUBECTL="${KUBECTL:-kubectl} --context kind-$CLUSTER"

# job <name> <app label> <identity secret> <command as a JSON array>: run a mesh client
# to completion and print its output; fail if it fails.
job() {
    local name="$1" app="$2" secret="$3" command="$4"
    $KUBECTL -n "$NS" delete job "$name" --ignore-not-found >/dev/null
    $KUBECTL -n "$NS" apply -f - >/dev/null <<EOF
apiVersion: batch/v1
kind: Job
metadata:
  name: $name
spec:
  backoffLimit: 0
  template:
    metadata:
      labels:
        app.kubernetes.io/name: $app
        app.kubernetes.io/part-of: midgard
    spec:
      restartPolicy: Never
      automountServiceAccountToken: false
      securityContext:
        runAsNonRoot: true
        runAsUser: 10001
        runAsGroup: 10001
        fsGroup: 10001
        seccompProfile: { type: RuntimeDefault }
      containers:
        - name: client
          image: $IMAGE
          imagePullPolicy: Never
          command: $command
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities: { drop: ["ALL"] }
          resources:
            requests: { cpu: 50m, memory: 64Mi }
            limits: { cpu: 500m, memory: 256Mi }
          volumeMounts:
            - { name: identity, mountPath: /identity, readOnly: true }
      volumes:
        - name: identity
          secret: { secretName: $secret, defaultMode: 288 }
EOF
    local ok=0
    $KUBECTL -n "$NS" wait --for=condition=complete "job/$name" --timeout=120s >/dev/null 2>&1 || ok=1
    $KUBECTL -n "$NS" logs "job/$name" 2>/dev/null || true
    $KUBECTL -n "$NS" delete job "$name" --wait=false >/dev/null
    return "$ok"
}

echo "--- inference over PQ-mTLS, audited"
probe='["mimir-probe","--identity-dir","/identity","--addr","mimir-server:7720","--purpose","investigation"]'
first="$(job mimir-probe mimir-probe mimir-probe-identity "$probe")"
echo "$first"
seq_before="$(sed -n 's/.*audit_seq=\([0-9]*\).*/\1/p' <<<"$first")"
[ -n "$seq_before" ]

echo "--- mimir-server restarts on its volume and continues its ledger"
$KUBECTL -n "$NS" rollout restart statefulset/mimir-server >/dev/null
$KUBECTL -n "$NS" rollout status statefulset/mimir-server --timeout=180s
second="$(job mimir-probe mimir-probe mimir-probe-identity "$probe")"
echo "$second"
grep -q "audit_seq=$((seq_before + 1))" <<<"$second"

echo "--- an installation checks in with the fleet manager"
job fleet-agent fleet-agent fleet-clients-identity \
    '["bifrost-agent","--identity-dir","/identity","--service","fleet-agent-1","--addr","bifrost-fleet:7740","--channel","stable","--version","0.1.0"]'
status="$(job fleet-operator fleet-operator fleet-clients-identity \
    '["bifrost-agent","--identity-dir","/identity","--service","fleet-operator","--addr","bifrost-fleet:7740","--status"]')"
echo "$status"
grep -q '"id":"fleet-agent-1"' <<<"$status"

echo "--- two signed observations fuse into one tracked entity"
for at in 100 110; do
    job feed-source feed-source heimdall-clients-identity \
        "[\"heimdall-source\",\"--identity-dir\",\"/identity\",\"--service\",\"feed-source-1\",\"--addr\",\"heimdall-feed:7730\",\"--kind\",\"air-track\",\"--entity\",\"KIND-1\",\"--observed-at\",\"$at\"]"
done
picture="$(job picture-analyst picture-analyst heimdall-clients-identity \
    '["heimdall-source","--identity-dir","/identity","--service","picture-analyst","--addr","heimdall-feed:7730","--picture"]')"
echo "$picture"
grep -q '"id":"KIND-1"' <<<"$picture"

echo "--- the Forge worker has synced its sources into the fabric"
synced=""
for _ in $(seq 1 30); do
    synced="$($KUBECTL -n "$NS" logs statefulset/forge-worker | grep '"job":"aircraft"' | grep '"outcome":"synced"' || true)"
    [ -n "$synced" ] && break
    sleep 2
done
echo "$synced" | head -1
[ -n "$synced" ]

echo "--- a pod with no mesh identity is not let through to mimir-server"
if job no-identity unlabelled mimir-probe-identity \
    '["mimir-probe","--identity-dir","/identity","--addr","mimir-server:7720","--purpose","investigation"]' \
    >/dev/null 2>&1; then
    echo "a pod outside mimir-server's ingress policy reached it" >&2
    exit 1
fi

echo "kind smoke: PASS"
