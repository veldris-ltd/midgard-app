#!/bin/sh
# Verify-before-run: the detached ML-DSA signature over the service binary (plus
# its SBOM + DSSE provenance) MUST re-verify against the pinned release trust key
# before the service is exec'd. A tampered/unsigned binary fails closed here — the
# container refuses to start. This is the same fail-closed gate the release
# pipeline and Bifrost pre-deploy use, enforced one last time at run time.
# [NFR-PQ-3] [AC-B-1] [RC-18]
set -eu

SERVICE="${MIDGARD_SERVICE:?MIDGARD_SERVICE (the service binary name) must be set}"
BIN="/app/${SERVICE}"
TRUST="${MIDGARD_RELEASE_TRUST_KEY:-/trust/release-trust.pub}"
BIND="${MIDGARD_BIND:-0.0.0.0:7710}"

echo "verify-before-run: checking ${BIN} against ${TRUST}"
if ! mg-verify --trust-anchor "${TRUST}" "${BIN}"; then
    echo "::fatal:: ${SERVICE} failed PQ signature/SBOM/provenance verification — refusing to start" >&2
    exit 1
fi
# Arguments given to the container are the service's own options (e.g. mimir-server's
# --identity-dir/--state-dir); with none, the service is started on MIDGARD_BIND.
if [ "$#" -gt 0 ]; then
    echo "verify-before-run: OK — starting ${SERVICE}"
    exec "${BIN}" "$@"
fi
echo "verify-before-run: OK — starting ${SERVICE} on ${BIND}"
exec "${BIN}" "${BIND}"
