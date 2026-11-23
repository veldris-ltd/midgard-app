#!/usr/bin/env bash
# The deploy kit: the scripts and pinned files an install from the air-gap bundle runs,
# as one tar the bundle carries and signs. [FR-B-5] [VLD-ANB-MIDGARD-0035]
#
#   bash deploy/air-gap-bundle/kit.sh OUT.tar
#
# It holds, at their paths in this repository: install.sh; set-digests.sh; externals.sh
# and externals.lock; build.sh and service.Dockerfile with release-artefacts.sh, the
# annotation list and the release's rows (supply-chain/release-rows.txt, the output of
# `release-artefacts.sh --rows` here), which install.sh --rebuild needs where there is no
# workspace to derive them from; and deploy/trust/, which the images embed, with the
# release anchor they were built with (MIDGARD_TRUST_ANCHORS, as for release-images, or
# deploy/trust/release-trust.pub) as release-trust.pub. A kit without one is refused.
# The anchors that verify the bundle and the release are never taken from it: install.sh
# checks the kit's against the operator's (README.md beside this file).
#
# EXTERNALS_LOCK and RELEASE_ARTEFACTS_ROWS name another lock and a frozen copy of the
# rows, as for externals.sh and release-artefacts.sh. Text is written with LF line
# endings, whatever the checkout's, and the tar is reproducible: sorted, with fixed
# times and owners.
set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: $0 OUT.tar" >&2; exit 2; }
out="$1"
cd "$(dirname "$0")/../.."

files=(deploy/air-gap-bundle/install.sh deploy/helm/set-digests.sh
       deploy/images/externals.sh deploy/images/build.sh deploy/images/service.Dockerfile
       tools/ci/release-artefacts.sh supply-chain/release-artefacts.txt)
while IFS= read -r f; do files+=("$f"); done < <(find deploy/trust -type f | sort)

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
for f in "${files[@]}"; do
    [ -f "$f" ] || { echo "kit: $f is missing" >&2; exit 1; }
    mkdir -p "$stage/$(dirname "$f")"
    tr -d '\r' < "$f" > "$stage/$f"
done
# The lock the bundle's external images were packed against (EXTERNALS_LOCK, as for
# externals.sh), and the rows (RELEASE_ARTEFACTS_ROWS, a frozen copy, or derived here).
lock="${EXTERNALS_LOCK:-deploy/images/externals.lock}"
[ -f "$lock" ] || { echo "kit: $lock is missing" >&2; exit 1; }
tr -d '\r' < "$lock" > "$stage/deploy/images/externals.lock"
mkdir -p "$stage/supply-chain"
if [ -n "${RELEASE_ARTEFACTS_ROWS:-}" ]; then
    tr -d '\r' < "$RELEASE_ARTEFACTS_ROWS" > "$stage/supply-chain/release-rows.txt"
else
    bash tools/ci/release-artefacts.sh --rows 2>/dev/null > "$stage/supply-chain/release-rows.txt"
fi
[ -s "$stage/supply-chain/release-rows.txt" ] || { echo "kit: release-artefacts.sh --rows gave nothing" >&2; exit 1; }
anchor="${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}"
[ -s "$anchor" ] || { echo "kit: the release anchor $anchor is missing; the key ceremony publishes it (deploy/trust/README.md)" >&2; exit 1; }
mkdir -p "$stage/deploy/trust"
tr -d '\r' < "$anchor" > "$stage/deploy/trust/release-trust.pub"

mkdir -p "$(dirname "$out")"
tar -C "$stage" --sort=name --mtime=@1 --owner=0 --group=0 --numeric-owner \
    --mode='u+rwX,go+rX,go-w' -cf "$out" deploy tools supply-chain
echo "kit: $(tar -tf "$out" | grep -vc '/$') file(s) -> $out"
