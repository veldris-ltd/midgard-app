#!/usr/bin/env bash
# Tests for make-starting-state.sh against a fake enclosure home built in a temporary
# directory (small fake files with the right names and modes, dummy passwords, paths
# with spaces). Never touches ~/Desktop/Midgard or the real Docker: a stub docker is
# first on PATH. Cleans up after itself.
#
#   bash tools/mac-app/test_make_starting_state.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
SCRIPT="$PWD/tools/mac-app/make-starting-state.sh"
VER=$(git describe --always --dirty 2> /dev/null) || VER=nogit
T=$(mktemp -d); T=$(cd "$T" && pwd -P)
SLEEPER=""
cleanup() {
  [ -z "$SLEEPER" ] || kill "$SLEEPER" 2>/dev/null || true
  chmod -R u+rwx "$T" 2>/dev/null || true; rm -rf "$T"
}
trap cleanup EXIT

H="$T/home dir"; B="$T/Application Support/build"; OUT="$B/starting-state"
PW1='dummy analyst pw: "quoted" \ back'
PW2='dummy-controller-pw'
fails=0; passes=0
ok() { passes=$((passes + 1)); }
bad() { echo "FAIL: $*"; fails=$((fails + 1)); }
check() {  # description, command...
  local d=$1; shift
  if "$@" > /dev/null 2>&1; then ok; else bad "$d"; fi
}
mode() { stat -f %Lp "$1"; }

# A stub docker, first on PATH; $T/docker-mode picks its behaviour.
mkdir -p "$T/bin"
cat > "$T/bin/docker" <<'STUB'
#!/bin/bash
mode=$(cat "$STUB_DIR/docker-mode" 2>/dev/null || echo clear)
if [ "$mode" = down ]; then
  echo "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?" >&2; exit 1
fi
case "$1 $2" in
  "ps -q") echo abc123 ;;
  "ps --format") echo other-box ;;
  inspect*) echo "/other-box|/var/lib/x|$(cat "$STUB_DIR/docker-mount")" ;;
  *) exit 1 ;;
esac
STUB
chmod +x "$T/bin/docker"
export STUB_DIR="$T" PATH="$T/bin:$PATH"
echo clear > "$T/docker-mode"; echo /opt/unrelated > "$T/docker-mount"

mkf() {  # path relative to $H, mode, [content]
  mkdir -p "$(dirname "$H/$1")"
  printf '%s' "${3:-fake $1}" > "$H/$1"
  chmod "$2" "$H/$1"
}
store() {  # path; frames of these lengths, then optionally N extra bytes ("+N")
  python3 -I -c '
import sys
out = b""
for a in sys.argv[2:]:
    if a.startswith("+"):
        out += b"x" * int(a[1:])
    else:
        n = int(a); out += n.to_bytes(4, "little") + b"s" * n
open(sys.argv[1], "wb").write(out)' "$@"
  chmod 600 "$1"
}
make_home() {
  chmod -R u+rwx "$H" 2>/dev/null || true; rm -rf "$H"; mkdir -p "$H"
  mkf airports/ontology.json 644 '{}'
  mkf airports/policy.json 644 '{}'
  mkf airports/identity.directory 600
  mkdir -p "$H/airports"; store "$H/airports/objects.store" 40 7 1000 0 3
  mkf airports/governance.ledger 600
  mkf airports/governance.ledger.damaged-20261016 600
  mkf airports/ontology.json.bak-20261020 644
  mkf airports/policy.json.before-live 644
  mkf airports/.DS_Store 644
  mkf airports/keys/idp.keystore 600
  mkf airports/keys/audit.keystore 600
  mkf airports/trust/idp.pub 644
  mkf airports/trust/audit.pub 644
  mkf airports/admin/passkeys.journal 600 ''
  mkf airports/admin/roles.journal 600 ''
  mkf airports/admin/roles.journal.bak-1 600
  mkf airports/admin/pending.journal 600 ''
  mkf airports/admin/privilege/actions.journal 600 ''
  mkf airports/admin/privilege/audit.ledger 600 ''
  # A lock left by a process that has exited.
  mkf airports/midgard.lock 644 "$(printf 'http-gateway\npid=999999\nstart=1\nos=macos\n')"
  local pair d s
  for pair in gateway:http-gateway feeds:picture-analyst mimir:mimir-server mimir-observer:mimir-observer \
      heimdall:heimdall-feed heimdall-advisor:heimdall-advisor heimdall-fabric:heimdall-fabric \
      forge:forge-worker mimir-probe:mimir-probe; do
    d=airports-mesh/${pair%%:*}; s=${pair#*:}
    mkf "$d/mesh.anchor" 644; mkf "$d/$s.cert" 644
    mkf "$d/$s.keystore" 600; mkf "$d/$s.audit.keystore" 600
  done
  mkf airports-mesh/feeds/adsb-lol.cert 644; mkf airports-mesh/feeds/adsb-lol.keystore 600
  mkf airports-mesh/.DS_Store 644
  mkf airports-mesh/gateway/http-gateway.keystore.bak-20261016 600
  for f in policy.json rules.json ttl.json alerts.json; do mkf "heimdall-poc/state/$f" 644 '{}'; done
  mkf heimdall-poc/state/rules.json.bak-20261018 644
  mkf heimdall-poc/state/feed.ledger 600
  mkf heimdall-poc/state/picture.sealed 600
  mkf heimdall-poc/state/alerts.sealed 600
  mkf heimdall-poc/state/history/1.sealed 600
  # Written by a container: its pid means nothing on this Mac.
  mkf heimdall-poc/state/feed.lock 644 "$(printf 'heimdall-feed\npid=1\nstart=1\nos=linux\n')"
  mkf heimdall-poc/feeds/state/status.json 644 '{"generatedAt":5,"sources":{"x":{}}}'
  mkf heimdall-poc/feeds/state/airports.json 644
  mkf heimdall-poc/feeds/feeds.log 644
  mkf heimdall-poc/analytics/secret 600
  mkf heimdall-poc/.venv/bin/python 755
  mkf mimir-state/policy.json 644 '{}'
  mkf mimir-state/mimir.ledger 600
  mkf live-feeds/data/jobs.json 644 '{"version":1,"jobs":[{"id":"a","source":{"file":"aircraft.csv"}},{"id":"w","source":{"file":"weather.csv"}}]}'
  mkf live-feeds/data/aircraft.csv 644 'key'
  mkf live-feeds/data/weather.csv 644 'key'
  mkf live-feeds/data/aircraft-sources.json 644
  mkf live-feeds/worker-state/worker.ledger 600
  mkf live-feeds/worker-state/status.json 644
  mkf live-feeds/logs/fetch.log 644
  mkf mimir-signing/model-signing.key 600
  mkf mimir-models/granite/model.gguf 644
  mkf native-run/gateway.log 644
  mkf grid-sam-password.txt 600
  mkf airports-passwords.txt 600 "$(printf 'analyst (internal): %s\ncontroller (restricted): %s\n' "$PW1" "$PW2")"
}
run() {  # runs the builder on the fake home; output in $T/log
  MIDGARD_DOCKER_STOPPED=1 MIDGARD_BUILD_DIR="$B" bash "$SCRIPT" "$H" > "$T/log" 2>&1
}
no_passwords_in_log() { ! grep -qF -e "$PW1" -e "$PW2" -e dummy "$T/log"; }
# No lock of ours left in the home, no build lock, no temporary siblings.
clean_after() {  # description
  check "$1: no lock of ours left" test -z "$(grep -l 'make-starting-state' "$H"/airports/*.lock "$H"/mimir-state/*.lock \
    "$H"/heimdall-poc/state/*.lock "$H"/live-feeds/worker-state/*.lock 2>/dev/null)"
  check "$1: only starting-state in the build dir" test "$(ls -A "$B")" = starting-state
}

# --- 1. A clean build ---------------------------------------------------------
make_home
cat > "$T/hook.sh" <<'HOOK'
mkdir -p "$STUB_DIR/seen"
for l in airports/midgard.lock mimir-state/mimir.lock heimdall-poc/state/feed.lock live-feeds/worker-state/worker.lock; do
  cp "$MIDGARD_TEST_HOME/$l" "$STUB_DIR/seen/$(basename "$l")" 2>/dev/null || true
done
echo "$PPID" > "$STUB_DIR/seen/ppid"
ps -p "$PPID" -o lstart= > "$STUB_DIR/seen/lstart"
HOOK
if MIDGARD_TEST_AFTER_COPY="bash '$T/hook.sh'" run; then ok; else bad "build failed:"; sed 's/^/    /' "$T/log"; fi
for p in airports/ontology.json airports/policy.json airports/identity.directory airports/objects.store \
    airports/keys/idp.keystore airports/keys/audit.keystore airports/trust/idp.pub airports/trust/audit.pub \
    airports/admin/passkeys.journal airports/admin/privilege/audit.ledger \
    airports-mesh/gateway/http-gateway.keystore airports-mesh/feeds/adsb-lol.cert \
    airports-mesh/mimir-probe/mesh.anchor \
    heimdall-poc/state/policy.json heimdall-poc/state/rules.json heimdall-poc/state/ttl.json \
    heimdall-poc/state/alerts.json heimdall-poc/feeds/state/status.json mimir-state/policy.json \
    live-feeds/data/jobs.json live-feeds/data/aircraft.csv live-feeds/data/weather.csv \
    demo-users.json VERSION; do
  check "included: $p" test -f "$OUT/$p"
done
check "included: empty live-feeds/worker-state/" test -d "$OUT/live-feeds/worker-state"
check "worker-state is empty" test -z "$(ls -A "$OUT/live-feeds/worker-state")"
for p in airports/governance.ledger airports/governance.ledger.damaged-20261016 \
    airports/ontology.json.bak-20261020 airports/policy.json.before-live airports/midgard.lock \
    airports/.DS_Store airports-mesh/.DS_Store airports-mesh/gateway/http-gateway.keystore.bak-20261016 \
    airports/admin/roles.journal.bak-1 heimdall-poc/state/rules.json.bak-20261018 heimdall-poc/state/feed.ledger \
    heimdall-poc/state/picture.sealed heimdall-poc/state/alerts.sealed heimdall-poc/state/history \
    heimdall-poc/state/feed.lock heimdall-poc/feeds/state/airports.json heimdall-poc/feeds/feeds.log \
    heimdall-poc/analytics heimdall-poc/.venv mimir-state/mimir.ledger live-feeds/data/aircraft-sources.json \
    live-feeds/logs mimir-signing mimir-models native-run grid-sam-password.txt airports-passwords.txt; do
  check "excluded: $p" test ! -e "$OUT/$p"
done
check "top level holds only the expected parts" test \
  "$(ls -A "$OUT" | tr '\n' ' ')" = "VERSION airports airports-mesh demo-users.json heimdall-poc live-feeds mimir-state "
check "output is 0700" test "$(mode "$OUT")" = 700
check "keystore stays 600" test "$(mode "$OUT/airports/keys/idp.keystore")" = 600
check "mesh keystore stays 600" test "$(mode "$OUT/airports-mesh/forge/forge-worker.audit.keystore")" = 600
check "objects.store stays 600" test "$(mode "$OUT/airports/objects.store")" = 600
check "objects.store copied whole" cmp "$H/airports/objects.store" "$OUT/airports/objects.store"
check "cert stays 644" test "$(mode "$OUT/airports-mesh/gateway/http-gateway.cert")" = 644
check "demo-users.json is 600" test "$(mode "$OUT/demo-users.json")" = 600
check "demo-users.json content" python3 -I -c '
import json, sys
want = [{"username": "analyst", "password": sys.argv[2], "label": "Analyst (INTERNAL)"},
        {"username": "controller", "password": sys.argv[3], "label": "Controller (RESTRICTED)"}]
sys.exit(json.load(open(sys.argv[1])) != want)' "$OUT/demo-users.json" "$PW1" "$PW2"
check "status.json is empty" test "$(cat "$OUT/heimdall-poc/feeds/state/status.json")" = '{"generatedAt":0,"sources":{}}'
check "VERSION line" grep -qxE "starting-state [0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z $VER" "$OUT/VERSION"
check "VERSION is one line" test "$(wc -l < "$OUT/VERSION" | tr -d ' ')" = 1
check "no symlinks" test -z "$(find "$OUT" -type l)"
check "store frames reported" grep -q '^objects.store: 5 frames, 1070 bytes, whole$' "$T/log"
check "sizes printed" grep -q '^  total' "$T/log"
check "600 files listed" grep -q '^  demo-users.json$' "$T/log"
check "no password in the output" no_passwords_in_log
clean_after "clean build"
# The locks held during the copy, in lock.rs's format, naming this script's process.
pid=$(cat "$T/seen/ppid")
for l in midgard.lock mimir.lock worker.lock; do
  check "held $l during the copy" python3 -I -c '
import sys, time, email.utils
lines = open(sys.argv[1]).read().split("\n")
assert lines[0] == "make-starting-state" and lines[1] == "pid=" + sys.argv[2] and lines[3] == "os=macos", lines
assert lines[4:] == [""], lines
start = int(lines[2][len("start="):])
lstart = time.mktime(time.strptime(open(sys.argv[3]).read().strip(), "%a %b %d %H:%M:%S %Y"))
assert abs(start / 1e6 - lstart) < 2, (start, lstart)' "$T/seen/$l" "$pid" "$T/seen/lstart"
done
check "a container's lock is left alone" grep -qx 'os=linux' "$T/seen/feed.lock"
check "the container's lock is still there" grep -qx 'os=linux' "$H/heimdall-poc/state/feed.lock"
check "the dead process's lock was taken over and released" test ! -e "$H/airports/midgard.lock"

# --- 2. A second run replaces the first, after an interrupted run --------------
echo marker > "$OUT/stale-file"
mkdir -p "$B/starting-state.tmp-abc123/x" "$B/starting-state.old-42"
mkdir "$B/.make-starting-state.lock"; echo 999999 > "$B/.make-starting-state.lock/pid"
make_home
if run; then ok; else bad "rebuild failed"; sed 's/^/    /' "$T/log"; fi
check "rebuild replaced the old output" test ! -e "$OUT/stale-file"
check "a dead run's build lock is taken over" grep -q 'removed starting-state.tmp-abc123' "$T/log"
clean_after "rebuild"
before=$(cat "$OUT/VERSION")

refuses() {  # description, expected text in the log
  if run; then bad "$1: did not refuse"; return; fi
  if grep -qF -- "$2" "$T/log"; then ok; else bad "$1: message lacks '$2'"; sed 's/^/    /' "$T/log"; fi
  check "$1: previous output kept" test "$(cat "$OUT/VERSION" 2>/dev/null)" = "$before"
  check "$1: no password in the output" no_passwords_in_log
  clean_after "$1"
}

# --- 3. Relative paths and the build directory ---------------------------------
make_home
if (cd "$T" && MIDGARD_DOCKER_STOPPED=1 MIDGARD_BUILD_DIR="rel build/x" bash "$SCRIPT" "home dir" > "$T/log" 2>&1); then ok
else bad "relative paths failed"; sed 's/^/    /' "$T/log"; fi
check "relative build dir is taken from the caller's directory" test -f "$T/rel build/x/starting-state/VERSION"
check "relative home is taken from the caller's directory" grep -qF "building the starting state from $H" "$T/log"
if (cd "$T" && MIDGARD_DOCKER_STOPPED=1 MIDGARD_HOME="home dir" MIDGARD_BUILD_DIR="home dir/cache" bash "$SCRIPT" > "$T/log" 2>&1); then
  bad "build dir inside the home was accepted"
else
  check "build dir inside the home refused" grep -q 'is inside' "$T/log"
fi
check "nothing created inside the home" test ! -e "$H/cache"

# --- 4. Refusals while the home is in use -------------------------------------
sleep 300 & SLEEPER=$!
make_home
printf 'mimir-server\npid=%s\nstart=1\nos=macos\n' "$SLEEPER" > "$H/mimir-state/mimir.lock"
refuses "live lock" "mimir-state/mimir.lock is held by running pid $SLEEPER"
check "live lock: the service's lock is left" grep -qx "pid=$SLEEPER" "$H/mimir-state/mimir.lock"
rm -f "$H/mimir-state/mimir.lock"

echo x > "$H/live-feeds/worker-state/worker.lock"; chmod 000 "$H/live-feeds/worker-state/worker.lock"
refuses "unreadable lock" "worker.lock cannot be read"
chmod 644 "$H/live-feeds/worker-state/worker.lock"; rm -f "$H/live-feeds/worker-state/worker.lock"

printf '  %s \n' "$SLEEPER" > "$H/heimdall-poc/feeds/feeds.pid"
refuses "feeds runner (pid with whitespace)" "the feeds runner"
rm -f "$H/heimdall-poc/feeds/feeds.pid"

echo "$SLEEPER" > "$H/native-run/heimdall.pid"
refuses "native suite" "run-native.sh stop"
echo 999999 > "$H/native-run/heimdall.pid"   # dead: not a refusal
if run; then ok; else bad "a dead native-run pid refused"; sed 's/^/    /' "$T/log"; fi
before=$(cat "$OUT/VERSION")
rm -f "$H/native-run/heimdall.pid"

mkdir "$B/.make-starting-state.lock"; echo "$SLEEPER" > "$B/.make-starting-state.lock/pid"
if run; then bad "concurrent run: did not refuse"
else check "concurrent run refused" grep -q "another make-starting-state.sh is running (pid $SLEEPER" "$T/log"; fi
check "concurrent run: previous output kept" test "$(cat "$OUT/VERSION")" = "$before"
check "concurrent run: its build lock is left" test "$(cat "$B/.make-starting-state.lock/pid")" = "$SLEEPER"
rm -rf "$B/.make-starting-state.lock"
kill "$SLEEPER" 2>/dev/null || true; wait "$SLEEPER" 2>/dev/null || true; SLEEPER=""

# --- 5. Docker (the stub) ------------------------------------------------------
dockrun() { MIDGARD_BUILD_DIR="$B" bash "$SCRIPT" "$H" > "$T/log" 2>&1; }
make_home
echo "$H" > "$T/docker-mount"
if dockrun; then bad "a container mounting the home was not refused"
else check "container mounting the home refused" grep -q '^  other-box$' "$T/log"; fi
echo "$(dirname "$T")" > "$T/docker-mount"
if dockrun; then bad "a container mounting a parent was not refused"
else check "container mounting a parent refused" grep -q '^  other-box$' "$T/log"; fi
echo "$H/airports" > "$T/docker-mount"
if dockrun; then bad "a container mounting a subdirectory was not refused"
else check "container mounting a subdirectory refused" grep -q '^  other-box$' "$T/log"; fi
echo "${H}2" > "$T/docker-mount"
if dockrun; then ok; else bad "a container mounting a sibling with a common prefix refused"; sed 's/^/    /' "$T/log"; fi
echo down > "$T/docker-mode"
if dockrun; then bad "Docker down was not refused"
else check "Docker down refused, naming MIDGARD_DOCKER_STOPPED" grep -q 'MIDGARD_DOCKER_STOPPED=1' "$T/log"; fi
echo clear > "$T/docker-mode"; echo /opt/unrelated > "$T/docker-mount"
if PATH=/usr/bin:/bin:/usr/sbin:/sbin MIDGARD_BUILD_DIR="$B" bash "$SCRIPT" "$H" > "$T/log" 2>&1; then
  check "no docker on PATH: says the check is skipped" grep -q 'docker is not on PATH; skipping' "$T/log"
else bad "no docker on PATH failed"; sed 's/^/    /' "$T/log"; fi
before=$(cat "$OUT/VERSION")
clean_after "docker"

# --- 6. Copy integrity ---------------------------------------------------------
make_home
store "$H/airports/objects.store" 40 7 1000 +2
refuses "truncated store header" "truncated frame header at byte 1059 (after 3 frames)"
store "$H/airports/objects.store" 40 7 1000
python3 -I -c 'import sys; p=sys.argv[1]; d=open(p,"rb").read(); open(p,"wb").write(d[:-10])' "$H/airports/objects.store"
refuses "truncated store body" "runs 10 bytes past the end"
make_home

if MIDGARD_TEST_AFTER_COPY='echo more >> "$MIDGARD_TEST_HOME/airports/policy.json"' run; then
  bad "a file changed during the copy was not refused"
else
  check "changed during copy refused" grep -qx '  airports/policy.json' "$T/log"
  check "changed during copy: previous output kept" test "$(cat "$OUT/VERSION")" = "$before"
  clean_after "changed during copy"
fi
make_home
if MIDGARD_TEST_AFTER_COPY='touch -t 202001010000 "$MIDGARD_TEST_HOME/airports-mesh/forge/mesh.anchor"' run; then
  bad "a file touched during the copy was not refused"
else
  check "touched during copy refused" grep -qx '  airports-mesh/forge/mesh.anchor' "$T/log"
fi

# The hook is ignored without an explicit MIDGARD_BUILD_DIR (the default build dir is
# under $HOME, pointed at the temp dir here).
make_home
if HOME="$T/user" MIDGARD_DOCKER_STOPPED=1 MIDGARD_TEST_AFTER_COPY='echo more >> "$MIDGARD_TEST_HOME/airports/policy.json"' \
    bash "$SCRIPT" "$H" > "$T/log" 2>&1; then
  check "hook ignored for the default build dir" test -f "$T/user/Library/Caches/midgard-build/starting-state/VERSION"
  check "hook did not run for the default build dir" test "$(cat "$H/airports/policy.json")" = '{}'
else bad "the hook ran for the default build dir"; sed 's/^/    /' "$T/log"; fi

# --- 7. Bad sources -----------------------------------------------------------
make_home
ln -s ../gateway/mesh.anchor "$H/airports-mesh/forge/extra.anchor"
refuses "symlink" "contains symbolic links"
rm -f "$H/airports-mesh/forge/extra.anchor"

printf 'analyst (internal): %s\n' "$PW1" > "$H/airports-passwords.txt"
refuses "missing password line" "controller (restricted)"
make_home

rm -f "$H/airports-mesh/heimdall-fabric/heimdall-fabric.cert"
refuses "missing identity file" "airports-mesh/heimdall-fabric/heimdall-fabric.cert"
make_home

rm -f "$H/live-feeds/data/weather.csv"
refuses "missing job file" "live-feeds/data/weather.csv"
make_home

rm -f "$H/heimdall-poc/state/rules.json"
refuses "missing rules" "heimdall-poc/state/rules.json"

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
