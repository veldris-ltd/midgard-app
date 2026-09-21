#!/usr/bin/env bash
# Stage the default model for compose: IBM Granite 3.3 2B Instruct, GGUF Q4_K_M,
# Apache-2.0. [FR-MI-1] [AC-MI-7] [FR-MS-4] [DD-2]
#
#   bash deploy/compose/stage-model.sh [PATH/TO/granite-3.3-2b-instruct-Q4_K_M.gguf]
#
# Midgard ships no model weights. Download the file yourself from the pinned revision
# below. This script copies it into the external volume `midgard-models`, unless it is
# already there. Then, with `mg-model` from the development image:
#   - it signs the weights into `granite/model.manifest`, refusing unless their SHA-256
#     is the one the upstream publishes;
#   - it exports the signing key's public half to `trust/model-signing.pub`;
#   - it verifies the directory the way mimir-server will.
# Safe to run again.
#
# The signing key is a DEVELOPMENT key, kept in the volume `midgard-model-signing` on
# this machine. In production the key stays in the staging enclave, and only signed
# manifests and the public key cross into the enclosure.
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
GGUF=granite-3.3-2b-instruct-Q4_K_M.gguf
SHA256=ac71e9e32c0bea919b409c5918f69ca74339854b0319c5065e4e9fb6d95c4852
REVISION=7cdf86ccd1f1bb3491c9b7017b033f2e51367397
URL="https://huggingface.co/ibm-granite/granite-3.3-2b-instruct-GGUF/resolve/$REVISION/$GGUF"
IMAGE="${MIDGARD_IMAGE:-midgard/dev:local}"
MODELS=midgard-models
SIGNING=midgard-model-signing
source="${1:-}"

docker image inspect "$IMAGE" >/dev/null 2>&1 || {
    echo "stage-model: build the development image first:" >&2
    echo "  docker compose -f deploy/compose/compose.yaml build init" >&2
    exit 1
}
docker volume create "$MODELS" >/dev/null
docker volume create "$SIGNING" >/dev/null

if docker run --rm -v "$MODELS:/models" "$IMAGE" test -f "/models/granite/$GGUF"; then
    echo "stage-model: the weights are already in $MODELS"
else
    if [ -z "$source" ] || [ ! -f "$source" ]; then
        echo "usage: $0 PATH/TO/$GGUF" >&2
        echo "download it from $URL" >&2
        exit 2
    fi
    echo "stage-model: copying $source into $MODELS"
    docker run --rm -i -u 0 --network none -v "$MODELS:/models" "$IMAGE" sh -c "
        mkdir -p /models/granite &&
        cat > /models/granite/$GGUF.part &&
        mv /models/granite/$GGUF.part /models/granite/$GGUF" < "$source"
fi

docker run --rm -u 0 --network none -v "$MODELS:/models" -v "$SIGNING:/keys" "$IMAGE" sh -c "
    set -e
    [ -f /keys/model-signing.keystore ] || mg-model keygen --out /keys
    mkdir -p /models/trust
    cp /keys/model-signing.pub /models/trust/model-signing.pub
    rm -f /models/granite/model.manifest
    mg-model sign --weights /models/granite/$GGUF \
        --id granite-3.3-2b-instruct-q4km --family Granite --version 3.3 \
        --quantisation Q4_K_M --licence Apache-2.0 --osi-approved --redistributable \
        --sha256 $SHA256 --keystore /keys/model-signing.keystore
    mg-model verify --dir /models/granite --trust /models/trust/model-signing.pub
    chmod -R a+rX /models"
echo "stage-model: $MODELS is ready"
