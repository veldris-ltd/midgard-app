#!/usr/bin/env bash
# Run a command in the digest-pinned toolchain image, with no network, on this checkout.
# [NFR-PRV-1] [NFR-OSS-1] [AC-M-6] [RT-47]
#
#   docker build -t midgard-acceptance:1.82.0 deploy/acceptance   # once, while connected
#   bash tools/ci/in-toolchain.sh just test
#
# The image (deploy/acceptance/Dockerfile) carries Rust 1.82.0 with rustfmt and clippy,
# and just; the workspace builds from its vendored crates. So every command runs with
# `--network none`: a step that needed the network would fail here rather than quietly
# fetch something. The checkout is mounted at /work, and target/ persists between
# commands. CI runs every build, test, lint and reproducibility step this way.
#
# Mounts the checkout directly, so it suits a Linux host (CI's runners). TOOLCHAIN_IMAGE
# names another tag.
set -euo pipefail

cd "$(dirname "$0")/../.."
IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"
[ "$#" -gt 0 ] || { echo "usage: $0 COMMAND [ARG...]" >&2; exit 2; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || {
    echo "in-toolchain: build the toolchain image first: docker build -t $IMAGE deploy/acceptance" >&2
    exit 1
}
exec docker run --rm --network none -v "$PWD:/work" -w /work \
    -e CARGO_TERM_COLOR=always "$IMAGE" "$@"
