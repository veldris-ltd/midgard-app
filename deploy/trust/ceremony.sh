#!/usr/bin/env bash
# The release key ceremony, as one run. [RT-35] [AC-RF-CA-6a] [NFR-PQ-3]
#
#   bash deploy/trust/ceremony.sh                 the guided ceremony: asks one question at a time
#   bash deploy/trust/ceremony.sh --custody DIR --signer PRINCIPAL=KEY.pub [--signer ...]
#                                 [--year YYYY] [--officer NAME] [--witness NAME]
#
# docs/ops/KEY_CEREMONY.md is the procedure. The ceremony itself is tools/mg-ceremony,
# the same program Windows runs as mg-ceremony.exe; this builds it from this checkout,
# offline, and runs it against this checkout. With flags it runs `mg-ceremony run`;
# with none, the guided ceremony. MG_CEREMONY names a prebuilt binary instead.
#
# It generates the release signing key (ML-DSA-87) and the Bifrost admission key
# (ML-DSA-65) into sealed keystores in DIR, outside this checkout; writes
# deploy/trust/release-trust.pub, bifrost-trust.pub and allowed_signers; checks them as
# the release does; and writes the record under docs/ops/ceremonies/. It commits
# nothing, pushes nothing and uploads nothing. If any published file already exists it
# refuses: rotation appends a key beside the old one (deploy/trust/README.md).
set -uo pipefail

cd "${CEREMONY_ROOT:-$(dirname "$0")/../..}" || exit 2

case "${1:-}" in
    -h | --help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
esac

if [ -n "${MG_CEREMONY:-}" ]; then
    mg_ceremony="$MG_CEREMONY"
else
    command -v cargo >/dev/null 2>&1 || {
        echo "ceremony: no cargo to build mg-ceremony; build it in the toolchain image and set MG_CEREMONY to it" >&2
        exit 1
    }
    echo "ceremony: building mg-ceremony from this checkout, offline" >&2
    cargo build --quiet --offline --release -p mg-ceremony || { echo "ceremony: mg-ceremony did not build" >&2; exit 1; }
    mg_ceremony=target/release/mg-ceremony
    [ -x "$mg_ceremony" ] || mg_ceremony="$mg_ceremony.exe"
fi

if [ "$#" -eq 0 ]; then
    exec "$mg_ceremony"
fi
exec "$mg_ceremony" run --repo . "$@"
