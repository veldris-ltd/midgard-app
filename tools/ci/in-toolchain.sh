#!/usr/bin/env bash
# Run a command in the digest-pinned toolchain image, with no network, on this checkout.
# [NFR-PRV-1] [NFR-OSS-1] [AC-M-6] [RT-47]
#
#   docker build -t midgard-acceptance:1.82.0 deploy/acceptance   # once, while connected
#   bash tools/ci/in-toolchain.sh just test
#   bash tools/ci/in-toolchain.sh --mount /keys/release.keystore:/keys/release.keystore:ro \
#       --env MIDGARD_SIGNING_KEYSTORE=/keys/release.keystore -- just gate mg-sbom
#
# The image (deploy/acceptance/Dockerfile) carries Rust 1.82.0 with rustfmt and clippy,
# and just; the workspace builds from its vendored crates. So every command runs with
# `--network none`: a step that needed the network would fail here rather than quietly
# fetch something. The checkout is mounted at /work, and target/ persists between
# commands. CI runs every build, test, lint and reproducibility step this way.
#
# --mount SRC:DST[:ro] and --env NAME=VALUE, repeated, hand the container a file or a
# setting it does not otherwise have — key material mounted read-only for a signing
# step, for instance. They come before the command; `--` ends them. Nothing is passed
# through implicitly: the container sees only what is named here.
#
# Mounts the checkout directly, so it suits a Linux host (CI's runners). TOOLCHAIN_IMAGE
# names another tag.
set -euo pipefail

cd "$(dirname "$0")/../.."
IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"

extra=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --mount) extra+=(-v "${2:?--mount needs SRC:DST[:ro]}"); shift 2 ;;
        --env) extra+=(-e "${2:?--env needs NAME=VALUE}"); shift 2 ;;
        --) shift; break ;;
        *) break ;;
    esac
done

[ "$#" -gt 0 ] || { echo "usage: $0 [--mount SRC:DST[:ro]]... [--env NAME=VALUE]... [--] COMMAND [ARG...]" >&2; exit 2; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || {
    echo "in-toolchain: build the toolchain image first: docker build -t $IMAGE deploy/acceptance" >&2
    exit 1
}
# The source revision, read here because the container cannot: the checkout is mounted
# from another user, and git declines to open a repository it does not own. The
# signing step records it in the provenance attestation.
revision="${MIDGARD_SOURCE_REVISION:-$(git rev-parse HEAD 2>/dev/null || true)}"
[ -z "$revision" ] || extra+=(-e "MIDGARD_SOURCE_REVISION=$revision")

exec docker run --rm --network none -v "$PWD:/work" -w /work \
    -e CARGO_TERM_COLOR=always "${extra[@]}" "$IMAGE" "$@"
