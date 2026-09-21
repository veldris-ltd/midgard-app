# Minimal, non-root, verify-before-run runtime image for one Midgard service.
#
# It is built FROM the SIGNED release artefact: the release gate (release.yml)
# produces `<service>` + `<service>.sig` + `<service>.att` + `<service>.sbom.cdx.json`,
# and this image copies that bundle in alongside `mg-verify` and the pinned public
# trust key. The entrypoint re-verifies the ML-DSA signature before exec'ing the
# service, so a tampered image cannot run. [NFR-PQ-3] [AC-B-1] [RC-18]
#
# Build context layout (assembled by the release pipeline — see release.yml):
#   bundle/<service>[.sig|.att|.sbom.cdx.json]   the signed artefact
#   mg-verify                                    the release verifier binary
#   release-trust.pub                            the pinned public trust key
#   entrypoint.sh                                verify-before-run
#
# Usage: docker build -f deploy/images/service.Dockerfile \
#          --build-arg SERVICE=http-gateway -t midgard/http-gateway:<ver> <context>
#
# NOTE: the base is pinned by digest for reproducibility. Production swaps it for a
# distroless/`scratch`+musl base; here it is the offline-available toolchain image.

ARG BASE=rust:1.82@sha256:d9c3c6f1264a547d84560e06ffd79ed7a799ce0bff0980b26cf10d29af888377
FROM ${BASE}

ARG SERVICE
ENV MIDGARD_SERVICE=${SERVICE}
ENV MIDGARD_RELEASE_TRUST_KEY=/trust/release-trust.pub

# A dedicated non-root, no-login service account.
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin midgard

COPY bundle/ /app/
COPY release-trust.pub /trust/release-trust.pub
COPY mg-verify /usr/local/bin/mg-verify
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
# Every binary the image carries is executable: an image may carry several (forge-worker
# runs forge-egress from its own image). The signatures, attestations and SBOMs beside
# them are not.
RUN chmod 0755 /usr/local/bin/entrypoint.sh /usr/local/bin/mg-verify \
    && find /app -type f ! -name '*.*' -exec chmod 0755 {} + \
    && test -x "/app/${SERVICE}" \
    && chown -R 10001:10001 /app /trust

USER 10001
EXPOSE 7710
# The root filesystem is mounted read-only by the Kubernetes securityContext; only
# an emptyDir at /tmp is writable. Nothing here writes outside /tmp.
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
