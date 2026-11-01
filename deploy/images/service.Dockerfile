# Minimal, non-root, verify-before-run runtime image for one Midgard service.
#
# It is built FROM the SIGNED release artefact: the release gate (release.yml)
# produces `<service>` + `<service>.sig` + `<service>.att` + `<service>.sbom.cdx.json`,
# and this image copies that bundle in alongside `mg-verify` and the pinned public
# trust key. The entrypoint, `mg-verify --run-service`, re-verifies the ML-DSA
# signature before exec'ing the service, so a tampered image cannot run.
# [NFR-PQ-3] [AC-B-1] [RC-18]
#
# Build context layout (assembled by the release pipeline, see release.yml):
#   bundle/<service>[.sig|.att|.sbom.cdx.json]   the signed artefact
#   mg-verify                                    the release verifier binary, and
#                                                the verify-before-run entrypoint
#   release-trust.pub                            the pinned public trust key
#
# Usage: docker build -f deploy/images/service.Dockerfile \
#          --build-arg SERVICE=http-gateway -t midgard/http-gateway:<ver> <context>
#
# Two stages, both pinned by digest. `prep`, on the offline-available toolchain image,
# only lays out the files, writes the service account and sets modes: it builds
# nothing (the binaries come from the release gate, built in the pinned toolchain and
# proven reproducible by mg-repro). The runtime stage is distroless `cc`: glibc and
# libgcc, no shell, no package manager, no compiler. It has no RUN step, because there
# is nothing to run one with. [VLD-ANB-MIDGARD-0068]
#
# The image is built to be BIT-REPRODUCIBLE: deploy/images/build.sh builds it twice,
# uncached, and requires the two image digests to be equal. Three things make that
# hold: the digest-pinned bases; `SOURCE_DATE_EPOCH` with the exporter's
# `rewrite-timestamp`, which fixes the created time and every file time in every
# layer; and nothing in a RUN step that writes the date into a file (see the service
# account below). [AC-M-6] [AC-RF-SC-5b]

ARG BASE=rust:1.82@sha256:d9c3c6f1264a547d84560e06ffd79ed7a799ce0bff0980b26cf10d29af888377
# gcr.io/distroless/cc-debian12:nonroot, by its multi-arch index digest, read with
# `docker buildx imagetools inspect gcr.io/distroless/cc-debian12:nonroot` on
# 2026-11-01 (linux/amd64 manifest sha256:777e96cf...ebbb). It MUST stay on the same
# Debian release as BASE (both bookworm, glibc 2.36): the gate-built binaries link
# glibc dynamically. build.sh starts every binary inside the built image, so a
# mismatch fails the build rather than a pod.
ARG RUNTIME=gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f

FROM ${BASE} AS prep

# Declared so BuildKit applies it to this stage's metadata and file times.
ARG SOURCE_DATE_EPOCH
ARG SERVICE

# A dedicated non-root, no-login service account. Written directly rather than with
# `useradd`, which stamps the day the account was created into /etc/shadow; the same
# Dockerfile would then build a different image tomorrow. The account has no shadow
# entry and no password: it cannot log in, which is the point.
RUN echo 'midgard:x:10001:10001::/nonexistent:/usr/sbin/nologin' >> /etc/passwd \
    && echo 'midgard:x:10001:' >> /etc/group

COPY bundle/ /app/
COPY release-trust.pub /trust/release-trust.pub
COPY mg-verify /usr/local/bin/mg-verify
# Every binary the image carries is executable: an image may carry several (forge-worker
# runs forge-egress from its own image). The signatures, attestations and SBOMs beside
# them are not.
RUN chmod 0755 /usr/local/bin/mg-verify \
    && find /app -type f ! -name '*.*' -exec chmod 0755 {} + \
    && test -x "/app/${SERVICE}" \
    && chown -R 10001:10001 /app

FROM ${RUNTIME}

# Declared so BuildKit applies it to this stage's metadata and file times.
ARG SOURCE_DATE_EPOCH
ARG SERVICE
ENV MIDGARD_SERVICE=${SERVICE}

# The toolchain image's account lines, with the service account appended.
COPY --from=prep /etc/passwd /etc/passwd
COPY --from=prep /etc/group /etc/group
COPY --from=prep --chown=10001:10001 /app /app
# The trust key and the verifier stay owned by root: the service cannot replace them.
COPY --from=prep /trust /trust
COPY --from=prep /usr/local/bin/mg-verify /usr/local/bin/mg-verify

USER 10001
EXPOSE 7710
# The root filesystem is mounted read-only by the Kubernetes securityContext; only
# an emptyDir at /tmp is writable. Nothing here writes outside /tmp.
#
# Verify-before-run, in exec form: the container's arguments follow --run-service and
# are the service's own options. The trust key is fixed here; nothing in the
# environment or the arguments can name another, and a container given no options
# refuses to start. [MG-SEC-06] [MG-SEC-14]
ENTRYPOINT ["/usr/local/bin/mg-verify", "--trust-anchor", "/trust/release-trust.pub", "--run-service"]
