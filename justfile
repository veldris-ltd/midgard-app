# Midgard suite — task runner. Platform-agnostic entry points the CI gate calls.
# Every buildable artefact flows through: build -> repro -> sbom -> scan -> sign -> verify.
# [NFR-OSS-1] [NFR-PQ-3] [NFR-PRV-1]

set shell := ["bash", "-uc"]

# Default: list tasks.
default:
    @just --list

# --- Workspace-wide -------------------------------------------------------

# Generate code from the proto contracts (source of truth for all SDKs). [DD-6]
gen:
    buf lint contracts
    buf generate contracts

# Guard the contract against breaking changes.
proto-breaking:
    buf breaking contracts --against '.git#branch=main,subdir=contracts'

# Build the whole workspace (each language builds its own members).
build: build-rust build-go build-ts build-py

build-rust:
    cargo build --workspace --release

build-go:
    @if [ -f go.work ] && command -v go >/dev/null 2>&1; then go build ./... ; else echo "no go modules"; fi

build-ts:
    @if [ -f pnpm-workspace.yaml ]; then pnpm -r build || true ; fi

build-py:
    @uv sync --frozen || echo "no python members yet"

# --- UI edges (dev) -------------------------------------------------------
# Run the browser-facing HTTP/JSON edge for the console UIs. Point a console at
# it with `VITE_GATEWAY=http pnpm --filter @midgard/console-midgard dev`.
gateway-http addr="127.0.0.1:7710":
    cargo run -p http-gateway -- {{addr}}

# Run the Midgard console against the real HTTP edge (start `just gateway-http` first).
console-midgard-http:
    VITE_GATEWAY=http pnpm --filter @midgard/console-midgard dev

# --- Services on one machine (docker compose) -----------------------------
# The HTTP edge and the Mimir inference plane, built from source into hardened
# containers (see deploy/compose/compose.yaml). Needs the acceptance toolchain image:
# `docker build -t midgard-acceptance:1.82.0 deploy/acceptance`.
compose-up:
    docker compose -f deploy/compose/compose.yaml up -d --build --wait

compose-down:
    docker compose -f deploy/compose/compose.yaml down

# One PQ-mTLS inference call to the running mimir-server, as a certified consumer.
compose-probe:
    docker compose -f deploy/compose/compose.yaml run --rm probe

# Build, start, probe and restart a throwaway compose enclosure, then tear it down.
compose-smoke:
    bash deploy/compose/smoke.sh

# Stage the default Granite model (Apache-2.0) for compose: copy the downloaded GGUF
# into the models volume, sign it with a development model key and verify it.
model-stage GGUF="":
    bash deploy/compose/stage-model.sh {{GGUF}}

# Serve Granite through compose on throwaway state: a certified inference call must be
# answered by the real model. Needs the llama.cpp server image pinned in
# deploy/compose/compose.granite.yaml.
compose-granite-smoke GGUF="":
    bash deploy/compose/granite-smoke.sh {{GGUF}}

# --- Services on a local Kubernetes cluster (kind + Helm) ----------------
# A three-node kind cluster running the chart in deploy/helm/midgard from an image built
# from this checkout. Needs kind, helm and kubectl on PATH, and the acceptance
# toolchain image (see compose-up).
kind-up:
    bash deploy/kind/up.sh

# Run each mesh client as a Job against the kind deployment.
kind-smoke:
    bash deploy/kind/smoke.sh

# Serve Granite on the kind cluster: load the pinned llama.cpp image and the model staged
# for compose (`just model-stage`) into the cluster, verify it there, and turn on the
# chart's modelServing. Keep it across `kind-up` with MIDGARD_KIND_MODEL=granite.
kind-granite:
    bash deploy/kind/model.sh

kind-down:
    kind delete cluster --name midgard

# Regenerate deploy/k8s/enclosure.yaml from the chart (CI checks that it matches).
render-k8s:
    bash deploy/k8s/render.sh

# The Forge PostgreSQL and S3 connectors against real servers (PostgreSQL 16 and
# MinIO in throwaway containers on a private network). Needs postgres:16, minio/minio
# and minio/mc locally.
live-connectors:
    bash deploy/acceptance/live-connectors.sh

# The deployment-tier acceptance suite (RC-40) against the cluster KUBECONFIG points
# at. Fails closed without real infrastructure; see deploy/acceptance/deployment.
deploy-accept:
    bash deploy/acceptance/deployment/run-deployment-tier.sh

# --- Code hygiene ---------------------------------------------------------

# Format the workspace. Settings are pinned in rustfmt.toml (defaults only), and
# the toolchain in rust-toolchain.toml, so this is deterministic across machines.
fmt:
    cargo fmt --all

# Fail if anything is unformatted. CI runs this. [NFR-OSS-1]
fmt-check:
    cargo fmt --all --check

# Lint at deny-warnings.
#
# IMPORTANT: like `test`, this CANNOT be `--workspace --all-targets` in one shot.
# The crypto crates gate their backend behind mutually-exclusive features and a
# `compile_error!` guards the combination, so a single workspace invocation
# unifies `test-backend` with `backend-fips` and fails to compile. The groupings
# below mirror `test`'s.
lint:
    cargo clippy -p pqc-agility -p pq-mtls -p audit-writer --features test-backend --all-targets -- -D warnings
    cargo clippy -p pqc-agility --features backend-fips --all-targets -- -D warnings
    cargo clippy -p policy -p iam -p gateway -p http-gateway -p service-runtime -p kms -p governance -p fabric -p scheduler --all-targets -- -D warnings
    cargo clippy -p attest -p mg-sign -p mg-verify -p mg-sbom -p mg-scan -p mg-repro -p mg-bundle -p mg-register -p mg-admin --all-targets -- -D warnings
    cargo clippy -p model-server -p mimir-server -p staging -p mimir-prompts -p oag-retriever -p agent-runtime --features backend-fips --all-targets -- -D warnings
    cargo clippy -p forge-engine -p forge-dataset-store -p forge-connector-sdk -p forge-pipeline -p forge-health -p forge-projection -p forge-ai-assist -p forge-worker --features backend-fips --all-targets -- -D warnings
    cargo clippy -p bifrost-rollout -p bifrost-fleet -p bifrost-fleet-server -p bifrost-risk -p bifrost-artefact -p bifrost-bundle --features backend-fips --all-targets -- -D warnings
    cargo clippy -p heimdall-picture -p heimdall-classify -p heimdall-decision-support -p heimdall-feed-gateway -p heimdall-feed-server -p heimdall-fusion -p heimdall-picture-sync -p heimdall-workflow --features backend-fips --all-targets -- -D warnings
    cargo clippy -p admin-identity -p admin-privilege -p admin-sim -p admin-config -p admin-breakglass -p admin-audit -p admin-ai-assist -p admin-service-identity --features backend-fips --all-targets -- -D warnings
    cargo clippy -p midgard-client-sync -p midgard-client-keystore -p midgard-client-store -p midgard-client-broker -p midgard-client-delivery -p midgard-client-core --features backend-fips --all-targets -- -D warnings
    cargo clippy -p midgard-client-enclosure -p midgard-client-inference --features test-backend --all-targets -- -D warnings

# Run all tests.
# IMPORTANT: the crypto crates gate their backend tests behind a feature; running
# `cargo test --workspace` with no features would compile those tests out (a green
# build that tested nothing). Exercise BOTH backends explicitly:
#   - `test-backend` — the deterministic INSECURE backend, for the shim/handshake/
#                      audit logic tests.
#   - `backend-fips` — the production pure-Rust FIPS 203/204/205 + X25519 backend.
# They are mutually exclusive (a compile_error guards it), so they run in separate
# invocations. Do NOT use `--all-features`.
test:
    cargo test -p pqc-agility -p pq-mtls -p audit-writer --features test-backend
    cargo test -p pqc-agility --features backend-fips --test fips_backend
    cargo test -p pqc-agility --features backend-fips --test aead_kat
    cargo test -p pqc-agility --features backend-fips --test agility_fips
    cargo test -p pq-mtls --features backend-fips --test real_fips_handshake
    cargo test -p pq-mtls --features backend-fips --test tcp_transport
    cargo test -p policy
    cargo test -p iam
    cargo test -p gateway
    cargo test -p http-gateway
    cargo test -p service-runtime
    cargo test -p kms
    cargo test -p governance
    cargo test -p fabric
    cargo test -p scheduler
    cargo test -p model-server --features backend-fips
    cargo test -p mimir-server --features backend-fips
    cargo test -p mimir-prompts -p oag-retriever -p agent-runtime --features backend-fips
    cargo test -p staging --features backend-fips
    cargo test -p forge-engine
    cargo test -p forge-dataset-store -p forge-connector-sdk -p forge-pipeline --features backend-fips
    cargo test -p forge-health -p forge-projection --features backend-fips
    cargo test -p forge-ai-assist
    cargo test -p forge-worker --features backend-fips
    cargo test -p bifrost-rollout -p bifrost-fleet -p bifrost-risk
    cargo test -p bifrost-fleet-server --features backend-fips
    cargo test -p bifrost-artefact -p bifrost-bundle --features backend-fips
    cargo test -p heimdall-picture -p heimdall-classify -p heimdall-decision-support
    cargo test -p heimdall-feed-gateway -p heimdall-fusion -p heimdall-picture-sync -p heimdall-workflow --features backend-fips
    cargo test -p heimdall-feed-server --features backend-fips
    cargo test -p admin-identity
    cargo test -p admin-privilege --features backend-fips
    cargo test -p admin-sim
    cargo test -p admin-config
    cargo test -p admin-breakglass
    cargo test -p admin-audit --features backend-fips
    cargo test -p admin-ai-assist
    cargo test -p admin-service-identity
    cargo test -p midgard-client-sync
    cargo test -p midgard-client-keystore --features backend-fips
    cargo test -p midgard-client-store --features backend-fips
    cargo test -p midgard-client-enclosure --features test-backend
    cargo test -p midgard-client-inference --features test-backend
    cargo test -p midgard-client-broker
    cargo test -p midgard-client-delivery --features backend-fips
    cargo test -p midgard-client-core --features backend-fips
    cargo test -p attest
    cargo test -p attest --no-default-features --features backend-fips
    cargo test -p mg-sign -p mg-sbom -p mg-scan -p mg-verify -p mg-repro
    cargo test -p mg-admin
    # go.work exists but declares no modules and there are no .go files, so this
    # is a no-op that only ever fails when Go is absent. Whether the Go workspace
    # stays at all is RF-SDK-4.
    @if [ -f go.work ] && command -v go >/dev/null 2>&1; then go test ./... ; fi

# Mimir acceptance verification (grows per Phase-2 prompt). Prompt 2.b: AC-MI-1,
# AC-MI-8. Run under `docker --network none` to also demonstrate AC-MI-1's
# no-outbound-dependency claim concretely.
verify-mimir:
    bash deploy/acceptance/verify-mimir.sh

# Consolidated acceptance verification, AC-M-1..9 (the Phase-1 Definition of Done).
# Maps each criterion to its tests, runs them, and reports PASS/FAIL with real
# output. Run under `docker --network none` to also demonstrate AC-M-5 (air-gap).
verify-core:
    bash deploy/acceptance/verify-core.sh

# Forge acceptance verification, AC-F-1..7 + EV-A6-* (the Phase-4 Definition of
# Done). Maps each criterion to its tests, runs them, and reports PASS/FAIL with
# real output. Run under `docker --network none` to also demonstrate AC-F-7.
verify-forge:
    bash deploy/acceptance/verify-forge.sh

# Bifrost acceptance verification, AC-B-1..6 + EV-A7-* (the Phase-5 Definition of
# Done). Maps each criterion to its tests, runs them, and reports PASS/FAIL with
# real output. Run under `docker --network none` to also demonstrate the air-gap.
verify-bifrost:
    bash deploy/acceptance/verify-bifrost.sh

# Admin Control Panel acceptance verification, the full AC-AD-1..13 matrix (the
# Phase-7 Definition of Done). Maps each criterion to its tests, runs them, and
# reports PASS/FAIL with real output. Run under `docker --network none` to also
# demonstrate AC-AD-12 (break-glass + audit air-gapped).
verify-admin:
    bash deploy/acceptance/verify-admin.sh

# Heimdall acceptance verification, AC-H-1,2,5,6,7 (the Phase-6.b Definition of
# Done: the fusion core). Maps each criterion to its tests, runs them, and reports
# PASS/FAIL with real output. Run under `docker --network none` to also demonstrate
# AC-H-7 (air-gap).
verify-heimdall:
    bash deploy/acceptance/verify-heimdall.sh

# Client Platforms acceptance verification. Prompt 8.b lands the shared-core
# subset of the Phase-8 Definition of Done: AC-C-1,2,3,4,13 (one core, at-rest
# seal, PQ-mTLS fail-closed, offline CRDT sync, clearance scoping). Maps each
# criterion to its tests and reports PASS/FAIL. Run under `docker --network none`
# to also demonstrate the air-gap posture. AC-C-5..12 land in Prompts 8.c/8.d.
verify-clients:
    bash deploy/acceptance/verify-clients.sh

# --- The release gate (§2 of the 0.b proposal) ----------------------------
# Each stage FAILS CLOSED. Run per artefact in CI; runnable locally for one.
#
# Two kinds of name appear below and they are NOT interchangeable:
#   <pkg>  a workspace package, e.g. `mg-sbom`   — what `repro` builds
#   <path> the built deployable, e.g. `target/release/mg-sbom` — what `sbom`,
#          `sign` and `verify` operate on, because they hash actual bytes.
# `gate` takes a package and derives the path. Unifying artefact identity across
# the whole gate is RF-SC-5.

# 1+2. Reproducibility: build twice, assert every artefact is bit-identical.
# No argument checks the WHOLE workspace (two builds, not two per package) and
# writes repro.json, which mg-sign embeds into the provenance attestation.
# A package name, or `--only a,b`, narrows it. [AC-M-6]
repro pkg="" jobs="":
    cargo run -q --release -p mg-repro -- {{pkg}} --out repro.json {{ if jobs == "" { "" } else { "--jobs " + jobs } }}

# 3. Emit a deterministic CycloneDX SBOM *beside the artefact*, which is where
# `sign` and `verify` look for the bill of materials they bind. [AC-B-4]
sbom path:
    cargo run -q --release -p mg-sbom -- --out "{{path}}.sbom.cdx.json" "{{path}}"

# 4. Vulnerability + licence scan over the SBOM. Sits between `sbom` and `sign`
# so a failing scan can never reach a signature. Offline: the advisory database
# is vendored. [FR-MS-6] [NFR-OSS-1]
scan path:
    cargo run -q --release -p mg-scan -- "{{path}}.sbom.cdx.json" --out scan-findings.json

# Advisory-only mode for PRs: reports everything, blocks only on CRITICAL, so
# exposure surfaces at review time without stalling unrelated work.
scan-advisory path:
    cargo run -q --release -p mg-scan -- "{{path}}.sbom.cdx.json" --advisory-only

# Refresh the vendored advisory database. THE ONLY ONLINE STEP in the gate,
# mirroring deploy/acceptance/Dockerfile's posture. Commit the result and review
# the diff — a shrinking advisory count means something went wrong.
advisory-db-refresh:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    git clone -q --depth 1 https://github.com/rustsec/advisory-db.git "$tmp/db"
    rev=$(git -C "$tmp/db" rev-parse HEAD)
    date=$(git -C "$tmp/db" show -s --format=%cs HEAD)
    rm -rf vendor/advisory-db/crates
    cp -r "$tmp/db/crates" vendor/advisory-db/
    cp "$tmp/db/LICENSE.txt" vendor/advisory-db/
    n=$(find vendor/advisory-db/crates -name 'RUSTSEC-*.md' | wc -l | tr -d ' ')
    sed -i.bak \
        -e "s|^revision = .*|revision = \"$rev\"|" \
        -e "s|^date = .*|date = \"$date\"|" \
        -e "s|^advisory_count = .*|advisory_count = $n|" \
        vendor/advisory-db/snapshot.toml
    rm -f vendor/advisory-db/snapshot.toml.bak
    echo "advisory-db: $n advisories at $rev ($date)"

# 5. PQ-sign an artefact with ML-DSA (detached sig + DSSE attestation). [NFR-PQ-3]
#
# The signing key is named explicitly and is never searched for: set
# MIDGARD_SIGNING_KEYSTORE to a keystore produced by `just keygen`. Unset means
# the stage fails, which is the point — a release that cannot sign must not pass.
sign path:
    #!/usr/bin/env bash
    set -euo pipefail
    test -n "${MIDGARD_SIGNING_KEYSTORE:-}" || { echo "MIDGARD_SIGNING_KEYSTORE is unset — refusing to sign" >&2; exit 1; }
    # Fold in mg-repro's report when one exists, so the attestation records which
    # artefacts were proven bit-reproducible for this build.
    repro=()
    [ -n "${MIDGARD_REPRO_REPORT:-}" ] && [ -f "${MIDGARD_REPRO_REPORT}" ] && repro=(--repro "${MIDGARD_REPRO_REPORT}")
    cargo run -q --release -p mg-sign -- sign --keystore "$MIDGARD_SIGNING_KEYSTORE" "${repro[@]}" "{{path}}"

# Generate a signing keystore and export its trust anchor. Local/dev use; real
# key ceremonies, rotation and distribution are RF-SC-8.
keygen keystore="mg.keystore" anchor="mg.pub":
    cargo run -q --release -p mg-sign -- keygen --keystore "{{keystore}}" --export-verifying-key "{{anchor}}"

# 7. Verify signature + SBOM + provenance before publish. Fails closed. [AC-B-1]
#
# Trust anchors are named explicitly and are never searched for: set
# MIDGARD_TRUST_ANCHORS to an anchor file exported by `just keygen`. Unset means
# the stage fails — "I forgot the anchors" and "this artefact is fine" must never
# look the same. Anchor distribution, rotation and revocation are RF-SC-8.
verify path:
    @test -n "${MIDGARD_TRUST_ANCHORS:-}" || { echo "MIDGARD_TRUST_ANCHORS is unset — refusing to verify" >&2; exit 1; }
    cargo run -q --release -p mg-verify -- "{{path}}" --trust-anchor "$MIDGARD_TRUST_ANCHORS"

# Full per-artefact gate, in order, for one binary. `pkg` is a workspace package and
# `bin` the binary it builds, by default the package's own name: the deployable is
# target/verified/<bin> (plus `.exe` on Windows). A package that builds several binaries
# (supply-chain/release-artefacts.txt) is gated once per binary. Every stage fails
# closed, so the first refusal stops the chain and nothing downstream runs.
gate pkg bin="":
    #!/usr/bin/env bash
    set -euo pipefail
    # `repro` builds twice under a normalised environment, proves the outputs are
    # bit-identical, and emits the PROVEN artefact. The gate then SBOMs, signs and
    # verifies that one — not a separately built copy. Signing anything else would
    # produce a signature over a binary a third party cannot reproduce from
    # source, which is the whole point of proving it. [AC-M-6] [AC-B-1]
    out=target/verified
    rm -rf "$out"
    cargo run -q --release -p mg-repro -- {{pkg}} --out repro.json --emit "$out"
    name="{{ if bin == "" { pkg } else { bin } }}"
    bin="$out/$name"
    # Prefer the .exe when one exists. Checking the extension-less path first
    # does not work: MSYS's `test -f` transparently resolves `foo` to `foo.exe`,
    # so the check passes and the native tools downstream are then handed a path
    # that does not exist.
    [ -f "$bin.exe" ] && bin="$bin.exe"
    [ -f "$bin" ] || { echo "gate: {{pkg}} produced no verified binary $name" >&2; exit 1; }
    just sbom "$bin"
    just scan "$bin"
    MIDGARD_REPRO_REPORT=repro.json just sign "$bin"
    just verify "$bin"
    echo "gate passed: {{pkg}} $name ($bin)"
