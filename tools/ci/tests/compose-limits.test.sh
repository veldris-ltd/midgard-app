#!/usr/bin/env bash
# Tests for the compose enclosure's resource limits and its edge network, and for the
# limits mg-install sizes from the server. [VLD-ANB-MIDGARD-0029]
# [VLD-ANB-MIDGARD-0061] [AC-38.1] [AC-38.5] [NFR-SEC-1]
#
#   bash tools/ci/tests/compose-limits.test.sh
#
# The claims under test: every long-running service compose renders, the Granite
# sidecar included, has a memory limit with no swap above it, a CPU share, a PID limit
# and a descriptor limit; the edge network does no masquerade (or is internal), so the
# gateway on it has no route out of the host; and the limits mg-install writes into
# the site overlay follow the server's cores and memory and are accepted by compose.
# The one-shot clients (probe, fleet-agent, fleet-status, feed-source, picture) are not
# held to it. The compose checks need `docker compose` and are skipped without it.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../.."
compose_dir="$root/deploy/compose"
installer="$root/deploy/installer/mg-install"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# ---- mg-install's sizing ---------------------------------------------------------------
# service_limits is a pure function of CPUS, RAM_MB and MODEL; take it out of the
# installer rather than run the installer.
eval "$(sed -n '/^service_limits() {/,/^}/p' "$installer" | tr -d '\r')"
if ! declare -F service_limits >/dev/null; then
    fail "mg-install defines service_limits"
    echo "1 failure(s)"
    exit 1
fi

# expect_limits LABEL SERVICE MEM CPUS PIDS: the overlay lines for SERVICE carry them
expect_limits() {
    local out want
    out="$(service_limits "$2")"
    want="$(printf '    mem_limit: %s\n    memswap_limit: %s\n    cpus: %s\n    pids_limit: %s' "$3" "$3" "$4" "$5")"
    if [ "$out" = "$want" ]; then pass "$1"; else fail "$1: got $(echo "$out" | tr -s ' \n' ' ')"; fi
}

CPUS=8 RAM_MB=32000 MODEL=granite
expect_limits "a team server: the gateway is capped at 1 GB, with 512 PIDs" gateway 1024m 1 512
expect_limits "a team server with Granite: Mimir has half the memory and all cores but two" \
    mimir-server 16000m 6 256
expect_limits "a team server with Granite: llama.cpp has half the memory and all cores but two" \
    mimir-llamacpp 16000m 6 256
expect_limits "a team server: other services have 512 MB and one core" bifrost-fleet 512m 1 256
expect_limits "a team server: the console has 512 MB and one core" console 512m 1 256

CPUS=4 RAM_MB=8000 MODEL=reference
expect_limits "an evaluation server: the gateway has an eighth of the memory" gateway 1000m 1 512
expect_limits "an evaluation server without Granite: Mimir has a quarter of the memory" \
    mimir-server 2000m 2 256

CPUS=2 RAM_MB=0 MODEL=reference
expect_limits "memory not measured: no service gets less than 256 MB" gateway 256m 1 512
expect_limits "two cores: Mimir still has one" mimir-server 256m 1 256

# ---- what compose renders --------------------------------------------------------------
if ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; then
    echo "skip  the compose checks: docker compose is not installed here"
    [ "$failures" -eq 0 ] && { echo "compose-limits: PASS"; exit 0; }
    echo "$failures failure(s)"
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    fail "python3 is installed (the rendered configuration is read with it)"
    echo "$failures failure(s)"
    exit 1
fi

cat > "$work/check.py" <<'PY'
# check.py SERVICE...: the rendered compose configuration on stdin gives each SERVICE
# every limit, non-zero, and the edge network no route out. Prints what is missing.
import json, sys

config = json.load(sys.stdin)
services = config.get("services", {})
missing = []
for name in sys.argv[1:]:
    svc = services.get(name)
    if svc is None:
        missing.append(f"{name}: not rendered")
        continue
    for key in ("mem_limit", "memswap_limit", "cpus", "pids_limit"):
        try:
            ok = float(svc.get(key) or 0) > 0
        except (TypeError, ValueError):
            ok = False
        if not ok:
            missing.append(f"{name}: {key}")
    nofile = (svc.get("ulimits") or {}).get("nofile")
    hard = nofile.get("hard", 0) if isinstance(nofile, dict) else (nofile or 0)
    if not int(hard) > 0:
        missing.append(f"{name}: ulimits.nofile")
    if svc.get("mem_limit") is not None and svc.get("memswap_limit") != svc.get("mem_limit"):
        missing.append(f"{name}: swap above the memory limit")
edge = config.get("networks", {}).get("edge", {})
opts = edge.get("driver_opts") or {}
if not (edge.get("internal") is True
        or str(opts.get("com.docker.network.bridge.enable_ip_masquerade")).lower() == "false"):
    missing.append("edge: masquerade is not disabled and the network is not internal")
print("\n".join(missing))
sys.exit(1 if missing else 0)
PY

# check LABEL SERVICES -- COMPOSE-ARGS...: compose renders, and check.py passes
check() {
    local label="$1" services="$2" json out
    shift 3
    if ! json="$(docker compose "$@" config --format json 2>"$work/err")"; then
        fail "$label: compose does not render it: $(tr '\n' ' ' <"$work/err")"
        return
    fi
    # shellcheck disable=SC2086  # the service names split on purpose
    if out="$(python3 -I "$work/check.py" $services <<<"$json")"; then
        pass "$label"
    else
        fail "$label: $(echo "$out" | tr '\n' ';')"
    fi
}

long_running="init gateway mimir-server bifrost-fleet heimdall-feed forge-worker"

check "compose.yaml: every long-running service runs under limits, and edge has no route out" \
    "$long_running" -- -f "$compose_dir/compose.yaml"
check "with Granite: the llama.cpp sidecar runs under limits too" \
    "$long_running mimir-llamacpp" -- \
    -f "$compose_dir/compose.yaml" -f "$compose_dir/compose.granite.yaml"

# The gateway's limits are its own, not the anchor's. [AC-38.2]
gateway_limits="$(docker compose -f "$compose_dir/compose.yaml" config --format json |
    python3 -I -c 'import json,sys; g=json.load(sys.stdin)["services"]["gateway"]; print(g["mem_limit"], g["pids_limit"], g["ulimits"]["nofile"]["hard"])' 2>/dev/null)"
if [ "$gateway_limits" = "1073741824 512 8192" ]; then
    pass "the gateway has 1 GB, 512 PIDs and 8192 descriptors"
else
    fail "the gateway has 1 GB, 512 PIDs and 8192 descriptors: got $gateway_limits"
fi

# The check catches a regression: the same file with the limits and the edge options
# taken out fails it.
sed -E '/^[[:space:]]*(mem_limit|memswap_limit|pids_limit):/d; /enable_ip_masquerade/d' \
    "$compose_dir/compose.yaml" > "$work/unlimited.yaml"
# shellcheck disable=SC2086  # the service names split on purpose
if json="$(docker compose -f "$work/unlimited.yaml" config --format json 2>/dev/null)" &&
    ! python3 -I "$work/check.py" $long_running <<<"$json" >/dev/null; then
    pass "a compose file without the limits or the edge option fails the check"
else
    fail "a compose file without the limits or the edge option fails the check"
fi

# An overlay as mg-install writes it, for a team server with every product and Granite,
# is accepted by compose and replaces the shipped sizes. [AC-38.5]
CPUS=8 RAM_MB=32000 MODEL=granite
{
    echo "services:"
    for svc in init gateway forge-worker mimir-server mimir-llamacpp bifrost-fleet heimdall-feed; do
        echo "  $svc:"
        service_limits "$svc"
    done
} > "$work/compose.site.yaml"
if [ "$(grep -c 'mem_limit' "$work/compose.site.yaml")" -ge 5 ]; then
    pass "the site overlay sizes at least five services"
else
    fail "the site overlay sizes at least five services"
fi
check "the site overlay is accepted by compose and keeps every limit" \
    "$long_running mimir-llamacpp" -- \
    -f "$compose_dir/compose.yaml" -f "$compose_dir/compose.granite.yaml" -f "$work/compose.site.yaml"
mimir="$(docker compose -f "$compose_dir/compose.yaml" -f "$compose_dir/compose.granite.yaml" \
    -f "$work/compose.site.yaml" config --format json |
    python3 -I -c 'import json,sys; m=json.load(sys.stdin)["services"]["mimir-server"]; print(m["mem_limit"], m["cpus"])' 2>/dev/null)"
if [ "$mimir" = "16777216000 6" ]; then
    pass "the overlay's sizes replace the shipped ones"
else
    fail "the overlay's sizes replace the shipped ones: mimir-server got $mimir"
fi

if [ "$failures" -eq 0 ]; then
    echo "compose-limits: PASS"
    exit 0
fi
echo "$failures failure(s)"
exit 1
