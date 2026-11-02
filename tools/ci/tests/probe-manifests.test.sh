#!/usr/bin/env bash
# Tests for the deployment tier's PQ-mTLS probe: its pods run with a non-zero group,
# and the CA seed both sides share is drawn for each run, logged and never committed.
# [VLD-ANB-MIDGARD-0075] [AC-39.4]
#
#   bash tools/ci/tests/probe-manifests.test.sh
#
# The manifest is read as text and, with python3 and PyYAML on PATH, parsed. The
# runner's check_cross_host_mtls is lifted out of run-deployment-tier.sh and run with
# kubectl replaced by a stub that keeps the manifest it is asked to apply.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
deployment="$here/../../../deploy/acceptance/deployment"
manifest="$deployment/pqmtls-probe.yaml"
runner="$deployment/run-deployment-tier.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# expect_count WANT PATTERN DESCRIPTION: the manifest holds PATTERN on WANT lines.
expect_count() {
    local got
    got="$(grep -c -- "$2" "$manifest")"
    if [ "$got" = "$1" ]; then pass "$3"; else fail "$3: wanted $1 line(s), found $got"; fi
}

# 1. Both pod specs run as user and group 10001, never the image's group 0.
expect_count 2 'runAsGroup: 10001' "both pod specs set runAsGroup 10001"
expect_count 2 'fsGroup: 10001' "both pod specs set fsGroup 10001"
# Run from the empty scratch directory, so nothing beside the caller shadows a module.
if command -v python3 >/dev/null 2>&1 && (cd "$work" && python3 -c 'import yaml') >/dev/null 2>&1; then
    if out="$(cd "$work" && python3 - "$manifest" 2>&1 <<'PY'
import sys, yaml
pods = []
for doc in yaml.safe_load_all(open(sys.argv[1])):
    if doc["kind"] == "Pod":
        pods.append((doc["metadata"]["name"], doc["spec"]))
    elif doc["kind"] == "Job":
        pods.append((doc["metadata"]["name"], doc["spec"]["template"]["spec"]))
assert len(pods) == 2, pods
for name, spec in pods:
    ctx = spec["securityContext"]
    for key in ("runAsUser", "runAsGroup", "fsGroup"):
        assert ctx.get(key) == 10001, f"{name}: {key} is {ctx.get(key)}"
    assert ctx.get("runAsNonRoot") is True, name
PY
)"; then
        pass "the parsed manifest gives both pods user, group and fsGroup 10001"
    else
        fail "the parsed manifest: $out"
    fi
else
    echo "skip  python3 with PyYAML is absent; the grep checks above stand in"
fi

# 2. No numeric CA seed is committed; the placeholder stands on both sides.
if grep -Eq -- '--ca-seed", *"[0-9]+"|--ca-seed [0-9]+' "$manifest"; then
    fail "a numeric --ca-seed is committed: $(grep -E -- '--ca-seed", *"[0-9]+"|--ca-seed [0-9]+' "$manifest")"
else
    pass "no numeric --ca-seed is committed"
fi
expect_count 2 '--ca-seed", "__CA_SEED__"\|--ca-seed __CA_SEED__' "the responder and the initiator both take __CA_SEED__"
if grep -q 'refusing to apply' "$runner" && grep -q 'grep -q "__CA_SEED__"' "$runner"; then
    pass "the runner refuses a manifest that still holds __CA_SEED__"
else
    fail "the runner has no __CA_SEED__ refusal"
fi

# 3. The runner draws a seed, logs it and applies the manifest with that seed on both
#    sides; a placeholder that survives rendering is refused before anything is applied.
sed -n '/^check_cross_host_mtls() {/,/^}/p' "$runner" > "$work/check.sh"
# run_check: the lifted check, with the runner's helpers and a kubectl stub that copies
# the manifest it applies to $work/applied.yaml. The probe image is given pinned by
# digest, as the runner requires (WP-37), so no docker is needed to resolve it.
# [VLD-ANB-MIDGARD-0012]
probe_image="registry.test/probe@sha256:$(printf 'a%.0s' $(seq 64))"
run_check() (
    # The runner's own helpers (probe_manifest, resolve_probe_image, the sentinel).
    # shellcheck source=/dev/null
    MIDGARD_DEPLOYMENT_TIER_FUNCTIONS_ONLY=1 . "$runner"
    # shellcheck disable=SC2034 # read by the lifted function
    HERE="$deployment" NS=test
    MIDGARD_PROBE_IMAGE="${MIDGARD_PROBE_IMAGE:-$probe_image}"
    rm -f "$work/applied.yaml"
    note() { echo "    $*"; }
    record() { echo "  >>> $2: $1"; }
    missing() { record FAIL "$1" "$2"; }
    have() { return 0; }
    native() { echo "$1"; }
    kubectl() {
        case "$*" in
            *" apply -f "*) cp "${*: -1}" "$work/applied.yaml" ;;
            *"app=pqmtls-responder"*) echo node-a ;;
            *"job-name=pqmtls-probe -o"*) echo node-b ;;
        esac
        return 0
    }
    # shellcheck source=/dev/null
    . "$work/check.sh"
    check_cross_host_mtls
)
out="$(run_check 2>&1)"
seed="$(sed -n 's/^ *probe CA seed: \([0-9][0-9]*\)$/\1/p' <<<"$out")"
if [ -n "$seed" ]; then
    pass "the runner logs the probe CA seed ($seed)"
else
    fail "the runner does not log a numeric probe CA seed: $out"
fi
if [ -f "$work/applied.yaml" ] && [ -n "$seed" ] &&
    [ "$(grep -c -- "--ca-seed\", \"$seed\"\|--ca-seed $seed;" "$work/applied.yaml")" = 2 ] &&
    ! grep -q "__CA_SEED__" "$work/applied.yaml"; then
    pass "the applied manifest carries the logged seed on both sides"
else
    fail "the applied manifest does not carry the logged seed on both sides: $out"
fi
case "$out" in
    *">>> RC-40.2: PASS"*) pass "the check passes when the stubbed cluster completes the Job" ;;
    *) fail "the check did not pass against the stub: $out" ;;
esac
second="$(run_check 2>&1 | sed -n 's/^ *probe CA seed: \([0-9][0-9]*\)$/\1/p')"
if [ -n "$second" ] && [ "$second" != "$seed" ]; then
    pass "each run draws its own seed"
else
    fail "two runs drew the same seed ($seed, $second)"
fi
out="$(MIDGARD_PROBE_IMAGE="registry.test/__CA_SEED__/probe@sha256:$(printf 'b%.0s' $(seq 64))" run_check 2>&1)"
if [ ! -f "$work/applied.yaml" ] && [[ "$out" == *"refusing to apply"*">>> RC-40.2: FAIL"* ]]; then
    pass "a placeholder that survives rendering is refused and nothing is applied"
else
    fail "a surviving placeholder was not refused: $out"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all probe-manifests tests passed"
