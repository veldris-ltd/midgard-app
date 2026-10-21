# A slim image for running a local Midgard enclosure from a host checkout: the binaries
# are not copied in but bind-mounted (the checkout at /work, run as
# ./target/release/<binary>), so a rebuild in the toolchain image deploys with a
# container restart, and the 2.5 GB toolchain image need not stay on disk while nothing
# is being built. The release binaries need only glibc and libgcc, both in the same
# pinned Debian the compose image runs on. socat forwards a container's loopback
# port to a model server on the host, so Mimir's sidecar stays on loopback.
#
#   docker build -t midgard-local-runtime:1 deploy/local-runtime
FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
RUN apt-get update \
    && apt-get install -y --no-install-recommends socat ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /work
