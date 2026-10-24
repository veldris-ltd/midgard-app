#!/usr/bin/env bash
# Run Bifrost's fleet manager locally, natively (no Docker), the way deploy/compose ran it:
# bifrost-fleet on loopback with its own mesh identity, policy and channels, and
# bifrost-agent playing installations that check in and an operator that reads the fleet.
#
#   bash tools/bifrost/run-local.sh setup    # once: build, a private enclosure, identities, state
#   bash tools/bifrost/run-local.sh start    # start bifrost-fleet (mesh 127.0.0.1:7750, health :9750)
#   bash tools/bifrost/run-local.sh demo     # three installations check in; one is behind
#   bash tools/bifrost/run-local.sh status   # the fleet as the operator sees it (JSON)
#   bash tools/bifrost/run-local.sh stop
#
# Everything lives in $BIFROST_HOME (default ~/Desktop/Midgard/bifrost-local), including its
# own enclosure (issuer keys and audit), so the suite's real enclosure (~/Desktop/Midgard/
# airports) is never touched and the gateway needs no stop. Ports avoid 7740, which the
# camera analytics use. The console (products/bifrost/ui) still shows demo data: it has no
# client for this service yet.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
H="${BIFROST_HOME:-$HOME/Desktop/Midgard/bifrost-local}"
TARGET="${CARGO_TARGET_DIR:-$HOME/Library/Caches/midgard-build/bifrost-target}"
BIN="$TARGET/release"
MESH="${BIFROST_MESH:-127.0.0.1:7750}"
HEALTH="${BIFROST_HEALTH:-127.0.0.1:9750}"
AGENTS=(fleet-agent-1 fleet-agent-2 fleet-agent-3)

die() { echo "run-local: $*" >&2; exit 1; }

build() {
    if [ ! -x "$BIN/bifrost-fleet" ] || [ ! -x "$BIN/bifrost-agent" ] || [ ! -x "$BIN/mg-admin" ]; then
        command -v cargo >/dev/null || PATH="/opt/homebrew/opt/rustup/bin:$PATH"
        (cd "$ROOT" && CARGO_TARGET_DIR="$TARGET" cargo build --offline --locked --release \
            -p bifrost-fleet-server -p mg-admin)
    fi
}

issue() { # issue <service> <roles> <out>
    if [ -f "$3/$1.identity" ] || ls "$3" 2>/dev/null | grep -q "^$1"; then
        echo "  $1: already has an identity"
    else
        "$BIN/mg-admin" mesh issue --state-dir "$H/enclosure" --service "$1" --roles "$2" --out "$3" >/dev/null
        echo "  $1 ($2)"
    fi
}

setup() {
    build
    mkdir -p "$H"
    if [ ! -d "$H/enclosure" ]; then
        "$BIN/mg-admin" init --state-dir "$H/enclosure" >/dev/null
        echo "a private enclosure for Bifrost: $H/enclosure"
    fi
    echo "mesh identities:"
    issue bifrost-fleet fleet-manager "$H/fleet-identity"
    for a in "${AGENTS[@]}"; do issue "$a" fleet-agent "$H/fleet-clients"; done
    issue fleet-operator fleet-operator "$H/fleet-clients"
    mkdir -p "$H/fleet-state"
    [ -f "$H/fleet-state/policy.json" ] || cp "$ROOT/deploy/compose/fleet-policy.json" "$H/fleet-state/policy.json"
    [ -f "$H/fleet-state/channels.json" ] || cp "$ROOT/deploy/compose/fleet-channels.json" "$H/fleet-state/channels.json"
    echo "state: $H/fleet-state (policy.json, channels.json: $(tr -d ' \n' < "$H/fleet-state/channels.json"))"
}

pid() { cat "$H/fleet.pid" 2>/dev/null || true; }
running() { local p; p="$(pid)"; [ -n "$p" ] && kill -0 "$p" 2>/dev/null; }

start() {
    [ -d "$H/fleet-identity" ] || die "run setup first"
    if running; then echo "bifrost-fleet already running (pid $(pid))"; return; fi
    : > "$H/fleet.log"
    "$BIN/bifrost-fleet" --identity-dir "$H/fleet-identity" --state-dir "$H/fleet-state" \
        --bind "$MESH" --health-bind "$HEALTH" >> "$H/fleet.log" 2>&1 &
    echo $! > "$H/fleet.pid"
    for _ in $(seq 1 50); do
        grep -q '"serving"' "$H/fleet.log" && break
        running || die "bifrost-fleet exited: $(tail -3 "$H/fleet.log")"
        sleep 0.2
    done
    grep -q '"serving"' "$H/fleet.log" || die "bifrost-fleet did not start: $(tail -3 "$H/fleet.log")"
    echo "bifrost-fleet running (pid $(pid)) on $MESH, health http://$HEALTH/readyz; log $H/fleet.log"
}

stop() {
    if running; then kill -TERM "$(pid)"; echo "bifrost-fleet stopped"; else echo "bifrost-fleet not running"; fi
    rm -f "$H/fleet.pid"
}

agent() { "$BIN/bifrost-agent" --identity-dir "$H/fleet-clients" --addr "$MESH" "$@"; }

demo() {
    running || die "start it first"
    echo "--- three installations check in on channel stable (desired $(tr -d ' \n' < "$H/fleet-state/channels.json"))"
    agent --service fleet-agent-1 --channel stable --version 0.1.0
    agent --service fleet-agent-2 --channel stable --version 0.0.9
    agent --service fleet-agent-3 --channel stable --version 0.1.0 --unhealthy
    echo "--- the fleet as the operator sees it"
    status
}

status() { running || die "start it first"; agent --service fleet-operator --status; }

case "${1:-}" in
    setup) setup ;;
    start) start ;;
    stop) stop ;;
    demo) demo ;;
    status) status ;;
    *) sed -n '2,15p' "$0"; exit 2 ;;
esac
