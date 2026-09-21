#!/usr/bin/env bash
# Produce a signed offline release bundle from signed artefact binaries.
# Usage: produce.sh OUT.mgbundle ID LABEL KEYSTORE  [<component>=]<id>:<ver>:<path> ...
# [FR-B-5] [RC-19]
#
# KEYSTORE is the Bifrost admission keystore (ML-DSA-65): the bundle and every artefact
# in it are signed with it, and `mg-bundle import` accepts nothing else. The release
# workflow's bundle job packs every gate-verified binary with its .sig and .att.
set -euo pipefail

if [ "$#" -lt 5 ]; then
    echo "usage: $0 OUT.mgbundle ID LABEL KEYSTORE [<component>=]<id>:<ver>:<path> ..." >&2
    exit 2
fi
OUT="$1"; ID="$2"; LABEL="$3"; KEYSTORE="$4"; shift 4

# Build the tool if a prebuilt binary is not on PATH.
MG_BUNDLE="${MG_BUNDLE:-mg-bundle}"
command -v "$MG_BUNDLE" >/dev/null 2>&1 || MG_BUNDLE="cargo run --release -q -p mg-bundle --"

$MG_BUNDLE pack --out "$OUT" --id "$ID" --label "$LABEL" --keystore "$KEYSTORE" "$@"
echo "produced signed offline bundle: $OUT"
