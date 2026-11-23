#!/usr/bin/env bash
# Tests for the pinning rules of tools/ci/pipelines.sh --check, against throwaway trees.
# [VLD-ANB-MIDGARD-0022] [AC-P3.1] [AC-P3.5] [NFR-OSS-1]
#
#   bash tools/ci/tests/pipelines.test.sh
#
# The claim under test: a GitLab image or service not pinned by digest, in any of the
# forms GitLab accepts, and a GitHub action not pinned to a commit, each fail the check
# and are printed as file:line; pinned references, a mirror in the project registry,
# the published toolchain image by its digest variable and a workflow in the repository
# pass; and the repository's own pipeline files pass.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../pipelines.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

D=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
SHA=fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09

# tree NAME GITLAB_IMAGE_BLOCK [GITHUB_USES]   a root whose pipelines both call job `a`
tree() {
    local root="$work/$1"
    mkdir -p "$root/.github/workflows" "$root/tools/ci"
    printf '#!/usr/bin/env bash\n[ "$1" = names ] && echo a\n' > "$root/tools/ci/run.sh"
    cat > "$root/.github/workflows/ci.yml" <<YAML
jobs:
  a:
    steps:
      - uses: ${3:-actions/checkout@$SHA} # v5.1.0
      - run: bash tools/ci/run.sh a
YAML
    {
        echo "job:"
        printf '%s\n' "$2"
        echo "  script:"
        echo "    - bash tools/ci/run.sh a"
    } > "$root/.gitlab-ci.yml"
    echo "$root"
}

run() { MIDGARD_PIPELINES_ROOT="$1" bash "$script" --check 2>&1; }

# expect_pass LABEL ROOT
expect_pass() {
    local out
    if out="$(run "$2")"; then pass "$1"; else fail "$1: refused: $out"; fi
}

# expect_fail LABEL ROOT TEXT   the check fails and prints TEXT
expect_fail() {
    local out
    if out="$(run "$2")"; then
        fail "$1: passed, expected a failure"
    elif grep -qF -- "$3" <<<"$out"; then
        pass "$1"
    else
        fail "$1: failed without printing '$3': $out"
    fi
}

expect_pass "an image pinned as name:tag@sha256:<64 hex> passes" \
    "$(tree pinned "  image: alpine:3.20@sha256:$D # alpine:3.20 resolved 2026-10-31")"

expect_fail "an image pinned only by tag fails, printed as file:line" \
    "$(tree tag '  image: alpine:3.20')" ".gitlab-ci.yml:2:   image: alpine:3.20"

expect_fail "a digest that is not 64 hex fails" \
    "$(tree short "  image: alpine:3.20@sha256:${D:0:40}")" ".gitlab-ci.yml:2:"

expect_fail "a digest in a trailing comment does not pin the image" \
    "$(tree comment "  image: alpine:3.20 # @sha256:$D")" ".gitlab-ci.yml:2:"

expect_fail "an unpinned service (- name:) fails" \
    "$(tree service "  image: docker:27-cli@sha256:$D
  services:
    - name: docker:27-dind")" ".gitlab-ci.yml:4:     - name: docker:27-dind"

expect_fail "an unpinned service in the short form fails" \
    "$(tree service-short "  services:
    - docker:27-dind")" ".gitlab-ci.yml:3:     - docker:27-dind"

expect_pass "pinned services in both forms pass" \
    "$(tree service-pinned "  services:
    - docker:27-dind@sha256:$D
    - name: docker:27-dind@sha256:$D
      alias: docker")"

expect_fail "an image given as a block with an unpinned name fails" \
    "$(tree block "  image:
    name: node:22-bookworm
    entrypoint: ['']")" ".gitlab-ci.yml:3:     name: node:22-bookworm"

expect_pass "an image given as a block with a pinned name passes" \
    "$(tree block-pinned "  image:
    name: node:22-bookworm@sha256:$D")"

expect_pass "a mirror in the project registry, pinned by digest, passes" \
    "$(tree mirror "  image: \$CI_REGISTRY_IMAGE/mirror/alpine:3.20@sha256:$D")"

expect_pass "the published toolchain image by its checked digest variable passes" \
    "$(tree toolchain '  image: $CI_REGISTRY_IMAGE/toolchain@$MIDGARD_TOOLCHAIN_DIGEST')"

expect_fail "any other image by a digest variable fails" \
    "$(tree other-var '  image: $CI_REGISTRY_IMAGE/other@$SOME_DIGEST')" ".gitlab-ci.yml:2:"

expect_fail "a GitHub action pinned by tag fails, printed as file:line" \
    "$(tree action-tag "  image: alpine:3.20@sha256:$D" "actions/checkout@v5")" \
    ".github/workflows/ci.yml:4:       - uses: actions/checkout@v5"

expect_fail "a GitHub action pinned to a short commit fails" \
    "$(tree action-short "  image: alpine:3.20@sha256:$D" "actions/checkout@${SHA:0:12}")" \
    ".github/workflows/ci.yml:4:"

expect_pass "a container action pinned by digest passes" \
    "$(tree docker-pinned "  image: alpine:3.20@sha256:$D" "docker://alpine:3.20@sha256:$D")"

expect_fail "a container action pinned by tag fails" \
    "$(tree docker-tag "  image: alpine:3.20@sha256:$D" "docker://alpine:3.20")" \
    ".github/workflows/ci.yml:4:"

expect_pass "a workflow in this repository needs no commit" \
    "$(tree local "  image: alpine:3.20@sha256:$D" "./.github/workflows/shell-audit.yml")"

if out="$(bash "$script" --check 2>&1)"; then
    pass "the repository's own pipeline files pass"
else
    fail "the repository's own pipeline files are refused: $out"
fi

# The merge-request jobs' image digest is declared in .gitlab-ci.yml, where a merge
# request's pipeline can read it, and must be the one tools/ci/toolchain.digest pins.
repo="$here/../../.."
declared="$(tr -d '\r' < "$repo/.gitlab-ci.yml" |
    sed -n 's/^  MIDGARD_TOOLCHAIN_DIGEST: *"\{0,1\}\([^"#[:space:]]*\)"\{0,1\}.*/\1/p' | head -n 1)"
pinned="$(tr -d '\r' < "$repo/tools/ci/toolchain.digest" | awk 'NF { last = $1 } END { print last }')"
if [ -z "$declared" ]; then
    fail ".gitlab-ci.yml declares MIDGARD_TOOLCHAIN_DIGEST in its top-level variables"
elif [ "$declared" = "$pinned" ]; then
    pass ".gitlab-ci.yml declares MIDGARD_TOOLCHAIN_DIGEST as the last line of tools/ci/toolchain.digest"
else
    fail ".gitlab-ci.yml declares MIDGARD_TOOLCHAIN_DIGEST: $declared, but tools/ci/toolchain.digest ends $pinned; set MIDGARD_TOOLCHAIN_DIGEST in .gitlab-ci.yml to it"
fi

# gitlab_job NAME: the lines of the top-level job NAME in .gitlab-ci.yml.
gitlab_job() {
    tr -d '\r' < "$repo/.gitlab-ci.yml" |
        awk -v job="$1" '$0 == job ":" { on = 1; print; next } on && /^[^ #]/ { on = 0 } on'
}
# The toolchain image is Debian, with no apk: a job running in it installs nothing, and
# what it needs is baked into deploy/acceptance/Dockerfile. [NFR-PRV-1]
apk_in_toolchain=""
for job in $(tr -d '\r' < "$repo/.gitlab-ci.yml" | sed -n 's/^\([a-z.][a-z0-9_.-]*\):$/\1/p'); do
    block="$(gitlab_job "$job")"
    { [ "$job" = .toolchain ] || grep -Eq '^  extends:.*[[ ]\.toolchain([],]|$)' <<<"$block"; } &&
        grep -q 'apk ' <<<"$block" && apk_in_toolchain+=" $job"
done
[ -z "$apk_in_toolchain" ] && pass "no GitLab job in the toolchain image runs apk" ||
    fail "GitLab jobs in the (Debian) toolchain image run apk:$apk_in_toolchain"
# The advisory refresh's write token is scoped to an environment only that job enters.
# [AC-P4.5]
gitlab_job advisory-refresh | grep -A2 '^  environment:$' | tr -d ' ' | tr '\n' ' ' |
    grep -q 'environment: name:advisory-refresh action:access' &&
    pass "GitLab: advisory-refresh runs in the advisory-refresh environment, as access only" ||
    fail "GitLab: advisory-refresh runs in the advisory-refresh environment, as access only"

if [ "$failures" -eq 0 ]; then
    echo "pipelines tests: all passed"
else
    echo "pipelines tests: $failures failed"
    exit 1
fi
