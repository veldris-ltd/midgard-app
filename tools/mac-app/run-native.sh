#!/usr/bin/env bash
# Run the suite natively from dist/mac/bin against an enclosure home (default
# ~/Desktop/Midgard), without Docker: the same services and arguments as the Docker
# containers, with host paths and loopback addresses. Logs and pids go to
# $MIDGARD_HOME/native-run/. Stop the Docker containers first: a state directory can be
# held by one process only.
#
#   bash tools/mac-app/run-native.sh start | stop | status
#   bash tools/mac-app/run-native.sh restart <service> | status <service>
#
# With MIDGARD_SUITE_ROOT set (Bifrost's suite agent sets it), each service is started
# from $MIDGARD_SUITE_ROOT/active/<binary>, the agent's link to the slot of the version it
# runs, instead of dist/mac/bin. Unset, nothing changes.
#
# `restart <service>` stops that one service and starts it again exactly as `start` does,
# waiting for its readiness line (exit 0 once ready, 1 if it exits or is not ready within
# 180 s). `status <service>` prints one word: ready (running, and its log since this start
# has the readiness line `start` waits for), unready (running, not yet) or stopped.
# <service> is a short name (gateway, mimir, heimdall, forge, replay) or a binary name
# (http-gateway, mimir-server, heimdall-feed, forge-worker, midgard-replay,
# heimdall-source). heimdall-source has no process of its own: midgard-replay runs the
# heimdall-source beside its own binary for each batch, so its restart only checks the
# binary is there (the replay's next batch uses it) and its status is ready when it is.
# Restarting the replay reuses the recording it was started with (or MIDGARD_REPLAY).
#
# With MIDGARD_REPLAY set to a recording directory (made by record.py), `start` also runs
# midgard-replay, which plays the recording into Heimdall and Forge's data folder in place
# of the live feeds; `stop` stops it (and its heimdall-source runs) first.
#
# WARNING: the Docker containers' current Linux binaries (built before commit c7d8054) do
# not honour macOS lock files. Do not `docker start` any midgard container (including via
# heimdall-poc/day-start.sh) while the native suite runs.
# The gateway ledger size cap (rotate-gateway-ledger.sh) works only for the container.
# `stop` leaves llama-server running: it is shared, and started by mimir-model.sh.
#
# `start` refuses while any container mounts the enclosure home, and fails closed if
# Docker is installed but cannot be queried. If Docker Desktop is quit (so no container
# can be running), set MIDGARD_DOCKER_STOPPED=1 to skip the Docker check.
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="$PWD/dist/mac/bin"
if [ -n "${MIDGARD_SUITE_ROOT:-}" ]; then
  [ -d "$MIDGARD_SUITE_ROOT/active" ] || {
    echo "MIDGARD_SUITE_ROOT=$MIDGARD_SUITE_ROOT has no active/ folder (bifrost-agent --suite makes it)"; exit 1; }
  BIN="$(cd "$MIDGARD_SUITE_ROOT/active" && pwd)"
fi
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
    replay)   echo "" ;;                                      # none: holds no state directory
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}
binary() {
  case $1 in
    gateway) echo http-gateway ;; mimir) echo mimir-server ;;
    heimdall) echo heimdall-feed ;; forge) echo forge-worker ;; replay) echo midgard-replay ;;
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}

# A service's short name, from either form; heimdall-source is "source".
short() {
  case $1 in
    gateway | http-gateway) echo gateway ;; mimir | mimir-server) echo mimir ;;
    heimdall | heimdall-feed) echo heimdall ;; forge | forge-worker) echo forge ;;
    replay | midgard-replay) echo replay ;; heimdall-source) echo source ;;
    *) echo "unknown service $1" >&2; exit 2 ;;
  esac
}
# The log line `launch` waits for: the service is ready.
ready_pattern() {
  case $1 in
    mimir | heimdall) echo '"serving"' ;; gateway) echo 'listening on' ;;
    # The first successful job run; "job run failed" lines do not count, so a worker
    # whose every job fails is reported not ready.
    forge) echo '"message":"job run"' ;; replay) echo '"replay started"' ;;
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}

# Whether the pid file names a live process running this service's binary; a pid file
# naming anything else is stale and is removed (the lock is left alone).
running() {
  [ -f "$RUN/$1.pid" ] || return 1
  local pid comm bin
  pid=$(cat "$RUN/$1.pid"); bin=$(binary "$1")
  [ -n "$bin" ] || return 1
  comm=$(ps -p "$pid" -o comm= 2>/dev/null) || comm=
  case $comm in
    "$bin" | */"$bin") return 0 ;;
  esac
  rm -f "$RUN/$1.pid"; return 1
}
launch() {  # name, ready-pattern, command...  (exits 1 unless it becomes ready)
  local name=$1 ready=$2; shift 2
  if running "$name"; then echo "$name already running"; return; fi
  local pid
  nohup "$@" > "$RUN/$name.log" 2>&1 &
  pid=$!
  echo "$pid" > "$RUN/$name.pid"
  # The fresh child may still show as nohup just after launch, so test its pid only.
  for _ in $(seq 1 180); do
    grep -qs "$ready" "$RUN/$name.log" && { echo "$name ready"; return; }
    kill -0 "$pid" 2>/dev/null || {
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
docker_refuse() { echo "cannot query Docker; refusing to touch lock files"; exit 1; }
# Print the names of running containers with a mount at, under or above $H; fails if
# Docker cannot be queried (a container may vanish between ps and inspect).
docker_conflicts() {
  local ids line name src hp d m
  hp=$(cd "$H" && pwd -P)
  ids=$(docker ps -q) || return 1
  [ -n "$ids" ] || return 0
  # shellcheck disable=SC2086  # one argument per container id
  docker inspect --format '{{.Name}}{{range .Mounts}}|{{.Source}}{{end}}' $ids > "$TMPF" || return 1
  while IFS= read -r line; do
    name=${line%%|*}; name=${name#/}
    [ "$line" != "${line#*|}" ] || continue
    line=${line#*|}
    while :; do
      src=${line%%|*}
      if [ -n "$src" ]; then
        # The source as given and, if it exists here, its physical path; a mount of /
        # becomes "", a prefix of every path.
        for m in "${src%/}" "$( (cd "$src" 2>/dev/null && pwd -P) || echo "$src")"; do
          m=${m%/}
          for d in "$H" "$hp"; do
            case $d/ in "$m"/*) echo "$name"; break 3 ;; esac   # $H is at or under the mount
            case $m/ in "$d"/*) echo "$name"; break 3 ;; esac   # the mount is under $H
          done
        done
      fi
      [ "$line" != "${line#*|}" ] || break
      line=${line#*|}
    done
  done < "$TMPF"
}
check_docker() {
  if [ "${MIDGARD_DOCKER_STOPPED:-}" = 1 ]; then
    echo "skipping the Docker check (MIDGARD_DOCKER_STOPPED=1)"; return 0
  fi
  command -v docker > /dev/null || return 0
  local err names
  if ! err=$(docker ps -q 2>&1 > /dev/null); then
    case $err in
      *"Cannot connect to the Docker daemon"* | *"docker daemon running"* | *"daemon is not running"*)
        echo "Docker is installed but not running. If no Midgard containers can be running" \
          "(Docker Desktop is quit), re-run with MIDGARD_DOCKER_STOPPED=1."; exit 1 ;;
    esac
    echo "$err"; docker_refuse
  fi
  names=$(docker ps --format '{{.Names}}') || docker_refuse
  if printf '%s\n' "$names" | grep -qE '^midgard-(gateway|heimdall|mimir|forge)$'; then
    echo "stop the Midgard Docker containers first"; exit 1
  fi
  TMPF=$(mktemp); trap 'rm -f "$TMPF"' EXIT
  names=$(docker_conflicts) || names=$(docker_conflicts) || docker_refuse
  if [ -n "$names" ]; then
    echo "these containers mount $H or a directory containing it; stop these containers first:"
    printf '  %s\n' $names; exit 1
  fi
}

start() {
  [ -d "$H" ] || { echo "enclosure home $H not found"; exit 1; }
  check_docker
  local b s lock
  for b in http-gateway heimdall-feed mimir-server forge-worker; do
    [ -x "$BIN/$b" ] || { echo "$BIN/$b is missing; run tools/mac-app/build-services.sh first"; exit 1; }
  done
  [ -f "$H/mimir-model.sh" ] || { echo "$H/mimir-model.sh is missing"; exit 1; }
  if [ -n "${MIDGARD_REPLAY:-}" ]; then
    [ -d "$MIDGARD_REPLAY" ] || { echo "recording $MIDGARD_REPLAY not found (MIDGARD_REPLAY)"; exit 1; }
    [ -f "$MIDGARD_REPLAY/meta.json" ] || { echo "$MIDGARD_REPLAY has no meta.json; not a recording"; exit 1; }
    [ -x "$BIN/midgard-replay" ] || { echo "$BIN/midgard-replay is missing; run tools/mac-app/build-services.sh first"; exit 1; }
  fi
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
  bash "$H/mimir-model.sh" start
  launch_one mimir
  launch_one gateway
  launch_one heimdall
  launch_one forge
  if [ -n "${MIDGARD_REPLAY:-}" ]; then
    launch_one replay
  fi
}

# Start one service (short name) the way `start` does, and wait for it to be ready.
launch_one() {
  case $1 in
    mimir)
      launch mimir "$(ready_pattern mimir)" "$BIN/mimir-server" \
        --identity-dir "$H/airports-mesh/mimir" --state-dir "$H/mimir-state" --purposes investigation \
        --bind 127.0.0.1:7720 --health-bind 127.0.0.1:9720 --observe-bind 127.0.0.1:7721 \
        --engine llama-cpp --model-dir "$H/mimir-models/granite" \
        --model-trust "$H/mimir-models/trust/model-signing.pub" --sidecar 127.0.0.1:8080 ;;
    gateway)
      local analytics=()
      if [ -f "$H/heimdall-poc/analytics/secret" ]; then
        analytics=(--analytics-addr 127.0.0.1:7740 --analytics-token-file "$H/heimdall-poc/analytics/secret")
      fi
      launch gateway "$(ready_pattern gateway)" "$BIN/http-gateway" \
        --state-dir "$H/airports" --bind 127.0.0.1:7710 \
        --allow-origin http://localhost:5173 --allow-origin http://localhost:5174 \
        --allow-origin http://localhost:5175 --allow-origin http://localhost:5176 \
        --mesh-bind 127.0.0.1:7711 --mesh-identity-dir "$H/airports-mesh/gateway" \
        --heimdall-addr 127.0.0.1:7730 --heimdall-identity-dir "$H/airports-mesh/feeds" \
        ${analytics[@]+"${analytics[@]}"} \
        --mimir-observe-addr 127.0.0.1:7721 --mimir-identity-dir "$H/airports-mesh/mimir-observer" \
        --forge-jobs "$H/live-feeds/data/jobs.json" \
        --forge-status "$H/live-feeds/worker-state/status.json" \
        --heimdall-feeds-status "$H/heimdall-poc/feeds/state/status.json" ;;
    heimdall)
      launch heimdall "$(ready_pattern heimdall)" "$BIN/heimdall-feed" \
        --identity-dir "$H/airports-mesh/heimdall" --state-dir "$H/heimdall-poc/state" \
        --bind 127.0.0.1:7730 --health-bind 127.0.0.1:9730 --track-ttl 300 \
        --mimir-addr 127.0.0.1:7720 --mimir-identity-dir "$H/airports-mesh/heimdall-advisor" \
        --fabric-addr 127.0.0.1:7711 --fabric-identity-dir "$H/airports-mesh/heimdall-fabric" ;;
    forge)
      launch forge "$(ready_pattern forge)" "$BIN/forge-worker" \
        --identity-dir "$H/airports-mesh/forge" --state-dir "$H/live-feeds/worker-state" \
        --jobs "$H/live-feeds/data/jobs.json" --gateway 127.0.0.1:7711 ;;
    replay)
      # The recording: MIDGARD_REPLAY, or the one the replay was last started with.
      local rec="${MIDGARD_REPLAY:-}"
      [ -n "$rec" ] || rec=$(cat "$RUN/replay.recording" 2>/dev/null) || rec=
      [ -n "$rec" ] || { echo "no recording for midgard-replay: set MIDGARD_REPLAY"; exit 1; }
      [ -f "$rec/meta.json" ] || { echo "$rec has no meta.json; not a recording"; exit 1; }
      echo "$rec" > "$RUN/replay.recording"
      launch replay "$(ready_pattern replay)" "$BIN/midgard-replay" \
        --recording "$rec" --identity-dir "$H/airports-mesh/feeds" \
        --forge-dir "$H/live-feeds/data" ;;
    *) echo "unknown service $1"; exit 2 ;;
  esac
}

# Stop the replay's heimdall-source runs with it: pause it so it cannot start another,
# end its children while they are still its children, then end it.
kill_replay() {  # pid
  local kids k
  kill -STOP "$1" 2>/dev/null || true
  kids=$(pgrep -P "$1") || kids=
  pkill -TERM -P "$1" 2>/dev/null || true
  kill "$1" 2>/dev/null || true
  kill -CONT "$1" 2>/dev/null || true
  for k in $kids; do
    for _ in $(seq 1 15); do kill -0 "$k" 2>/dev/null || break; sleep 1; done
    kill -0 "$k" 2>/dev/null && echo "replay child $k did not exit within 15 s"
  done
  return 0
}

# Stop one service (short name); fails if it did not exit (its lock is then left).
stop_one() {
  local s=$1 pid lock
  if running "$s"; then
    pid=$(cat "$RUN/$s.pid"); lock=$(lock_path "$s")
    if [ "$s" = replay ]; then kill_replay "$pid"; else kill "$pid" 2>/dev/null || true; fi
    for _ in $(seq 1 15); do running "$s" || break; sleep 1; done
    if running "$s"; then
      echo "$s did not exit within 15 s (pid $pid); its lock is left in place"
      return 1
    fi
    if [ -n "$lock" ] && [ -f "$lock" ] && grep -qx "pid=$pid" "$lock"; then rm -f "$lock"; fi
    echo "$s stopped"
  fi
  rm -f "$RUN/$s.pid"
}

stop() {
  local s
  for s in replay forge heimdall gateway mimir; do
    stop_one "$s" || true
  done
}

# Stop one service and start it again as `start` does; exit 0 once it is ready.
restart() {
  local s lock
  s=$(short "$1")
  if [ "$s" = source ]; then
    [ -x "$BIN/heimdall-source" ] || { echo "$BIN/heimdall-source is missing"; exit 1; }
    echo "heimdall-source has no process of its own; midgard-replay runs $BIN/heimdall-source for each batch"
    return
  fi
  [ -d "$H" ] || { echo "enclosure home $H not found"; exit 1; }
  check_docker
  [ -x "$BIN/$(binary "$s")" ] || { echo "$BIN/$(binary "$s") is missing"; exit 1; }
  mkdir -p "$RUN"
  stop_one "$s" || { echo "not restarting $s"; exit 1; }
  # As in start: a lock no native process of ours holds, not written on macOS, was left
  # by a stopped container.
  lock=$(lock_path "$s")
  if [ -n "$lock" ] && [ -e "$lock" ] && [ -r "$lock" ] && ! grep -qx 'os=macos' "$lock"; then
    rm -f "$lock"; echo "removed stale lock $lock (left by a stopped container)"
  fi
  if [ "$s" = mimir ]; then bash "$H/mimir-model.sh" start; fi
  launch_one "$s"
}

# One word for one service: ready, unready or stopped.
status_one() {
  local s
  s=$(short "$1")
  if [ "$s" = source ]; then
    if [ -x "$BIN/heimdall-source" ]; then echo ready; else echo stopped; fi
  elif ! running "$s"; then echo stopped
  elif grep -qs "$(ready_pattern "$s")" "$RUN/$s.log"; then echo ready
  else echo unready
  fi
}

status() {
  local s
  for s in mimir gateway heimdall forge replay; do
    if running "$s"; then echo "$s running (pid $(cat "$RUN/$s.pid"))"; else echo "$s stopped"; fi
  done
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  status) if [ -n "${2:-}" ]; then status_one "$2"; else status; fi ;;
  restart) [ -n "${2:-}" ] || { echo "restart <service>"; exit 2; }; restart "$2" ;;
  *) echo "start|stop|status [<service>]|restart <service>"; exit 2 ;;
esac
