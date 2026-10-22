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
# folder (objects.store alone is about 250 MB); relative paths are taken from the
# caller's working directory. It is built in a temporary sibling and swapped in, so a
# failed run leaves the previous starting state as it was; one run at a time.
#
# The home's stores must be quiet: the script refuses while any container mounts the
# home (as run-native.sh; with Docker quit, set MIDGARD_DOCKER_STOPPED=1), while any
# service lock in the state directories is held by a running process, or while the
# native suite, the feeds runner, the live-feeds fetcher or the loops that write into
# the home are running. While it copies it holds the four services' directory locks
# (services/runtime/src/lock.rs), so none of them can start; afterwards it checks that
# no copied file changed during the copy and that the copied objects.store is whole
# (length-prefixed frames ending exactly at its end).
#
# Kept: the gateway's configuration, keys, identity directory and object store (no
# audit ledger: it starts fresh), every mesh identity, Heimdall's and Mimir's policies
# and rules (picture, ledgers and history start empty; the replay fills them), Forge's
# jobs and current CSVs, an empty feeds status, demo-users.json for the sign-in (built
# from airports-passwords.txt; the passwords are never printed or put on a command
# line) and VERSION. Never kept: mimir-signing/, the password files, analytics, any
# .venv, the models (the app bundles them), native-run/, logs. The starting state must
# contain no symlinks (Home.prepare refuses them).
#
# Test hook: MIDGARD_TEST_AFTER_COPY, if set together with MIDGARD_BUILD_DIR, is run with
# bash (MIDGARD_TEST_HOME set to the home) between the copy and the check that nothing
# changed. For tools/mac-app/test_make_starting_state.sh only; ignored for the default
# build directory.
set -euo pipefail
umask 077
REPO=$(cd "$(dirname "$0")/../.." && pwd)

# An absolute physical path for $1, which need not exist yet (its existing part is
# resolved, symlinks included; the rest is appended).
abspath() {
  local p=$1 rest=""
  case $p in /*) ;; *) p="$PWD/$p" ;; esac
  while [ ! -d "$p" ]; do
    rest="/$(basename "$p")$rest"; p=$(dirname "$p")
  done
  case "$rest/" in */../* | */./*) echo "cannot resolve $1 (. or .. in its missing part)" >&2; return 1 ;; esac
  echo "$(cd "$p" && pwd -P)$rest"
}

H="${1:-${MIDGARD_HOME:-$HOME/Desktop/Midgard}}"
[ -d "$H" ] || { echo "enclosure home $H not found"; exit 1; }
H=$(cd "$H" && pwd -P)
[ -d "$H/airports" ] || { echo "$H has no airports/; not an enclosure home"; exit 1; }
BUILD=$(abspath "${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}")
OUT="$BUILD/starting-state"
case "$BUILD/" in "$H"/*) echo "the build directory $BUILD is inside $H; use one outside it (storage budget)"; exit 1 ;; esac

TMP=""; TMPF=""; BLOCK=""; HELD=()
cleanup() {
  local l
  [ -z "$TMPF" ] || rm -f "$TMPF"
  if [ -n "$TMP" ] && [ -d "$TMP" ]; then chmod -R u+rwx "$TMP" 2>/dev/null || true; rm -rf "$TMP"; fi
  for l in ${HELD[@]+"${HELD[@]}"}; do
    if grep -qx "pid=$$" "$l" 2>/dev/null; then rm -f "$l"; fi
  done
  [ -z "$BLOCK" ] || rm -rf "$BLOCK"
}
trap cleanup EXIT

alive() {  # pid
  case $1 in '' | *[!0-9]*) return 1 ;; esac
  kill -0 "$1" 2>/dev/null || ps -p "$1" > /dev/null 2>&1
}

# Run a command, killing it after $1 seconds (macOS has no timeout(1)).
with_timeout() {
  local t=$1 p w rc=0; shift
  "$@" & p=$!
  ( sleep "$t"; kill "$p" 2>/dev/null ) > /dev/null 2>&1 & w=$!
  wait "$p" || rc=$?
  kill "$w" 2>/dev/null || true; wait "$w" 2>/dev/null || true
  return "$rc"
}
dk() { with_timeout "${MIDGARD_DOCKER_TIMEOUT:-20}" docker "$@"; }

# --- Safety: the home must not be in use ------------------------------------------

# As run-native.sh: refuse while any container can hold the home's directories, and
# fail closed if Docker is installed but cannot be queried.
docker_refuse() { echo "cannot query Docker; refusing to read the enclosure's stores"; exit 1; }
# Print the names of running containers with a mount at, under or above $H; fails if
# Docker cannot be queried (a container may vanish between ps and inspect).
docker_conflicts() {
  local ids line name src m
  ids=$(dk ps -q) || return 1
  [ -n "$ids" ] || return 0
  # shellcheck disable=SC2086  # one argument per container id
  dk inspect --format '{{.Name}}{{range .Mounts}}|{{.Source}}{{end}}' $ids > "$TMPF" || return 1
  while IFS= read -r line; do
    name=${line%%|*}; name=${name#/}
    [ "$line" != "${line#*|}" ] || continue
    line=${line#*|}
    while :; do
      src=${line%%|*}
      if [ -n "$src" ]; then
        # The source as given and, if it exists here, its physical path ($H already is
        # one); a mount of / becomes "", a prefix of every path.
        for m in "${src%/}" "$( (cd "$src" 2>/dev/null && pwd -P) || echo "$src")"; do
          m=${m%/}
          case $H/ in "$m"/*) echo "$name"; break 2 ;; esac   # $H is at or under the mount
          case $m/ in "$H"/*) echo "$name"; break 2 ;; esac   # the mount is under $H
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
  if ! command -v docker > /dev/null; then
    echo "docker is not on PATH; skipping the Docker check"; return 0
  fi
  local err names
  if ! err=$(dk ps -q 2>&1 > /dev/null); then
    case $err in
      *"Cannot connect to the Docker daemon"* | *"docker daemon running"* | *"daemon is not running"*)
        echo "Docker is installed but not running. If no Midgard containers can be running" \
          "(Docker Desktop is quit), re-run with MIDGARD_DOCKER_STOPPED=1."; exit 1 ;;
    esac
    echo "$err"; docker_refuse
  fi
  names=$(dk ps --format '{{.Names}}') || docker_refuse
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

# Each service's state directory and lock file (as run-native.sh's lock_path).
LOCKS=(airports:midgard.lock mimir-state:mimir.lock heimdall-poc/state:feed.lock live-feeds/worker-state:worker.lock)
lock_field() { sed -n "s/^$1=//p" "$2" | head -1; }

busy=()
# Locks: "pid=N" and "os=…" lines. A lock written by a container (os other than macos)
# names a pid inside it, meaningless here; the Docker check covers containers.
check_locks() {
  local spec dir lock pid os
  for spec in "${LOCKS[@]}"; do
    dir="$H/${spec%%:*}"
    for lock in "$dir"/*.lock; do
      [ -e "$lock" ] || continue
      if [ ! -r "$lock" ]; then busy+=("${lock#"$H"/} cannot be read; check what holds it"); continue; fi
      pid=$(lock_field pid "$lock" | tr -cd '0-9')
      os=$(lock_field os "$lock")
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
  [ -e "$H/$1" ] || return 0
  if [ ! -r "$H/$1" ]; then busy+=("$1 cannot be read; check whether $2"); return 0; fi
  pid=$(tr -cd '0-9' < "$H/$1")
  if alive "$pid"; then busy+=("$2 is running (pid $pid, from $1)"); fi
}
check_pids() {
  local f
  for f in "$H"/native-run/*.pid; do
    [ -e "$f" ] || continue
    check_pidfile "${f#"$H"/}" "the native suite (run tools/mac-app/run-native.sh stop)"
  done
  check_pidfile heimdall-poc/feeds/feeds.pid "the feeds runner (bash heimdall-poc/feeds/stop.sh)"
  check_pidfile heimdall-poc/live_loop.pid "Heimdall's aircraft loop (bash heimdall-poc/day-stop.sh)"
  check_pidfile heimdall-poc/rotate-loop.pid "Heimdall's ledger rotation loop (bash heimdall-poc/rotate-loop.sh stop)"
  check_pidfile live-feeds/logs/fetch.pid "the live-feeds fetcher (bash live-feeds/stop.sh)"
  check_pidfile rotate-gateway-ledger.pid "the gateway ledger rotation loop (bash rotate-gateway-ledger.sh loop stop)"
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

# --- One run at a time; leftovers of interrupted runs -----------------------------

mkdir -p "$BUILD"
lockdir="$BUILD/.make-starting-state.lock"
if ! mkdir "$lockdir" 2>/dev/null; then
  pid=$(tr -cd '0-9' < "$lockdir/pid" 2>/dev/null) || pid=
  if [ -z "$pid" ] || alive "$pid"; then
    echo "another make-starting-state.sh is running (${pid:+pid $pid, }$lockdir); if none is, remove $lockdir"
    exit 1
  fi
  rm -rf "$lockdir"
  mkdir "$lockdir" || { echo "another make-starting-state.sh took $lockdir"; exit 1; }
fi
BLOCK=$lockdir
echo $$ > "$BLOCK/pid"
for d in "$BUILD"/starting-state.tmp-* "$BUILD"/starting-state.old-*; do
  [ -e "$d" ] || continue
  chmod -R u+rwx "$d" 2>/dev/null || true; rm -rf "$d"; echo "removed ${d#"$BUILD"/} (left by an earlier run)"
done

# --- Hold the services' locks while copying ---------------------------------------

# This process's start time as lock.rs records it on macOS (proc_pidinfo
# PROC_PIDTBSDINFO: pbi_start_tvsec * 1e6 + pbi_start_tvusec), so a service finding the
# lock after this script was killed can tell it is stale.
start=$(python3 -I -c '
import ctypes, struct, sys
lib = ctypes.CDLL("/usr/lib/libproc.dylib")
lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
buf = ctypes.create_string_buffer(136)  # sizeof(struct proc_bsdinfo)
if lib.proc_pidinfo(int(sys.argv[1]), 3, 0, buf, 136) != 136:
    print("unknown")
else:
    sec, usec = struct.unpack_from("<QQ", buf, 120)
    print(sec * 1000000 + usec)
' $$) || start=unknown
for spec in "${LOCKS[@]}"; do
  dir="$H/${spec%%:*}"; lock="$dir/${spec#*:}"
  [ -d "$dir" ] || { echo "missing in $H: ${spec%%:*}/"; exit 1; }
  if [ -e "$lock" ]; then
    # check_locks refused live macOS holders; what is left was written by a container
    # or a dead process. A foreign lock already keeps every native service out.
    if [ "$(lock_field os "$lock")" != macos ]; then continue; fi
    # Its holder may have started since check_locks looked: check again just before.
    pid=$(lock_field pid "$lock" | tr -cd '0-9')
    if alive "$pid"; then
      echo "${lock#"$H"/} is now held by running pid $pid ($(ps -p "$pid" -o comm= 2>/dev/null || echo '?')); stop that service and re-run"
      exit 1
    fi
    rm -f "$lock"; echo "took over the stale lock ${lock#"$H"/}"
  fi
  if ( set -C; printf 'make-starting-state\npid=%s\nstart=%s\nos=macos\n' $$ "$start" > "$lock" ) 2> /dev/null; then
    HELD+=("$lock")
  else
    echo "${lock#"$H"/} was just taken by another process; stop it and re-run"; exit 1
  fi
done

# --- Build ------------------------------------------------------------------------

TMP=$(mktemp -d "$BUILD/starting-state.tmp-XXXXXX")
echo "building the starting state from $H"

SOURCES=()
for f in ontology.json policy.json identity.directory objects.store trust keys admin; do SOURCES+=("airports/$f"); done
SOURCES+=(airports-mesh)
for f in policy.json rules.json ttl.json alerts.json; do SOURCES+=("heimdall-poc/state/$f"); done
SOURCES+=(mimir-state/policy.json live-feeds/data/jobs.json)
n=0
for f in "$H"/live-feeds/data/*.csv; do
  [ -f "$f" ] || continue
  SOURCES+=("live-feeds/data/$(basename "$f")"); n=$((n + 1))
done
[ "$n" -gt 0 ] || { echo "no *.csv in $H/live-feeds/data"; exit 1; }
for s in "${SOURCES[@]}"; do [ -e "$H/$s" ] || { echo "missing in $H: $s"; exit 1; }; done

# Size, modification time and inode of every file to copy, to detect a writer.
snapshot() {
  python3 -I -c '
import os, sys
root = sys.argv[1]
for rel in sys.argv[2:]:
    top = os.path.join(root, rel)
    if os.path.isdir(top):
        paths = [os.path.join(d, f) for d, _, files in os.walk(top) for f in files]
    else:
        paths = [top]
    for p in paths:
        st = os.lstat(p)
        print(os.path.relpath(p, root), st.st_size, st.st_mtime_ns, st.st_ino)
' "$H" "${SOURCES[@]}" | sort
}
before=$(snapshot)
for s in "${SOURCES[@]}"; do
  mkdir -p "$TMP/$(dirname "$s")"
  cp -pRX "$H/$s" "$TMP/$s"   # modes kept (keystores stay 0600), no extended attributes
done
# The test hook runs only for an explicitly chosen build directory, never the default.
if [ -n "${MIDGARD_TEST_AFTER_COPY:-}" ] && [ -n "${MIDGARD_BUILD_DIR:-}" ]; then
  MIDGARD_TEST_HOME="$H" bash -c "$MIDGARD_TEST_AFTER_COPY"
fi
after=$(snapshot)
if [ "$before" != "$after" ]; then
  echo "files changed in $H while they were copied; something is still writing. Changed:"
  diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") | sed -n 's/^[<>] \(.*\) [0-9]* [0-9]* [0-9]*$/  \1/p' | sort -u
  exit 1
fi

# The object store as DurableObjectStore::open reads it (services/fabric/src/durable.rs):
# frames of a little-endian u32 length and that many bytes, ending exactly at the end.
python3 -I -c '
import os, sys
path = sys.argv[1]
size = os.path.getsize(path)
pos = frames = 0
with open(path, "rb") as f:
    while pos < size:
        head = f.read(4)
        if len(head) < 4:
            sys.exit(f"objects.store is damaged: truncated frame header at byte {pos} (after {frames} frames)")
        n = int.from_bytes(head, "little")
        if pos + 4 + n > size:
            sys.exit(f"objects.store is damaged: frame {frames} at byte {pos} runs {pos + 4 + n - size} bytes past the end")
        pos += 4 + n
        f.seek(pos)
        frames += 1
print(f"objects.store: {frames} frames, {size} bytes, whole")
' "$TMP/airports/objects.store"

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

ver=$(git -C "$REPO" describe --always --dirty 2> /dev/null) || ver=nogit
echo "starting-state $(date -u +%Y-%m-%dT%H:%M:%SZ) $ver" > "$TMP/VERSION"

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
while IFS= read -r f; do
  [ -z "$f" ] || REQUIRED+=("live-feeds/data/$f")
done <<< "$jobfiles"
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
if ! mv "$TMP" "$OUT"; then
  [ -z "$OLD" ] || mv "$OLD" "$OUT"
  echo "could not move the new starting state into place; the previous one is kept"; exit 1
fi
TMP=""
if [ -n "$OLD" ]; then chmod -R u+rwx "$OLD" 2>/dev/null || true; rm -rf "$OLD"; fi

echo "starting state: $OUT ($(head -1 "$OUT/VERSION"))"
echo "sizes:"
for p in airports airports-mesh heimdall-poc mimir-state live-feeds demo-users.json VERSION; do
  printf '  %-16s %s\n' "$p" "$(du -sh "$OUT/$p" | cut -f1)"
done
printf '  %-16s %s\n' total "$(du -sh "$OUT" | cut -f1)"
echo "files with mode 600:"
find "$OUT" -type f -perm 600 | sed "s|^$OUT/|  |" | sort
