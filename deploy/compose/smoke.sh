#!/usr/bin/env bash
# Compose smoke test, on throwaway volumes under its own project name so it never
# touches a running `midgard` project: build and start the enclosure, check the
# edge answers, make a PQ-mTLS inference call as a certified consumer, restart
# mimir-server and check its audit ledger was recovered and continues. Tears
# everything down, volumes included, on exit. [RC-33] [RC-09] [FR-MI-8]
set -euo pipefail

cd "$(dirname "$0")"
compose() { docker compose -p midgard-smoke -f compose.yaml "$@"; }
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
compose run --rm feed-source --kind air-track --entity AC-1 --attrs speed=420 --observed-at 100
compose run --rm feed-source --kind air-track --entity AC-1 --attrs speed=430 --observed-at 110
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

echo "compose smoke: PASS"
