#!/usr/bin/env bash
# Run the suite natively from dist/mac/bin against an enclosure home (default
# ~/Desktop/Midgard), without Docker: the same services and arguments as the Docker
# containers, with host paths and loopback addresses. Logs and pids go to
# $MIDGARD_HOME/native-run/. Stop the Docker containers first: a state directory can be
# held by one process only.
#
#   bash tools/mac-app/run-native.sh start | stop | status
#
# WARNING: the Docker containers' current Linux binaries (built before commit c7d8054) do
# not honour macOS lock files. Do not `docker start` any midgard container (including via
# heimdall-poc/day-start.sh) while the native suite runs.
# The gateway ledger size cap (rotate-gateway-ledger.sh) works only for the container.
# `stop` leaves llama-server running: it is shared, and started by mimir-model.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="$PWD/dist/mac/bin"
H="${MIDGARD_HOME:-$HOME/Desktop/Midgard}"
RUN="$H/native-run"

# Each service's directory lock (services/runtime/src/lock.rs). A service stopped with
# SIGTERM, or a container, leaves its lock file behind.
lock_path() {
  case $1 in
    gateway)  echo "$H/airports/midgard.lock" ;;              # services/http-gateway/src/state_dir.rs
    mimir)    echo "$H/mimir-state/mimir.lock" ;;             # mimir/mimir-server/src/main.rs
    heimdall) echo "$H/heimdall-poc/state/feed.lock" ;;       # heimdall/feed-server/src/main.rs
    forge)    echo "$H/live-feeds/worker-state/worker.lock" ;; # forge/worker/src/main.rs
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}
binary() {
  case $1 in
    gateway) echo http-gateway ;; mimir) echo mimir-server ;;
    heimdall) echo heimdall-feed ;; forge) echo forge-worker ;;
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}

# Whether the pid file names a live process running this service's binary; a pid file
# naming anything else is stale and is removed (the lock is left alone).
running() {
  [ -f "$RUN/$1.pid" ] || return 1
  local pid comm bin
  pid=$(cat "$RUN/$1.pid"); bin=$(binary "$1")
  comm=$(ps -p "$pid" -o comm= 2>/dev/null) || comm=
  case $comm in
    "$bin" | */"$bin") return 0 ;;
  esac
  rm -f "$RUN/$1.pid"; return 1
}
launch() {  # name, ready-pattern, command...
  local name=$1 ready=$2; shift 2
  if running "$name"; then echo "$name already running"; return; fi
  nohup "$@" > "$RUN/$name.log" 2>&1 &
  echo $! > "$RUN/$name.pid"
  for _ in $(seq 1 180); do
    grep -qs "$ready" "$RUN/$name.log" && { echo "$name ready"; return; }
    running "$name" || {
      echo "$name exited; see $RUN/$name.log"; tail -5 "$RUN/$name.log"
      echo "run tools/mac-app/run-native.sh stop to clean up"; exit 1
    }
    sleep 1
  done
  echo "$name not ready after 180 s; see $RUN/$name.log"
  echo "run tools/mac-app/run-native.sh stop to clean up"; exit 1
}

# Refuse while any container can hold the enclosure's directories. Fails closed: if the
# docker CLI exists but cannot be queried, nothing is touched.
check_docker() {
  command -v docker > /dev/null || return 0
  local ids names hp
  ids=$(docker ps -q) || { echo "cannot query Docker; refusing to touch lock files"; exit 1; }
  [ -n "$ids" ] || return 0
  names=$(docker ps --format '{{.Names}}') || { echo "cannot query Docker; refusing to touch lock files"; exit 1; }
  if printf '%s\n' "$names" | grep -qE '^midgard-(gateway|heimdall|mimir|forge)$'; then
    echo "stop the Midgard Docker containers first"; exit 1
  fi
  hp=$(cd "$H" && pwd -P)
  # shellcheck disable=SC2086  # one argument per container id
  names=$(docker inspect --format '{{.Name}} {{range .Mounts}}{{.Source}} {{end}}' $ids) \
    || { echo "cannot query Docker; refusing to touch lock files"; exit 1; }
  names=$(printf '%s\n' "$names" | grep -F -e "$H" -e "$hp" | cut -d' ' -f1 | sed 's|^/||') || true
  if [ -n "$names" ]; then
    echo "these containers mount $H; stop these containers first:"; printf '  %s\n' $names; exit 1
  fi
}

start() {
  check_docker
  local b s lock
  for b in http-gateway heimdall-feed mimir-server forge-worker; do
    [ -x "$BIN/$b" ] || { echo "$BIN/$b is missing; run tools/mac-app/build-services.sh first"; exit 1; }
  done
  [ -f "$H/mimir-model.sh" ] || { echo "$H/mimir-model.sh is missing"; exit 1; }
  mkdir -p "$RUN"
  # No container can hold these directories, so a lock that no native process of ours
  # holds and that was not written on macOS was left by a stopped container.
  for s in mimir gateway heimdall forge; do
    lock=$(lock_path "$s")
    [ -e "$lock" ] || continue
    if [ ! -r "$lock" ]; then echo "cannot read $lock; leaving it"; continue; fi
    if ! running "$s" && ! grep -qx 'os=macos' "$lock"; then
      rm -f "$lock"; echo "removed stale lock $lock (left by a stopped container)"
    fi
  done
  local analytics=()
  if [ -f "$H/heimdall-poc/analytics/secret" ]; then
    analytics=(--analytics-addr 127.0.0.1:7740 --analytics-token-file "$H/heimdall-poc/analytics/secret")
  fi
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
    ${analytics[@]+"${analytics[@]}"} \
    --mimir-observe-addr 127.0.0.1:7721 --mimir-identity-dir "$H/airports-mesh/mimir-observer" \
    --forge-jobs "$H/live-feeds/data/jobs.json" \
    --forge-status "$H/live-feeds/worker-state/status.json" \
    --heimdall-feeds-status "$H/heimdall-poc/feeds/state/status.json"
  launch heimdall '"serving"' "$BIN/heimdall-feed" \
    --identity-dir "$H/airports-mesh/heimdall" --state-dir "$H/heimdall-poc/state" \
    --bind 127.0.0.1:7730 --health-bind 127.0.0.1:9730 --track-ttl 300 \
    --mimir-addr 127.0.0.1:7720 --mimir-identity-dir "$H/airports-mesh/heimdall-advisor" \
    --fabric-addr 127.0.0.1:7711 --fabric-identity-dir "$H/airports-mesh/heimdall-fabric"
  # Ready on the first successful job run; "job run failed" lines do not count, so a
  # worker whose every job fails is reported not ready.
  launch forge '"message":"job run"' "$BIN/forge-worker" \
    --identity-dir "$H/airports-mesh/forge" --state-dir "$H/live-feeds/worker-state" \
    --jobs "$H/live-feeds/data/jobs.json" --gateway 127.0.0.1:7711
}

stop() {
  local s pid lock
  for s in forge heimdall gateway mimir; do
    if running "$s"; then
      pid=$(cat "$RUN/$s.pid"); lock=$(lock_path "$s")
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 15); do running "$s" || break; sleep 1; done
      if running "$s"; then
        echo "$s did not exit within 15 s (pid $pid); its lock is left in place"
        continue
      fi
      if [ -f "$lock" ] && grep -qx "pid=$pid" "$lock"; then rm -f "$lock"; fi
      echo "$s stopped"
    fi
    rm -f "$RUN/$s.pid"
  done
}

status() {
  local s
  for s in mimir gateway heimdall forge; do
    if running "$s"; then echo "$s running (pid $(cat "$RUN/$s.pid"))"; else echo "$s stopped"; fi
  done
}

case "${1:-status}" in start) start ;; stop) stop ;; status) status ;; *) echo "start|stop|status"; exit 2 ;; esac
