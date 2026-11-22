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
# outside ARG...   run it as a developer or the kms runner does, with no hand-back (the
#                  hand-back has its own cases below, and wraps the command on Linux)
outside() { env -u MIDGARD_IN_TOOLCHAIN MIDGARD_TOOLCHAIN_OWNER= PATH="$work/bin:$PATH" bash "$script" "$@" 2>&1; }

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

# The hand-back. A second stub runs what docker would have run in the container, here,
# with a stub chown that records its arguments and fails when CHOWN_FAILS is set.
mkdir -p "$work/runbin" "$work/tree/target"
cat > "$work/runbin/docker" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$DOCKER_LOG"
[ "$1" = run ] || exit 0
while [ "$#" -gt 0 ] && [ "$1" != "$IMAGE_UNDER_TEST" ]; do
    case "$1" in -e) export "$2"; shift 2 ;; *) shift ;; esac
done
shift
cd "$TREE" && exec "$@"
STUB
cat > "$work/runbin/chown" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$CHOWN_LOG"
[ -z "${CHOWN_FAILS:-}" ]
STUB
chmod +x "$work/runbin/docker" "$work/runbin/chown"
# handed OWNER ARG...   run the script outside the image with the running stub
handed() {
    local owner="$1"
    shift
    env -u MIDGARD_IN_TOOLCHAIN MIDGARD_TOOLCHAIN_OWNER="$owner" TOOLCHAIN_IMAGE=img:test \
        IMAGE_UNDER_TEST=img:test TREE="$work/tree" CHOWN_LOG="$work/chown.log" \
        PATH="$work/runbin:$PATH" bash "$script" "$@" 2>&1
}

rm -f "$DOCKER_LOG" "$work/chown.log"
handed 1001:1001 bash -c 'exit 0' >/dev/null
rc=$?
if [ "$rc" = 0 ] && grep -q -- '-e MIDGARD_TOOLCHAIN_OWNER=1001:1001 ' "$DOCKER_LOG" &&
    [ "$(cat "$work/chown.log" 2>/dev/null)" = "-R 1001:1001 target" ]; then
    pass "target/ is handed back to the calling user when the command ends"
else
    fail "hand-back: rc $rc, docker: $(cat "$DOCKER_LOG" 2>/dev/null), chown: $(cat "$work/chown.log" 2>/dev/null)"
fi

rm -f "$DOCKER_LOG" "$work/chown.log"
handed 1001:1001 bash -c 'exit 7' >/dev/null
rc=$?
if [ "$rc" = 7 ] && [ "$(cat "$work/chown.log" 2>/dev/null)" = "-R 1001:1001 target" ]; then
    pass "a failing command still hands target/ back, and keeps its exit status"
else
    fail "hand-back after a failure: rc $rc, chown: $(cat "$work/chown.log" 2>/dev/null)"
fi

rm -f "$DOCKER_LOG" "$work/chown.log"
out="$(CHOWN_FAILS=1 handed 1001:1001 true)"
rc=$?
if [ "$rc" = 1 ] && grep -q 'could not hand target/ back to 1001:1001' <<<"$out"; then
    pass "a hand-back that fails fails the run"
else
    fail "a failed hand-back: rc $rc, output: $out"
fi

rm -f "$DOCKER_LOG" "$work/chown.log"
handed "" true >/dev/null
rc=$?
if [ "$rc" = 0 ] && [ ! -e "$work/chown.log" ] && ! grep -q 'MIDGARD_TOOLCHAIN_OWNER\| sh -c ' "$DOCKER_LOG"; then
    pass "with no owner the command is run as given"
else
    fail "no owner: rc $rc, docker: $(cat "$DOCKER_LOG" 2>/dev/null)"
fi

if [ "$failures" -eq 0 ]; then
    echo "in-toolchain tests: all passed"
else
    echo "in-toolchain tests: $failures failed"
    exit 1
fi
