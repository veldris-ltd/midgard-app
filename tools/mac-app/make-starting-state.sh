#!/usr/bin/env bash
# Build the Mac app's starting state from an enclosure home (default ~/Desktop/Midgard):
# the configuration, identities and initial data the services need, which Midgard.app
# copies into ~/Library/Application Support/Midgard on first launch and on Reset demo.
# The services then run against that copy exactly as run-native.sh runs them against
# $MIDGARD_HOME (see SuiteSpecs.make in apps/mac/Sources/MidgardShellCore/ServiceSpec.swift).
#
#   bash tools/mac-app/make-starting-state.sh [ENCLOSURE_HOME]
#
# The home comes from the argument, else MIDGARD_HOME. The output goes to
# ${MIDGARD_BUILD_DIR:-~/Library/Caches/midgard-build}/starting-state, outside the suite
# folder (objects.store alone is about 250 MB); it is built in a temporary sibling and
# swapped in, so a failed run leaves the previous starting state as it was.
#
# The home's stores must be quiet: the script refuses while any container mounts the
# home (as run-native.sh; with Docker quit, set MIDGARD_DOCKER_STOPPED=1), while any
# service lock in the state directories is held by a running process, or while the
# native suite, the feeds runner, the live-feeds fetcher or the loops that write into
# the home are running.
#
# Kept: the gateway's configuration, keys, identity directory and object store (no
# audit ledger: it starts fresh), every mesh identity, Heimdall's and Mimir's policies
# and rules (picture, ledgers and history start empty; the replay fills them), Forge's
# jobs and current CSVs, an empty feeds status, demo-users.json for the sign-in (built
# from airports-passwords.txt; the passwords are never printed or put on a command
# line) and VERSION. Never kept: mimir-signing/, the password files, analytics, any
# .venv, the models (the app bundles them), native-run/, logs. The starting state must
# contain no symlinks (Home.prepare refuses them).
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO="$PWD"
H="${1:-${MIDGARD_HOME:-$HOME/Desktop/Midgard}}"
BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
OUT="$BUILD/starting-state"

[ -d "$H" ] || { echo "enclosure home $H not found"; exit 1; }
H=$(cd "$H" && pwd -P)
[ -d "$H/airports" ] || { echo "$H has no airports/; not an enclosure home"; exit 1; }
mkdir -p "$BUILD"
BUILD=$(cd "$BUILD" && pwd -P); OUT="$BUILD/starting-state"
case "$BUILD/" in "$H"/*) echo "the build directory $BUILD is inside $H; use one outside it (storage budget)"; exit 1 ;; esac

TMP=""; TMPF=""
cleanup() {
  [ -z "$TMPF" ] || rm -f "$TMPF"
  if [ -n "$TMP" ] && [ -d "$TMP" ]; then chmod -R u+rwx "$TMP" 2>/dev/null || true; rm -rf "$TMP"; fi
}
trap cleanup EXIT

# --- Safety: the home must not be in use ------------------------------------------

# As run-native.sh: refuse while any container can hold the home's directories, and
# fail closed if Docker is installed but cannot be queried.
docker_refuse() { echo "cannot query Docker; refusing to read the enclosure's stores"; exit 1; }
# Print the names of running containers with a mount at, under or above $H; fails if
# Docker cannot be queried (a container may vanish between ps and inspect).
docker_conflicts() {
  local ids line name src d m
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
        for m in "${src%/}" "$( (cd "$src" 2>/dev/null && pwd -P) || echo "$src")"; do
          m=${m%/}
          for d in "$H"; do   # $H is already its physical path
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
  TMPF=$(mktemp)
  names=$(docker_conflicts) || names=$(docker_conflicts) || docker_refuse
  if [ -n "$names" ]; then
    echo "these containers mount $H or a directory containing it; stop these containers first:"
    printf '  %s\n' $names; exit 1
  fi
}

alive() {  # pid
  case $1 in '' | *[!0-9]*) return 1 ;; esac
  kill -0 "$1" 2>/dev/null || ps -p "$1" > /dev/null 2>&1
}

busy=()
# Service locks (services/runtime/src/lock.rs): "pid=N" and "os=…" lines. A lock written
# by a container (os other than macos) names a pid inside it, meaningless here; the
# Docker check covers containers.
check_locks() {
  local dir lock pid os
  for dir in airports mimir-state heimdall-poc/state live-feeds/worker-state; do
    for lock in "$H/$dir"/*.lock; do
      [ -f "$lock" ] || continue
      if [ ! -r "$lock" ]; then busy+=("$lock cannot be read; check what holds it"); continue; fi
      pid=$(sed -n 's/^pid=//p' "$lock" | head -1)
      os=$(sed -n 's/^os=//p' "$lock" | head -1)
      if [ -n "$os" ] && [ "$os" != macos ]; then continue; fi
      if alive "$pid"; then
        busy+=("${lock#"$H"/} is held by running pid $pid ($(ps -p "$pid" -o comm= 2>/dev/null || echo '?')): stop that service")
      fi
    done
  done
}
# Pid files of the processes that write into the home: file (relative to $H), what to stop.
check_pidfile() {  # file, what
  local pid
  [ -f "$H/$1" ] || return 0
  pid=$(head -1 "$H/$1" 2>/dev/null) || pid=
  if alive "$pid"; then busy+=("$2 is running (pid $pid, from $1)"); fi
}
check_pids() {
  local f
  for f in "$H"/native-run/*.pid; do
    [ -f "$f" ] || continue
    check_pidfile "${f#"$H"/}" "the native suite: run tools/mac-app/run-native.sh stop;"
  done
  check_pidfile heimdall-poc/feeds/feeds.pid "the feeds runner: bash heimdall-poc/feeds/stop.sh;"
  check_pidfile heimdall-poc/live_loop.pid "Heimdall's aircraft loop: bash heimdall-poc/day-stop.sh;"
  check_pidfile heimdall-poc/rotate-loop.pid "Heimdall's ledger rotation loop: bash heimdall-poc/rotate-loop.sh stop;"
  check_pidfile live-feeds/logs/fetch.pid "the live-feeds fetcher: bash live-feeds/stop.sh;"
  check_pidfile rotate-gateway-ledger.pid "the gateway ledger rotation loop: bash rotate-gateway-ledger.sh loop stop;"
}

check_docker
check_locks
check_pids
if [ ${#busy[@]} -gt 0 ]; then
  echo "the enclosure home $H is in use; its stores must be quiet. Stop these first:"
  printf '  %s\n' "${busy[@]}"
  exit 1
fi

PW="$H/airports-passwords.txt"
[ -f "$PW" ] || { echo "$PW not found (needed for demo-users.json)"; exit 1; }

# --- Build ------------------------------------------------------------------------

TMP=$(mktemp -d "$BUILD/starting-state.tmp-XXXXXX")
echo "building the starting state from $H"

copy() {  # path relative to $H: a file, or a directory copied whole (modes kept)
  [ -e "$H/$1" ] || { echo "missing in $H: $1"; exit 1; }
  mkdir -p "$TMP/$(dirname "$1")"
  cp -pR "$H/$1" "$TMP/$1"
}

for f in ontology.json policy.json identity.directory objects.store trust keys admin; do
  copy "airports/$f"
done
copy airports-mesh
for f in policy.json rules.json ttl.json alerts.json; do copy "heimdall-poc/state/$f"; done
copy mimir-state/policy.json
copy live-feeds/data/jobs.json
n=0
for f in "$H"/live-feeds/data/*.csv; do
  [ -f "$f" ] || continue
  copy "live-feeds/data/$(basename "$f")"; n=$((n + 1))
done
[ "$n" -gt 0 ] || { echo "no *.csv in $H/live-feeds/data"; exit 1; }
mkdir -p "$TMP/live-feeds/worker-state" "$TMP/heimdall-poc/feeds/state"
printf '{"generatedAt":0,"sources":{}}\n' > "$TMP/heimdall-poc/feeds/state/status.json"

# Leftovers inside the copied directories: backups, damaged or old ledgers, locks.
find "$TMP" \( -name '*.bak*' -o -name '*.before-live' -o -name '*.damaged*' -o -name '*.lock' \
  -o -name 'governance.ledger*' -o -name '.DS_Store' -o -name '__pycache__' -o -name '.venv' \) \
  -prune -print | while IFS= read -r p; do echo "  dropped ${p#"$TMP"/}"; rm -rf "$p"; done

# demo-users.json: the passwords go from the file to the JSON inside Python, never
# through a command line or the terminal.
python3 -I - "$PW" "$TMP/demo-users.json" <<'PY'
import json, os, sys
src, dst = sys.argv[1], sys.argv[2]
users = [("analyst", "analyst (internal): ", "Analyst (INTERNAL)"),
         ("controller", "controller (restricted): ", "Controller (RESTRICTED)")]
with open(src, encoding="utf-8") as f:
    lines = [l.rstrip("\r\n") for l in f]
out = []
for name, prefix, label in users:
    found = [l[len(prefix):] for l in lines if l.startswith(prefix)]
    if len(found) != 1 or not found[0]:
        sys.exit(f"{src}: expected one non-empty line starting '{prefix.strip()}'")
    out.append({"username": name, "password": found[0], "label": label})
fd = os.open(dst, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w", encoding="utf-8") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
PY
chmod 600 "$TMP/demo-users.json"

sha=$(git -C "$REPO" rev-parse --short HEAD)
echo "starting-state $(date -u +%Y-%m-%dT%H:%M:%SZ) $sha" > "$TMP/VERSION"

# --- Checks -----------------------------------------------------------------------

links=$(find "$TMP" -type l)
if [ -n "$links" ]; then
  echo "the starting state contains symbolic links (Home.prepare refuses them):"
  printf '%s\n' "$links" | sed "s|^$TMP/|  |"; exit 1
fi

forbidden=$(find "$TMP" \( -name mimir-signing -o -name 'grid-sam-password.txt' -o -name 'airports-passwords.txt' \
  -o -name analytics -o -name .venv -o -name mimir-models -o -name native-run -o -name logs -o -name '*.log' \) -print)
if [ -n "$forbidden" ]; then
  echo "the starting state contains what it must not:"; printf '%s\n' "$forbidden" | sed "s|^$TMP/|  |"; exit 1
fi

# Every path SuiteSpecs.make / run-native.sh reads (directories end in /), with each
# service identity's files (services/runtime/src/identity.rs).
REQUIRED=(
  airports/ontology.json airports/policy.json airports/identity.directory airports/objects.store
  airports/keys/idp.keystore airports/keys/audit.keystore airports/trust/idp.pub airports/trust/audit.pub
  airports/admin/
  mimir-state/policy.json
  heimdall-poc/state/policy.json heimdall-poc/state/rules.json heimdall-poc/state/ttl.json heimdall-poc/state/alerts.json
  heimdall-poc/feeds/state/status.json
  live-feeds/data/jobs.json live-feeds/worker-state/
  demo-users.json VERSION
)
# identity dir : service name
for pair in gateway:http-gateway feeds:picture-analyst mimir:mimir-server mimir-observer:mimir-observer \
    heimdall:heimdall-feed heimdall-advisor:heimdall-advisor heimdall-fabric:heimdall-fabric forge:forge-worker; do
  d=airports-mesh/${pair%%:*}; s=${pair#*:}
  REQUIRED+=("$d/mesh.anchor" "$d/$s.cert" "$d/$s.keystore" "$d/$s.audit.keystore")
done
# The files Forge's jobs read (relative to jobs.json).
jobfiles=$(python3 -I -c '
import json, sys
for job in json.load(open(sys.argv[1]))["jobs"]:
    src = job.get("source")
    if isinstance(src, dict) and isinstance(src.get("file"), str):
        print(src["file"])
' "$TMP/live-feeds/data/jobs.json") || { echo "cannot read the jobs in live-feeds/data/jobs.json"; exit 1; }
for f in $jobfiles; do REQUIRED+=("live-feeds/data/$f"); done
missing=()
for p in "${REQUIRED[@]}"; do
  case $p in
    */) [ -d "$TMP/$p" ] || missing+=("$p") ;;
    *) [ -f "$TMP/$p" ] || missing+=("$p") ;;
  esac
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "the starting state lacks what the services need:"; printf '  %s\n' "${missing[@]}"; exit 1
fi

# --- Swap in ----------------------------------------------------------------------

OLD=""
if [ -e "$OUT" ]; then OLD="$BUILD/starting-state.old-$$"; mv "$OUT" "$OLD"; fi
mv "$TMP" "$OUT"; TMP=""
if [ -n "$OLD" ]; then chmod -R u+rwx "$OLD" 2>/dev/null || true; rm -rf "$OLD"; fi

echo "starting state: $OUT ($(head -1 "$OUT/VERSION"))"
echo "sizes:"
for p in airports airports-mesh heimdall-poc mimir-state live-feeds demo-users.json VERSION; do
  printf '  %-16s %s\n' "$p" "$(du -sh "$OUT/$p" | cut -f1)"
done
printf '  %-16s %s\n' total "$(du -sh "$OUT" | cut -f1)"
echo "files with mode 600:"
find "$OUT" -type f -perm 600 | sed "s|^$OUT/|  |" | sort
