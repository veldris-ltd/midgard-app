#!/usr/bin/env bash
# Tests for the toolchain digest: the toolchain-publish job, and the `toolchain` job
# inside the published image. [VLD-ANB-MIDGARD-0043] [AC-P3.4] [AC-M-6]
#
#   bash tools/ci/tests/toolchain-publish.test.sh
#
# The claim under test: toolchain-publish fails, naming tools/ci/toolchain.digest,
# when the digest it pushed is not the committed one (or none is committed), passes
# when it is, and pushes nothing when the committed image is already published from
# this Dockerfile; on GitHub it is a no-op. Inside the image, `toolchain` fails closed,
# naming the file, unless MIDGARD_TOOLCHAIN_DIGEST is the committed digest. A stub
# `docker` on PATH stands in for the daemon and the registry, in a copy of the CI
# scripts.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

D1=1111111111111111111111111111111111111111111111111111111111111111
D2=2222222222222222222222222222222222222222222222222222222222222222
REG=registry.invalid/group/midgard-app

root="$work/root"
mkdir -p "$root/tools/ci" "$root/deploy/acceptance" "$work/bin"
cp "$here/../run.sh" "$here/../lib.sh" "$root/tools/ci/"
printf 'FROM scratch\n' > "$root/deploy/acceptance/Dockerfile"
KEY="$( (sha256sum "$root/deploy/acceptance/Dockerfile" 2>/dev/null || shasum -a 256 "$root/deploy/acceptance/Dockerfile") | cut -c1-16)"

# The daemon and registry: pull succeeds only when STUB_PULL_OK=1; an image's label is
# STUB_LABEL; a pushed image's repo digest is STUB_DIGEST.
cat > "$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$DOCKER_LOG"
case "$1" in
    login) cat >/dev/null ;;
    pull) [ "${STUB_PULL_OK:-0}" = 1 ] || exit 1 ;;
    image)
        case "$*" in
            *RepoDigests*) echo "$CI_REGISTRY_IMAGE/toolchain@$STUB_DIGEST" ;;
            *Labels*) echo "${STUB_LABEL:-}" ;;
        esac
        ;;
esac
exit 0
STUB
chmod +x "$work/bin/docker"
export DOCKER_LOG="$work/docker.log"

# digest_file LINE   tools/ci/toolchain.digest ending in LINE
digest_file() { printf '# the toolchain\n# dockerfile-sha16: %s\n%s\n' "$KEY" "$1" > "$root/tools/ci/toolchain.digest"; }

# job NAME VAR=VALUE...   run a job as a GitLab job would, with the stub docker
job() {
    local name="$1"
    shift
    rm -f "$DOCKER_LOG"
    env -u GITHUB_ACTIONS -u MIDGARD_IN_TOOLCHAIN -u MIDGARD_TOOLCHAIN_DIGEST -u STUB_PULL_OK \
        GITLAB_CI=true CI_REGISTRY_IMAGE="$REG" PATH="$work/bin:$PATH" "$@" \
        bash "$root/tools/ci/run.sh" "$name" 2>&1
}

# expect_fail LABEL TEXT -- JOB VAR=VALUE...
expect_fail() {
    local label="$1" text="$2" out
    shift 3
    if out="$(job "$@")"; then
        fail "$label: passed, expected a failure"
    elif grep -qF -- "$text" <<<"$out"; then
        pass "$label"
    else
        fail "$label: failed without naming '$text': $out"
    fi
}

# expect_pass LABEL -- JOB VAR=VALUE...
expect_pass() {
    local label="$1" out
    shift 2
    if out="$(job "$@")"; then pass "$label"; else fail "$label: refused: $out"; fi
}

pushed() { grep -q "^push $REG/toolchain:$KEY$" "$DOCKER_LOG" 2>/dev/null; }

# ---- toolchain-publish -------------------------------------------------------------

digest_file "sha256:$D1"
expect_fail "a pushed digest that differs from the committed one fails, naming the file" \
    "commit the new digest to tools/ci/toolchain.digest" -- toolchain-publish STUB_DIGEST="sha256:$D2"
if pushed && grep -q "sha256:$D2" <<<"$(job toolchain-publish STUB_DIGEST="sha256:$D2")"; then
    pass "the failing job pushed the image and printed its digest"
else
    fail "the failing job did not push and print the digest"
fi

expect_pass "a pushed digest equal to the committed one passes" -- toolchain-publish STUB_DIGEST="sha256:$D1"
if pushed && grep -q "^build --label dev.midgard.toolchain.dockerfile=$KEY " "$DOCKER_LOG"; then
    pass "the image is built from the Dockerfile, labelled with it, and pushed under its sha16"
else
    fail "build and push: $(cat "$DOCKER_LOG")"
fi

expect_pass "the committed image already published from this Dockerfile passes" -- \
    toolchain-publish STUB_PULL_OK=1 STUB_LABEL="$KEY" STUB_DIGEST="sha256:$D2"
if ! pushed && ! grep -q '^build ' "$DOCKER_LOG"; then
    pass "and nothing is built or pushed"
else
    fail "an already published image was rebuilt or pushed: $(cat "$DOCKER_LOG")"
fi

expect_fail "a committed image built from another Dockerfile is rebuilt, and the new digest fails" \
    "tools/ci/toolchain.digest" -- toolchain-publish STUB_PULL_OK=1 STUB_LABEL=0000000000000000 STUB_DIGEST="sha256:$D2"

digest_file unpublished
expect_fail "with no digest committed yet the job fails, naming the file" \
    "commit the new digest to tools/ci/toolchain.digest" -- toolchain-publish STUB_DIGEST="sha256:$D1"

expect_fail "a digest the registry does not report fails" \
    "could not read the digest" -- toolchain-publish STUB_DIGEST=latest

rm -f "$DOCKER_LOG"
if out="$(env -u GITLAB_CI GITHUB_ACTIONS=true PATH="$work/bin:$PATH" bash "$root/tools/ci/run.sh" toolchain-publish 2>&1)" &&
    grep -q 'not applicable: GitHub builds the toolchain per run' <<<"$out" && [ ! -e "$DOCKER_LOG" ]; then
    pass "on GitHub the job says it does not apply, and touches nothing"
else
    fail "on GitHub: $out"
fi

# ---- toolchain, inside the published image -----------------------------------------

digest_file "sha256:$D1"
expect_pass "inside the image, the committed digest passes" -- \
    toolchain MIDGARD_IN_TOOLCHAIN=1 MIDGARD_TOOLCHAIN_DIGEST="sha256:$D1"
if [ ! -e "$DOCKER_LOG" ]; then
    pass "and docker is not invoked"
else
    fail "the in-image check called docker: $(cat "$DOCKER_LOG")"
fi

expect_fail "inside the image, a MIDGARD_TOOLCHAIN_DIGEST that differs fails closed, naming the file" \
    "tools/ci/toolchain.digest pins sha256:$D1" -- toolchain MIDGARD_IN_TOOLCHAIN=1 MIDGARD_TOOLCHAIN_DIGEST="sha256:$D2"

expect_fail "inside the image, an unset MIDGARD_TOOLCHAIN_DIGEST fails closed" \
    "<unset>" -- toolchain MIDGARD_IN_TOOLCHAIN=1

digest_file unpublished
expect_fail "inside the image, a digest file with no digest fails closed" \
    "pins no published digest yet" -- toolchain MIDGARD_IN_TOOLCHAIN=1 MIDGARD_TOOLCHAIN_DIGEST=unpublished

if [ "$failures" -eq 0 ]; then
    echo "toolchain-publish tests: all passed"
else
    echo "toolchain-publish tests: $failures failed"
    exit 1
fi
