#!/usr/bin/env bash
# Stage the default model for compose: IBM Granite 3.3 2B Instruct, GGUF Q4_K_M,
# Apache-2.0. [FR-MI-1] [AC-MI-7] [FR-MS-4] [DD-2] [VLD-ANB-MIDGARD-0028]
#
#   bash deploy/compose/stage-model.sh [--development] [PATH/TO/granite-3.3-2b-instruct-Q4_K_M.gguf]
#   bash deploy/compose/stage-model.sh --help
#
# Midgard ships no model weights. Download the file yourself from the pinned revision
# below. This script copies it into the external volume `midgard-models`, unless it is
# already there. Then, with `mg-model` from the development image, it signs the weights
# into `granite/model.manifest`, refusing unless their SHA-256 is the one the upstream
# publishes, and verifies the directory the way mimir-server will.
#
# The key mimir-server trusts never goes into `midgard-models`: whoever could write the
# weights could otherwise replace the key that vouches for them as well.
#
# By default the signing key is the production model-signing key: a keystore provisioned
# from the staging enclave into the volume `midgard-model-signing` as
# `model-signing.keystore`, with the passphrase its root is wrapped under in
# `midgard-model-signing-passphrase` as `passphrase`. Its public half is the pinned
# anchor `deploy/trust/model-signing.pub`, published by the key ceremony and carried in
# the image at /etc/midgard/trust. The model must verify against that file, or the
# manifest is removed and the script fails; without the anchor or the keystore it exits
# 1 before anything is signed.
#
# --development signs with a DEVELOPMENT key instead, generated on this machine into
# `midgard-model-signing` as a keystore version 2, its root wrapped under a random
# passphrase kept in `midgard-model-signing-passphrase` (a version 1 keystore an earlier
# release wrote is migrated first). [VLD-ANB-MIDGARD-0007] Only its public half is
# copied, into a volume of its own, `midgard-model-trust`, which mimir-server mounts
# read-only at /model-trust when compose is started with
# deploy/compose/compose.dev-model.yaml as well. It prints a warning: such a deployment
# trusts a key made on the install host.
#
# Safe to run again.
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
SIGNING_PASSPHRASE=midgard-model-signing-passphrase
TRUST=midgard-model-trust
ANCHOR=deploy/trust/model-signing.pub

usage() {
    cat <<EOF
usage: bash deploy/compose/stage-model.sh [--development] [PATH/TO/$GGUF]

Copies the Granite weights into the volume $MODELS, signs them and verifies them.
  (default)      sign with the keystore provisioned from the staging enclave into
                 $SIGNING (model-signing.keystore; its passphrase in
                 $SIGNING_PASSPHRASE as 'passphrase'). The model must verify
                 against $ANCHOR, committed by the key ceremony.
  --development  sign with a development key generated here (keystore version 2 under
                 a passphrase), its public half in the volume $TRUST; start compose
                 with deploy/compose/compose.dev-model.yaml as well.
  --help         print this and exit.
EOF
}

development=0
source=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --development) development=1; shift ;;
        -h | --help) usage; exit 0 ;;
        -*) echo "stage-model: unknown option $1" >&2; usage >&2; exit 2 ;;
        *) source="$1"; shift ;;
    esac
done

if [ "$development" = 0 ] && [ ! -s "$ANCHOR" ]; then
    echo "stage-model: $ANCHOR is not committed. The key ceremony publishes the" >&2
    echo "  model-signing key's public half there (docs/ops/KEY_CEREMONY.md), and mimir-server" >&2
    echo "  trusts no other. For a development install, run again with --development." >&2
    exit 1
fi
docker image inspect "$IMAGE" >/dev/null 2>&1 || {
    echo "stage-model: build the development image first:" >&2
    echo "  docker compose -f deploy/compose/compose.yaml build init" >&2
    exit 1
}
docker volume create "$MODELS" >/dev/null
if [ "$development" = 1 ]; then
    docker volume create "$SIGNING" >/dev/null
    docker volume create "$SIGNING_PASSPHRASE" >/dev/null
    docker volume create "$TRUST" >/dev/null
else
    # Provisioned from the staging enclave, never generated here.
    docker run --rm --network none -v "$SIGNING:/keys:ro" "$IMAGE" \
        test -f /keys/model-signing.keystore || {
        echo "stage-model: $SIGNING holds no model-signing.keystore. Provision the production" >&2
        echo "  model-signing keystore from the staging enclave into it, and its passphrase into" >&2
        echo "  $SIGNING_PASSPHRASE as 'passphrase'; or run again with --development." >&2
        exit 1
    }
    docker run --rm --network none -v "$SIGNING_PASSPHRASE:/keystore-passphrase:ro" "$IMAGE" \
        test -s /keystore-passphrase/passphrase || {
        echo "stage-model: $SIGNING_PASSPHRASE holds no 'passphrase' for the model-signing keystore" >&2
        exit 1
    }
fi

if docker run --rm -v "$MODELS:/models" "$IMAGE" test -f "/models/granite/$GGUF"; then
    echo "stage-model: the weights are already in $MODELS"
else
    if [ -z "$source" ] || [ ! -f "$source" ]; then
        usage >&2
        echo "download it from $URL" >&2
        exit 2
    fi
    echo "stage-model: copying $source into $MODELS"
    docker run --rm -i -u 0 --network none -v "$MODELS:/models" "$IMAGE" sh -c "
        mkdir -p /models/granite &&
        cat > /models/granite/$GGUF.part &&
        mv /models/granite/$GGUF.part /models/granite/$GGUF" < "$source"
fi

SIGN="mg-model sign --weights /models/granite/$GGUF \
        --id granite-3.3-2b-instruct-q4km --family Granite --version 3.3 \
        --quantisation Q4_K_M --licence Apache-2.0 --osi-approved --redistributable \
        --sha256 $SHA256 --keystore /keys/model-signing.keystore"

if [ "$development" = 1 ]; then
    docker run --rm -u 0 --network none -v "$MODELS:/models" -v "$SIGNING:/keys" \
        -v "$SIGNING_PASSPHRASE:/keystore-passphrase" -v "$TRUST:/model-trust" \
        -e MIDGARD_KEYSTORE_PASSPHRASE_FILE=/keystore-passphrase/passphrase "$IMAGE" sh -c "
        set -e
        if [ ! -s /keystore-passphrase/passphrase ]; then
            (umask 077; head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \\n' >/keystore-passphrase/passphrase)
        fi
        if [ -f /keys/model-signing.keystore ]; then
            mg-admin keystore migrate --keystore /keys/model-signing.keystore
        else
            mg-model keygen --out /keys
        fi
        cp /keys/model-signing.pub /model-trust/model-signing.pub
        rm -f /models/granite/model.manifest
        $SIGN
        mg-model verify --dir /models/granite --trust /model-trust/model-signing.pub
        chmod -R a+rX /models /model-trust"
    echo "WARNING: this deployment trusts a development model-signing key; production anchors come from deploy/trust" >&2
    echo "stage-model: $MODELS is ready, and its trust key is in $TRUST. Start compose with"
    echo "  -f deploy/compose/compose.granite.yaml -f deploy/compose/compose.dev-model.yaml"
else
    # The committed anchor goes in on stdin: the model must verify against it, or the
    # keystore is not the one the ceremony pinned, and nothing is left signed.
    docker run --rm -i -u 0 --network none -v "$MODELS:/models" -v "$SIGNING:/keys:ro" \
        -v "$SIGNING_PASSPHRASE:/keystore-passphrase:ro" \
        -e MIDGARD_KEYSTORE_PASSPHRASE_FILE=/keystore-passphrase/passphrase "$IMAGE" sh -c "
        set -e
        cat > /tmp/model-signing.pub
        rm -f /models/granite/model.manifest
        $SIGN
        if ! mg-model verify --dir /models/granite --trust /tmp/model-signing.pub; then
            rm -f /models/granite/model.manifest
            echo 'stage-model: the keystore in $SIGNING does not hold the key $ANCHOR pins; nothing is left signed' >&2
            exit 1
        fi
        chmod -R a+rX /models" < "$ANCHOR"
    echo "stage-model: $MODELS is ready, signed under the key $ANCHOR pins"
fi
