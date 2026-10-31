#!/usr/bin/env bash
# Tests for tools/ci/in-toolchain.sh: in place inside the toolchain image, docker
# outside it. [VLD-ANB-MIDGARD-0043] [AC-P3.3] [NFR-PRV-1]
#
#   bash tools/ci/tests/in-toolchain.test.sh
#
# The claim under test: with MIDGARD_IN_TOOLCHAIN=1 the command runs in the checkout
# with the variables the container would have had, its exit status is the command's,
# a --mount is refused, and docker is never invoked; without it, the command goes to
# `docker run --rm --network none` as before. A stub `docker` on PATH records every
# call.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../in-toolchain.sh"
repo="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

mkdir -p "$work/bin"
cat > "$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$DOCKER_LOG"
exit 0
STUB
chmod +x "$work/bin/docker"
export DOCKER_LOG="$work/docker.log"

# inside ARG...    run the script as GitLab's merge-request jobs do
inside() { env MIDGARD_IN_TOOLCHAIN=1 PATH="$work/bin:$PATH" bash "$script" "$@" 2>&1; }
# outside ARG...   run it as a developer or the kms runner does
outside() { env -u MIDGARD_IN_TOOLCHAIN PATH="$work/bin:$PATH" bash "$script" "$@" 2>&1; }

rm -f "$DOCKER_LOG"
out="$(MIDGARD_SOURCE_REVISION=abc123 inside --env FOO=bar -- bash -c 'echo "$FOO|$CARGO_TERM_COLOR|$CARGO_NET_OFFLINE|$MIDGARD_SOURCE_REVISION|$(pwd -P)"')"
if [ "$out" = "bar|always|true|abc123|$(cd "$repo" && pwd -P)" ] && [ ! -e "$DOCKER_LOG" ]; then
    pass "inside the image the command runs in the checkout with its variables, cargo offline, and docker is not invoked"
else
    fail "in-place run: got '$out', docker log: $(cat "$DOCKER_LOG" 2>/dev/null)"
fi

rm -f "$DOCKER_LOG"
inside bash -c 'exit 7' >/dev/null
rc=$?
if [ "$rc" = 7 ] && [ ! -e "$DOCKER_LOG" ]; then
    pass "inside the image the exit status is the command's"
else
    fail "in-place exit status: got $rc"
fi

rm -f "$DOCKER_LOG"
if out="$(inside --mount /keys/a:/keys/a:ro -- true)"; then
    fail "a --mount inside the image was accepted"
elif grep -q 'nothing can be mounted' <<<"$out" && [ ! -e "$DOCKER_LOG" ]; then
    pass "a --mount inside the image is refused, not ignored"
else
    fail "a --mount inside the image: $out"
fi

rm -f "$DOCKER_LOG"
outside --env FOO=bar -- just test >/dev/null
if grep -q '^image inspect ' "$DOCKER_LOG" 2>/dev/null &&
    grep -q '^run --rm --network none .*-e FOO=bar .*just test$' "$DOCKER_LOG"; then
    pass "without MIDGARD_IN_TOOLCHAIN the command goes to docker run --network none"
else
    fail "outside the image, docker saw: $(cat "$DOCKER_LOG" 2>/dev/null)"
fi

rm -f "$DOCKER_LOG"
env MIDGARD_IN_TOOLCHAIN=0 PATH="$work/bin:$PATH" bash "$script" true >/dev/null 2>&1
if grep -q '^run --rm --network none ' "$DOCKER_LOG" 2>/dev/null; then
    pass "only MIDGARD_IN_TOOLCHAIN=1 means in place"
else
    fail "MIDGARD_IN_TOOLCHAIN=0 did not use docker"
fi

if [ "$failures" -eq 0 ]; then
    echo "in-toolchain tests: all passed"
else
    echo "in-toolchain tests: $failures failed"
    exit 1
fi
