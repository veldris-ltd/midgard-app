#!/usr/bin/env bash
# Run Bifrost's control plane locally, natively (no Docker), the way deploy/compose ran it:
# bifrost-server (the fleet manager and the artefact registry; bifrost-fleet is its older
# name) on loopback with its own mesh identity, policy and channels, and bifrost-agent
# playing installations that check in and an operator that reads the fleet.
#
#   bash tools/bifrost/run-local.sh setup    # once: build, a private enclosure, identities, state
#   bash tools/bifrost/run-local.sh setup --real-enclosure   # identities from the suite's enclosure
#   bash tools/bifrost/run-local.sh start    # start bifrost-server (mesh 127.0.0.1:7750, health :9750)
#   bash tools/bifrost/run-local.sh demo     # three installations check in; one is behind
#   bash tools/bifrost/run-local.sh status   # the fleet as the operator sees it (JSON)
#   bash tools/bifrost/run-local.sh release list|create|promote|rollback|rollouts [...]
#                                            # bifrost-release release ..., as release-manager
#   bash tools/bifrost/run-local.sh agent start [--initial-version V] | agent stop | agent status
#                                            # the suite agent (bifrost-agent --suite, mac-suite)
#   bash tools/bifrost/run-local.sh stop
#
# `agent start` runs bifrost-agent in suite mode as installation mac-suite on channel
# stable, in the background (pid $BIFROST_HOME/agent.pid, log $BIFROST_HOME/agent.log):
# its suite root is $BIFROST_SUITE_ROOT (default $BIFROST_HOME/suite), its runner this
# checkout's tools/mac-app/run-native.sh with MIDGARD_SUITE_ROOT exported (so services
# restart from the suite root's active/ links), its release-signer anchors
# fleet-state/trust/release-signers, and it adopts this checkout's dist/mac/bin as
# --initial-version on its first run (only if their digests match that release in the
# registry), restarting the running services from their slots once. It returns when the
# agent has recovered and adopted (it writes suite/agent.ready). The runner gets
# MIDGARD_REPLAY=$BIFROST_REPLAY (default the hour-1 recording in ~/Library/Application
# Support/Midgard-dev/recordings; BIFROST_REPLAY=none for no replay). If Docker's daemon
# cannot be reached, MIDGARD_DOCKER_STOPPED=1 is exported for the runner; run-native's
# Docker check must pass before the agent starts. `agent stop` sends SIGTERM and waits
# (up to 10 minutes) while the agent finishes the swap in progress.
#
# Everything lives in $BIFROST_HOME (default ~/Desktop/Midgard/bifrost-local). Ports avoid
# 7740, which the camera analytics use. The console (products/bifrost/ui) still shows demo
# data: it has no client for this service yet.
#
# Plain `setup` makes Bifrost its own private enclosure ($BIFROST_HOME/enclosure: issuer
# keys and audit), so the suite's real enclosure is never touched and the gateway needs no
# stop. It issues bifrost-fleet (fleet-manager), three fleet agents, the suite agent
# mac-suite (fleet-agent), fleet-operator and
# release-manager (RegisterArtefact, ReadArtefacts, VerifyArtefact). Agents may only
# DownloadArtefact from the registry; operators ReadArtefacts (list) and VerifyArtefact.
#
# `setup --real-enclosure` issues the same identities from the suite's real enclosure
# ($MIDGARD_ENCLOSURE, default ~/Desktop/Midgard/airports) instead, the server's as
# bifrost-server, into $BIFROST_MESH_OUT (default ~/Desktop/Midgard/airports-mesh/
# bifrost)/{server,clients}; an identity already there is kept. `mg-admin mesh issue`
# appends to the enclosure's governance ledger, so it refuses unless the suite is
# stopped: no suite binary running (http-gateway, heimdall-*, mimir-server, forge-worker,
# midgard-replay, mg-admin, or anything from a dist/mac/bin), no lock file in the
# enclosure (any holder, live or not: a stale one is for an operator to remove), and no
# Docker container running (set MIDGARD_DOCKER_STOPPED=1 when Docker Desktop is quit).
# Before issuing, it backs the enclosure up to ~/Library/Caches/midgard-build/
# airports-backup-<date>.tar.gz and then removes the previous such backup (one is kept).
# The choice is recorded in $BIFROST_HOME/mesh.conf, which start, demo, status and
# release.sh read. The first switch moves the private enclosure, its identities and the
# server's sealed state (fleet.journal, fleet.ledger, registry/, releases.sealed: sealed under the old
# identity, so the new one cannot open them) to $BIFROST_HOME/private-enclosure/.
#
# Both setups make fleet-state/ (policy.json, channels.json) and fleet-state/trust/
# release-signers/, where the registry's release-signer public keys go (`<alg> <key-id>
# <hex>` .pub files, ML-DSA-65; release.sh --init-key puts one there). With none there,
# every upload is refused as an unknown signer. An older policy.json without the
# registry's actions is replaced (the old one is kept as policy.json.pre-registry), and
# one from before the read split (no DownloadArtefact) likewise (kept as
# policy.json.pre-b2), and one from before releases and rollouts (no CreateRelease) too
# (kept as policy.json.pre-b2t2). rings.json, the rollout rings (canary forge-worker,
# then midgard-replay + heimdall-feed + heimdall-source, then mimir-server, then
# http-gateway; soak 60 s, readiness timeout 180 s), is copied from deploy/compose when
# absent. channels.json only seeds a channel the orchestrator does not know yet.
# bifrost-server reads its policy and rings at start: restart it after.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
H="${BIFROST_HOME:-$HOME/Desktop/Midgard/bifrost-local}"
TARGET="${CARGO_TARGET_DIR:-$HOME/Library/Caches/midgard-build/bifrost-target}"
BIN="$TARGET/release"
MESH="${BIFROST_MESH:-127.0.0.1:7750}"
HEALTH="${BIFROST_HEALTH:-127.0.0.1:9750}"
AGENTS=(fleet-agent-1 fleet-agent-2 fleet-agent-3)
SUITE_AGENT=mac-suite
SUITE="${BIFROST_SUITE_ROOT:-$H/suite}"
REPLAY="${BIFROST_REPLAY:-$HOME/Library/Application Support/Midgard-dev/recordings/hour-1}"
ENCLOSURE="${MIDGARD_ENCLOSURE:-$HOME/Desktop/Midgard/airports}"
MESH_OUT="${BIFROST_MESH_OUT:-$HOME/Desktop/Midgard/airports-mesh/bifrost}"
BACKUPS="${MIDGARD_BACKUP_DIR:-$HOME/Library/Caches/midgard-build}"
SUITE_BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay mg-admin)
CONF="$H/mesh.conf"

die() { echo "run-local: $*" >&2; exit 1; }

# Where the server's identity and the clients' are: mesh.conf (setup --real-enclosure) or
# the private enclosure's defaults.
conf() { [ -f "$CONF" ] && sed -n "s/^$1=//p" "$CONF" | head -1 || true; }
SERVER_ID="$(conf server_identity)"; SERVER_ID="${SERVER_ID:-$H/fleet-identity}"
SERVICE="$(conf service)"; SERVICE="${SERVICE:-bifrost-fleet}"
CLIENTS="$(conf clients)"; CLIENTS="${CLIENTS:-$H/fleet-clients}"

build() {
    command -v cargo >/dev/null || PATH="/opt/homebrew/opt/rustup/bin:$PATH"
    (cd "$ROOT" && CARGO_TARGET_DIR="$TARGET" cargo build --offline --locked --release -q \
        -p bifrost-fleet-server -p mg-admin)
}

has_identity() { [ -f "$2/$1.keystore" ] || [ -f "$2/$1.cert" ]; }

issue() { # issue <enclosure> <service> <roles> <out>
    if has_identity "$2" "$4"; then
        echo "  $2: already has an identity"
    else
        "$BIN/mg-admin" mesh issue --state-dir "$1" --service "$2" --roles "$3" --out "$4" >/dev/null
        echo "  $2 ($3)"
    fi
}

pid() { cat "$H/fleet.pid" 2>/dev/null || true; }
running() { local p; p="$(pid)"; [ -n "$p" ] && kill -0 "$p" 2>/dev/null; }

# fleet-state: policy, channels and the release-signer trust directory.
state() {
    mkdir -p "$H/fleet-state/trust/release-signers"
    if [ -f "$H/fleet-state/policy.json" ] && ! grep -q RegisterArtefact "$H/fleet-state/policy.json"; then
        mv "$H/fleet-state/policy.json" "$H/fleet-state/policy.json.pre-registry"
        echo "  policy.json had no registry actions: replaced (old copy policy.json.pre-registry)"
    elif [ -f "$H/fleet-state/policy.json" ] && ! grep -q DownloadArtefact "$H/fleet-state/policy.json"; then
        local keep="$H/fleet-state/policy.json.pre-b2"
        [ ! -e "$keep" ] || keep="$keep.$(date +%Y%m%d-%H%M%S)"
        mv "$H/fleet-state/policy.json" "$keep"
        echo "  policy.json predates the split read actions (DownloadArtefact, VerifyArtefact): replaced (old copy $(basename "$keep")); restart bifrost-server to apply it"
    elif [ -f "$H/fleet-state/policy.json" ] && ! grep -q CreateRelease "$H/fleet-state/policy.json"; then
        local keep="$H/fleet-state/policy.json.pre-b2t2"
        [ ! -e "$keep" ] || keep="$keep.$(date +%Y%m%d-%H%M%S)"
        mv "$H/fleet-state/policy.json" "$keep"
        echo "  policy.json predates releases and rollouts (CreateRelease, Promote, Rollback, ...): replaced (old copy $(basename "$keep")); restart bifrost-server to apply it"
    fi
    [ -f "$H/fleet-state/policy.json" ] || cp "$ROOT/deploy/compose/fleet-policy.json" "$H/fleet-state/policy.json"
    [ -f "$H/fleet-state/channels.json" ] || cp "$ROOT/deploy/compose/fleet-channels.json" "$H/fleet-state/channels.json"
    [ -f "$H/fleet-state/rings.json" ] || cp "$ROOT/deploy/compose/fleet-rings.json" "$H/fleet-state/rings.json"
    echo "state: $H/fleet-state (policy.json, rings.json, channels.json: $(tr -d ' \n' < "$H/fleet-state/channels.json"))"
    echo "release signers: $H/fleet-state/trust/release-signers ($(ls "$H/fleet-state/trust/release-signers" | grep -c '\.pub$' || true) .pub files)"
}

setup() {
    [ ! -f "$CONF" ] || die "$H uses the real enclosure ($CONF); run setup --real-enclosure"
    build
    mkdir -p "$H"
    if [ ! -d "$H/enclosure" ]; then
        "$BIN/mg-admin" init --state-dir "$H/enclosure" >/dev/null
        echo "a private enclosure for Bifrost: $H/enclosure"
    fi
    echo "mesh identities:"
    issue "$H/enclosure" bifrost-fleet fleet-manager "$H/fleet-identity"
    for a in "${AGENTS[@]}" "$SUITE_AGENT"; do issue "$H/enclosure" "$a" fleet-agent "$H/fleet-clients"; done
    issue "$H/enclosure" fleet-operator fleet-operator "$H/fleet-clients"
    issue "$H/enclosure" release-manager release-manager "$H/fleet-clients"
    state
}

# Refuse unless nothing of the suite can be writing to $ENCLOSURE. Fails closed.
suite_stopped() {
    local n p busy="" locks
    for n in "${SUITE_BINARIES[@]}"; do
        p="$(pgrep -x "$n" || true)"
        [ -z "$p" ] || busy="$busy $n (pid $(echo $p))"
    done
    p="$(pgrep -f '/dist/mac/bin/' || true)"
    [ -z "$p" ] || busy="$busy dist/mac/bin process(es) (pid $(echo $p))"
    [ -z "$busy" ] || die "the suite is running:$busy. Stop it first (tools/mac-app/run-native.sh stop); mg-admin must not write the enclosure while a service does."
    locks="$(find "$ENCLOSURE" -maxdepth 1 -name '*.lock' 2>/dev/null)" || die "cannot list $ENCLOSURE"
    [ -z "$locks" ] || die "$ENCLOSURE is locked ($(echo $locks)): a service holds it, or one was killed and left its lock. Stop the suite; if nothing is running, remove the lock and re-run."
    if [ "${MIDGARD_DOCKER_STOPPED:-}" = 1 ]; then
        echo "the suite is stopped (Docker check skipped: MIDGARD_DOCKER_STOPPED=1)"
    elif command -v docker >/dev/null; then
        local ids
        ids="$(docker ps -q 2>/dev/null)" || die "cannot query Docker. If Docker Desktop is quit (no container can run), re-run with MIDGARD_DOCKER_STOPPED=1"
        [ -z "$ids" ] || die "Docker containers are running ($(docker ps --format '{{.Names}}' | tr '\n' ' ')); stop them first: one may hold $ENCLOSURE"
        echo "the suite is stopped (no suite process, no enclosure lock, no container)"
    else
        echo "the suite is stopped (no suite process, no enclosure lock)"
    fi
}

# One backup of the enclosure: write the new one, check it, then drop the previous ones.
backup() {
    local need_kb free_kb tmp out old
    need_kb=$(( $(du -sk "$ENCLOSURE" | cut -f1) + 5 * 1024 * 1024 ))
    free_kb="$(df -k "$HOME" | awk 'NR==2 {print $4}')"
    [ "$free_kb" -ge "$need_kb" ] || die "only $((free_kb / 1024)) MB free; the backup needs the enclosure's size plus 5 GB"
    mkdir -p "$BACKUPS"
    out="$BACKUPS/airports-backup-$(date +%Y%m%d-%H%M%S).tar.gz"
    tmp="$out.partial"
    tar -czf "$tmp" -C "$(dirname "$ENCLOSURE")" "$(basename "$ENCLOSURE")"
    local want got
    want="$(find "$ENCLOSURE" -type f | wc -l | tr -d ' ')"
    got="$(tar -tzf "$tmp" | grep -vc '/$' || true)"
    [ "$want" = "$got" ] && tar -tzf "$tmp" | grep -q "/keys/idp.keystore$" \
        || { rm -f "$tmp"; die "the backup lists $got files, the enclosure has $want; nothing was issued"; }
    mv "$tmp" "$out"
    for old in "$BACKUPS"/airports-backup-*.tar.gz; do
        [ "$old" = "$out" ] || [ ! -f "$old" ] || { rm -f "$old"; echo "  removed the previous backup $old"; }
    done
    echo "backup of $ENCLOSURE: $out ($(du -h "$out" | cut -f1)); restore with: tar -xzf $out -C $(dirname "$ENCLOSURE")"
}

setup_real() {
    [ -d "$ENCLOSURE" ] && [ -f "$ENCLOSURE/keys/idp.keystore" ] || die "$ENCLOSURE is not an enclosure (no keys/idp.keystore)"
    if [ ! -f "$CONF" ] && running; then
        die "bifrost-server is running on the private identity, whose sealed state this moves aside; run-local.sh stop first"
    fi
    build
    mkdir -p "$H"
    local srv="$MESH_OUT/server" cli="$MESH_OUT/clients" missing=0 s
    has_identity bifrost-server "$srv" || missing=1
    for s in "${AGENTS[@]}" "$SUITE_AGENT" fleet-operator release-manager; do has_identity "$s" "$cli" || missing=1; done
    if [ "$missing" = 1 ]; then
        suite_stopped
        backup
        mkdir -p "$srv" "$cli"
        chmod 700 "$MESH_OUT" "$srv" "$cli"
        echo "mesh identities from $ENCLOSURE:"
        issue "$ENCLOSURE" bifrost-server fleet-manager "$srv"
        for s in "${AGENTS[@]}" "$SUITE_AGENT"; do issue "$ENCLOSURE" "$s" fleet-agent "$cli"; done
        issue "$ENCLOSURE" fleet-operator fleet-operator "$cli"
        issue "$ENCLOSURE" release-manager release-manager "$cli"
    else
        echo "mesh identities: all already issued in $MESH_OUT (the enclosure is not touched)"
    fi

    if [ ! -f "$CONF" ]; then
        # The server's sealed state belongs to the private identity; keep it, apart.
        local a="$H/private-enclosure" f moved=""
        for f in enclosure fleet-identity fleet-clients fleet-state/fleet.journal fleet-state/fleet.ledger fleet-state/registry fleet-state/releases.sealed; do
            [ -e "$H/$f" ] || continue
            mkdir -p "$a/fleet-state"
            [ ! -e "$a/$f" ] || die "$a/$f already exists; move it away first"
            mv "$H/$f" "$a/$f"; moved="$moved $f"
        done
        [ -z "$moved" ] || echo "the private enclosure and its sealed state moved to $a:$moved"
        cat > "$CONF" <<EOF
# written by run-local.sh setup --real-enclosure; read by start, demo, status and release.sh
enclosure=$ENCLOSURE
server_identity=$srv
service=bifrost-server
clients=$cli
EOF
        echo "recorded in $CONF"
    fi
    state
}

start() {
    [ -d "$SERVER_ID" ] || die "run setup first"
    if running; then echo "bifrost-server already running (pid $(pid))"; return; fi
    : > "$H/fleet.log"
    "$BIN/bifrost-server" --service "$SERVICE" --identity-dir "$SERVER_ID" \
        --state-dir "$H/fleet-state" --bind "$MESH" --health-bind "$HEALTH" >> "$H/fleet.log" 2>&1 &
    echo $! > "$H/fleet.pid"
    for _ in $(seq 1 50); do
        grep -q '"serving"' "$H/fleet.log" && break
        running || die "bifrost-server exited: $(tail -3 "$H/fleet.log")"
        sleep 0.2
    done
    grep -q '"serving"' "$H/fleet.log" || die "bifrost-server did not start: $(tail -3 "$H/fleet.log")"
    echo "bifrost-server running as $SERVICE (pid $(pid)) on $MESH, health http://$HEALTH/readyz; log $H/fleet.log"
}

stop() {
    local p lock="$H/fleet-state/fleet.lock"
    p="$(pid)"
    if running; then
        kill -TERM "$p"
        for _ in $(seq 1 50); do running || break; sleep 0.2; done
        if running; then echo "bifrost-server (pid $p) has not exited yet"; return; fi
        echo "bifrost-server stopped"
        # SIGTERM skips the lock's release; drop it when it names the process just stopped
        # (older builds wrote no os= line, which a new start would refuse as foreign).
        if [ -f "$lock" ] && grep -qx "pid=$p" "$lock"; then rm -f "$lock"; fi
    else
        echo "bifrost-server not running"
    fi
    rm -f "$H/fleet.pid"
}

agent() { "$BIN/bifrost-agent" --identity-dir "$CLIENTS" --addr "$MESH" "$@"; }

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

# The suite agent.
agent_pid() { cat "$H/agent.pid" 2>/dev/null || true; }
agent_running() { local p; p="$(agent_pid)"; [ -n "$p" ] && kill -0 "$p" 2>/dev/null; }

# Docker for the runner's check: when the daemon cannot be reached (Docker Desktop quit),
# no container can run, so MIDGARD_DOCKER_STOPPED=1 is exported; then run-native's own
# check must pass, or the agent is not started.
agent_docker() {
    local err
    if [ "${MIDGARD_DOCKER_STOPPED:-}" != 1 ] && command -v docker >/dev/null; then
        if ! err="$(docker ps -q 2>&1 >/dev/null)"; then
            case "$err" in
                *"Cannot connect to the Docker daemon"* | *"docker daemon running"* | *"daemon is not running"*)
                    echo "Docker's daemon is not running: no container can hold the suite (MIDGARD_DOCKER_STOPPED=1)"
                    export MIDGARD_DOCKER_STOPPED=1 ;;
            esac
        fi
    fi
    MIDGARD_SUITE_ROOT="$SUITE" "$ROOT/tools/mac-app/run-native.sh" check-docker \
        || die "the runner's Docker check failed (above); the agent was not started"
}

agent_start() {
    local initial=() p
    case "${1:-}" in
        "") ;;
        --initial-version) [ -n "${2:-}" ] || die "--initial-version needs a version"; initial=(--initial-version "$2") ;;
        *) die "agent start takes only --initial-version V" ;;
    esac
    [ -x "$BIN/bifrost-agent" ] || die "no $BIN/bifrost-agent; run setup first"
    has_identity "$SUITE_AGENT" "$CLIENTS" || die "no $SUITE_AGENT identity in $CLIENTS; run setup (it issues one)"
    if agent_running; then echo "bifrost-agent already running (pid $(agent_pid))"; return; fi
    # The replay stays in the suite: the runner restarts it with this recording.
    if [ "$REPLAY" = none ]; then
        unset MIDGARD_REPLAY
    else
        [ -f "$REPLAY/meta.json" ] || die "no recording at $REPLAY (BIFROST_REPLAY; set BIFROST_REPLAY=none to run without the replay)"
        export MIDGARD_REPLAY="$REPLAY"
    fi
    mkdir -p "$SUITE/active"
    agent_docker
    MIDGARD_SUITE_ROOT="$SUITE" nohup "$BIN/bifrost-agent" --suite \
        --identity-dir "$CLIENTS" --addr "$MESH" --service "$SUITE_AGENT" --install-id "$SUITE_AGENT" \
        --channel stable --suite-root "$SUITE" --runner "$ROOT/tools/mac-app/run-native.sh" \
        --trust-dir "$H/fleet-state/trust/release-signers" --adopt-from "$ROOT/dist/mac/bin" \
        ${initial[@]+"${initial[@]}"} >> "$H/agent.log" 2>&1 < /dev/null &
    p=$!
    echo "$p" > "$H/agent.pid"
    # Running means recovered and adopted (adoption restarts the suite from its slots, so
    # this can take minutes): the agent writes its pid to agent.ready then.
    echo "bifrost-agent (pid $p) starting; waiting for recovery and adoption (log $H/agent.log)"
    for i in $(seq 1 1200); do
        [ "$(cat "$SUITE/agent.ready" 2>/dev/null)" = "$p" ] && break
        agent_running || { rm -f "$H/agent.pid"; die "bifrost-agent exited: $(tail -3 "$H/agent.log")"; }
        [ $((i % 30)) != 0 ] || echo "  still starting after $i s: $(tail -1 "$H/agent.log" | cut -c1-160)"
        sleep 1
    done
    [ "$(cat "$SUITE/agent.ready" 2>/dev/null)" = "$p" ] || die "bifrost-agent (pid $p) is not ready after 20 minutes; see $H/agent.log"
    echo "bifrost-agent (suite, $SUITE_AGENT) running (pid $p); suite root $SUITE; replay ${MIDGARD_REPLAY:-none}"
}

# SIGTERM: the agent finishes (or reverts) the swap in progress, waits for its runner,
# and exits. A restart can take minutes, so this waits up to 10 minutes.
agent_stop() {
    local p left
    p="$(agent_pid)"
    if agent_running; then
        kill -TERM "$p"
        echo "bifrost-agent (pid $p) stopping: finishing any swap in progress"
        for i in $(seq 1 600); do
            agent_running || break
            [ $((i % 30)) != 0 ] || echo "  still finishing after $i s"
            sleep 1
        done
        if agent_running; then echo "bifrost-agent (pid $p) has not exited after 10 minutes; see $H/agent.log"; return 1; fi
        left="$(pgrep -f "$ROOT/tools/mac-app/run-native.sh (restart|status)" || true)"
        [ -z "$left" ] || echo "warning: runner processes still running: $(echo $left)"
        echo "bifrost-agent stopped"
    else
        echo "bifrost-agent not running"
    fi
    rm -f "$H/agent.pid"
}

agent_status() {
    local s v
    if agent_running; then echo "bifrost-agent running (pid $(agent_pid)), suite root $SUITE"; else echo "bifrost-agent not running"; fi
    for s in forge-worker midgard-replay heimdall-feed heimdall-source mimir-server http-gateway; do
        v="$(readlink "$SUITE/active/$s" 2>/dev/null || true)"
        echo "  $s: ${v:-not adopted}"
    done
    [ ! -f "$SUITE/swap.journal" ] || echo "  a swap is journalled: $(tr '\n' ' ' < "$SUITE/swap.journal")"
    [ ! -f "${MIDGARD_HOME:-$HOME/Desktop/Midgard}/native-run/agent-paused" ] || echo "  the suite is paused (run-native.sh stop): the agent changes nothing"
    [ ! -f "$H/agent.log" ] || { echo "--- $H/agent.log (last 5)"; tail -5 "$H/agent.log"; }
}

release() {
    running || die "start it first"
    "$BIN/bifrost-release" release "$1" --identity-dir "$CLIENTS" --addr "$MESH" \
        --service release-manager "${@:2}"
}

case "${1:-}" in
    setup)
        case "${2:-}" in
            "") setup ;;
            --real-enclosure) setup_real ;;
            *) die "setup takes no option but --real-enclosure" ;;
        esac ;;
    start) start ;;
    stop) stop ;;
    demo) demo ;;
    status) status ;;
    agent)
        case "${2:-}" in
            start) agent_start "${@:3}" ;;
            stop) agent_stop ;;
            status) agent_status ;;
            *) die "agent needs start, stop or status" ;;
        esac ;;
    release) [ -n "${2:-}" ] || die "release needs list, create, promote, rollback or rollouts"; release "${@:2}" ;;
    *) sed -n '2,17p' "$0"; exit 2 ;;
esac
