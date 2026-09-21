#!/usr/bin/env bash
# Signing-runner preflight for the release workflow. [NFR-PQ-3] [AC-B-1] [RC-07]
#
# Asserts the runner is provisioned as docs/ops/PKI.md ("Signing runner") requires, and
# fails closed naming every item that is missing, before any job builds or signs.
#
#   bash tools/ci/release-preflight.sh
#
# Environment (the workflow maps runner secrets onto these; none has a default):
#   MIDGARD_SIGNING_KEYSTORE   the release signing keystore (ML-DSA-87)
#   MIDGARD_BIFROST_KEYSTORE   the Bifrost admission keystore (ML-DSA-65): registry
#                              admission and the air-gap bundle
#   MIDGARD_BIFROST_REGISTRY   the Bifrost artefact registry directory
#   TOOLCHAIN_IMAGE            the toolchain image (default midgard-acceptance:1.82.0)
set -uo pipefail

cd "$(dirname "$0")/../.."
TOOLCHAIN_IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"
problems=0
ok() { echo "preflight: ok — $*"; }
bad() {
    echo "::error::preflight: $* (see docs/ops/PKI.md, \"Signing runner\")"
    problems=$((problems + 1))
}

# 1. The pinned toolchain and the service image base, provisioned while connected:
#    every build runs in the toolchain image with no network.
if ! command -v docker >/dev/null 2>&1; then
    bad "docker is not installed"
else
    if docker image inspect "$TOOLCHAIN_IMAGE" >/dev/null 2>&1; then
        tools="$(docker run --rm --network none "$TOOLCHAIN_IMAGE" sh -c 'rustc --version; just --version' 2>&1)"
        if grep -q '^rustc 1\.82\.0 ' <<<"$tools" && grep -q '^just ' <<<"$tools"; then
            ok "toolchain image $TOOLCHAIN_IMAGE ($(head -1 <<<"$tools"))"
        else
            bad "$TOOLCHAIN_IMAGE does not carry rustc 1.82.0 and just: $tools"
        fi
    else
        bad "the toolchain image $TOOLCHAIN_IMAGE is not present; build it from deploy/acceptance/Dockerfile while connected"
    fi
    base="$(sed -n 's/^ARG BASE=//p' deploy/images/service.Dockerfile | head -1)"
    if docker image inspect "$base" >/dev/null 2>&1; then
        ok "service image base $base"
    else
        bad "the service image base $base is not present; pull it while connected"
    fi
fi

# 2. The offline advisory database the scan stage reads.
if [ -s supply-chain/advisories.toml ]; then
    ok "offline advisory database"
else
    bad "supply-chain/advisories.toml is missing"
fi

# 3. Key material: named explicitly, readable, and never inside the checkout.
for var in MIDGARD_SIGNING_KEYSTORE MIDGARD_BIFROST_KEYSTORE; do
    path="${!var:-}"
    if [ -z "$path" ]; then
        bad "$var is not configured (runner secret ${var}_PATH)"
    elif [ ! -r "$path" ] || [ -d "$path" ]; then
        bad "$var names $path, which is not a readable file"
    elif [[ "$(cd "$(dirname "$path")" && pwd)/" == "$PWD/"* ]]; then
        bad "$var names a file inside the repository checkout"
    else
        ok "$var is configured"
    fi
done
registry="${MIDGARD_BIFROST_REGISTRY:-}"
if [ -z "$registry" ]; then
    bad "MIDGARD_BIFROST_REGISTRY is not configured (runner secret MIDGARD_BIFROST_REGISTRY_PATH)"
elif [ ! -d "$registry" ] || [ ! -w "$registry" ]; then
    bad "MIDGARD_BIFROST_REGISTRY names $registry, which is not a writable directory"
else
    ok "Bifrost registry $registry"
fi

# 4. The pinned public anchors, committed by the key ceremony (RT-35).
anchor() { # anchor FILE ALG WHAT
    local file="$1" alg="$2" what="$3"
    if [ ! -s "$file" ]; then
        bad "$file is not committed; the key ceremony (RT-35) publishes the $what's public half there"
    elif grep -qiE 'sk=|private' "$file"; then
        bad "$file holds private key material"
    elif tr -d '\r' < "$file" | grep -qE "^$alg [^ ]+ [0-9a-fA-F]+$"; then
        ok "$file pins an $alg key"
    else
        bad "$file has no \"$alg <key-id> <hex>\" line"
    fi
}
anchor deploy/trust/release-trust.pub ML-DSA-87 "release signing key"
anchor deploy/trust/bifrost-trust.pub ML-DSA-65 "Bifrost admission key"

if [ "$problems" -gt 0 ]; then
    echo "preflight: $problems problem(s); the release stops here, before anything is built or signed"
    exit 1
fi
echo "preflight: the signing runner is provisioned"
