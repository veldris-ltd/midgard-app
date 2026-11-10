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

# --- key material reaches the signing jobs [VLD-ANB-MIDGARD-0007] ---------------------
# Each item names the runner secret that supplies it, as docs/ops/PKI.md does; and
# release.yml maps exactly that secret into every job on the signing runner, so a
# variable the preflight asks for cannot be one the workflow never passes.
out="$(preflight GITHUB_WORKSPACE="$work/ws")"
for pair in MIDGARD_SIGNING_KEYSTORE=MIDGARD_SIGNING_KEYSTORE_PATH \
    MIDGARD_BIFROST_KEYSTORE=MIDGARD_BIFROST_KEYSTORE_PATH \
    MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE=MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE \
    MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE=MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE; do
    var="${pair%%=*}" secret="${pair#*=}"
    grep -qF "$var is not configured (runner secret $secret)" <<<"$out" &&
        pass "an unset $var names the runner secret $secret" ||
        fail "an unset $var names the runner secret $secret: $out"
done

workflow="$here/../../../.github/workflows/release.yml"
# The env of each job that runs on [self-hosted, kms], as "job VAR=SECRET" lines.
kms_env="$(tr -d '\r' <"$workflow" | awk '
    /^  [a-z][a-z0-9-]*:$/ { job = $1; sub(/:$/, "", job); kms[job] = 0 }
    /^    runs-on: \[self-hosted, kms\]/ { kms[job] = 1 }
    /^      MIDGARD_[A-Z_]+: \$\{\{ secrets\.[A-Z_]+ \}\}/ {
        v = $1; sub(/:$/, "", v); s = $0; sub(/.*secrets\./, "", s); sub(/ .*/, "", s)
        env[job] = env[job] " " v "=" s }
    END { for (j in kms) if (kms[j]) print j env[j] }')"
[ "$(wc -l <<<"$kms_env")" -ge 2 ] && pass "release.yml has its signing-runner jobs" ||
    fail "release.yml has its signing-runner jobs: $kms_env"
while read -r job mapped; do
    for pair in MIDGARD_SIGNING_KEYSTORE=MIDGARD_SIGNING_KEYSTORE_PATH \
        MIDGARD_BIFROST_KEYSTORE=MIDGARD_BIFROST_KEYSTORE_PATH \
        MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE=MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE \
        MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE=MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE \
        MIDGARD_BIFROST_REGISTRY=MIDGARD_BIFROST_REGISTRY_PATH; do
        grep -qw -- "$pair" <<<"$mapped" &&
            pass "release.yml passes ${pair%%=*} to the $job job, from secret ${pair#*=}" ||
            fail "release.yml passes ${pair%%=*} to the $job job, from secret ${pair#*=}: $mapped"
    done
done <<<"$kms_env"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all release-preflight tests passed"
