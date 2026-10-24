#!/usr/bin/env bash
# Run the suite natively from dist/mac/bin against an enclosure home (default
# ~/Desktop/Midgard), without Docker: the same services and arguments as the Docker
# containers, with host paths and loopback addresses. Logs and pids go to
# $MIDGARD_HOME/native-run/. Stop the Docker containers first: a state directory can be
# held by one process only.
#
#   bash tools/mac-app/run-native.sh start | stop | status
#   bash tools/mac-app/run-native.sh restart <service> | status <service> | check-docker
#
# With MIDGARD_REPLAY set to a recording directory (made by record.py), `start` also runs
# midgard-replay, which plays the recording into Heimdall and Forge's data folder in place
# of the live feeds; `stop` stops it (and its heimdall-source runs) first.
#
# Managed by Bifrost's suite agent (MIDGARD_SUITE_ROOT set): each service is launched from
# the slot its $MIDGARD_SUITE_ROOT/active/<binary> link names, resolved at launch, and the
# version launched is written to native-run/<service>.version. midgard-replay is given
# --source-bin active/heimdall-source, so each batch runs the linked heimdall-source.
# native-run/suite-root records the suite root; while it names one, `start` and `restart`
# without MIDGARD_SUITE_ROOT refuse (running dist/mac/bin under a managed suite would put
# builds the agent does not know about in place; remove native-run/suite-root to run
# unmanaged again). Unmanaged, nothing else changes.
#
# `stop` also writes native-run/agent-paused, which `start` removes: while it is there the
# agent changes nothing and `restart` starts nothing (exit 3, "left stopped").
#
# `restart <service>` stops that one service and launches it again exactly as `start`
# does, waiting for its readiness line: exit 0 once ready, 3 if the suite is paused, 1 if
# it exits or is not ready within 180 s. `status <service>` is read-only and prints one
# line: `ready <version>` (running, its log since this launch has the readiness line, and
# it was launched from the version now linked), `unready <version>`, `stopped` or `paused`.
# <version> is the version it was launched from: native-run/<service>.version holds
# "<pid> <version>" and is trusted only for the live pid in the pid file, otherwise `-`
# (and never ready when managed). <service> is a short name
# (gateway, mimir, heimdall, forge, replay) or a binary name (http-gateway, mimir-server,
# heimdall-feed, forge-worker, midgard-replay, heimdall-source).
#
# heimdall-source has no process of its own (midgard-replay runs it per batch). Its
# restart and status run the linked binary with an unknown option and expect its usage
# line and exit code 2: proof that it executes and is heimdall-source.
#
# Each service has a lock, native-run/<service>.busy (a folder holding its owner's pid),
# taken by start, stop and restart for that service; status reports `unready` while it is
# held. A lock whose owner has exited is removed. Restarting the replay reuses the
# recording it was started with (or MIDGARD_REPLAY). A launch keeps the previous log as
# <service>.log.1. MIDGARD_READY_TIMEOUT (default 180 s) is how long a launch waits for
# readiness.
#
# WARNING: the Docker containers' current Linux binaries (built before commit c7d8054) do
# not honour macOS lock files. Do not `docker start` any midgard container (including via
# heimdall-poc/day-start.sh) while the native suite runs.
# The gateway ledger size cap (rotate-gateway-ledger.sh) works only for the container.
# `stop` leaves llama-server running: it is shared, and started by mimir-model.sh.
#
# `start` and `restart` refuse while any container mounts the enclosure home, and fail
# closed if Docker is installed but cannot be queried. If Docker Desktop is quit (so no
# container can be running), set MIDGARD_DOCKER_STOPPED=1 to skip the Docker check.
# MIDGARD_DOCKER_STOPPED=auto skips the check only while Docker's daemon cannot be
# reached, deciding on each call (the suite agent uses it). `check-docker` runs only the
# check. `restart` exits 4 (not attempted, nothing changed) when it cannot try now: the
# Docker or managed-suite check fails, the service's lock is held past
# MIDGARD_LOCK_TIMEOUT (600 s), or the old process will not stop.
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="$PWD/dist/mac/bin"
MANAGED=""
if [ -n "${MIDGARD_SUITE_ROOT:-}" ]; then
  [ -d "$MIDGARD_SUITE_ROOT/active" ] || {
    echo "MIDGARD_SUITE_ROOT=$MIDGARD_SUITE_ROOT has no active/ folder (bifrost-agent --suite makes it)"; exit 1; }
  MANAGED="$(cd "$MIDGARD_SUITE_ROOT" && pwd)"
  BIN="$MANAGED/active"
fi
H="${MIDGARD_HOME:-$HOME/Desktop/Midgard}"
RUN="$H/native-run"
TMPF=""
HELD=()

# A service lock folder holds only its owner's pid file.
drop_lock_dir() {
  rm -f "${1:?}/pid"
  rmdir "${1:?}" 2>/dev/null || true
}
cleanup() {
  [ -z "$TMPF" ] || rm -f "$TMPF"
  local s
  for s in ${HELD[@]+"${HELD[@]}"}; do
    if [ "$(cat "$RUN/$s.busy/pid" 2>/dev/null)" = "$$" ]; then drop_lock_dir "$RUN/$s.busy"; fi
  done
  return 0
}
trap cleanup EXIT

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
    source) echo heimdall-source ;;
    *) echo "unknown service $1" >&2; exit 1 ;;
  esac
}

# A service's short name, from either form; heimdall-source is "source".
short() {
  case $1 in
    gateway | http-gateway) echo gateway ;; mimir | mimir-server) echo mimir ;;
    heimdall | heimdall-feed) echo heimdall ;; forge | forge-worker) echo forge ;;
    replay | midgard-replay) echo replay ;; source | heimdall-source) echo source ;;
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

# The version the service's active link names now ("-" unmanaged, or not a slot link).
linked_version() {
  local bin target v
  [ -n "$MANAGED" ] || { echo -; return; }
  bin=$(binary "$1")
  target=$(readlink "$BIN/$bin" 2>/dev/null) || { echo -; return; }
  case $target in
    ../releases/*/"$bin") v=${target#../releases/}; echo "${v%/"$bin"}" ;;
    *) echo - ;;
  esac
}
# What to execute for a service at version $2: its slot when managed, else dist/mac/bin.
exe_for() {
  local bin
  bin=$(binary "$1")
  if [ -n "$MANAGED" ] && [ "$2" != - ]; then echo "$MANAGED/releases/$2/$bin"; else echo "$BIN/$bin"; fi
}

# Whether the pid file names a live process running this service's binary. Read-only.
alive() {
  [ -f "$RUN/$1.pid" ] || return 1
  local pid comm bin
  pid=$(cat "$RUN/$1.pid"); bin=$(binary "$1")
  comm=$(ps -p "$pid" -o comm= 2>/dev/null) || comm=
  case $comm in
    "$bin" | */"$bin") return 0 ;;
  esac
  return 1
}
# As alive; a pid file naming anything else is stale and is removed (the lock is left
# alone). For start, stop and restart, which hold the service's lock.
running() {
  alive "$1" && return 0
  rm -f "$RUN/$1.pid"; return 1
}

# Whether a lock owner is a live run-native.sh (a pid reused by anything else is not).
live_owner() {
  [ -n "$1" ] && kill -0 "$1" 2>/dev/null || return 1
  case $(ps -p "$1" -o command= 2>/dev/null) in *run-native.sh*) return 0 ;; esac
  return 1
}
# Remove a stale lock: rename it aside (atomic, so of two contenders only one gets it),
# then check that what was moved is still the stale owner's; if a live owner took the
# lock in between, put it back.
break_stale() {  # dir, stale owner
  local aside="$1.stale.$$"
  mv "$1" "$aside" 2>/dev/null || return 0
  if [ "$(cat "$aside/pid" 2>/dev/null)" = "$2" ]; then
    drop_lock_dir "$aside"; echo "removed the stale lock $1 (owner ${2:-none} is not a running run-native.sh)"
  else
    mv "$aside" "$1" 2>/dev/null || echo "warning: could not put back the lock moved to $aside"
  fi
}
# The service's lock: a folder holding the owner's pid. Re-entrant for this script. A
# lock whose owner is not a live run-native.sh (or that has had no owner for 30 s) is
# stale and broken. Waits up to MIDGARD_LOCK_TIMEOUT (600) s for a live owner, then exits
# 4 (not attempted).
lock_service() {
  local d="$RUN/$1.busy" owner=""
  mkdir -p "$RUN"
  for _ in $(seq 1 "${MIDGARD_LOCK_TIMEOUT:-600}"); do
    if mkdir "$d" 2>/dev/null; then
      echo "$$" > "$d/pid"; HELD+=("$1"); return 0
    fi
    owner=$(cat "$d/pid" 2>/dev/null) || owner=
    [ "$owner" = "$$" ] && return 0
    if [ -n "$owner" ] && ! live_owner "$owner"; then break_stale "$d" "$owner"; continue; fi
    if [ -z "$owner" ] && [ $(( $(date +%s) - $(stat -f %m "$d" 2>/dev/null || date +%s) )) -gt 30 ]; then
      break_stale "$d" ""; continue
    fi
    sleep 1
  done
  echo "$1 is busy (lock $d held by pid ${owner:-?}); not attempted"; exit 4
}
unlock_service() {
  if [ "$(cat "$RUN/$1.busy/pid" 2>/dev/null)" = "$$" ]; then drop_lock_dir "$RUN/$1.busy"; fi
  return 0
}
held_by_other() {
  local owner
  [ -d "$RUN/$1.busy" ] || return 1
  owner=$(cat "$RUN/$1.busy/pid" 2>/dev/null) || owner=
  [ -z "$owner" ] || live_owner "$owner"
}
# The version the running process was launched from: native-run/<s>.version holds
# "<pid> <version>", trusted only for the live pid in the pid file; "-" otherwise.
running_version() {
  local vp vv pid
  alive "$1" || { echo -; return; }
  pid=$(cat "$RUN/$1.pid" 2>/dev/null) || pid=
  read -r vp vv < "$RUN/$1.version" 2>/dev/null || { echo -; return; }
  if [ -n "$pid" ] && [ "$vp" = "$pid" ] && [ -n "${vv:-}" ]; then echo "$vv"; else echo -; fi
}
# A managed suite: say which script to use.
managed_note() {
  local root
  root=$(cat "$RUN/suite-root" 2>/dev/null) || root=
  [ -n "$root" ] || return 0
  echo "note: this suite is managed by Bifrost's agent (suite root $root). Use only this script," \
    "$PWD/tools/mac-app/run-native.sh (md_bifrost), with MIDGARD_SUITE_ROOT=$root; other checkouts'" \
    "run-native.sh (midgard-app's) do not know the suite root and would run dist/mac/bin."
}

# A managed suite must be run managed.
check_managed() {
  local root
  if [ -n "$MANAGED" ]; then mkdir -p "$RUN"; echo "$MANAGED" > "$RUN/suite-root"; return 0; fi
  root=$(cat "$RUN/suite-root" 2>/dev/null) || root=
  if [ -n "$root" ] && [ -d "$root/active" ]; then
    echo "the suite is managed by Bifrost's agent (suite root $root):"
    echo "  run with MIDGARD_SUITE_ROOT=$root, or remove $RUN/suite-root to run dist/mac/bin unmanaged"
    exit 1
  fi
}

launch() {  # name, ready-pattern, version, command...  (exits 1 unless it becomes ready)
  local name=$1 ready=$2 version=$3; shift 3
  if running "$name"; then
    echo "$name already running"
    if [ -n "$MANAGED" ] && [ "$(running_version "$name")" = - ]; then
      echo "warning: $name (pid $(cat "$RUN/$name.pid")) was not launched by this script managed;" \
        "its version is unknown and it is reported unready until restarted (restart $name)"
    fi
    return
  fi
  local pid
  if [ -f "$RUN/$name.log" ]; then mv -f "$RUN/$name.log" "$RUN/$name.log.1"; fi
  rm -f "$RUN/$name.version"
  nohup "$@" > "$RUN/$name.log" 2>&1 &
  pid=$!
  echo "$pid" > "$RUN/$name.pid"
  echo "$pid $version" > "$RUN/$name.version"
  # The fresh child may still show as nohup just after launch, so test its pid only.
  for _ in $(seq 1 "${MIDGARD_READY_TIMEOUT:-180}"); do
    grep -qs "$ready" "$RUN/$name.log" && { echo "$name ready"; return; }
    kill -0 "$pid" 2>/dev/null || {
      echo "$name exited; see $RUN/$name.log"; tail -5 "$RUN/$name.log"
      echo "run tools/mac-app/run-native.sh stop to clean up"; exit 1
    }
    sleep 1
  done
  echo "$name not ready after ${MIDGARD_READY_TIMEOUT:-180} s; see $RUN/$name.log"
  echo "run tools/mac-app/run-native.sh stop to clean up"; exit 1
}

# heimdall-source executes and is heimdall-source: an unknown option gets its usage line
# and exit code 2.
check_source() {
  local out rc=0
  [ -x "$BIN/heimdall-source" ] || return 1
  out=$("$BIN/heimdall-source" --bifrost-check 2>&1) || rc=$?
  [ "$rc" = 2 ] && printf '%s\n' "$out" | grep -q 'usage: heimdall-source'
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
        # auto: decided on each call, so a daemon that comes back is checked again.
        if [ "${MIDGARD_DOCKER_STOPPED:-}" = auto ]; then
          echo "Docker's daemon is not running: no container can hold $H (MIDGARD_DOCKER_STOPPED=auto)"; return 0
        fi
        echo "Docker is installed but not running. If no Midgard containers can be running" \
          "(Docker Desktop is quit), re-run with MIDGARD_DOCKER_STOPPED=1."; exit 1 ;;
    esac
    echo "$err"; docker_refuse
  fi
  names=$(docker ps --format '{{.Names}}') || docker_refuse
  if printf '%s\n' "$names" | grep -qE '^midgard-(gateway|heimdall|mimir|forge)$'; then
    echo "stop the Midgard Docker containers first"; exit 1
  fi
  TMPF=$(mktemp)
  names=$(docker_conflicts) || names=$(docker_conflicts) || { rm -f "$TMPF"; docker_refuse; }
  rm -f "$TMPF"
  if [ -n "$names" ]; then
    echo "these containers mount $H or a directory containing it; stop these containers first:"
    printf '  %s\n' $names; exit 1
  fi
}

start() {
  [ -d "$H" ] || { echo "enclosure home $H not found"; exit 1; }
  check_managed
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
  rm -f "$RUN/agent-paused"
  # No container can hold these directories, so a lock that no native process of ours
  # holds and that was not written on macOS was left by a stopped container.
  for s in mimir gateway heimdall forge; do
    lock=$(lock_path "$s")
    [ -e "$lock" ] || continue
    if [ ! -r "$lock" ]; then echo "cannot read $lock; leaving it"; continue; fi
    if ! alive "$s" && ! grep -qx 'os=macos' "$lock"; then
      rm -f "$lock"; echo "removed stale lock $lock (left by a stopped container)"
    fi
  done
  managed_note
  bash "$H/mimir-model.sh" start
  for s in mimir gateway heimdall forge; do
    lock_service "$s"; launch_one "$s"; unlock_service "$s"
  done
  if [ -n "${MIDGARD_REPLAY:-}" ]; then
    lock_service replay; launch_one replay; unlock_service replay
  fi
}

# Launch one service (short name) the way `start` does, and wait for it to be ready.
launch_one() {
  local v exe
  v=$(linked_version "$1")
  exe=$(exe_for "$1" "$v")
  case $1 in
    mimir)
      launch mimir "$(ready_pattern mimir)" "$v" "$exe" \
        --identity-dir "$H/airports-mesh/mimir" --state-dir "$H/mimir-state" --purposes investigation \
        --bind 127.0.0.1:7720 --health-bind 127.0.0.1:9720 --observe-bind 127.0.0.1:7721 \
        --engine llama-cpp --model-dir "$H/mimir-models/granite" \
        --model-trust "$H/mimir-models/trust/model-signing.pub" --sidecar 127.0.0.1:8080 ;;
    gateway)
      local analytics=()
      if [ -f "$H/heimdall-poc/analytics/secret" ]; then
        analytics=(--analytics-addr 127.0.0.1:7740 --analytics-token-file "$H/heimdall-poc/analytics/secret")
      fi
      launch gateway "$(ready_pattern gateway)" "$v" "$exe" \
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
      launch heimdall "$(ready_pattern heimdall)" "$v" "$exe" \
        --identity-dir "$H/airports-mesh/heimdall" --state-dir "$H/heimdall-poc/state" \
        --bind 127.0.0.1:7730 --health-bind 127.0.0.1:9730 --track-ttl 300 \
        --mimir-addr 127.0.0.1:7720 --mimir-identity-dir "$H/airports-mesh/heimdall-advisor" \
        --fabric-addr 127.0.0.1:7711 --fabric-identity-dir "$H/airports-mesh/heimdall-fabric" ;;
    forge)
      launch forge "$(ready_pattern forge)" "$v" "$exe" \
        --identity-dir "$H/airports-mesh/forge" --state-dir "$H/live-feeds/worker-state" \
        --jobs "$H/live-feeds/data/jobs.json" --gateway 127.0.0.1:7711 ;;
    replay)
      # The recording: MIDGARD_REPLAY, or the one the replay was last started with.
      local rec="${MIDGARD_REPLAY:-}" source=()
      [ -n "$rec" ] || rec=$(cat "$RUN/replay.recording" 2>/dev/null) || rec=
      [ -n "$rec" ] || { echo "no recording for midgard-replay: set MIDGARD_REPLAY"; exit 1; }
      [ -f "$rec/meta.json" ] || { echo "$rec has no meta.json; not a recording"; exit 1; }
      echo "$rec" > "$RUN/replay.recording"
      # Managed, each batch runs the heimdall-source linked at the time.
      [ -z "$MANAGED" ] || source=(--source-bin "$BIN/heimdall-source")
      launch replay "$(ready_pattern replay)" "$v" "$exe" \
        --recording "$rec" --identity-dir "$H/airports-mesh/feeds" \
        --forge-dir "$H/live-feeds/data" ${source[@]+"${source[@]}"} ;;
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

# Stop one service (short name) under its lock; fails if it did not exit (its lock file
# is then left).
stop_one() {
  local s=$1 pid lock
  lock_service "$s"
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
  mkdir -p "$RUN"
  # The user stopped the suite: Bifrost's agent changes nothing until `start`.
  echo "stopped by run-native.sh stop at $(date '+%Y-%m-%d %H:%M:%S')" > "$RUN/agent-paused"
  managed_note
  for s in replay forge heimdall gateway mimir; do
    stop_one "$s" || true
    unlock_service "$s"
  done
}

# Stop one service and launch it again as `start` does: 0 once ready, 3 if the suite is
# paused (nothing is started), 1 otherwise.
restart() {
  local s lock
  s=$(short "$1")
  if [ -f "$RUN/agent-paused" ]; then
    echo "the suite is paused (run-native.sh stop); $s left stopped"; exit 3
  fi
  if [ "$s" = source ]; then
    lock_service source
    if [ -f "$RUN/agent-paused" ]; then echo "the suite is paused (run-native.sh stop)"; exit 3; fi
    check_source || { echo "$BIN/heimdall-source does not run as heimdall-source"; exit 1; }
    linked_version source > "$RUN/source.version"
    echo "heimdall-source ready (midgard-replay runs $BIN/heimdall-source for each batch)"
    return
  fi
  # Exit 4 (not attempted, nothing changed) when it cannot try now.
  [ -d "$H" ] || { echo "enclosure home $H not found"; exit 4; }
  ( check_managed ) || exit 4
  ( check_docker ) || exit 4
  [ -x "$BIN/$(binary "$s")" ] || { echo "$BIN/$(binary "$s") is missing"; exit 1; }
  mkdir -p "$RUN"
  lock_service "$s"
  # The user may have stopped the suite while this waited for the lock.
  if [ -f "$RUN/agent-paused" ]; then
    echo "the suite is paused (run-native.sh stop); $s left as it is"; exit 3
  fi
  stop_one "$s" || { echo "not restarting $s"; exit 4; }
  # As in start: a lock no native process of ours holds, not written on macOS, was left
  # by a stopped container.
  lock=$(lock_path "$s")
  if [ -n "$lock" ] && [ -e "$lock" ] && [ -r "$lock" ] && ! grep -qx 'os=macos' "$lock"; then
    rm -f "$lock"; echo "removed stale lock $lock (left by a stopped container)"
  fi
  if [ "$s" = mimir ]; then bash "$H/mimir-model.sh" start; fi
  launch_one "$s"
}

# One line for one service: ready <version>, unready <version>, stopped or paused.
# Read-only.
status_one() {
  local s v lv
  s=$(short "$1")
  if [ -f "$RUN/agent-paused" ]; then echo paused; return; fi
  lv=$(linked_version "$s")
  if [ "$s" = source ]; then
    v=$(cat "$RUN/source.version" 2>/dev/null) || v=-
    if ! [ -x "$BIN/heimdall-source" ]; then echo stopped
    elif held_by_other source; then echo "unready $v"
    elif check_source && { [ -z "$MANAGED" ] || [ "$v" = "$lv" ]; }; then echo "ready $v"
    else echo "unready $v"
    fi
    return
  fi
  v=$(running_version "$s")
  # A restart or stop in progress: whatever it finds, the service is not ready.
  if held_by_other "$s"; then echo "unready $v"; return; fi
  if ! alive "$s"; then echo stopped; return; fi
  if [ -n "$MANAGED" ] && [ "$v" = - ]; then echo "unready -"
  elif grep -qs "$(ready_pattern "$s")" "$RUN/$s.log" && { [ -z "$MANAGED" ] || [ "$v" = "$lv" ]; }; then
    echo "ready $v"
  else echo "unready $v"
  fi
}

status() {
  local s
  for s in mimir gateway heimdall forge replay; do
    if alive "$s"; then echo "$s running (pid $(cat "$RUN/$s.pid"))"; else echo "$s stopped"; fi
  done
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  status) if [ -n "${2:-}" ]; then status_one "$2"; else status; fi ;;
  restart) [ -n "${2:-}" ] || { echo "restart <service>"; exit 2; }; restart "$2" ;;
  check-docker) check_docker; echo "Docker check passed" ;;
  *) echo "start|stop|status [<service>]|restart <service>|check-docker"; exit 2 ;;
esac
