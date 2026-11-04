#!/usr/bin/env bash
# Tests for the job-completed hook check and the runner isolation checks in
# tools/ci/release-preflight.sh. [RC-07] [VLD-ANB-MIDGARD-0081] [AC-68.5]
# [VLD-ANB-MIDGARD-0035] [AC-P2.4]
#
#   bash tools/ci/tests/release-preflight.test.sh
#
# The claim under test: on a self-hosted runner with no job-completed cleanup hook the
# preflight fails closed, naming the hook and pointing to docs/ops/PKI.md; with the
# hook, that item passes; a hosted runner is not asked for one. The preflight's other
# items have no inputs here (a stub docker that has no images, no keystores), so the
# run fails on them either way; only the hook item's lines are checked.
#
# And: the runner's copy of allowed_signers, and each keystore, is refused when it lies
# inside the job's workspace and accepted outside it, and an unset copy is refused, each
# failure pointing to docs/ops/PKI.md.
#
# And: each keystore passphrase file, a runner secret of its own, is refused when it is
# not configured, naming the secret PKI.md names, or names no readable file; and passes
# when it names one outside the workspace. [VLD-ANB-MIDGARD-0007]
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../release-preflight.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

mkdir -p "$work/stubs"
printf '#!/usr/bin/env bash\nexit 1\n' > "$work/stubs/docker"
chmod +x "$work/stubs/docker"
printf '#!/usr/bin/env bash\nrm -rf "$GITHUB_WORKSPACE"\n' > "$work/cleanup.sh"

preflight() { # preflight VAR=VALUE...: run with only these runner settings
    env -u RUNNER_ENVIRONMENT -u ACTIONS_RUNNER_HOOK_JOB_COMPLETED -u MIDGARD_SIGNING_KEYSTORE \
        -u MIDGARD_BIFROST_KEYSTORE -u MIDGARD_BIFROST_REGISTRY -u MIDGARD_ALLOWED_SIGNERS_PATH \
        -u MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE -u MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE \
        -u GITHUB_WORKSPACE -u CI_PROJECT_DIR PATH="$work/stubs:$PATH" \
        "$@" bash "$script" 2>&1
}

if out="$(preflight RUNNER_ENVIRONMENT=self-hosted)"; then
    fail "a self-hosted runner with no hook fails closed"
else
    grep 'ACTIONS_RUNNER_HOOK_JOB_COMPLETED is not set' <<<"$out" | grep -q 'docs/ops/PKI.md, "Signing runner"' &&
        pass "a self-hosted runner with no hook fails closed, pointing to PKI.md" ||
        fail "the missing hook is named, with the pointer: $out"
fi

out="$(preflight RUNNER_ENVIRONMENT=self-hosted ACTIONS_RUNNER_HOOK_JOB_COMPLETED="$work/absent.sh")"
grep -q "names $work/absent.sh, which is not a readable file" <<<"$out" &&
    pass "a hook that names no file is refused" || fail "a hook that names no file is refused: $out"

out="$(preflight RUNNER_ENVIRONMENT=self-hosted ACTIONS_RUNNER_HOOK_JOB_COMPLETED="$work/cleanup.sh")"
grep -q "ok — job-completed hook $work/cleanup.sh" <<<"$out" && ! grep -q 'HOOK_JOB_COMPLETED' <<<"$out" &&
    pass "with the hook installed, the item passes" || fail "with the hook installed, the item passes: $out"

out="$(preflight RUNNER_ENVIRONMENT=github-hosted)"
grep -q 'job-completed\|HOOK_JOB_COMPLETED' <<<"$out" && fail "a hosted runner is not asked for a hook: $out" ||
    pass "a hosted runner is not asked for a hook"

# --- runner isolation [VLD-ANB-MIDGARD-0035] -----------------------------------------
mkdir -p "$work/ws" "$work/runner"
echo 'release@example.invalid namespaces="git" ssh-ed25519 AAAA' > "$work/ws/allowed_signers"
cp "$work/ws/allowed_signers" "$work/runner/allowed_signers"
echo keystore > "$work/ws/release.keystore"
echo keystore > "$work/runner/release.keystore"
pki='docs/ops/PKI.md, "Signing runner"'

out="$(preflight GITHUB_WORKSPACE="$work/ws")"
grep 'MIDGARD_ALLOWED_SIGNERS_PATH is not configured' <<<"$out" | grep -q "$pki" &&
    pass "a runner with no pinned allowed_signers fails closed, pointing to PKI.md" ||
    fail "a runner with no pinned allowed_signers fails closed, pointing to PKI.md: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_ALLOWED_SIGNERS_PATH="$work/ws/allowed_signers")"
grep "MIDGARD_ALLOWED_SIGNERS_PATH names a file inside the job's workspace" <<<"$out" | grep -q "$pki" &&
    pass "pinned allowed_signers inside the workspace is refused, pointing to PKI.md" ||
    fail "pinned allowed_signers inside the workspace is refused, pointing to PKI.md: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_ALLOWED_SIGNERS_PATH="$work/runner/allowed_signers")"
grep -q 'ok — MIDGARD_ALLOWED_SIGNERS_PATH is configured, outside the workspace' <<<"$out" &&
    pass "pinned allowed_signers outside the workspace passes" ||
    fail "pinned allowed_signers outside the workspace passes: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_SIGNING_KEYSTORE="$work/ws/release.keystore")"
grep "MIDGARD_SIGNING_KEYSTORE names a file inside the job's workspace" <<<"$out" | grep -q "$pki" &&
    pass "a keystore inside the workspace is refused, pointing to PKI.md" ||
    fail "a keystore inside the workspace is refused, pointing to PKI.md: $out"

# The job's workspace, not the checkout the script runs from; GitLab names it too.
out="$(preflight CI_PROJECT_DIR="$work/ws" MIDGARD_SIGNING_KEYSTORE="$work/ws/release.keystore")"
grep -q "MIDGARD_SIGNING_KEYSTORE names a file inside the job's workspace" <<<"$out" &&
    pass "the GitLab project directory is the workspace" || fail "the GitLab project directory is the workspace: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_SIGNING_KEYSTORE="$work/runner/release.keystore")"
grep -q 'ok — MIDGARD_SIGNING_KEYSTORE is configured' <<<"$out" &&
    pass "a keystore outside the workspace passes" || fail "a keystore outside the workspace passes: $out"

# --- keystore passphrase files [VLD-ANB-MIDGARD-0007] --------------------------------
# A stand-in, not a passphrase: the preflight only checks the file is there.
echo 'test-only, not a passphrase' > "$work/runner/release.passphrase"
echo 'test-only, not a passphrase' > "$work/ws/release.passphrase"
out="$(preflight GITHUB_WORKSPACE="$work/ws")"
for var in MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE; do
    grep "$var is not configured (runner secret $var)" <<<"$out" | grep -q "$pki" &&
        pass "an unset $var is refused, naming its runner secret and PKI.md" ||
        fail "an unset $var is refused, naming its runner secret and PKI.md: $out"
done
grep -q 'MIDGARD_SIGNING_KEYSTORE is not configured (runner secret MIDGARD_SIGNING_KEYSTORE_PATH)' <<<"$out" &&
    pass "an unset keystore still names its _PATH runner secret" ||
    fail "an unset keystore still names its _PATH runner secret: $out"

for var in MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE; do
    out="$(preflight GITHUB_WORKSPACE="$work/ws" "$var=$work/runner/absent.passphrase")"
    grep "$var names $work/runner/absent.passphrase, which is not a readable file" <<<"$out" | grep -q "$pki" &&
        pass "a $var naming no file is refused" || fail "a $var naming no file is refused: $out"
done

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE="$work/ws/release.passphrase")"
grep -q "MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE names a file inside the job's workspace" <<<"$out" &&
    pass "a passphrase file inside the workspace is refused" ||
    fail "a passphrase file inside the workspace is refused: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws" MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE="$work/runner/release.passphrase")"
grep -q 'ok — MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE is configured' <<<"$out" &&
    pass "a passphrase file outside the workspace passes" ||
    fail "a passphrase file outside the workspace passes: $out"
! grep -q 'test-only, not a passphrase' <<<"$out" &&
    pass "the preflight never prints a passphrase file's contents" ||
    fail "the preflight never prints a passphrase file's contents: $out"

out="$(preflight GITHUB_WORKSPACE="$work/ws")"
grep -q 'the docker daemon is not reachable' <<<"$out" &&
    pass "an unreachable docker daemon is named" || fail "an unreachable docker daemon is named: $out"
grep -q 'does not start the toolchain container with --network none' <<<"$out" &&
    pass "a workspace whose in-toolchain.sh does not use --network none is refused" ||
    fail "a workspace whose in-toolchain.sh does not use --network none is refused: $out"
out="$(preflight GITHUB_WORKSPACE="$here/../../..")"
grep -q 'ok — builds run in the toolchain container with --network none' <<<"$out" &&
    pass "this repository's in-toolchain.sh uses --network none" ||
    fail "this repository's in-toolchain.sh uses --network none: $out"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all release-preflight tests passed"
