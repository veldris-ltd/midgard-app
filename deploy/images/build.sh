#!/usr/bin/env bash
# Build the verify-before-run release images from gate-verified artefacts.
# [RC-18] [AC-B-1] [NFR-PQ-3] [RC-07]
#
#   bash deploy/images/build.sh --version V --verified DIR --trust FILE \
#       [--registry REGISTRY] [--out DIR]
#
# For each image named in supply-chain/release-artefacts.txt it:
#   1. assembles a build context from the gate's output (DIR holds, per binary,
#      <bin>, <bin>.sig, <bin>.att and <bin>.sbom.cdx.json), the gate-verified
#      mg-verify, the pinned trust key and the entrypoint;
#   2. builds deploy/images/service.Dockerfile FROM its digest-pinned base, with no
#      network, TWICE and uncached, and requires the two image digests to be equal —
#      the image's reproducibility stage, as mg-repro is a binary's;
#   3. runs mg-verify inside the built image against every binary it carries, exactly
#      as the entrypoint will before it starts one — a binary that would not start is
#      caught here, not on a cluster.
#
# It writes <out>/<image>.oci.tar, the image as an OCI archive, and <out>/images.lock:
# one "<registry>/<image>:<version> <image digest>" line per image, sorted, which the
# release workflow signs like any other artefact. Nothing is pushed. Default registry:
# registry.enclosure.local/midgard (the chart's default). Needs docker with buildx.
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
# docker may be a Windows binary under Git Bash: hand it native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }
version="" verified="" trust="" registry=registry.enclosure.local/midgard out=target/images
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version) version="$2"; shift 2 ;;
        --verified) verified="$2"; shift 2 ;;
        --trust) trust="$2"; shift 2 ;;
        --registry) registry="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        *) echo "build: unknown option $1" >&2; exit 2 ;;
    esac
done
[ -n "$version" ] && [ -n "$verified" ] && [ -n "$trust" ] || {
    echo "usage: $0 --version V --verified DIR --trust FILE [--registry R] [--out DIR]" >&2
    exit 2
}
[ -s "$trust" ] || { echo "build: trust key $trust is missing or empty" >&2; exit 1; }
for f in mg-verify mg-verify.sig mg-verify.att mg-verify.sbom.cdx.json; do
    [ -f "$verified/$f" ] || { echo "build: $verified/$f is missing (mg-verify must pass the gate first)" >&2; exit 1; }
done

# The image's identity: the digest of its manifest, read from the archive's index.
image_digest() { tar -xOf "$1" index.json | grep -o 'sha256:[0-9a-f]\{64\}' | head -1; }

images="$(bash tools/ci/release-artefacts.sh --images | tr -d '[]"' | tr ',' ' ')"
mkdir -p "$out"
lock="$out/images.lock"
: > "$lock.unsorted"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for image in $images; do
    bins="$(bash tools/ci/release-artefacts.sh --bins "$image")"
    ctx="$work/$image"
    mkdir -p "$ctx/bundle"
    for bin in $bins; do
        for ext in "" .sig .att .sbom.cdx.json; do
            [ -f "$verified/$bin$ext" ] || {
                echo "build: $verified/$bin$ext is missing — $bin has not passed the gate" >&2
                exit 1
            }
            cp "$verified/$bin$ext" "$ctx/bundle/"
        done
    done
    cp "$verified/mg-verify" "$ctx/mg-verify"
    cp "$trust" "$ctx/release-trust.pub"
    # A checkout with CRLF line endings would give the entrypoint a `#!/bin/sh\r` line,
    # which no Linux kernel can exec: every container would die before verifying.
    tr -d '\r' < deploy/images/entrypoint.sh > "$ctx/entrypoint.sh"

    tag="$registry/$image:$version"
    echo "--- $tag ($(echo $bins))"
    # Twice, uncached, into OCI archives. SOURCE_DATE_EPOCH fixes the created time
    # and, with rewrite-timestamp, every file time in every layer; what is left is
    # content, and two builds of the same content must be the same image.
    for run in 1 2; do
        docker buildx build --network none --no-cache -f deploy/images/service.Dockerfile \
            --build-arg SERVICE="$image" --build-arg SOURCE_DATE_EPOCH=1 \
            --output "type=oci,dest=$(native "$work/$image.$run.tar"),name=$tag,rewrite-timestamp=true" \
            "$(native "$ctx")"
    done
    digest="$(image_digest "$work/$image.1.tar")"
    second="$(image_digest "$work/$image.2.tar")"
    [ -n "$digest" ] || { echo "build: could not read the digest of $tag" >&2; exit 1; }
    if [ "$digest" != "$second" ]; then
        echo "build: $tag is NOT reproducible: two builds gave $digest and $second" >&2
        exit 1
    fi
    echo "  reproducible: $digest"
    cp "$work/$image.1.tar" "$out/$image.oci.tar"
    docker load -i "$(native "$out/$image.oci.tar")" >/dev/null
    # Through the image's own entrypoint, exactly as a pod starts it: the entrypoint must
    # run, verify each binary it carries, and hand over to it. The binary is given
    # --help so it exits at once; only the entrypoint's verdict is checked.
    for bin in $bins; do
        started="$(docker run --rm --network none -e MIDGARD_SERVICE="$bin" "$tag" --help 2>&1 || true)"
        grep -q "verify-before-run: OK — starting $bin" <<<"$started" || {
            echo "build: $tag did not verify and start $bin through its entrypoint:" >&2
            sed 's/^/  /' <<<"$started" >&2
            exit 1
        }
        echo "  $bin: verified before run"
    done
    echo "$tag $digest" >> "$lock.unsorted"
done

sort "$lock.unsorted" > "$lock"
rm -f "$lock.unsorted"
echo "build: $(wc -l < "$lock" | tr -d ' ') image(s), each built twice to the same digest and verified inside itself -> $lock"
