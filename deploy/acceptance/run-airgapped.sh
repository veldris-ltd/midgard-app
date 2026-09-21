#!/usr/bin/env bash
# ============================================================================
# Midgard suite — full acceptance under genuine air-gap.
#
# Builds the pinned offline toolchain image ONCE (the only step that touches the
# network), then runs the entire acceptance suite — core + all product phases —
# inside `docker run --network none`. A clean pass proves the whole platform
# builds and verifies with NO outbound connectivity. [NFR-PRV-1] [AC-M-5]
#
# Dependencies already resolve offline from /vendor (see .cargo/config.toml); the
# baked rustfmt+clippy components remove the only remaining network dependency
# (rustup channel sync). See deploy/acceptance/Dockerfile for the why.
#
# Usage (Linux / CI):     bash deploy/acceptance/run-airgapped.sh
# Usage (Windows Git Bash): MSYS_NO_PATHCONV=1 REPO='D:\path\to\midgard' \
#                             bash deploy/acceptance/run-airgapped.sh
# ============================================================================
set -euo pipefail

IMAGE="${IMAGE:-midgard-acceptance:1.82.0}"
REPO="${REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
DOCKERFILE="deploy/acceptance/Dockerfile"

echo ">>> [1/2] Baking $IMAGE (one-time online step) ..."
# No COPY in the Dockerfile, so send an empty build context via stdin — avoids
# shipping target/, vendor/, and node_modules/ to the daemon.
docker build -t "$IMAGE" - < "$REPO/$DOCKERFILE"

echo ">>> [2/2] Running the full acceptance suite under --network none ..."
docker run --rm --network none -v "$REPO":/work -w /work "$IMAGE" bash -c '
    set -uo pipefail
    rc=0
    echo "### toolchain (must resolve offline) ###"
    rustup show | sed "s/^/    /"
    bash deploy/acceptance/verify-core.sh || rc=1
    for p in mimir forge bifrost heimdall admin clients; do
        bash deploy/acceptance/verify-$p.sh || rc=1
    done
    bash deploy/acceptance/verify-shells-accessibility.sh || rc=1
    echo
    if [ "$rc" -eq 0 ]; then
        echo ">>> FULL SUITE PASSED, AIR-GAPPED (--network none)."
    else
        echo ">>> FULL SUITE FAILED (fail closed)."
    fi
    exit "$rc"
'
