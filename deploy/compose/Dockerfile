# Development image for docker compose: the Midgard binaries compose runs, built
# from this source tree against the vendored crates, with no network during the
# build. [NFR-OSS-1] [NFR-PRV-1] [RC-33]
#
# This is NOT the release image. deploy/images/service.Dockerfile is built from a
# signed release artefact and re-verifies its signature before it runs; this one
# is built from whatever is checked out. Use it for development and evaluation.
#
# The build stage is the pinned acceptance toolchain, which already carries the
# components rust-toolchain.toml asks for. Build it once with
#   docker build -t midgard-acceptance:1.82.0 deploy/acceptance
# The runtime stage is a slim Debian of the same release the toolchain is built
# on, pinned by digest.
#
# Build context: the repository root (see /.dockerignore).

ARG TOOLCHAIN=midgard-acceptance:1.82.0
ARG RUNTIME=debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171

FROM ${TOOLCHAIN} AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/src/target,id=midgard-compose-target \
    cargo build --release --offline --locked \
        -p http-gateway -p mg-admin -p mimir-server -p bifrost-fleet-server \
        -p heimdall-feed-server -p forge-worker -p mg-model --bins \
    && cargo build --release --offline --locked \
        -p pq-mtls --features backend-fips --example pqmtls_probe \
    && mkdir -p /out \
    && cp target/release/examples/pqmtls_probe /out/pqmtls-probe \
    && cp target/release/http-gateway target/release/mg-admin \
          target/release/mimir-server target/release/mimir-probe \
          target/release/bifrost-fleet target/release/bifrost-agent \
          target/release/heimdall-feed target/release/heimdall-source \
          target/release/forge-worker target/release/forge-egress \
          target/release/mg-model /out/

FROM ${RUNTIME}

# A dedicated non-root, no-login service account, and the mount points its volumes
# are created from (a new named volume takes this ownership).
# -K lets this system account take uid 10001 without useradd warning that it is above
# the distribution's system range.
RUN useradd --system -K SYS_UID_MAX=10001 --uid 10001 --no-create-home --shell /usr/sbin/nologin midgard \
    && mkdir -p /state /identity /vol/core-state /vol/mimir-identity /vol/mimir-state \
                /vol/probe-identity /vol/fleet-identity /vol/fleet-state /vol/fleet-clients \
                /vol/heimdall-identity /vol/heimdall-state /vol/heimdall-clients \
                /vol/gateway-identity /vol/forge-identity /mesh-identity /serve \
    && chown 10001:10001 /state /identity /vol/core-state /vol/mimir-identity \
                         /vol/mimir-state /vol/probe-identity /vol/fleet-identity \
                         /vol/fleet-state /vol/fleet-clients /vol/heimdall-identity \
                         /vol/heimdall-state /vol/heimdall-clients /vol/gateway-identity \
                         /vol/forge-identity /mesh-identity /serve \
    && chmod 0755 /serve

COPY --from=build /out/ /usr/local/bin/
COPY deploy/compose/init.sh /usr/local/bin/midgard-init
COPY deploy/compose/ready.sh /usr/local/bin/midgard-ready
COPY deploy/compose/mimir-policy.json deploy/compose/fleet-policy.json \
     deploy/compose/fleet-channels.json deploy/compose/heimdall-policy.json \
     deploy/compose/core-ontology.json deploy/compose/core-policy.json /etc/midgard/
COPY deploy/compose/forge/ /etc/midgard/forge/
RUN chmod 0755 /usr/local/bin/midgard-init /usr/local/bin/midgard-ready

USER 10001
