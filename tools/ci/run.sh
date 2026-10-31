#!/usr/bin/env bash
# One entry point for every CI job, on every CI system. [NFR-OSS-1] [RC-07]
#
#   bash tools/ci/run.sh list              the jobs
#   bash tools/ci/run.sh platform          what this script thinks it is running on
#   bash tools/ci/run.sh <job> [ARG...]    run one
#
# The pipeline definitions — .github/workflows/*.yml and .gitlab-ci.yml — contain no
# build logic. Each of their jobs is one line: `bash tools/ci/run.sh <job>`. What a job
# does is written here, once, so the two pipelines cannot drift apart in substance,
# and a developer can run any job locally exactly as CI does. `tools/ci/pipelines.sh
# --check` (the `selftest` job) fails if either pipeline calls a job that does not
# exist or the two call different sets.
#
# Jobs marked [toolchain] run inside the digest-pinned toolchain image with no network
# (tools/ci/in-toolchain.sh); build it first with the `toolchain` job. The rest run on
# the host. GitLab's merge-request jobs already run inside the published toolchain image
# (MIDGARD_IN_TOOLCHAIN=1), so there the [toolchain] jobs run in place, offline by
# `cargo --offline` and the vendor check rather than by a network sandbox.
set -euo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib.sh

IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"
export TOOLCHAIN_IMAGE="$IMAGE"
TOOLCHAIN_LABEL=dev.midgard.toolchain.dockerfile
# Where a saved copy of the toolchain image is kept between jobs, when the CI system
# caches this directory.
CACHE_DIR="${MIDGARD_CI_CACHE:-.ci-cache}"
# The digest of the published toolchain image GitLab's merge-request jobs run in.
TOOLCHAIN_DIGEST_FILE=tools/ci/toolchain.digest

# gitleaks, pinned by release and by the checksums that release publishes.
GITLEAKS_VERSION=8.30.1
GITLEAKS_SHA256_linux_x64=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
GITLEAKS_SHA256_linux_arm64=e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080
GITLEAKS_SHA256_darwin_x64=dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709
GITLEAKS_SHA256_darwin_arm64=b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5

# name|where|what — the one list of jobs. `list` prints it and pipelines.sh reads it.
JOBS='
selftest|host|the CI scripts test themselves, and both pipelines call the same jobs
provenance|host|changed sources under governed trees cite a requirement
vendor|host|/vendor is exactly the crates Cargo.lock names, and Cargo is pinned to it
claims|host|every row of the claims ledger cites files, lines and tests that exist
gate-record|host|the committed gate record is what its logs give, and the status tables are what it gives
numbers|host|docs/compliance/NUMBERS.md matches the tree, and no swept document quotes a figure that differs from it
secrets|host|no credential is committed (gitleaks)
k8s-render|host|deploy/k8s/enclosure.yaml is what the Helm chart renders, and each secrets provider renders as it should (needs helm)
toolchain|host|build or restore the digest-pinned toolchain image (the only online step); inside it, check it is the pinned one
toolchain-publish|host|push the toolchain image to the GitLab project registry; its digest must be the committed one
fmt|toolchain|formatting
lint|toolchain|clippy at deny-warnings
build|toolchain|release build of the workspace, whose graph never enables the test backend
test|toolchain|the test suite, both crypto backends; its log is kept in target/gate
repro|toolchain|two normalised builds of the workspace are bit-identical
scan|toolchain|advisory and licence scan of the dependency graph, PR mode
acceptance|host|the full acceptance suite under a genuine air gap; its log is kept in target/gate
js-build|host|install the JS workspace and build every JavaScript artefact twice, bit-identical
release-dry-run|toolchain|the whole release gate with throwaway keys, publishing nothing
release-preflight|host|the signing runner is provisioned
release-tag|host|the release tag is annotated, signed, and verifies against deploy/trust/allowed_signers
release|toolchain|gate every binary, admit each to Bifrost, account for all of them
release-images|host|build the verify-before-run images from the gated binaries
release-bundle|toolchain|pack the signed air-gap bundle
release-publish|host|attach the release to a draft pre-release
shell-audit|host|build one client shell and audit it on this runner: shell-audit <shell> <release>
shell-audit-gate|host|every shell has a fresh passing audit record: shell-audit-gate <release>
'

in_toolchain() { bash tools/ci/in-toolchain.sh "$@"; }

need() {
    command -v "$1" >/dev/null 2>&1 || {
        ci_error "$2"
        exit 1
    }
}

# The release this run is for: the tag when there is one, else the workspace version.
release_name() {
    local tag
    tag="$(ci_tag)"
    if [ -n "$tag" ]; then
        echo "${tag#v}"
    else
        bash tools/ci/release-artefacts.sh --version
    fi
}

# The arguments that hand the signing material to the toolchain container: each
# keystore and its passphrase file mounted read-only at a fixed path, the registry
# read-write, and the variables renamed to match. Nothing else from the runner's
# environment goes in. [VLD-ANB-MIDGARD-0007]
signing_mounts() {
    local -n into="$1"
    into=()
    : "${MIDGARD_SIGNING_KEYSTORE:?the release signing keystore is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_BIFROST_KEYSTORE:?the Bifrost admission keystore is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_BIFROST_REGISTRY:?the Bifrost registry directory is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE:?the release keystore passphrase is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE:?the admission keystore passphrase is not configured (docs/ops/PKI.md)}"
    into+=(--mount "$MIDGARD_SIGNING_KEYSTORE:/keys/release.keystore:ro"
           --env MIDGARD_SIGNING_KEYSTORE=/keys/release.keystore
           --mount "$MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE:/keys/release.passphrase:ro"
           --env MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE=/keys/release.passphrase
           --mount "$MIDGARD_BIFROST_KEYSTORE:/keys/admission.keystore:ro"
           --env MIDGARD_BIFROST_KEYSTORE=/keys/admission.keystore
           --mount "$MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE:/keys/admission.passphrase:ro"
           --env MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE=/keys/admission.passphrase
           --mount "$MIDGARD_BIFROST_REGISTRY:/registry"
           --env MIDGARD_BIFROST_REGISTRY=/registry)
}

job_selftest() {
    bash tools/ci/pipelines.sh --check
    for t in tools/ci/tests/*.test.sh; do
        ci_group "$t"
        bash "$t"
        ci_endgroup
    done
}

job_provenance() { bash tools/ci/provenance-lint.sh; }

job_vendor() { bash tools/ci/vendor-check.sh; }

job_claims() { bash tools/ci/claims.sh --check --strict; }

# `gate-record --release` is the release's form: the record must also be of the commit
# being released, apart from the record itself and the prose generated from it.
job_gate_record() { bash tools/ci/gate-record.sh --check "$@"; }

job_numbers() { bash tools/ci/numbers.sh --check; }

job_secrets() {
    local bin=gitleaks os arch want got dir
    if ! command -v gitleaks >/dev/null 2>&1; then
        case "$(uname -s)" in Linux) os=linux ;; Darwin) os=darwin ;; *) ci_error "secrets: install gitleaks $GITLEAKS_VERSION on this platform"; exit 1 ;; esac
        case "$(uname -m)" in x86_64|amd64) arch=x64 ;; aarch64|arm64) arch=arm64 ;; *) ci_error "secrets: no pinned gitleaks for $(uname -m)"; exit 1 ;; esac
        want_var="GITLEAKS_SHA256_${os}_${arch}"
        want="${!want_var}"
        dir="$CACHE_DIR/gitleaks-$GITLEAKS_VERSION"
        mkdir -p "$dir"
        if [ ! -x "$dir/gitleaks" ]; then
            need curl "secrets: curl is needed to fetch gitleaks"
            curl -fsSL -o "$dir/gitleaks.tar.gz" \
                "https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS_VERSION/gitleaks_${GITLEAKS_VERSION}_${os}_${arch}.tar.gz"
            got="$( (sha256sum "$dir/gitleaks.tar.gz" 2>/dev/null || shasum -a 256 "$dir/gitleaks.tar.gz") | awk '{print $1}')"
            if [ "$got" != "$want" ]; then
                rm -f "$dir/gitleaks.tar.gz"
                ci_error "secrets: the gitleaks download does not match its pinned checksum (got $got)"
                exit 1
            fi
            tar -xzf "$dir/gitleaks.tar.gz" -C "$dir" gitleaks
        fi
        bin="$dir/gitleaks"
    fi
    "$bin" detect --source . --config .gitleaks.toml --redact --no-banner
}

job_k8s_render() {
    need "${HELM:-helm}" "k8s-render: helm is not on PATH"
    bash deploy/k8s/render.sh --check
    bash deploy/helm/midgard/tests/render-providers.sh
}

# Restore the toolchain image from the cache directory when the CI system kept one for
# this Dockerfile; build it, and save it for next time, when it did not.
# The Dockerfile an image was built from, as recorded in its label; empty when the
# image is absent or carries no label.
toolchain_built_from() {
    docker image inspect --format "{{ index .Config.Labels \"$TOOLCHAIN_LABEL\" }}" "$IMAGE" 2>/dev/null || true
}

# The first 16 hex digits of the Dockerfile's SHA-256: the label an image carries, and
# the tag the published image is pushed under.
toolchain_key() {
    (sha256sum deploy/acceptance/Dockerfile 2>/dev/null || shasum -a 256 deploy/acceptance/Dockerfile) | cut -c1-16
}

# The digest tools/ci/toolchain.digest pins: its first line that is not a comment, when
# that is a sha256 digest; empty when the file holds none.
committed_toolchain_digest() {
    local line
    line="$(tr -d '\r' < "$TOOLCHAIN_DIGEST_FILE" 2>/dev/null | sed 's/#.*//' | awk 'NF { print $1; exit }')"
    [[ "$line" =~ ^sha256:[0-9a-f]{64}$ ]] && echo "$line"
    return 0
}

# Inside the published image (GitLab's merge-request jobs) there is nothing to build:
# the job is the check that the image GitLab started is the one this commit pins. The
# image is chosen from the CI/CD variable MIDGARD_TOOLCHAIN_DIGEST before any script
# runs, so a variable left behind when the file moved on fails here, closed.
# [VLD-ANB-MIDGARD-0043]
toolchain_assert_pinned() {
    local want recorded
    want="$(committed_toolchain_digest)"
    [ -n "$want" ] || {
        ci_error "toolchain: $TOOLCHAIN_DIGEST_FILE pins no published digest yet; run toolchain-publish on main and commit the digest it prints" "$TOOLCHAIN_DIGEST_FILE"
        exit 1
    }
    [ "${MIDGARD_TOOLCHAIN_DIGEST:-}" = "$want" ] || {
        ci_error "toolchain: this job runs in toolchain@${MIDGARD_TOOLCHAIN_DIGEST:-<unset>}, but $TOOLCHAIN_DIGEST_FILE pins $want; set the CI/CD variable MIDGARD_TOOLCHAIN_DIGEST from $TOOLCHAIN_DIGEST_FILE" "$TOOLCHAIN_DIGEST_FILE"
        exit 1
    }
    recorded="$(sed -n 's/^# *dockerfile-sha16: *\([0-9a-f]\{16\}\).*/\1/p' "$TOOLCHAIN_DIGEST_FILE" | head -1)"
    [ "$recorded" = "$(toolchain_key)" ] ||
        ci_warning "toolchain: $TOOLCHAIN_DIGEST_FILE was published from Dockerfile ${recorded:-<unrecorded>}, not this one ($(toolchain_key)); run toolchain-publish once this is merged"
    echo "toolchain: running in the pinned toolchain image, $want"
}

job_toolchain() {
    local key tarball
    if [ "${MIDGARD_IN_TOOLCHAIN:-}" = 1 ]; then
        toolchain_assert_pinned
        return
    fi
    need docker "toolchain: docker is not available"
    # The tag names the Rust version and says nothing about the rest of the Dockerfile,
    # so an image built before the Dockerfile last changed still answers to it. The
    # label does say: an image is current only when it records this Dockerfile.
    key="$(toolchain_key)"
    if [ "$(toolchain_built_from)" = "$key" ]; then
        echo "toolchain: $IMAGE is already present and current"
        return
    fi
    if docker image inspect "$IMAGE" >/dev/null 2>&1; then
        echo "toolchain: $IMAGE was built from another Dockerfile; replacing it"
    fi
    tarball="$CACHE_DIR/toolchain-$key.tar"
    if [ -f "$tarball" ]; then
        docker load -i "$tarball"
    fi
    if [ "$(toolchain_built_from)" != "$key" ]; then
        # No COPY in the Dockerfile, so the build context is empty: nothing of the
        # checkout is sent to the daemon.
        docker build --label "$TOOLCHAIN_LABEL=$key" -t "$IMAGE" - < deploy/acceptance/Dockerfile
        if [ "${MIDGARD_CI_SAVE_TOOLCHAIN:-1}" = 1 ]; then
            mkdir -p "$CACHE_DIR"
            rm -f "$CACHE_DIR"/toolchain-*.tar
            docker save -o "$tarball" "$IMAGE" || ci_warning "toolchain: could not save the image for reuse"
        fi
    fi
}

# Build the toolchain image and push it to the GitLab project registry, where the
# merge-request jobs run it by digest. Runs only on the privileged runner, by hand, on a
# protected branch. The digest a push gets cannot be known before it, so the job fails,
# printing it, until it is committed to tools/ci/toolchain.digest; once it is, a rerun
# finds that image already published from this Dockerfile and pushes nothing.
# [VLD-ANB-MIDGARD-0043]
job_toolchain_publish() {
    local key repo want got line
    if [ "$(ci_platform)" = github ]; then
        echo "toolchain-publish: not applicable: GitHub builds the toolchain per run"
        return 0
    fi
    need docker "toolchain-publish: docker is not available"
    [ -n "${CI_REGISTRY_IMAGE:-}" ] || {
        ci_error "toolchain-publish: CI_REGISTRY_IMAGE is not set; this job publishes to a GitLab project registry"
        exit 1
    }
    key="$(toolchain_key)"
    repo="$CI_REGISTRY_IMAGE/toolchain"
    want="$(committed_toolchain_digest)"
    if [ -n "${CI_REGISTRY:-}" ] && [ -n "${CI_REGISTRY_USER:-}" ]; then
        printf '%s' "${CI_REGISTRY_PASSWORD:-${CI_JOB_TOKEN:-}}" |
            docker login --username "$CI_REGISTRY_USER" --password-stdin "$CI_REGISTRY" >/dev/null
    fi
    if [ -n "$want" ] && docker pull "$repo@$want" >/dev/null 2>&1 &&
        [ "$(docker image inspect --format "{{ index .Config.Labels \"$TOOLCHAIN_LABEL\" }}" "$repo@$want" 2>/dev/null)" = "$key" ]; then
        echo "toolchain-publish: $repo@$want is published, built from this Dockerfile ($key); nothing to push"
        return 0
    fi
    MIDGARD_CI_SAVE_TOOLCHAIN=0 job_toolchain
    docker tag "$IMAGE" "$repo:$key"
    docker push "$repo:$key"
    got=""
    while IFS= read -r line; do
        line="${line%$'\r'}"
        [[ "$line" == "$repo@"* ]] && { got="${line#"$repo@"}"; break; }
    done < <(docker image inspect --format '{{range .RepoDigests}}{{println .}}{{end}}' "$repo:$key")
    [[ "$got" =~ ^sha256:[0-9a-f]{64}$ ]] || {
        ci_error "toolchain-publish: pushed $repo:$key but could not read the digest it was given"
        exit 1
    }
    echo "toolchain-publish: pushed $repo:$key as $repo@$got"
    if [ "$got" != "$want" ]; then
        ci_error "toolchain-publish: the pushed image is $got, but $TOOLCHAIN_DIGEST_FILE pins ${want:-nothing}; commit the new digest to $TOOLCHAIN_DIGEST_FILE, then set MIDGARD_TOOLCHAIN_DIGEST to it" "$TOOLCHAIN_DIGEST_FILE"
        echo "$TOOLCHAIN_DIGEST_FILE should end:"
        echo "# dockerfile-sha16: $key"
        echo "$got"
        exit 1
    fi
}

job_fmt() { in_toolchain just fmt-check; }
job_lint() { in_toolchain just lint; }
# The release build, then the check that nothing in its graph enables the insecure
# test backend. [MG-SEC-15]
job_build() {
    in_toolchain just build-rust
    in_toolchain bash tools/ci/no-test-backend.sh
}
# The two parts of the acceptance gate go through gate-run.sh, which runs them exactly
# as before and keeps the log and its metadata in target/gate. The pipelines keep that
# directory; tools/ci/gate-record.sh makes the gate record from it.
job_test() { bash tools/ci/gate-run.sh test; }

# Four jobs pinned, so the report is the same on a runner of any size. repro.json is
# the artefact the pipelines keep.
job_repro() { in_toolchain just repro "" 4; }

# The SBOM is derived from Cargo.lock, so it needs no built binary to describe.
job_scan() {
    in_toolchain bash -c 'mkdir -p target/ci && just sbom target/ci/workspace && just scan-advisory target/ci/workspace'
}

job_acceptance() { bash tools/ci/gate-run.sh acceptance; }

# The JavaScript artefacts, built where node is — the Rust toolchain image has none —
# and left in target/js-out for the release to gate. `pnpm install` is the online step
# here, as building the toolchain image is for Rust; --frozen-lockfile makes it install
# exactly what pnpm-lock.yaml pins, by integrity hash, or fail. A runner with no route
# to the registry installs from a store filled in advance (`pnpm fetch`).
job_js_build() {
    need node "js-build: node is not on PATH"
    if ! command -v pnpm >/dev/null 2>&1; then
        need corepack "js-build: neither pnpm nor corepack is on PATH"
        corepack enable pnpm
    fi
    pnpm install --frozen-lockfile --prefer-offline
    bash tools/ci/js-artefacts.sh build --all --out "${MIDGARD_JS_OUT:-target/js-out}"
}

job_release_dry_run() { in_toolchain bash tools/ci/release.sh --dry-run "$@"; }

job_release_preflight() { bash tools/ci/release-preflight.sh; }

# A release is built from a signed tag, verified here before anything is built or signed.
job_release_tag() { bash tools/ci/verify-tag.sh "$@"; }

job_release() {
    local mounts
    signing_mounts mounts
    in_toolchain "${mounts[@]}" -- bash tools/ci/release.sh
}

# The images are built on the host (the toolchain container has no docker), from the
# gated binaries; the lock file naming them is then signed like any other artefact.
# The chart is rendered from the lock's digests, also on the host (helm), and the
# values file and the rendering are signed beside it, so a release deploys each
# service's signed image by digest and never by a tag. [VLD-ANB-MIDGARD-0012]
job_release_images() {
    local version out=target/release-out mounts
    version="$(release_name)"
    [ -s "$out/release-manifest.txt" ] || {
        ci_error "release-images: $out holds no completed release; run the release job first"
        exit 1
    }
    need "${HELM:-helm}" "release-images: helm is not on PATH; it renders the release chart"
    bash deploy/images/build.sh --version "$version" --verified "$out" \
        --trust "${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}" --out "$out/images"
    bash deploy/helm/set-digests.sh "$out/images/images.lock" deploy/helm/midgard/values.yaml \
        --out "$out/chart/values.yaml"
    "${HELM:-helm}" template midgard deploy/helm/midgard -f "$out/chart/values.yaml" \
        --namespace default | tr -d '\r' > "$out/chart/enclosure.yaml"
    signing_mounts mounts
    in_toolchain "${mounts[@]}" \
        --env "MIDGARD_TRUST_ANCHORS=${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}" -- \
        bash -c "just sbom $out/images/images.lock && just scan $out/images/images.lock \
                 && just sign $out/images/images.lock && just verify $out/images/images.lock \
                 && for f in $out/chart/values.yaml $out/chart/enclosure.yaml; do \
                        just sbom \$f && just sign \$f && just verify \$f || exit 1; done"
}

# Every gated binary with its signature and provenance, packed and signed for import
# on the far side of an air gap.
job_release_bundle() {
    local version out=target/release-out mounts specs=() component
    version="$(release_name)"
    [ -s "$out/release-manifest.txt" ] || {
        ci_error "release-bundle: $out holds no completed release; run the release job first"
        exit 1
    }
    local id file
    # id version component file — what the release gated, binaries and JS alike.
    while read -r id _ component file; do
        [ -n "$id" ] || continue
        specs+=("$component=$id:$version:$out/$file"
                "attestation=$id.sig:$version:$out/$file.sig"
                "attestation=$id.att:$version:$out/$file.att")
    done < "$out/artefacts.txt"
    signing_mounts mounts
    in_toolchain "${mounts[@]}" -- bash deploy/air-gap-bundle/produce.sh \
        "$out/midgard-$version.mgbundle" "midgard-$version" "v$version" \
        /keys/admission.keystore "${specs[@]}"
}

# A DRAFT pre-release at most: nothing here makes a release public. On GitHub that is
# a draft release. GitLab has no draft state, so the files go to the project's package
# registry and the release entry is left for a person to create.
job_release_publish() {
    local tag out=target/release-out files=() f
    tag="${1:-${MIDGARD_PUBLISH_TAG:-$(ci_tag)}}"
    [[ "$tag" =~ ^v[0-9A-Za-z.+-]+$ ]] || [ -z "$tag" ] || {
        ci_error "release-publish: $tag is not a release tag"
        exit 1
    }
    [ -n "$tag" ] || {
        ci_error "release-publish: no tag to publish as"
        exit 1
    }
    [ -s "$out/release-manifest.txt" ] || {
        ci_error "release-publish: $out holds no completed release"
        exit 1
    }
    while IFS= read -r f; do files+=("$f"); done < <(
        find "$out" -maxdepth 2 -type f \
            \( -name '*.sig' -o -name '*.att' -o -name '*.sbom.cdx.json' -o -name '*.repro.json' \
               -o -name '*.mgbundle' -o -name 'images.lock' -o -name 'release-manifest.txt' \
               -o -path "$out/chart/*.yaml" \) | sort
        awk 'NF { print $4 }' "$out/artefacts.txt" | while read -r file; do echo "$out/$file"; done
    )
    # The gate run the release rests on, with its raw logs, and the numbers sheet: what a
    # reader needs to check every figure the release states. The preflight has already
    # required that the record covers this commit.
    local gate
    gate="$(tr -d ' \r\n' < docs/compliance/gate-runs/CURRENT 2>/dev/null || true)"
    [ -n "$gate" ] && [ -d "docs/compliance/gate-runs/$gate" ] || {
        ci_error "release-publish: no gate run is recorded (docs/compliance/gate-runs/CURRENT)"
        exit 1
    }
    files+=("docs/compliance/NUMBERS.md" "docs/compliance/gate-runs/$gate.md" "docs/compliance/gate-runs/$gate"/*)
    case "$(ci_platform)" in
        github)
            need gh "release-publish: the gh CLI is needed"
            gh release create "$tag" --draft --prerelease --verify-tag \
                --title "Midgard $tag" --notes "Draft pre-release assembled by the release pipeline. Verify any binary with mg-verify against deploy/trust/release-trust.pub." \
                "${files[@]}"
            ;;
        gitlab)
            need curl "release-publish: curl is needed"
            : "${CI_JOB_TOKEN:?}" "${CI_API_V4_URL:?}" "${CI_PROJECT_ID:?}"
            for f in "${files[@]}"; do
                curl --fail --silent --show-error --header "JOB-TOKEN: $CI_JOB_TOKEN" --upload-file "$f" \
                    "$CI_API_V4_URL/projects/$CI_PROJECT_ID/packages/generic/midgard/${tag#v}/$(basename "$f")" >/dev/null
                echo "uploaded $(basename "$f")"
            done
            echo "release-publish: ${#files[@]} file(s) are in the package registry as midgard/${tag#v}; no release entry was created"
            ;;
        *)
            ci_error "release-publish: only runs in a pipeline; ${#files[@]} file(s) are ready in $out"
            exit 1
            ;;
    esac
}

# Build one shell and audit it on this runner, writing the record the release gate
# reads. Fails closed when the shell has no buildable project: an audit is never
# recorded for a UI that did not run. [RC-49]
job_shell_audit() {
    local shell="${1:?usage: shell-audit <shell> <release>}" release="${2:-$(release_name)}"
    local root="clients/shells/$shell" report="shell-audit/$shell-$release.json" failed
    [ -f "$root/accessibility.toml" ] || {
        ci_error "shell-audit: there is no shell called $shell"
        exit 2
    }
    export MIDGARD_CI_COMMIT="$(ci_sha)" MIDGARD_CI_RUNNER="$(ci_runner)"
    mkdir -p shell-audit
    no_project() {
        ci_error "shell-audit: the $shell shell has no buildable project yet ($1 is missing) — nothing ran, so nothing is recorded (RC-49)"
        exit 1
    }
    case "$shell" in
        web|extension)
            [ -f "$root/package.json" ] || no_project "$root/package.json"
            need node "shell-audit: node is needed for the $shell audit"
            (cd "$root" && npm ci && npm run build)
            (cd "$root" && exec npx --yes http-server dist -p 4173 -s >/dev/null 2>&1) &
            local server=$!
            trap "kill $server 2>/dev/null || true" EXIT
            npx --yes wait-on http://127.0.0.1:4173
            node clients/shells/web/a11y/axe-audit.mjs --url http://127.0.0.1:4173 --shell "$shell" \
                --release "$release" --out "$root/audits/$release.toml" --report "$report"
            ;;
        desktop-tauri)
            failed="$(bash clients/shells/tools/tauri-webview-axe.sh "$release")"
            bash clients/shells/tools/record-audit.sh "$shell" "$release" "$(ci_runner)" \
                "WebKitGTK webview" "tauri-driver + axe-core" "$(cargo tauri --version 2>/dev/null || echo unknown)" \
                "$report" $failed
            ;;
        linux-gtk)
            failed="$(bash clients/shells/tools/atspi-walk.sh "$release")"
            bash clients/shells/tools/record-audit.sh "$shell" "$release" "$(ci_runner)" \
                "GTK4 under Xvfb" "AT-SPI2 tree walk" "$(pkg-config --modversion atspi-2 2>/dev/null || echo unknown)" \
                "$report" $failed
            ;;
        apple)
            [ -e "$root/Package.swift" ] || compgen -G "$root/*.xcodeproj" >/dev/null || no_project "an Xcode project in $root"
            need xcodebuild "shell-audit: the apple audit needs a macOS runner with Xcode"
            failed="$(bash "$root/audit.sh" "$release" "$report")"
            bash clients/shells/tools/record-audit.sh "$shell" "$release" "$(ci_runner)" \
                "${MIDGARD_AUDIT_DEVICE:-iOS simulator}" "XCUITest + XCUIAccessibilityAudit" \
                "$(xcodebuild -version | head -1)" "$report" $failed
            ;;
        android)
            [ -x "$root/gradlew" ] || no_project "$root/gradlew"
            failed="$(bash "$root/audit.sh" "$release" "$report")"
            bash clients/shells/tools/record-audit.sh "$shell" "$release" "$(ci_runner)" \
                "${MIDGARD_AUDIT_DEVICE:-Android emulator}" "Espresso + Accessibility Test Framework" \
                "$("$root/gradlew" --version 2>/dev/null | sed -n 's/^Gradle //p' | head -1)" "$report" $failed
            ;;
        *)
            ci_error "shell-audit: no audit is defined for $shell"
            exit 2
            ;;
    esac
}

job_shell_audit_gate() {
    bash deploy/acceptance/verify-shell-audits.sh "${1:-$(release_name)}"
}

case "${1:-}" in
    list)
        awk -F'|' 'NF == 3 { printf "%-18s %-10s %s\n", $1, "[" $2 "]", $3 }' <<<"$JOBS"
        ;;
    names)
        awk -F'|' 'NF == 3 { print $1 }' <<<"$JOBS"
        ;;
    platform)
        echo "ci system: $(ci_platform)"
        echo "forge:     $(ci_forge)"
        echo "commit:    $(ci_sha)"
        echo "tag:       $(ci_tag)"
        echo "runner:    $(ci_runner)"
        ;;
    '')
        echo "usage: $0 list | platform | <job> [ARG...]" >&2
        exit 2
        ;;
    *)
        job="$1"
        shift
        if ! awk -F'|' -v j="$job" '$1 == j { found = 1 } END { exit !found }' <<<"$JOBS"; then
            echo "run: there is no job called $job (see: $0 list)" >&2
            exit 2
        fi
        "job_${job//-/_}" "$@"
        ;;
esac
