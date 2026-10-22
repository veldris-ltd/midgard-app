#!/usr/bin/env bash
# Tests for make-starting-state.sh against a fake enclosure home built in a temporary
# directory (small fake files with the right names and modes, dummy passwords). Never
# touches ~/Desktop/Midgard or Docker (MIDGARD_DOCKER_STOPPED=1). Cleans up after itself.
#
#   bash tools/mac-app/test_make_starting_state.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
SCRIPT="$PWD/tools/mac-app/make-starting-state.sh"
T=$(mktemp -d)
SLEEPER=""
cleanup() {
  [ -z "$SLEEPER" ] || kill "$SLEEPER" 2>/dev/null || true
  chmod -R u+rwx "$T" 2>/dev/null || true; rm -rf "$T"
}
trap cleanup EXIT

H="$T/home"; B="$T/build"; OUT="$B/starting-state"
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

mkf() {  # path relative to $H, mode, [content]
  mkdir -p "$(dirname "$H/$1")"
  printf '%s' "${3:-fake $1}" > "$H/$1"
  chmod "$2" "$H/$1"
}
make_home() {
  rm -rf "$H"; mkdir -p "$H"
  mkf airports/ontology.json 644 '{}'
  mkf airports/policy.json 644 '{}'
  mkf airports/identity.directory 600
  mkf airports/objects.store 600
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
  # Strays inside directories copied whole.
  mkf airports-mesh/.DS_Store 644
  mkf airports-mesh/gateway/http-gateway.keystore.bak-20261016 600
  mkf airports/admin/roles.journal.bak-1 600
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

# --- 1. A clean build ---------------------------------------------------------
make_home
if run; then ok; else bad "build failed:"; sed 's/^/    /' "$T/log"; fi
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
check "keystore stays 600" test "$(mode "$OUT/airports/keys/idp.keystore")" = 600
check "mesh keystore stays 600" test "$(mode "$OUT/airports-mesh/forge/forge-worker.audit.keystore")" = 600
check "objects.store stays 600" test "$(mode "$OUT/airports/objects.store")" = 600
check "cert stays 644" test "$(mode "$OUT/airports-mesh/gateway/http-gateway.cert")" = 644
check "demo-users.json is 600" test "$(mode "$OUT/demo-users.json")" = 600
check "demo-users.json content" python3 -I -c '
import json, sys
want = [{"username": "analyst", "password": sys.argv[2], "label": "Analyst (INTERNAL)"},
        {"username": "controller", "password": sys.argv[3], "label": "Controller (RESTRICTED)"}]
sys.exit(json.load(open(sys.argv[1])) != want)' "$OUT/demo-users.json" "$PW1" "$PW2"
check "status.json is empty" test "$(cat "$OUT/heimdall-poc/feeds/state/status.json")" = '{"generatedAt":0,"sources":{}}'
sha=$(git rev-parse --short HEAD)
check "VERSION line" grep -qE "^starting-state [0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z $sha\$" "$OUT/VERSION"
check "VERSION is one line" test "$(wc -l < "$OUT/VERSION" | tr -d ' ')" = 1
check "no symlinks" test -z "$(find "$OUT" -type l)"
check "sizes printed" grep -q '^  total' "$T/log"
check "600 files listed" grep -q '^  demo-users.json$' "$T/log"
check "no password in the output" no_passwords_in_log
check "no temporary siblings left" test "$(ls -A "$B")" = starting-state

# --- 2. A second run replaces the first ---------------------------------------
echo marker > "$OUT/stale-file"
if run; then ok; else bad "rebuild failed"; sed 's/^/    /' "$T/log"; fi
check "rebuild replaced the old output" test ! -e "$OUT/stale-file"
check "no temporary siblings after rebuild" test "$(ls -A "$B")" = starting-state
before=$(cat "$OUT/VERSION")

refuses() {  # description, expected text in the log
  if run; then bad "$1: did not refuse"; return; fi
  if grep -qF -- "$2" "$T/log"; then ok; else bad "$1: message lacks '$2'"; sed 's/^/    /' "$T/log"; fi
  check "$1: previous output kept" test "$(cat "$OUT/VERSION" 2>/dev/null)" = "$before"
  check "$1: no temporary siblings" test "$(ls -A "$B")" = starting-state
  check "$1: no password in the output" no_passwords_in_log
}

# --- 3. Refusals while the home is in use -------------------------------------
sleep 300 & SLEEPER=$!
printf 'mimir-server\npid=%s\nstart=1\nos=macos\n' "$SLEEPER" > "$H/mimir-state/mimir.lock"
refuses "live lock" "mimir-state/mimir.lock is held by running pid $SLEEPER"
rm -f "$H/mimir-state/mimir.lock"

echo "$SLEEPER" > "$H/heimdall-poc/feeds/feeds.pid"
refuses "feeds runner" "the feeds runner"
rm -f "$H/heimdall-poc/feeds/feeds.pid"

echo "$SLEEPER" > "$H/native-run/heimdall.pid"
refuses "native suite" "run-native.sh stop"
echo 999999 > "$H/native-run/heimdall.pid"   # dead: not a refusal
if run; then ok; else bad "a dead native-run pid refused"; sed 's/^/    /' "$T/log"; fi
before=$(cat "$OUT/VERSION")
rm -f "$H/native-run/heimdall.pid"
kill "$SLEEPER" 2>/dev/null || true; wait "$SLEEPER" 2>/dev/null || true; SLEEPER=""

# --- 4. Bad sources -----------------------------------------------------------
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
