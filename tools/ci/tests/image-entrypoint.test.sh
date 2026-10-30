#!/usr/bin/env bash
# Tests for deploy/images/entrypoint.sh, the release image's verify-before-run gate,
# with a stand-in mg-verify that records how it was called. [MG-SEC-06] [MG-SEC-14]
#
#   bash tools/ci/tests/image-entrypoint.test.sh
#
# The claims under test: the binary is always verified against the pinned trust key at
# /trust/release-trust.pub, and no environment variable can point it at another key; a
# failed verification refuses to start; and a container given no options refuses to
# start rather than starting the service on a default address.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
entrypoint="$here/../../../deploy/images/entrypoint.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# A stand-in mg-verify: records its arguments, then refuses, so nothing is exec'd.
mkdir -p "$work/bin"
cat > "$work/bin/mg-verify" <<STUB
#!/bin/sh
echo "\$*" > "$work/called"
exit 1
STUB
chmod +x "$work/bin/mg-verify"
echo "ML-DSA-87 attacker-key 00" > "$work/attacker.pub"

run() { # run [ENV=VALUE...] -- [ARGS...]
    local envs=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do envs+=("$1"); shift; done
    shift
    rm -f "$work/called"
    env -i PATH="$work/bin:/usr/bin:/bin" MIDGARD_SERVICE=http-gateway "${envs[@]}" \
        sh "$entrypoint" "$@" > "$work/out" 2>&1
}

# 1. The trust key cannot be swapped through the environment.
run MIDGARD_RELEASE_TRUST_KEY="$work/attacker.pub" TRUST="$work/attacker.pub" -- \
    --state-dir /state
status=$?
called="$(cat "$work/called" 2>/dev/null || true)"
if [ "$called" = "--trust-anchor /trust/release-trust.pub /app/http-gateway" ]; then
    pass "mg-verify is given the pinned trust key, whatever the environment says"
else
    fail "mg-verify was called as: ${called:-<not called>}"
fi
if grep -q "attacker" "$work/out" "$work/called" 2>/dev/null; then
    fail "the attacker's key path reached the entrypoint's output or mg-verify"
else
    pass "the attacker's key path is never used"
fi
[ "$status" -eq 1 ] && grep -q "refusing to start" "$work/out" \
    && pass "a failed verification refuses to start" \
    || fail "a failed verification must exit 1 and refuse (got $status)"

# 2. No options: refused before anything runs.
run --
status=$?
[ "$status" -eq 2 ] && pass "no options exits 2" || fail "no options must exit 2 (got $status)"
[ ! -e "$work/called" ] && pass "no options starts nothing" || fail "no options still ran mg-verify"
grep -q "refusing to start" "$work/out" && pass "no options says it refuses" \
    || fail "no options must say it refuses: $(cat "$work/out")"

# 3. The variable is gone from the image and the entrypoint.
if grep -q "MIDGARD_RELEASE_TRUST_KEY\|MIDGARD_BIND" "$entrypoint" "$here/../../../deploy/images/service.Dockerfile"; then
    fail "the image still mentions MIDGARD_RELEASE_TRUST_KEY or MIDGARD_BIND"
else
    pass "the image reads no trust-key or bind variable"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all image-entrypoint tests passed"
