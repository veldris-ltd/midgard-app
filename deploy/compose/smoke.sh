#!/usr/bin/env bash
# Compose smoke test, on throwaway volumes under its own project name so it never
# touches a running `midgard` project: build and start the enclosure, check the
# edge answers, make a PQ-mTLS inference call as a certified consumer, restart
# mimir-server and check its audit ledger was recovered and continues. Tears
# everything down, volumes included, on exit. [RC-33] [RC-09] [FR-MI-8]
set -euo pipefail

cd "$(dirname "$0")"
compose() { MIDGARD_EDGE_BRIDGE=mg-smoke0 docker compose -p midgard-smoke -f compose.yaml "$@"; }
trap 'compose --profile probe --profile fleet --profile heimdall down -v --remove-orphans >/dev/null 2>&1 || true' EXIT

compose up -d --build --wait gateway mimir-server bifrost-fleet heimdall-feed forge-worker

echo "--- the edge answers"
curl -fsS http://127.0.0.1:7710/api/v1/health
echo

echo "--- inference over PQ-mTLS, audited"
first="$(compose run --rm probe)"
echo "$first"
grep -q 'audit_seq=0' <<<"$first"

echo "--- mimir-server restarts on its recovered ledger"
compose restart mimir-server
compose up -d --wait mimir-server
second="$(compose run --rm probe)"
echo "$second"
grep -q 'audit_seq=1' <<<"$second"

echo "--- an installation checks in with the fleet manager"
compose run --rm fleet-agent --channel stable --version 0.0.9
status="$(compose run --rm fleet-status)"
echo "$status"
grep -q '"id":"fleet-agent-1"' <<<"$status"
grep -q '"converged":false' <<<"$status"

echo "--- it upgrades and the fleet converges"
compose run --rm fleet-agent --channel stable --version 0.1.0
grep -q '"converged":true' <<<"$(compose run --rm fleet-status)"

echo "--- two signed observations fuse into one tracked entity"
# Dated by the host clock: heimdall-feed refuses an observed_at far from its own.
now="$(date +%s)"
compose run --rm feed-source --kind air-track --entity AC-1 --attrs speed=420 --observed-at "$now"
compose run --rm feed-source --kind air-track --entity AC-1 --attrs speed=430 --observed-at "$((now + 10))"
picture="$(compose run --rm picture)"
echo "$picture"
grep -q '"id":"AC-1"' <<<"$picture"
grep -q '"observations":2' <<<"$picture"

echo "--- the Forge worker syncs its sources into the enclosure's fabric"
synced=""
for _ in $(seq 1 30); do
    synced="$(compose logs forge-worker | grep '"job":"aircraft"' | grep '"outcome":"synced"' || true)"
    [ -n "$synced" ] && break
    sleep 2
done
echo "$synced" | head -1
grep -q '3 created, 0 updated, 3 link(s) added' <<<"$synced"

# Every container runs under its memory, PID and descriptor limits, and the gateway
# cannot open a connection out of the host. [VLD-ANB-MIDGARD-0029]
# [VLD-ANB-MIDGARD-0061] [AC-38.2] [AC-38.3]
positive() { [[ "$1" =~ ^[1-9][0-9]*$ ]]; }

echo "--- every service runs under limits"
for id in $(compose ps -q); do
    limits="$(docker inspect -f '{{.Name}} {{.HostConfig.Memory}} {{.HostConfig.PidsLimit}} {{range .HostConfig.Ulimits}}{{if eq .Name "nofile"}}{{.Hard}}{{end}}{{end}}' "$id")"
    echo "$limits"
    read -r name memory pids nofile <<<"$limits"
    if ! positive "$memory" || ! positive "$pids" || ! positive "${nofile:-}"; then
        echo "$name runs without a memory, PID or descriptor limit" >&2
        exit 1
    fi
done

echo "--- the gateway has no route out of the host"
if [[ "$(docker info --format '{{.OperatingSystem}}' 2>/dev/null)" == *"Docker Desktop"* ]]; then
    # Docker Desktop's VM forwards the bridge's subnet itself, so the bridge setting
    # alone does not stop it. On a server, mg-install's DOCKER-USER rule does, and
    # Desktop is not a server target. [VLD-ANB-MIDGARD-0061]
    echo "SKIP: Docker Desktop routes the edge bridge out whatever its settings, and is"
    echo "      not a server target; on a Linux server mg-install's firewall rule"
    echo "      (midgard-edge-egress, in DOCKER-USER) is what stops the gateway reaching out"
else
    # The control: the same probe, from the gateway's image and networks, reaches the
    # gateway itself over the mesh, so a failure below is the network's, not the probe's.
    compose run --rm --no-deps -T --entrypoint timeout gateway 10 \
        bash -c 'exec 3<>/dev/tcp/gateway/7710'
    if compose run --rm --no-deps -T --entrypoint timeout gateway 10 \
        bash -c 'exec 3<>/dev/tcp/1.1.1.1/443'; then
        echo "the gateway opened a connection to 1.1.1.1:443" >&2
        exit 1
    fi
    echo "no connection out"
fi

echo "compose smoke: PASS"
