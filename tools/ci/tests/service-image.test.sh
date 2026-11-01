#!/usr/bin/env bash
# Tests for deploy/images/service.Dockerfile, the release service image, read as text
# (no docker needed). [AC-B-1] [NFR-PQ-3] [MG-SEC-06] [MG-SEC-14] [VLD-ANB-MIDGARD-0068]
#
#   bash tools/ci/tests/service-image.test.sh
#
# The claims under test: the image has two stages, and the one it runs on is a
# distroless cc-debian12 image pinned by digest, with no RUN step after it (it has no
# shell to run one); its entrypoint is mg-verify --run-service in exec form, given
# the pinned trust key at /trust/release-trust.pub and nothing else; nothing in the
# image reads a trust-key or bind variable; and the shell entrypoint is gone.
#
# What mg-verify --run-service does with that command line (refuses an unverified
# binary, refuses a container given no options with exit 2, ignores any other anchor
# named in the environment or the container's arguments) is proven against the real
# binary by the run_service_* tests in tools/mg-verify/tests/ac_rf_sc_2.rs.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../.."
dockerfile="$root/deploy/images/service.Dockerfile"
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

df="$(tr -d '\r' < "$dockerfile")"

# 1. Two stages; the second runs on a digest-pinned distroless cc image.
froms="$(grep -c '^FROM' <<<"$df")"
[ "$froms" -eq 2 ] && pass "two stages" || fail "expected two FROM lines, found $froms"
runtime="$(sed -n 's/^ARG RUNTIME=//p' <<<"$df")"
if grep -qE '^gcr\.io/distroless/cc-debian12:nonroot@sha256:[0-9a-f]{64}$' <<<"$runtime"; then
    pass "the runtime is distroless cc-debian12, pinned by digest"
else
    fail "ARG RUNTIME must name gcr.io/distroless/cc-debian12:nonroot@sha256:<64 hex>, not: ${runtime:-<none>}"
fi
last_from="$(grep '^FROM' <<<"$df" | tail -1)"
[ "$last_from" = 'FROM ${RUNTIME}' ] && pass "the final stage is FROM \${RUNTIME}" \
    || fail "the final stage must be FROM \${RUNTIME}, not: $last_from"
base="$(sed -n 's/^ARG BASE=//p' <<<"$df")"
grep -qE '@sha256:[0-9a-f]{64}$' <<<"$base" && pass "the prep stage base is pinned by digest" \
    || fail "ARG BASE must be pinned by digest: ${base:-<none>}"

# 2. No RUN after the runtime stage begins: distroless has no shell.
if awk '/^FROM \$\{RUNTIME\}/{f=1} f && /^RUN/{bad=1} END{exit bad}' <<<"$df"; then
    pass "no RUN step in the runtime stage"
else
    fail "a RUN step follows FROM \${RUNTIME}"
fi

# 3. The entrypoint is mg-verify --run-service, exec form, with the pinned key only.
entry="$(grep '^ENTRYPOINT' <<<"$df")"
want='ENTRYPOINT ["/usr/local/bin/mg-verify", "--trust-anchor", "/trust/release-trust.pub", "--run-service"]'
[ "$entry" = "$want" ] && pass "the entrypoint verifies against the pinned key, then runs the service" \
    || fail "the entrypoint must be: $want (got: ${entry:-<none>})"
[ "$(grep -c '^ENTRYPOINT\|^CMD' <<<"$df")" -eq 1 ] && pass "no CMD supplies default options" \
    || fail "only one ENTRYPOINT and no CMD: a container must be given its options"

# 4. No trust-key or bind variable in the image or in the entrypoint's code.
if grep -q "MIDGARD_RELEASE_TRUST_KEY\|MIDGARD_BIND" "$dockerfile" "$root/tools/mg-verify/src/main.rs"; then
    fail "the image or mg-verify still mentions MIDGARD_RELEASE_TRUST_KEY or MIDGARD_BIND"
else
    pass "the image reads no trust-key or bind variable"
fi

# 5. The shell entrypoint is gone, and nothing under deploy/images refers to it.
[ ! -e "$root/deploy/images/entrypoint.sh" ] && pass "deploy/images/entrypoint.sh is gone" \
    || fail "deploy/images/entrypoint.sh still exists"
if grep -rq "entrypoint\.sh" "$root/deploy/images"; then
    fail "deploy/images still refers to entrypoint.sh: $(grep -rl 'entrypoint\.sh' "$root/deploy/images" | tr '\n' ' ')"
else
    pass "nothing under deploy/images refers to entrypoint.sh"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all service-image tests passed"
