#!/usr/bin/env bash
# Signing-runner preflight for the release workflow. [NFR-PQ-3] [AC-B-1] [RC-07]
# [VLD-ANB-MIDGARD-0035]
#
# Asserts the runner is provisioned as docs/ops/PKI.md ("Signing runner") requires, and
# fails closed naming every item that is missing, before any job builds or signs.
#
#   bash tools/ci/release-preflight.sh
#
# The release pipeline runs this from a checkout of main beside the tagged one
# (.ci-trusted), so the job's workspace, not this script's checkout, is what key
# material must stay out of.
#
# Environment (the workflow maps runner secrets onto these; none has a default). The
# keystore and registry variables come from secrets named with a _PATH suffix; the two
# passphrase variables from secrets of their own name (docs/ops/PKI.md):
#   MIDGARD_SIGNING_KEYSTORE   the release signing keystore (ML-DSA-87)
#   MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE, MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE
#                              the passphrases those keystores' roots are wrapped under
#   MIDGARD_BIFROST_KEYSTORE   the Bifrost admission keystore (ML-DSA-65): registry
#                              admission and the air-gap bundle
#   MIDGARD_BIFROST_REGISTRY   the Bifrost artefact registry directory
#   MIDGARD_ALLOWED_SIGNERS_PATH
#                              the runner's own copy of allowed_signers, which release
#                              tags are verified against (tools/ci/verify-tag.sh)
#   MIDGARD_EXTERNAL_IMAGES    the external images, as `deploy/images/externals.sh fetch`
#                              left them, for the air-gap bundle
#   TOOLCHAIN_IMAGE            the toolchain image (default midgard-acceptance:1.82.0)
set -uo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib.sh
TOOLCHAIN_IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"
workspace="${GITHUB_WORKSPACE:-${CI_PROJECT_DIR:-$PWD}}"
workspace="$(cd "$workspace" 2>/dev/null && pwd -P || echo "$workspace")"
problems=0
ok() { echo "preflight: ok — $*"; }
bad() {
    ci_error "preflight: $* (see docs/ops/PKI.md, \"Signing runner\")"
    problems=$((problems + 1))
}
# in_workspace PATH: whether PATH lies in the job's workspace, where a checkout, and
# whatever the checked-out code chooses to read, can reach it.
in_workspace() { [[ "$(cd "$(dirname "$1")" 2>/dev/null && pwd -P)/" == "$workspace/"* ]]; }

# 1. The pinned toolchain and the service image bases, provisioned while connected:
#    every build runs in the toolchain image with no network.
if ! command -v docker >/dev/null 2>&1; then
    bad "docker is not installed"
else
    if docker image inspect "$TOOLCHAIN_IMAGE" >/dev/null 2>&1; then
        tools="$(docker run --rm --network none "$TOOLCHAIN_IMAGE" sh -c 'rustc --version; just --version' 2>&1)"
        if grep -q '^rustc 1\.82\.0 ' <<<"$tools" && grep -q '^just ' <<<"$tools"; then
            ok "toolchain image $TOOLCHAIN_IMAGE ($(head -1 <<<"$tools"))"
        else
            bad "$TOOLCHAIN_IMAGE does not carry rustc 1.82.0 and just: $tools"
        fi
    else
        bad "the toolchain image $TOOLCHAIN_IMAGE is not present; build it from deploy/acceptance/Dockerfile while connected"
    fi
    # Both stages of the service image: the toolchain base its prep stage lays files out
    # on, and the distroless RUNTIME base it runs on. [VLD-ANB-MIDGARD-0068]
    for arg in BASE RUNTIME; do
        base="$(sed -n "s/^ARG $arg=//p" deploy/images/service.Dockerfile | tr -d '\r' | head -1)"
        if [ -z "$base" ]; then
            bad "deploy/images/service.Dockerfile declares no ARG $arg"
        elif docker image inspect "$base" >/dev/null 2>&1; then
            ok "service image $arg $base"
        else
            bad "the service image $arg $base is not present; pull it while connected"
        fi
    done
fi
# The images Midgard runs but does not build (deploy/images/externals.lock: the seed,
# model and egress images, and the kind node), fetched while connected as OCI archives
# that keep their pinned digests, for release-bundle to pack. [VLD-ANB-MIDGARD-0035]
externals="${MIDGARD_EXTERNAL_IMAGES:-}"
if [ -z "$externals" ]; then
    bad "MIDGARD_EXTERNAL_IMAGES is not configured (runner secret MIDGARD_EXTERNAL_IMAGES_PATH): it names the directory \`deploy/images/externals.sh fetch\` fills while connected"
elif in_workspace "$externals/."; then
    bad "MIDGARD_EXTERNAL_IMAGES names a directory inside the job's workspace ($workspace); keep it outside every checkout"
elif found="$(bash deploy/images/externals.sh check "$externals" 2>&1)"; then
    ok "the external images in $externals, each as pinned"
else
    bad "the external images in $externals are not all there as pinned: $(grep -v '^externals: ok' <<<"$found" | tr '\n' ' ')"
fi

# 2. What the scan stage reads, all of it local: the vendored advisory database, and
#    the licence policy and exception tables. (The exception table is not the database:
#    a runner with only that would pass a check that looked no further.)
declared="$(tr -d '\r' < vendor/advisory-db/snapshot.toml 2>/dev/null | sed -n 's/^advisory_count *= *\([0-9]*\).*/\1/p' | head -1)"
present="$(find vendor/advisory-db/crates -name 'RUSTSEC-*.md' 2>/dev/null | wc -l | tr -d ' ')"
if [ -z "$declared" ]; then
    bad "vendor/advisory-db/snapshot.toml is missing or declares no advisory_count; run \`just advisory-db-refresh\` while connected"
elif [ "$present" != "$declared" ]; then
    bad "vendor/advisory-db holds $present advisories but its snapshot declares $declared"
else
    ok "offline advisory database ($present advisories)"
fi
for policy in supply-chain/licences.toml supply-chain/advisories.toml; do
    [ -s "$policy" ] && ok "$policy" || bad "$policy is missing"
done

# 2b. The JavaScript artefacts are built on the runner itself (the toolchain image
#     carries no node), from a package store filled while connected.
if command -v node >/dev/null 2>&1 && { command -v pnpm >/dev/null 2>&1 || command -v corepack >/dev/null 2>&1; }; then
    ok "node $(node --version) with pnpm, for the JavaScript artefacts"
else
    bad "node and pnpm (or corepack) are not installed; the UI bundles and the SDK package cannot be built"
fi

# 2c. The release chart: rendered on the runner with helm, its images pinned by the
#     digests set-digests.sh lifts from the signed images.lock. [VLD-ANB-MIDGARD-0012]
if command -v "${HELM:-helm}" >/dev/null 2>&1; then
    ok "helm, to render the digest-pinned release chart"
else
    bad "helm is not installed; the release chart cannot be rendered"
fi
if [ -f deploy/helm/set-digests.sh ] && [ -x deploy/helm/set-digests.sh ]; then
    ok "deploy/helm/set-digests.sh"
else
    bad "deploy/helm/set-digests.sh is missing or not executable"
fi

# 3. Key material: named explicitly, readable, and never inside the checkout.
# Each keystore's passphrase is a separate secret, mounted read-only like the keystore;
# without it the keystore does not open. [VLD-ANB-MIDGARD-0007]
for var in MIDGARD_SIGNING_KEYSTORE MIDGARD_BIFROST_KEYSTORE \
    MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE; do
    path="${!var:-}"
    case "$var" in
        *_PASSPHRASE_FILE) secret="$var" ;;
        *) secret="${var}_PATH" ;;
    esac
    if [ -z "$path" ]; then
        bad "$var is not configured (runner secret $secret)"
    elif [ ! -r "$path" ] || [ -d "$path" ]; then
        bad "$var names $path, which is not a readable file"
    elif in_workspace "$path"; then
        bad "$var names a file inside the job's workspace ($workspace); keep key material outside it"
    else
        ok "$var is configured"
    fi
done
registry="${MIDGARD_BIFROST_REGISTRY:-}"
if [ -z "$registry" ]; then
    bad "MIDGARD_BIFROST_REGISTRY is not configured (runner secret MIDGARD_BIFROST_REGISTRY_PATH)"
elif [ ! -d "$registry" ] || [ ! -w "$registry" ]; then
    bad "MIDGARD_BIFROST_REGISTRY names $registry, which is not a writable directory"
else
    ok "Bifrost registry $registry"
fi

# 4. The pinned public anchors, committed by the key ceremony (RT-35).
anchor() { # anchor FILE ALG WHAT
    local file="$1" alg="$2" what="$3"
    if [ ! -s "$file" ]; then
        bad "$file is not committed; the key ceremony (RT-35) publishes the $what's public half there"
    elif grep -qiE 'sk=|private' "$file"; then
        bad "$file holds private key material"
    elif tr -d '\r' < "$file" | grep -qE "^$alg [^ ]+ [0-9a-fA-F]+$"; then
        ok "$file pins an $alg key"
    else
        bad "$file has no \"$alg <key-id> <hex>\" line"
    fi
}
anchor deploy/trust/release-trust.pub ML-DSA-87 "release signing key"
anchor deploy/trust/bifrost-trust.pub ML-DSA-65 "Bifrost admission key"
# The anchor mimir-server's load gate and `mg-model verify` trust, carried in the images
# at /etc/midgard/trust: never a key from the volume that holds the weights.
# [VLD-ANB-MIDGARD-0028]
anchor deploy/trust/model-signing.pub ML-DSA-65 "model signing key"

# 5. Nothing outlives a job on a self-hosted runner: a job-completed hook removes the work
#    directory, so no checkout, token or build output is left beside the signing
#    material for the next job. (Checkouts no longer persist the workflow token either.)
#    [VLD-ANB-MIDGARD-0081]
if [ "${RUNNER_ENVIRONMENT:-}" = self-hosted ]; then
    hook="${ACTIONS_RUNNER_HOOK_JOB_COMPLETED:-}"
    if [ -z "$hook" ]; then
        bad "ACTIONS_RUNNER_HOOK_JOB_COMPLETED is not set on this self-hosted runner; install the job-completed cleanup hook so no workspace outlives a job"
    elif [ ! -r "$hook" ]; then
        bad "ACTIONS_RUNNER_HOOK_JOB_COMPLETED names $hook, which is not a readable file"
    else
        ok "job-completed hook $hook"
    fi
fi

# 6. The runner is isolated from what it releases. [VLD-ANB-MIDGARD-0035]
#    - Release tags verify against the runner's own copy of allowed_signers, outside the
#      workspace, so a key the tagged commit adds cannot verify that tag.
#    - The docker daemon answers, and every build step goes through in-toolchain.sh,
#      which starts the toolchain container with no network.
signers="${MIDGARD_ALLOWED_SIGNERS_PATH:-}"
if [ -z "$signers" ]; then
    bad "MIDGARD_ALLOWED_SIGNERS_PATH is not configured (runner secret MIDGARD_ALLOWED_SIGNERS_PATH): release tags are verified against the runner's copy of allowed_signers"
elif [ ! -s "$signers" ] || [ -d "$signers" ]; then
    bad "MIDGARD_ALLOWED_SIGNERS_PATH names $signers, which is not a non-empty file"
elif in_workspace "$signers"; then
    bad "MIDGARD_ALLOWED_SIGNERS_PATH names a file inside the job's workspace ($workspace); the runner's copy must live outside it"
elif grep -q 'PRIVATE KEY' "$signers"; then
    bad "MIDGARD_ALLOWED_SIGNERS_PATH names a file holding private key material"
else
    ok "MIDGARD_ALLOWED_SIGNERS_PATH is configured, outside the workspace"
fi
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    ok "the docker daemon is reachable"
else
    bad "the docker daemon is not reachable (docker info failed)"
fi
if grep -Eq '^exec docker run [^#]*--network none' "$workspace/tools/ci/in-toolchain.sh" 2>/dev/null; then
    ok "builds run in the toolchain container with --network none"
else
    bad "$workspace/tools/ci/in-toolchain.sh does not start the toolchain container with --network none"
fi

if [ "$problems" -gt 0 ]; then
    echo "preflight: $problems problem(s); the release stops here, before anything is built or signed"
    exit 1
fi
echo "preflight: the signing runner is provisioned"
