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
# the host.
set -euo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib.sh

IMAGE="${TOOLCHAIN_IMAGE:-midgard-acceptance:1.82.0}"
export TOOLCHAIN_IMAGE="$IMAGE"
TOOLCHAIN_LABEL=dev.midgard.toolchain.dockerfile
# Where a saved copy of the toolchain image is kept between jobs, when the CI system
# caches this directory.
CACHE_DIR="${MIDGARD_CI_CACHE:-.ci-cache}"

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
k8s-render|host|deploy/k8s/enclosure.yaml is what the Helm chart renders (needs helm)
toolchain|host|build or restore the digest-pinned toolchain image (the only online step)
fmt|toolchain|formatting
lint|toolchain|clippy at deny-warnings
build|toolchain|release build of the workspace
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
windows-build|host|release build of the suite services on a Windows x64 runner (Git Bash, MSVC)
windows-test|host|clippy and tests of the whole Rust workspace on a Windows x64 runner (Git Bash, MSVC)
windows-smoke|host|the services end to end on a fixture recording on a Windows x64 runner, after windows-build
windows-shell-build|host|locked restore, release build and win-x64 single-file publish of the C# shell and midgard-frames.exe (apps/windows; .NET 10 SDK)
windows-shell-test|host|locked restore and the xUnit tests of the C# shell and of midgard-frames.exe against a pinned ffmpeg (apps/windows; .NET 10 SDK)
windows-shell-e2e|host|the C# shell end to end on a Windows x64 runner: Midgard.exe --headless-check on a package of the services, consoles, camera analytics and a fixture
windows-analytics|host|the camera analytics on the pinned Windows runtime (tools/win-app/analytics.lock): imports, one analysis of a synthetic clip with midgard-frames.exe, compared with ffmpeg
windows-llama|host|the pinned Windows llama.cpp (tools/mac-app/llama.lock) laid out by tools/win-app/fetch-llama.sh: --version, then a tiny test model loaded and answering, as the shell runs it
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
# keystore mounted read-only at a fixed path, the registry read-write, and the
# variables renamed to match. Nothing else from the runner's environment goes in.
signing_mounts() {
    local -n into="$1"
    into=()
    : "${MIDGARD_SIGNING_KEYSTORE:?the release signing keystore is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_BIFROST_KEYSTORE:?the Bifrost admission keystore is not configured (docs/ops/PKI.md)}"
    : "${MIDGARD_BIFROST_REGISTRY:?the Bifrost registry directory is not configured (docs/ops/PKI.md)}"
    into+=(--mount "$MIDGARD_SIGNING_KEYSTORE:/keys/release.keystore:ro"
           --env MIDGARD_SIGNING_KEYSTORE=/keys/release.keystore
           --mount "$MIDGARD_BIFROST_KEYSTORE:/keys/admission.keystore:ro"
           --env MIDGARD_BIFROST_KEYSTORE=/keys/admission.keystore
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
}

# Restore the toolchain image from the cache directory when the CI system kept one for
# this Dockerfile; build it, and save it for next time, when it did not.
# The Dockerfile an image was built from, as recorded in its label; empty when the
# image is absent or carries no label.
toolchain_built_from() {
    docker image inspect --format "{{ index .Config.Labels \"$TOOLCHAIN_LABEL\" }}" "$IMAGE" 2>/dev/null || true
}

job_toolchain() {
    local key tarball
    need docker "toolchain: docker is not available"
    # The tag names the Rust version and says nothing about the rest of the Dockerfile,
    # so an image built before the Dockerfile last changed still answers to it. The
    # label does say: an image is current only when it records this Dockerfile.
    key="$( (sha256sum deploy/acceptance/Dockerfile 2>/dev/null || shasum -a 256 deploy/acceptance/Dockerfile) | cut -c1-16)"
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

job_fmt() { in_toolchain just fmt-check; }
job_lint() { in_toolchain just lint; }
job_build() { in_toolchain just build-rust; }
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
job_release_images() {
    local version out=target/release-out mounts
    version="$(release_name)"
    [ -s "$out/release-manifest.txt" ] || {
        ci_error "release-images: $out holds no completed release; run the release job first"
        exit 1
    }
    bash deploy/images/build.sh --version "$version" --verified "$out" \
        --trust "${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}" --out "$out/images"
    signing_mounts mounts
    in_toolchain "${mounts[@]}" \
        --env "MIDGARD_TRUST_ANCHORS=${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}" -- \
        bash -c "just sbom $out/images/images.lock && just scan $out/images/images.lock \
                 && just sign $out/images/images.lock && just verify $out/images/images.lock"
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
               -o -name '*.mgbundle' -o -name 'images.lock' -o -name 'release-manifest.txt' \) | sort
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

# The services the Windows presentation app ships, built natively on a Windows x64
# runner with the pinned toolchain and the vendored crates (no network after the
# toolchain install). The binaries are the same six tools/mac-app/build-services.sh
# builds; heimdall-source comes from the heimdall-feed-server package. [NFR-OSS-1]
WINDOWS_TARGET=x86_64-pc-windows-msvc
WINDOWS_PACKAGES=(-p http-gateway -p heimdall-feed-server -p mimir-server -p forge-worker -p midgard-replay)
WINDOWS_BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
# The pinned toolchain for the Windows target, installed by rustup on the runner.
windows_toolchain() {
    local channel
    need rustup "$1: rustup is needed (the GitHub windows runners carry it)"
    channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
    ci_group "toolchain $channel ($WINDOWS_TARGET)"
    rustup toolchain install "$channel" --profile minimal --component clippy --target "$WINDOWS_TARGET"
    rustc --version
    cargo --version
    ci_endgroup
}

job_windows_build() {
    local dir b missing=0
    windows_toolchain windows-build
    cargo build --offline --locked --release --target "$WINDOWS_TARGET" "${WINDOWS_PACKAGES[@]}" --bins
    dir="target/$WINDOWS_TARGET/release"
    for b in "${WINDOWS_BINARIES[@]}"; do
        if [ -f "$dir/$b.exe" ]; then
            echo "$b.exe: $(wc -c < "$dir/$b.exe" | tr -d ' ') bytes"
        else
            ci_error "windows-build: $dir/$b.exe was not built"
            missing=1
        fi
    done
    ls -l "$dir"/*.exe
    [ "$missing" = 0 ] || return 1
    windows_services_artifact "$dir"
}

# The artefact the pipeline keeps (midgard-services-windows-x64-<sha>, 30 days) and
# tools/win-app/fetch-artifacts.sh downloads: the six services and a BUILD.txt naming
# the commit, toolchain and date, in target/windows-services.
WINDOWS_ARTIFACT_DIR=target/windows-services
windows_services_artifact() {
    local b out="$WINDOWS_ARTIFACT_DIR"
    rm -rf "$out"
    mkdir -p "$out"
    for b in "${WINDOWS_BINARIES[@]}"; do
        cp "$1/$b.exe" "$out/"
    done
    {
        echo "commit: $(ci_sha)"
        echo "target: $WINDOWS_TARGET"
        echo "toolchain: $(rustc --version)"
        echo "cargo: $(cargo --version)"
        echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    } >"$out/BUILD.txt"
    cat "$out/BUILD.txt"
}

# The whole Rust workspace, linted and tested natively on Windows. The workspace cannot
# be built in one `cargo test --workspace`: the INSECURE `test-backend` and the real
# `backend-fips` refuse to build together (crypto/pqc-agility), so the feature sets
# are chosen per crate. This runs what `just lint` and `just test` run, line by line,
# then the workspace members each recipe leaves out, with their default features.
# A crate that cannot apply on Windows goes in WINDOWS_TEST_EXCLUDE with a one-line
# reason; none does today. It applies to the leftover packages only (those no recipe
# line names): a crate a recipe line names is linted and tested as the recipe says.
WINDOWS_TEST_EXCLUDE=()
# The arguments of recipe $1's `cargo $2 ...` lines (words 3 on), one line each: a line
# is one whose first word is `cargo` (or `@cargo`, just's quiet prefix) and second is
# $2. Fails when there are none, so a renamed recipe cannot pass with nothing run.
recipe_cargo_lines() {
    awk -v r="$1:" -v c="$2" '
        $1 == r { on = 1; next }
        on && /^[^ \t#]/ { on = 0 }
        on && ($1 == "cargo" || $1 == "@cargo") && $2 == c {
            line = $0
            sub(/^[ \t]*@?cargo[ \t]+[^ \t]+[ \t]*/, "", line)
            print line
            n++
        }
        END { exit n == 0 }' justfile
}
# The workspace members' package names. This assumes the root Cargo.toml lists
# `members = [` with one quoted path per line up to a closing `]` on its own line, no
# globs, and that each member's first `name = "..."` line is its package name (the
# [package] table comes first).
workspace_packages() {
    local m
    for m in $(sed -n '/^members *= *\[/,/^\]/p' Cargo.toml | sed -n 's/^ *"\(.*\)",*/\1/p'); do
        sed -n 's/^name *= *"\(.*\)"/\1/p' "$m/Cargo.toml" | head -n 1
    done
}
# `-p NAME` for each workspace member that none of `lines` (recipe_cargo_lines output)
# names, nor WINDOWS_TEST_EXCLUDE.
windows_rest_packages() {
    local named pkg rest=()
    named=" $(grep -oE -- '-p [A-Za-z0-9_-]+' <<<"$1" | awk '{ print $2 }' | tr '\n' ' ') ${WINDOWS_TEST_EXCLUDE[*]} "
    for pkg in $(workspace_packages); do
        case "$named" in *" $pkg "*) ;; *) rest+=(-p "$pkg") ;; esac
    done
    echo "${rest[*]}"
}
job_windows_test() {
    local failed=0 line lint_lines test_lines rest_lint rest_test
    local common=(--offline --locked --target "$WINDOWS_TARGET")
    windows_toolchain windows-test
    lint_lines="$(recipe_cargo_lines lint clippy)" || {
        ci_error "windows-test: the justfile's lint recipe has no cargo clippy line"
        exit 1
    }
    test_lines="$(recipe_cargo_lines test test)" || {
        ci_error "windows-test: the justfile's test recipe has no cargo test line"
        exit 1
    }
    rest_lint="$(windows_rest_packages "$lint_lines")"
    rest_test="$(windows_rest_packages "$test_lines")"
    echo "not in just lint: ${rest_lint:-none}"
    echo "not in just test: ${rest_test:-none}"
    # Every step runs, so one run shows every lint and every test failure. Cargo reads
    # nothing: the loop's input is the recipe.
    while read -r line; do
        echo "+ cargo clippy $line"
        # shellcheck disable=SC2086 # the recipe line is split into its words
        cargo clippy "${common[@]}" $line </dev/null || failed=1
    done <<<"$lint_lines"
    if [ -n "$rest_lint" ]; then
        echo "+ cargo clippy $rest_lint --all-targets -- -D warnings"
        # shellcheck disable=SC2086
        cargo clippy "${common[@]}" $rest_lint --all-targets -- -D warnings || failed=1
    fi
    while read -r line; do
        echo "+ cargo test $line"
        # shellcheck disable=SC2086
        cargo test "${common[@]}" $line --no-fail-fast </dev/null || failed=1
    done <<<"$test_lines"
    if [ -n "$rest_test" ]; then
        echo "+ cargo test $rest_test"
        # shellcheck disable=SC2086
        cargo test "${common[@]}" $rest_test --no-fail-fast || failed=1
    fi
    return "$failed"
}

# The suite end to end on Windows: the release services windows-build made, a synthetic
# fixture recording (tools/win-app/make-fixture.py) and a stub model server, started,
# checked and stopped by tools/win-app/run-suite.py. mg-admin and mg-model, which build
# the working copy, are built here. Nothing is downloaded. The services' logs are left
# in target/windows-smoke/logs for the pipeline to keep when the job fails.
job_windows_smoke() {
    local dir="target/$WINDOWS_TARGET/release" out=target/windows-smoke py
    windows_toolchain windows-smoke
    cargo build --offline --locked --release --target "$WINDOWS_TARGET" -p mg-admin -p mg-model --bins
    py="$(command -v python || command -v python3)" || {
        ci_error "windows-smoke: python is not on PATH"
        exit 1
    }
    "$py" --version
    # Relative paths: a native Windows python is handed no POSIX path to translate.
    rm -rf "$out"
    mkdir -p "$out/logs"
    "$py" tools/win-app/make-fixture.py "$out/fixture"
    "$py" tools/win-app/run-suite.py --bin "$dir" --fixture "$out/fixture" --logs "$out/logs"
}

# The Windows app's C# shell (apps/windows, docs/plans/2026-10-23-windows-app-w2-shell.md):
# Midgard.Core, the Midgard WinForms exe and the xUnit tests, with the .NET 10 SDK that
# apps/windows/global.json pins (the GitHub windows runners carry it). Packages come only
# from nuget.org (apps/windows/nuget.config) and only as each project's committed
# packages.lock.json says: the restore is locked, so a package or version changes only by
# a commit that changes a lock file. [NFR-OSS-1]
WINDOWS_SHELL=apps/windows
WINDOWS_SHELL_PROJECTS=(Midgard.Core Midgard Midgard.Core.Tests MidgardFrames MidgardFrames.Tests)
WINDOWS_SHELL_OUT=target/windows-shell
WINDOWS_SHELL_TEST_OUT=target/windows-shell-test
windows_dotnet() {
    export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1
    need dotnet "$1: the .NET 10 SDK is needed (the GitHub windows runners carry it)"
    ci_group "dotnet SDKs"
    dotnet --list-sdks
    ci_endgroup
    # From apps/windows, so global.json picks the SDK: this fails when no 10.0.4xx SDK is there
    # (the band is pinned, latest patch, so a new feature band on the runners changes nothing).
    local version
    version="$(cd "$WINDOWS_SHELL" && dotnet --version)"
    echo "$1: .NET SDK $version"
}
# The locked restore of the solution. When it fails, or a lock file is missing, an
# unlocked restore shows what the lock files would have to say, for review and commit,
# and the job fails all the same.
windows_shell_restore() {
    local p missing=0
    for p in "${WINDOWS_SHELL_PROJECTS[@]}"; do
        if [ ! -f "$WINDOWS_SHELL/$p/packages.lock.json" ]; then
            ci_error "$1: $WINDOWS_SHELL/$p/packages.lock.json is missing"
            missing=1
        fi
    done
    if [ "$missing" = 0 ] && dotnet restore "$WINDOWS_SHELL/Midgard.sln" --locked-mode; then
        return 0
    fi
    dotnet restore "$WINDOWS_SHELL/Midgard.sln" --force-evaluate || true
    for p in "${WINDOWS_SHELL_PROJECTS[@]}"; do
        echo "--- what restore resolves for $WINDOWS_SHELL/$p/packages.lock.json:"
        cat "$WINDOWS_SHELL/$p/packages.lock.json" 2>/dev/null || echo "(nothing)"
    done
    ci_error "$1: the NuGet lock files do not match the projects; review the files above and commit them"
    return 1
}

job_windows_shell_build() {
    local exe="$WINDOWS_SHELL_OUT/Midgard.exe" frames="$WINDOWS_SHELL_OUT/midgard-frames.exe"
    windows_dotnet windows-shell-build
    windows_shell_restore windows-shell-build
    dotnet build "$WINDOWS_SHELL/Midgard.sln" -c Release --no-restore
    rm -rf "$WINDOWS_SHELL_OUT"
    dotnet publish "$WINDOWS_SHELL/Midgard/Midgard.csproj" -c Release -r win-x64 --self-contained \
        -p:PublishSingleFile=true --no-restore -o "$WINDOWS_SHELL_OUT"
    # midgard-frames.exe, the analytics' clip decoder (W3 Task 1), beside it.
    dotnet publish "$WINDOWS_SHELL/MidgardFrames/MidgardFrames.csproj" -c Release -r win-x64 --self-contained \
        -p:PublishSingleFile=true --no-restore -o "$WINDOWS_SHELL_OUT"
    ls -l "$WINDOWS_SHELL_OUT"
    if [ ! -f "$exe" ] || [ ! -f "$frames" ]; then
        ci_error "windows-shell-build: $exe or $frames was not published"
        return 1
    fi
    # The published helper runs: no arguments is bad usage (exit 2, the usage on stderr).
    local status=0
    "$frames" 2>"$WINDOWS_SHELL_OUT/frames-usage.txt" || status=$?
    if [ "$status" != 2 ] || ! grep -q '^midgard-frames: usage:' "$WINDOWS_SHELL_OUT/frames-usage.txt"; then
        cat "$WINDOWS_SHELL_OUT/frames-usage.txt"
        ci_error "windows-shell-build: midgard-frames.exe with no arguments exited $status, not 2 with its usage"
        return 1
    fi
    rm -f "$WINDOWS_SHELL_OUT/frames-usage.txt"
    echo "Midgard.exe: $(wc -c < "$exe" | tr -d ' ') bytes; midgard-frames.exe: $(wc -c < "$frames" | tr -d ' ') bytes;" \
        "the publish folder: $(du -sk "$WINDOWS_SHELL_OUT" | cut -f1) KB"
    windows_shell_artifact "$exe" "$frames"
}

# The artefact the pipeline keeps (midgard-shell-windows-x64-<sha>, 30 days) and
# tools/win-app/fetch-artifacts.sh downloads: Midgard.exe, midgard-frames.exe, vcruntime/ (the
# Visual C++ runtime the package ships, windows_vcruntime), notices/ (the licences of what the
# two exes carry, windows_shell_notices) and a BUILD.txt naming the commit,
# runtime, SDK, the runtime's toolset and date, in target/windows-shell-artifact.
WINDOWS_SHELL_ARTIFACT_DIR=target/windows-shell-artifact
windows_shell_artifact() {
    local out="$WINDOWS_SHELL_ARTIFACT_DIR"
    rm -rf "$out"
    mkdir -p "$out"
    cp "$1" "$2" "$out/"
    ci_group "the Visual C++ runtime (vcruntime/)"
    windows_vcruntime "$out/vcruntime"
    ci_endgroup
    ci_group "the .NET runtime's and WebView2's licences (notices/)"
    windows_shell_notices "$out/notices"
    ci_endgroup
    {
        echo "commit: $(ci_sha)"
        echo "runtime: win-x64 (self-contained, single file)"
        echo "sdk: $(cd "$WINDOWS_SHELL" && dotnet --version)"
        echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "vcruntime: $(sed -n 's/^toolset: //p' "$out/vcruntime/VERSIONS.txt" 2>/dev/null)"
    } >"$out/BUILD.txt"
    cat "$out/BUILD.txt"
}

# The Visual C++ runtime, shipped app-local (the Visual C++ Redistributable's app-local
# deployment: the DLLs beside the programs that import them), since a clean Windows 11 does
# not have it. The set is what the package's binaries import that Windows lacks, found with
# tools/win-app/pe-deps.py / check-imports.py (W4 Task 1): the services import VCRUNTIME140;
# llama.cpp's DLLs MSVCP140, VCRUNTIME140 and VCRUNTIME140_1; onnxruntime MSVCP140 and
# MSVCP140_1 (the analytics runtime brings only vcruntime140 and vcruntime140_1, and numpy its
# own renamed msvcp140). check-imports.py on the package layout (windows-shell-e2e) fails if a
# binary comes to need another.
# VCRUNTIME_DLLS, VCRUNTIME_FOLDERS (the package folders that get a copy) and
# win_place_vcruntime are tools/win-app/layout.sh's, which build-zip.sh uses too.
. tools/win-app/layout.sh
# Copies the x64 Microsoft.VC14x.CRT DLLs of the runner's Visual Studio (its default redist
# version) into $1, with VERSIONS.txt: the toolset, the source, each file's version and
# SHA-256, and the licence notice.
windows_vcruntime() {
    local out=$1 vswhere vs ver crt f py
    vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    [ -f "$vswhere" ] || { ci_error "vcruntime: vswhere.exe is not on this runner"; return 1; }
    vs="$("$vswhere" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
        -property installationPath | tr -d '\r')"
    [ -n "$vs" ] || { ci_error "vcruntime: no Visual Studio with the C++ tools on this runner"; return 1; }
    vs="$(cygpath -u "$vs")"
    ver="$(tr -d '\r\n ' < "$vs/VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt")"
    crt="$(ls -d "$vs/VC/Redist/MSVC/$ver/x64/"Microsoft.VC14*.CRT 2>/dev/null | head -n 1 || true)"
    [ -n "$crt" ] && [ -d "$crt" ] || { ci_error "vcruntime: no x64 CRT redist folder for $ver under $vs"; return 1; }
    py="$(command -v python || command -v python3)"
    rm -rf "$out"
    mkdir -p "$out"
    for f in "${VCRUNTIME_DLLS[@]}"; do
        [ -f "$crt/$f" ] || { ci_error "vcruntime: $crt has no $f"; return 1; }
        cp "$crt/$f" "$out/"
    done
    {
        echo "toolset: $ver"
        echo "visual studio: $("$vswhere" -latest -products '*' -property catalog_productDisplayVersion | tr -d '\r')"
        echo "source: $(basename "$(dirname "$(dirname "$crt")")")/x64/$(basename "$crt")"
        echo "# file, FileVersion, SHA-256"
        for f in "${VCRUNTIME_DLLS[@]}"; do
            echo "$("$py" -I tools/win-app/pe-deps.py --versions "$out/$f" | tr -d '\r') $( (sha256sum "$out/$f" 2>/dev/null || shasum -a 256 "$out/$f") | awk '{print $1}')"
        done
        echo "# Microsoft Visual C++ runtime files, (c) Microsoft Corporation. Redistributed, unmodified"
        echo "# and app-local, as the Visual Studio licence terms' Distributable Code allows for the files"
        echo "# Visual Studio's Redist list names (the VC\\Redist\\MSVC folder):"
        echo "# https://visualstudio.microsoft.com/license-terms/ (Distributable Code), and the Redist list"
        echo "# for the Visual Studio version above (learn.microsoft.com/visualstudio/releases/<year>/redistribution);"
        echo "# deployment: https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files"
    } >"$out/VERSIONS.txt"
    cat "$out/VERSIONS.txt"
    # What licence files the installation carries, for the record (W4 Task 1).
    ls "$vs/Licenses" 2>/dev/null | head -20 || true
    find "$vs/VC/Redist/MSVC/$ver" -maxdepth 1 -type f | head -20 || true
}
# The licences of what Midgard.exe and midgard-frames.exe carry inside them, into $1 for the
# package's resources\licenses: the self-contained .NET runtime packs the publish used (the
# *.Runtime.win-x64 download dependencies in Midgard's obj/project.assets.json of the shared
# frameworks it references: .NET's and
# Windows Desktop's LICENSE.TXT and THIRD-PARTY-NOTICES.TXT) and the WebView2 SDK's LICENSE.txt
# (its version from Midgard's packages.lock.json), from the NuGet global packages folder (or
# the SDK's packs folder), with VERSIONS.txt naming each. Run after a restore.
windows_shell_notices() {
    local out=$1 py nuget packs line name ver dir short f n=0
    py="$(command -v python || command -v python3)"
    nuget="$(dotnet nuget locals global-packages --list | sed -n 's/^global-packages: *//p' | tr -d '\r')"
    [ -n "$nuget" ] || { ci_error "notices: dotnet names no NuGet global packages folder"; return 1; }
    nuget="$(cygpath -u "$nuget" 2>/dev/null || echo "$nuget")"
    packs="$(dirname "$(command -v dotnet)")/packs"
    rm -rf "$out"
    mkdir -p "$out"
    echo "# package, version, where the licence files came from" >"$out/VERSIONS.txt"
    while IFS=' ' read -r name ver; do
        [ -n "$name" ] || continue
        case "$name" in
            Microsoft.NETCore.App.Runtime.*) short=dotnet-runtime ;;
            Microsoft.WindowsDesktop.App.Runtime.*) short=windowsdesktop-runtime ;;
            Microsoft.AspNetCore.App.Runtime.*) short=aspnetcore-runtime ;;
            Microsoft.Web.WebView2) short=WebView2 ;;
            *) ci_error "notices: no short name for $name"; return 1 ;;
        esac
        dir="$nuget/$(echo "$name" | tr '[:upper:]' '[:lower:]')/$ver"
        [ -d "$dir" ] || dir="$packs/$name/$ver"
        [ -d "$dir" ] || { ci_error "notices: $name $ver is neither in $nuget nor in $packs"; return 1; }
        local got=0
        for f in "$dir"/*; do
            [ -f "$f" ] || continue
            case "$(basename "$f" | tr '[:lower:]' '[:upper:]')" in
                LICENSE.TXT|LICENSE.MD|LICENSE) cp "$f" "$out/$short-LICENSE.txt"; got=1 ;;
                THIRD-PARTY-NOTICES.TXT|THIRDPARTYNOTICES.TXT|NOTICE.TXT) cp "$f" "$out/$short-THIRD-PARTY-NOTICES.txt" ;;
            esac
        done
        [ "$got" = 1 ] || { ls -la "$dir"; ci_error "notices: $name $ver has no LICENSE file in $dir"; return 1; }
        echo "$name $ver $dir" >>"$out/VERSIONS.txt"
        n=$((n + 1))
    done < <("$py" -I - "$WINDOWS_SHELL/Midgard/obj/project.assets.json" "$WINDOWS_SHELL/Midgard/packages.lock.json" <<'PY2'
import json, sys
assets = json.load(open(sys.argv[1], encoding="utf-8-sig"))
seen = set()
for fw in assets["project"]["frameworks"].values():
    # Only the shared frameworks the app references (the restore also lists packs it never
    # uses, such as ASP.NET Core's, without downloading them). Microsoft.NETCore.App is implicit.
    used = {"Microsoft.NETCore.App"} | set(fw.get("frameworkReferences", {}))
    for dep in fw.get("downloadDependencies", []):
        name, ver = dep["name"], dep["version"].strip("[]").split(",")[0].strip()
        if (name.endswith(".Runtime.win-x64") and name[:-len(".Runtime.win-x64")] in used
                and name not in seen):
            seen.add(name)
            print(name, ver)
lock = json.load(open(sys.argv[2], encoding="utf-8-sig"))
for tfm in lock["dependencies"].values():
    w = tfm.get("Microsoft.Web.WebView2")
    if w:
        print("Microsoft.Web.WebView2", w["resolved"])
        break
PY2
)
    grep -q '^Microsoft.NETCore.App.Runtime.win-x64 ' "$out/VERSIONS.txt" \
        || { ci_error "notices: the .NET runtime pack was not found in obj/project.assets.json"; return 1; }
    grep -q '^Microsoft.Web.WebView2 ' "$out/VERSIONS.txt" \
        || { ci_error "notices: WebView2's version was not found in packages.lock.json"; return 1; }
    cat "$out/VERSIONS.txt"
    ls -l "$out"
}
# tools/win-app/check-imports.py on package $2 (report to $3-before/after.txt) before and after
# the runtime in $1 goes in: before, it must fail on Visual C++ runtime DLLs only; after, pass.
windows_check_imports() {
    local rt=$1 pkg=$2 rep=$3 py other
    py="$(command -v python || command -v python3)"
    if "$py" -I tools/win-app/check-imports.py "$pkg" --report "$rep-before.txt" >/dev/null; then
        ci_error "check-imports: $pkg passed without the Visual C++ runtime; is the check reading the imports?"
        return 1
    fi
    other="$(tr -d '\r' < "$rep-before.txt" | grep '^MISSING' | grep -viE ' imports (vcruntime140|vcruntime140_1|msvcp140|msvcp140_1)\.dll$' || true)"
    if [ -n "$other" ]; then
        printf '%s\n' "$other"
        ci_error "check-imports: $pkg imports DLLs a clean Windows 11 lacks besides the Visual C++ runtime"
        return 1
    fi
    echo "check-imports, before the runtime: $(tail -n 1 "$rep-before.txt")"
    win_place_vcruntime "$rt" "$pkg"
    "$py" -I tools/win-app/check-imports.py "$pkg" --report "$rep-after.txt"
}

# ffmpeg for the frames helper's tests only (making the synthetic clips, and the decode
# they are compared with): gyan.dev's essentials build, pinned by its SHA-256. Never shipped.
FFMPEG_WIN_VERSION=9.0.2
FFMPEG_WIN_SHA256=4705843ccaaf54257c16ad90f3e952ece33c17df964ecf7bfdbb0f49c7171077
windows_ffmpeg() {
    local dir="$CACHE_DIR/ffmpeg-$FFMPEG_WIN_VERSION" name="ffmpeg-$FFMPEG_WIN_VERSION-essentials_build" got sevenzip
    if [ ! -x "$dir/$name/bin/ffmpeg.exe" ]; then
        need curl "$1: curl is needed to fetch ffmpeg"
        mkdir -p "$dir"
        curl -fsSL -o "$dir/ffmpeg.7z" \
            "https://github.com/GyanD/codexffmpeg/releases/download/$FFMPEG_WIN_VERSION/$name.7z"
        got="$( (sha256sum "$dir/ffmpeg.7z" 2>/dev/null || shasum -a 256 "$dir/ffmpeg.7z") | awk '{print $1}')"
        if [ "$got" != "$FFMPEG_WIN_SHA256" ]; then
            rm -f "$dir/ffmpeg.7z"
            ci_error "$1: the ffmpeg download does not match its pinned checksum (got $got)"
            exit 1
        fi
        sevenzip="$(command -v 7z || command -v "/c/Program Files/7-Zip/7z.exe")" || {
            ci_error "$1: 7-Zip is needed to unpack ffmpeg"
            exit 1
        }
        "$sevenzip" x -y -bd -o"$dir" "$dir/ffmpeg.7z" "$name/bin/ffmpeg.exe" "$name/bin/ffprobe.exe" >/dev/null
        rm -f "$dir/ffmpeg.7z"
    fi
    "$dir/$name/bin/ffmpeg.exe" -hide_banner -version | sed -n 1p >&2
    echo "$dir/$name/bin/ffmpeg.exe"
}

# The camera analytics' Python runtime, numpy, onnxruntime and model for the Windows app
# (W3 Task 2), laid out by tools/win-app/fetch-analytics.sh from tools/win-app/analytics.lock
# into $CACHE_DIR/build/win/analytics (downloads checked by SHA-256). Prints that folder.
windows_analytics_runtime() {
    ci_group "the analytics runtime (tools/win-app/fetch-analytics.sh)" >&2
    MIDGARD_BUILD_DIR="$PWD/$CACHE_DIR/build" bash tools/win-app/fetch-analytics.sh >&2
    ci_endgroup >&2
    echo "$CACHE_DIR/build/win/analytics"
}
# The fixture camera with a clip (tools/win-app/make-fixture.py --camera-clip).
ANALYTICS_CLIP_CAMERA=JamCams_00000.00001

job_windows_shell_test() {
    windows_dotnet windows-shell-test
    windows_shell_restore windows-shell-test
    # Results and, for a test that hangs 5 minutes, a dump of the test host go to
    # $WINDOWS_SHELL_TEST_OUT (uploaded when the job fails).
    rm -rf "$WINDOWS_SHELL_TEST_OUT"
    mkdir -p "$WINDOWS_SHELL_TEST_OUT"
    local status=0 ffmpeg report="$WINDOWS_SHELL_TEST_OUT/frames-comparison.txt"
    dotnet test "$WINDOWS_SHELL/Midgard.Core.Tests/Midgard.Core.Tests.csproj" -c Release --no-restore \
        --logger "console;verbosity=normal" --logger "trx;LogFileName=Midgard.Core.Tests.trx" \
        --results-directory "$WINDOWS_SHELL_TEST_OUT" --blame-hang-timeout 5m || status=$?
    # The frames helper's tests: synthetic clips made by ffmpeg, the helper's frames compared
    # with ffmpeg's; the comparison is recorded in $report (and printed).
    # (The GitHub Windows Server 2025 runners decode H.264 without the Server-Media-Foundation
    # feature: docs/plans/2026-10-24-windows-app-w3-analytics.md, Task 1 as built.)
    ffmpeg="$(windows_ffmpeg windows-shell-test)"
    MIDGARD_FFMPEG="$(cygpath -aw "$ffmpeg")" MIDGARD_FRAMES_REPORT="$(cygpath -aw "$report")" \
        dotnet test "$WINDOWS_SHELL/MidgardFrames.Tests/MidgardFrames.Tests.csproj" -c Release --no-restore \
        --logger "console;verbosity=normal" --logger "trx;LogFileName=MidgardFrames.Tests.trx" \
        --results-directory "$WINDOWS_SHELL_TEST_OUT" --blame-hang-timeout 5m || status=$?
    if [ -f "$report" ]; then
        echo "--- midgard-frames against ffmpeg $FFMPEG_WIN_VERSION:"
        cat "$report"
    fi
    return "$status"
}

# The camera analytics on Windows (docs/plans/2026-10-24-windows-app-w3-analytics.md, Task 2):
# the runtime fetch-analytics.sh lays out, its python.exe importing numpy and onnxruntime,
# then tools/win-app/check-analytics.py on a synthetic clip (make-camera-clip.py, drawn and
# encoded here with the tests' ffmpeg; no footage in git) in a fixture recording:
# analytics.py --once as a program, one analysis as a module (a land-camera-ai observation),
# and midgard-frames.exe against ffmpeg (frames, detections, tracks). Its report, the clip
# and the sizes go to $WINDOWS_ANALYTICS_OUT/report.
WINDOWS_ANALYTICS_OUT=target/windows-analytics
job_windows_analytics() {
    local out="$WINDOWS_ANALYTICS_OUT" root rt ffmpeg
    windows_dotnet windows-analytics
    windows_shell_restore windows-analytics
    rm -rf "$out"
    mkdir -p "$out/report"
    dotnet publish "$WINDOWS_SHELL/MidgardFrames/MidgardFrames.csproj" -c Release -r win-x64 --self-contained \
        -p:PublishSingleFile=true --no-restore -o "$out/frames"
    ffmpeg="$(windows_ffmpeg windows-analytics)"
    root="$(windows_analytics_runtime)"
    rt="$root/runtime"
    # Relative paths: a native Windows python is handed no POSIX path to translate.
    "$rt/python.exe" -I -c "import numpy, onnxruntime; print(numpy.__version__, onnxruntime.__version__, onnxruntime.get_available_providers())"
    "$rt/python.exe" -I - "$rt" "$root/models/yolox_s.onnx" "$root/licenses" <<'PY' | tee "$out/report/sizes.txt"
import os, sys, tempfile, zipfile
def size(p):
    return os.path.getsize(p) if os.path.isfile(p) else sum(
        os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(p) for f in fs)
for label, p in zip(("runtime (helpers\\analytics)", "model (resources\\models\\yolox)", "licences"), sys.argv[1:]):
    with tempfile.TemporaryFile() as t:
        with zipfile.ZipFile(t, "w", zipfile.ZIP_DEFLATED) as z:
            if os.path.isfile(p):
                z.write(p, os.path.basename(p))
            for d, _, fs in os.walk(p):
                for f in fs:
                    z.write(os.path.join(d, f), os.path.relpath(os.path.join(d, f), p))
        zipped = t.tell()
    print(f"{label}: {size(p) / 1e6:.1f} MB, {zipped / 1e6:.1f} MB deflated in a ZIP")
PY
    cp "$root/manifest.txt" "$out/report/analytics-manifest.txt"
    "$rt/python.exe" -I tools/win-app/make-camera-clip.py --ffmpeg "$ffmpeg" --still "$out/report/clip.jpg" "$out/report/clip.mp4"
    "$rt/python.exe" -I tools/win-app/make-fixture.py --camera-clip "$out/report/clip.mp4" \
        --camera-still "$out/report/clip.jpg" "$out/recording"
    "$rt/python.exe" -I -B tools/win-app/check-analytics.py --recording "$out/recording" \
        --camera "$ANALYTICS_CLIP_CAMERA" --frames-helper "$out/frames/midgard-frames.exe" \
        --model-dir "$root/models" --ffmpeg "$ffmpeg" --ffprobe "$(dirname "$ffmpeg")/ffprobe.exe" \
        --report "$out/report/analytics-check.txt"
}

# The Windows llama.cpp (docs/plans/2026-10-24-windows-app-w4-package.md, Task 1): the folder
# tools/win-app/fetch-llama.sh lays out from tools/mac-app/llama.lock (downloads checked by
# SHA-256; it runs `llama-server.exe --version` itself), then tools/win-app/check-llama.py
# starts that llama-server.exe as the shell does on llama.cpp's own tiny test model
# (stories260K, 1.2 MB, from ggml-org/models at a pinned revision, checked by SHA-256; never
# shipped): ready, /props, a streamed completion, the CPU backend it loaded. The report and
# the server's log go to $WINDOWS_LLAMA_OUT/report.
WINDOWS_LLAMA_OUT=target/windows-llama
LLAMA_TEST_MODEL_URL=https://huggingface.co/ggml-org/models/resolve/499bc8821c6b12b4e53c5bffcb21ec206f212d81/tinyllamas/stories260K.gguf
LLAMA_TEST_MODEL_SHA256=270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d
job_windows_llama() {
    local out="$WINDOWS_LLAMA_OUT" root="$CACHE_DIR/build/win/llama" model="$CACHE_DIR/llama-test/stories260K.gguf" got py build
    py="$(command -v python || command -v python3)" || {
        ci_error "windows-llama: python is not on PATH"
        exit 1
    }
    "$py" --version
    rm -rf "$out"
    mkdir -p "$out/report"
    ci_group "the model server (tools/win-app/fetch-llama.sh)"
    MIDGARD_BUILD_DIR="$PWD/$CACHE_DIR/build" bash tools/win-app/fetch-llama.sh
    ci_endgroup
    cp "$root/manifest.txt" "$out/report/llama-manifest.txt"
    if [ ! -f "$model" ] || [ "$( (sha256sum "$model" 2>/dev/null || shasum -a 256 "$model") | awk '{print $1}')" != "$LLAMA_TEST_MODEL_SHA256" ]; then
        need curl "windows-llama: curl is needed to fetch the test model"
        mkdir -p "$(dirname "$model")"
        curl --proto '=https' --proto-redir '=https' -fsSL --retry 3 -o "$model.part" "$LLAMA_TEST_MODEL_URL"
        got="$( (sha256sum "$model.part" 2>/dev/null || shasum -a 256 "$model.part") | awk '{print $1}')"
        if [ "$got" != "$LLAMA_TEST_MODEL_SHA256" ]; then
            rm -f "$model.part"
            ci_error "windows-llama: the test model does not match its pinned checksum (got $got)"
            exit 1
        fi
        mv -f "$model.part" "$model"
    fi
    build="$(sed -n 's/^build=//p' tools/mac-app/llama.lock | tr -d '\r')"
    # The server as the package holds it (helpers\llama\), with the app-local Visual C++
    # runtime: check-imports.py fails without it and passes with it, and the check runs on it.
    rm -rf "$out/package"
    mkdir -p "$out/package/helpers"
    cp -R "$root/server" "$out/package/helpers/llama"
    windows_vcruntime "$out/vcruntime"
    windows_check_imports "$out/vcruntime" "$out/package" "$out/report/imports"
    # Relative paths: a native Windows python is handed no POSIX path to translate.
    "$py" -I tools/win-app/check-llama.py --server "$out/package/helpers/llama/llama-server.exe" --build "$build" \
        --model "$model" --report "$out/report/llama-check.txt" --log "$out/report/llama-server.log"
}

# The real shell end to end on Windows (docs/plans/2026-10-23-windows-app-w2-shell.md,
# Task 7). It builds what a package holds and lays it out with the presenters' ZIP's own
# layout (tools/win-app/layout.sh's win_layout, which build-zip.sh uses; W4 Task 4):
#   Midgard\Midgard.exe                 the published single-file shell
#   Midgard\helpers\                    the six services (built here; the cache makes it
#                                       quick), midgard-frames.exe, llama\ (fetch-llama.sh)
#                                       and analytics\ (fetch-analytics.sh), with the
#                                       Visual C++ runtime app-local
#   Midgard\resources\                  consoles, home, starting-state, models, analytics,
#                                       recording, licenses
# with the fixture (tools/win-app/make-fixture.py; its camera clip drawn by
# make-camera-clip.py with the tests' ffmpeg) for the recording, and make-starting-state.py's
# starting state and stub model for the real ones; only the install sheet is left out. The
# imports (check-imports.py) and the package (check-package.py) are checked as build-zip.sh
# checks them. Then it runs `Midgard.exe --headless-check` (Midgard/HeadlessCheck.cs) with the working
# copy and logs in %LOCALAPPDATA%\MidgardE2E and tools/win-app/stub_llm.py, run by this
# python.exe, as the model server. Its report, home.png, heimdall.png and the logs go to
# $WINDOWS_SHELL_E2E_OUT/report for the pipeline to keep, pass or fail. The consoles need
# the JS workspace installed (pnpm, from the lock file).
WINDOWS_SHELL_E2E_OUT=target/windows-shell-e2e
job_windows_shell_e2e() {
    local dir="target/$WINDOWS_TARGET/release" out="$WINDOWS_SHELL_E2E_OUT" py pkg app status=0
    windows_toolchain windows-shell-e2e
    cargo build --offline --locked --release --target "$WINDOWS_TARGET" "${WINDOWS_PACKAGES[@]}" \
        -p mg-admin -p mg-model --bins
    windows_dotnet windows-shell-e2e
    windows_shell_restore windows-shell-e2e
    rm -rf "$out"
    mkdir -p "$out/report/logs"
    dotnet publish "$WINDOWS_SHELL/Midgard/Midgard.csproj" -c Release -r win-x64 --self-contained \
        -p:PublishSingleFile=true --no-restore -o "$out/publish"
    dotnet publish "$WINDOWS_SHELL/MidgardFrames/MidgardFrames.csproj" -c Release -r win-x64 --self-contained \
        -p:PublishSingleFile=true --no-restore -o "$out/publish"
    local ffmpeg analytics
    ffmpeg="$(windows_ffmpeg windows-shell-e2e)"
    analytics="$(windows_analytics_runtime)"
    need node "windows-shell-e2e: node is not on PATH (the consoles are built with it)"
    node --version
    if ! command -v pnpm >/dev/null 2>&1; then
        need corepack "windows-shell-e2e: neither pnpm nor corepack is on PATH"
        corepack enable pnpm
    fi
    ci_group "consoles"
    pnpm install --frozen-lockfile --prefer-offline
    bash tools/mac-app/build-consoles.sh
    ci_endgroup
    py="$(command -v python || command -v python3)" || {
        ci_error "windows-shell-e2e: python is not on PATH"
        exit 1
    }
    "$py" --version

    # What the layout takes, as build-zip.sh gives it from the CI artefacts and the Mac's
    # resources: the publish folder (the shell artefact's two exes), the services built here,
    # the pinned llama.cpp and analytics runtime, the consoles, the fixture recording (its
    # camera clip drawn by make-camera-clip.py with the tests' ffmpeg) with a starting state
    # and stub model made from it (make-starting-state.py), the .NET and WebView2 licences,
    # and the Visual C++ runtime.
    ci_group "the model server (tools/win-app/fetch-llama.sh)"
    MIDGARD_BUILD_DIR="$PWD/$CACHE_DIR/build" bash tools/win-app/fetch-llama.sh
    ci_endgroup
    ci_group "the .NET runtime's and WebView2's licences, the Visual C++ runtime"
    windows_shell_notices "$out/notices"
    windows_vcruntime "$out/vcruntime"
    ci_endgroup
    # Relative paths: a native Windows python is handed no POSIX path to translate.
    "$analytics/runtime/python.exe" -I tools/win-app/make-camera-clip.py --ffmpeg "$ffmpeg" \
        --still "$out/clip.jpg" "$out/clip.mp4"
    "$py" tools/win-app/make-fixture.py --camera-clip "$out/clip.mp4" --camera-still "$out/clip.jpg" \
        "$out/fixture/recording"
    "$py" tools/win-app/make-starting-state.py --tools "$dir" --fixture "$out/fixture/recording" \
        --out "$out/fixture/starting-state" --models "$out/fixture/models"

    # The package, by build-zip.sh's own layout (tools/win-app/layout.sh); only the install
    # sheet (a PDF rendered on the Mac) is left out.
    pkg="$out/package/Midgard"
    mkdir -p "$out/package"
    ci_group "layout (tools/win-app/layout.sh)"
    RES_PYTHON="$py" WL_SHELL="$out/publish" WL_SERVICES="$dir" WL_LLAMA="$CACHE_DIR/build/win/llama/server" \
        WL_ANALYTICS="$analytics" WL_CONSOLES=dist/mac/consoles WL_STATE="$out/fixture/starting-state" \
        WL_MODELS="$out/fixture/models" WL_RECORDING="$out/fixture/recording" WL_NOTICES="$out/notices" \
        WL_VCRUNTIME="$out/vcruntime" WL_SHEET= win_layout "$pkg"
    ci_endgroup
    # Every import resolvable on a clean Windows 11: not before the Visual C++ runtime is in,
    # and then yes (tools/win-app/check-imports.py, as build-zip.sh runs it); then the
    # package checks build-zip.sh makes before it zips (tools/win-app/check-package.py).
    ci_group "check-imports, check-package"
    windows_check_imports "$out/vcruntime" "$pkg" "$out/report/imports"
    "$py" -I tools/win-app/check-package.py "$pkg" --report "$out/report/package.txt"
    ci_endgroup
    echo "package: $(du -sk "$pkg" | cut -f1) KB"
    ls -l "$pkg" "$pkg/helpers" "$pkg/resources" "$pkg/resources/licenses"

    # The working copy and logs where a presenter's would be (%LOCALAPPDATA%), so the
    # folder check sees a real profile's permissions; fresh for each run.
    app="$(cygpath -u "$LOCALAPPDATA")/MidgardE2E"
    rm -rf "$app"
    # The check refuses to run without the opt-in and both folders set (HeadlessGuard.cs):
    # first that refusal, from the real exe, which must touch nothing.
    local refused=0 real had=0
    real="$(cygpath -u "$LOCALAPPDATA")/Midgard"
    [ -e "$real" ] && had=1
    MIDGARD_APP_HOME="$(cygpath -w "$app/Home")" MIDGARD_APP_LOGS="$(cygpath -w "$app/Logs")" \
        MIDGARD_APP_WEBVIEW="$(cygpath -w "$app/WebView")" "$pkg/Midgard.exe" --headless-check "$(cygpath -w "$out/refused.json")" || refused=$?
    if [ "$refused" != 3 ] || [ -e "$app" ] || [ -e "$out/refused.json" ]; then
        ci_error "windows-shell-e2e: --headless-check without MIDGARD_HEADLESS_CHECK=1 did not refuse (exit $refused)"
        return 1
    fi
    MIDGARD_HEADLESS_CHECK=1 MIDGARD_APP_HOME="$(cygpath -w "$app/Home")" MIDGARD_APP_LOGS="$(cygpath -w "$app/Logs")" \
        MIDGARD_APP_WEBVIEW="$(cygpath -w "$app/WebView")" "$pkg/Midgard.exe" --headless-check "$(cygpath -w "$out/report/report.json")" \
        --model-stub "$(cygpath -w "$py")" "$(cygpath -w "$PWD/tools/win-app/stub_llm.py")" || status=$?
    cp -R "$app/Logs/." "$out/report/logs/" 2>/dev/null || true
    # Nothing of a presenter's own: the real %LOCALAPPDATA%\Midgard (working copy, logs,
    # browser profile, settings) must not have been made.
    if [ "$had" = 0 ] && [ -e "$real" ]; then
        ci_error "windows-shell-e2e: the check touched $real (a presenter's folder)"
        ls -la "$real"
        status=1
    fi
    if [ ! -f "$out/report/report.json" ]; then
        ci_error "windows-shell-e2e: Midgard.exe wrote no report (exit $status)"
        return 1
    fi
    ci_group "report.json"
    cat "$out/report/report.json"
    ci_endgroup
    "$py" - "$out/report/report.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1], encoding="utf-8"))
for s in r["steps"]:
    print(f"{'ok  ' if s['ok'] else 'FAIL'} {s['step']} ({s['took_s']} s) {s.get('detail') or ''}"[:400])
print(f"window: {r['facts'].get('window')}; screenshots: {', '.join(r['screenshots']) or 'none'}; "
      f"requests {len(r['requests'])}, blocked {len(r['blockedRequests'])}")
print(f"camera analytics: {json.dumps(r['facts'].get('analyse'))}")
print(f"{r['result']}: {r['passed']} passed, {r['failed']} failed in {r['took_s']} s")
PY
    if [ "$status" != 0 ]; then
        ci_error "windows-shell-e2e: Midgard.exe --headless-check failed (exit $status)"
        return 1
    fi
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
