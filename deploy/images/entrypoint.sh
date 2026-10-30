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

# Safe by default: a service started with no options is refused, never started on a
# default address. The gateway in particular needs its state directory (--state-dir);
# its development mode (--dev) listens on loopback only, so it is of no use in an
# image. [MG-SEC-06]
if [ "$#" -eq 0 ]; then
    echo "::fatal:: ${SERVICE} was started with no options — refusing to start. Pass the service's own options as the container's arguments (for http-gateway: --state-dir DIR --bind ADDR ...)." >&2
    exit 2
fi

echo "verify-before-run: checking ${BIN} against ${TRUST}"
if ! mg-verify --trust-anchor "${TRUST}" "${BIN}"; then
    echo "::fatal:: ${SERVICE} failed PQ signature/SBOM/provenance verification — refusing to start" >&2
    exit 1
fi
# Arguments given to the container are the service's own options (e.g. mimir-server's
# --identity-dir/--state-dir).
echo "verify-before-run: OK — starting ${SERVICE}"
exec "${BIN}" "$@"
