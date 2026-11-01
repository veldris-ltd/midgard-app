#!/usr/bin/env bash
# Serve Granite on the local kind cluster: load the pinned llama.cpp image and the model
# staged for compose into the cluster, verify the model there, and turn on the chart's
# modelServing for mimir-server. Safe to run again. [FR-MI-1] [AC-MI-7] [RC-33]
#
#   bash deploy/kind/model.sh
#
# Downloads nothing. It needs:
#   - the cluster from deploy/kind/up.sh
#   - the model staged by `bash deploy/compose/stage-model.sh` (the midgard-models volume:
#     the weights, the signed manifest and the model-signing public key)
#   - the llama.cpp image already pulled by its pinned digest
#
# The weights go into a PersistentVolumeClaim, `mimir-models`, on the node that holds
# mimir-server's own state volume, so the pod can mount both; a loader pod streams them
# in and runs `mg-model verify` there before anything uses them. mimir-server verifies
# them again, itself, before it serves, and stages the copy it verified into the pod's
# /serve emptyDir; llama.cpp mounts only that, read-only, and starts once
# /serve/staged.ready exists, so it never loads from the claim. [MG-SEC-14]
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
CLUSTER="${MIDGARD_KIND_CLUSTER:-midgard}"
NS="${MIDGARD_NAMESPACE:-midgard-enclosure}"
KIND="${KIND:-kind}"
HELM="${HELM:-helm}"
KUBECTL="${KUBECTL:-kubectl} --context kind-$CLUSTER"
IMAGE="${MIDGARD_IMAGE:-midgard/dev:local}"
LLAMA_PINNED=ghcr.io/ggml-org/llama.cpp@sha256:bbf6fde2eedbe1b6c8ee9d93a54ea4606e90a4eda18681a7f8b4b6ed0cbc9f00
LLAMA_LOCAL=midgard/llama-cpp:bbf6fde2
GGUF=granite-3.3-2b-instruct-Q4_K_M.gguf
LOADER=mimir-model-loader

native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }
fromvolume() { docker run --rm --network none -v midgard-models:/models "$IMAGE" "$@"; }

fromvolume test -f /models/granite/model.manifest || {
    echo "model: stage it first: bash deploy/compose/stage-model.sh PATH/TO/$GGUF" >&2
    exit 1
}
# mimir-server loads only a format 2 manifest, signed over its own bytes. [MG-SEC-14]
fromvolume sh -c 'head -n 1 /models/granite/model.manifest | grep -qx "midgard-model-manifest 2"' || {
    echo "model: the manifest predates format 2; sign it again: bash deploy/compose/stage-model.sh" >&2
    exit 1
}
docker image inspect "$LLAMA_PINNED" >/dev/null 2>&1 || {
    echo "model: pull the llama.cpp image first: docker pull ${LLAMA_PINNED/@/:server@}" >&2
    exit 1
}
node="$($KUBECTL -n "$NS" get pod mimir-server-0 -o jsonpath='{.spec.nodeName}' 2>/dev/null || true)"
[ -n "$node" ] || { echo "model: no mimir-server-0 in $NS; run deploy/kind/up.sh first" >&2; exit 1; }

work="$(mktemp -d)"
cleanup() {
    rm -rf "$work"
    $KUBECTL -n "$NS" delete pod "$LOADER" --ignore-not-found --wait=false >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "--- loading the llama.cpp image into the cluster"
# The pinned reference is a multi-platform index, and only this host's platform is
# pulled; `kind load docker-image` imports every platform the index names and fails on
# the absent ones. Save just this platform and load that archive.
platform="$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')"
docker tag "$LLAMA_PINNED" "$LLAMA_LOCAL"
docker save --platform "$platform" -o "$(native "$work/llama-cpp.tar")" "$LLAMA_LOCAL"
$KIND load image-archive "$(native "$work/llama-cpp.tar")" --name "$CLUSTER"

echo "--- the model-signing trust key"
fromvolume cat /models/trust/model-signing.pub > "$work/model-signing.pub"
$KUBECTL -n "$NS" create configmap mimir-model-trust \
    --from-file="$(native "$work/model-signing.pub")" --dry-run=client -o yaml \
    | $KUBECTL -n "$NS" apply -f - >/dev/null

echo "--- the model volume, on $node beside mimir-server's state"
$KUBECTL -n "$NS" apply -f - >/dev/null <<EOF
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: mimir-models
  labels: { app.kubernetes.io/part-of: midgard }
spec:
  accessModes: [ReadWriteOnce]
  resources: { requests: { storage: 3Gi } }
EOF
$KUBECTL -n "$NS" delete pod "$LOADER" --ignore-not-found >/dev/null
$KUBECTL -n "$NS" apply -f - >/dev/null <<EOF
apiVersion: v1
kind: Pod
metadata:
  name: $LOADER
  labels:
    app.kubernetes.io/name: $LOADER
    app.kubernetes.io/part-of: midgard
spec:
  # A node selector, not nodeName: nodeName bypasses the scheduler, and the volume
  # (WaitForFirstConsumer) is only provisioned once the scheduler has placed a pod.
  nodeSelector:
    kubernetes.io/hostname: $node
  restartPolicy: Never
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 10001
    runAsGroup: 10001
    fsGroup: 10001
    seccompProfile: { type: RuntimeDefault }
  containers:
    - name: loader
      image: $IMAGE
      imagePullPolicy: Never
      command: ["sleep", "3600"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities: { drop: ["ALL"] }
      resources:
        requests: { cpu: 100m, memory: 64Mi }
        limits: { cpu: "1", memory: 256Mi }
      volumeMounts:
        - { name: models, mountPath: /models }
        - { name: trust, mountPath: /model-trust, readOnly: true }
  volumes:
    - name: models
      persistentVolumeClaim: { claimName: mimir-models }
    - name: trust
      configMap: { name: mimir-model-trust }
EOF
$KUBECTL -n "$NS" wait --for=condition=Ready "pod/$LOADER" --timeout=120s >/dev/null

verify() {
    $KUBECTL -n "$NS" exec "$LOADER" -- \
        mg-model verify --dir /models/granite --trust /model-trust/model-signing.pub
}
if verify 2>/dev/null; then
    echo "model: the verified model is already in the volume"
else
    echo "--- streaming the model into the volume (1.5 GB)"
    $KUBECTL -n "$NS" exec "$LOADER" -- sh -c 'mkdir -p /models/granite && rm -f /models/granite/*'
    for file in "$GGUF" model.manifest; do
        fromvolume cat "/models/granite/$file" \
            | $KUBECTL -n "$NS" exec -i "$LOADER" -- \
                sh -c "cat > /models/granite/$file.part && mv /models/granite/$file.part /models/granite/$file"
    done
    verify
fi

echo "--- serving the model"
$HELM upgrade --install midgard deploy/helm/midgard \
    --kube-context "kind-$CLUSTER" --namespace default \
    -f deploy/kind/values.yaml -f deploy/kind/values-granite.yaml
$KUBECTL -n "$NS" rollout status statefulset/mimir-server --timeout=600s
$KUBECTL -n "$NS" logs mimir-server-0 -c mimir-server | grep '"message":"serving"' | tail -1
# What llama.cpp was given: the staged copy and its signed digest. [MG-SEC-14]
echo "model: staged for llama.cpp: $($KUBECTL -n "$NS" exec mimir-server-0 -c mimir-server -- cat /serve/staged.ready)"
echo "model: mimir-server is serving Granite"
