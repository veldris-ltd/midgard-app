#!/usr/bin/env bash
# Run the suite natively from dist/mac/bin against an enclosure home (default
# ~/Desktop/Midgard), without Docker: the same services and arguments as the Docker
# containers, with host paths and loopback addresses. Logs and pids go to
# $MIDGARD_HOME/native-run/. Stop the Docker containers first: a state directory can be
# held by one process only.
#
#   bash tools/mac-app/run-native.sh start | stop | status
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="$PWD/dist/mac/bin"
H="${MIDGARD_HOME:-$HOME/Desktop/Midgard}"
RUN="$H/native-run"
mkdir -p "$RUN"

# Each service's directory lock (services/runtime/src/lock.rs). A service stopped with
# SIGTERM, or a container, leaves its lock file behind.
lock_path() {
  case $1 in
    gateway)  echo "$H/airports/midgard.lock" ;;              # services/http-gateway/src/state_dir.rs
    mimir)    echo "$H/mimir-state/mimir.lock" ;;             # mimir/mimir-server/src/main.rs
    heimdall) echo "$H/heimdall-poc/state/feed.lock" ;;       # heimdall/feed-server/src/main.rs
    forge)    echo "$H/live-feeds/worker-state/worker.lock" ;; # forge/worker/src/main.rs
  esac
}

running() { [ -f "$RUN/$1.pid" ] && kill -0 "$(cat "$RUN/$1.pid")" 2>/dev/null; }
launch() {  # name, ready-pattern, command...
  local name=$1 ready=$2; shift 2
  if running "$name"; then echo "$name already running"; return; fi
  nohup "$@" > "$RUN/$name.log" 2>&1 &
  echo $! > "$RUN/$name.pid"
  for _ in $(seq 1 180); do
    grep -q "$ready" "$RUN/$name.log" && { echo "$name ready"; return; }
    running "$name" || { echo "$name exited; see $RUN/$name.log"; tail -5 "$RUN/$name.log"; exit 1; }
    sleep 1
  done
  echo "$name not ready after 180 s; see $RUN/$name.log"; exit 1
}

start() {
  if docker ps --format '{{.Names}}' 2>/dev/null | grep -qE '^midgard-(gateway|heimdall|mimir|forge)$'; then
    echo "stop the Midgard Docker containers first"; exit 1
  fi
  for b in http-gateway heimdall-feed mimir-server forge-worker; do
    [ -x "$BIN/$b" ] || { echo "$BIN/$b is missing; run tools/mac-app/build-services.sh first"; exit 1; }
  done
  [ -f "$H/mimir-model.sh" ] || { echo "$H/mimir-model.sh is missing"; exit 1; }
  # No container is running, so a lock that no native process of ours holds and that was
  # not written on macOS was left by a stopped container.
  local s lock
  for s in mimir gateway heimdall forge; do
    lock=$(lock_path "$s")
    if [ -f "$lock" ] && ! running "$s" && ! grep -qx 'os=macos' "$lock"; then
      rm -f "$lock"; echo "removed stale lock $lock (left by a stopped container)"
    fi
  done
  bash "$H/mimir-model.sh" start
  launch mimir '"serving"' "$BIN/mimir-server" \
    --identity-dir "$H/airports-mesh/mimir" --state-dir "$H/mimir-state" --purposes investigation \
    --bind 127.0.0.1:7720 --health-bind 127.0.0.1:9720 --observe-bind 127.0.0.1:7721 \
    --engine llama-cpp --model-dir "$H/mimir-models/granite" \
    --model-trust "$H/mimir-models/trust/model-signing.pub" --sidecar 127.0.0.1:8080
  launch gateway 'listening on' "$BIN/http-gateway" \
    --state-dir "$H/airports" --bind 127.0.0.1:7710 \
    --allow-origin http://localhost:5173 --allow-origin http://localhost:5174 \
    --allow-origin http://localhost:5175 --allow-origin http://localhost:5176 \
    --mesh-bind 127.0.0.1:7711 --mesh-identity-dir "$H/airports-mesh/gateway" \
    --heimdall-addr 127.0.0.1:7730 --heimdall-identity-dir "$H/airports-mesh/feeds" \
    --mimir-observe-addr 127.0.0.1:7721 --mimir-identity-dir "$H/airports-mesh/mimir-observer" \
    --forge-jobs "$H/live-feeds/data/jobs.json" \
    --forge-status "$H/live-feeds/worker-state/status.json" \
    --heimdall-feeds-status "$H/heimdall-poc/feeds/state/status.json"
  launch heimdall '"serving"' "$BIN/heimdall-feed" \
    --identity-dir "$H/airports-mesh/heimdall" --state-dir "$H/heimdall-poc/state" \
    --bind 127.0.0.1:7730 --health-bind 127.0.0.1:9730 --track-ttl 300 \
    --mimir-addr 127.0.0.1:7720 --mimir-identity-dir "$H/airports-mesh/heimdall-advisor" \
    --fabric-addr 127.0.0.1:7711 --fabric-identity-dir "$H/airports-mesh/heimdall-fabric"
  launch forge '"job run' "$BIN/forge-worker" \
    --identity-dir "$H/airports-mesh/forge" --state-dir "$H/live-feeds/worker-state" \
    --jobs "$H/live-feeds/data/jobs.json" --gateway 127.0.0.1:7711
}

stop() {
  local s
  for s in forge heimdall gateway mimir; do
    if running "$s"; then
      kill "$(cat "$RUN/$s.pid")"
      for _ in $(seq 1 15); do running "$s" || break; sleep 1; done
      if running "$s"; then
        echo "$s did not exit within 15 s (pid $(cat "$RUN/$s.pid")); its lock is left in place"
        continue
      fi
      rm -f "$(lock_path "$s")"
      echo "$s stopped"
    fi
    rm -f "$RUN/$s.pid"
  done
}

status() {
  for s in mimir gateway heimdall forge; do
    if running "$s"; then echo "$s running (pid $(cat "$RUN/$s.pid"))"; else echo "$s stopped"; fi
  done
}

case "${1:-status}" in start) start ;; stop) stop ;; status) status ;; *) echo "start|stop|status"; exit 2 ;; esac
