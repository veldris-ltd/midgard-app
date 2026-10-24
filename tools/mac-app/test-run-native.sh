#!/usr/bin/env bash
# Tests for run-native.sh's managed mode (Bifrost's suite agent), against a scratch
# MIDGARD_HOME and stand-in binaries, never the real suite: launches from the linked slot
# and records the version; status is read-only and version-true; restart of one service;
# the per-service lock and stale locks; stop pauses and restart then starts nothing; a
# managed suite refuses an unmanaged start; heimdall-source is checked by running it;
# the replay gets --source-bin; logs rotate.
#
#   bash tools/mac-app/test-run-native.sh      # needs cc (Xcode command line tools)
set -euo pipefail
cd "$(dirname "$0")/../.."
RN="$PWD/tools/mac-app/run-native.sh"
T=$(mktemp -d "${TMPDIR:-/tmp}/run-native-test.XXXXXX")
T=$(cd "$T" && pwd)
export MIDGARD_HOME="$T/home" MIDGARD_DOCKER_STOPPED=1 MIDGARD_READY_TIMEOUT=5
SUITE="$T/suite"
FAILS=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; FAILS=$((FAILS + 1)); }
expect() {  # description, expected, actual
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: expected [$2], got [$3]"; fi
}
managed() { MIDGARD_SUITE_ROOT="$SUITE" bash "$RN" "$@"; }
finish() {
  MIDGARD_SUITE_ROOT="$SUITE" bash "$RN" stop > /dev/null 2>&1 || true
  pkill -f "$T/" 2> /dev/null || true
  rm -rf "${T:?}"
}
trap finish EXIT

# Stand-ins: one program that acts as whichever service its name says. -DUNREADY never
# prints the readiness line (and as heimdall-source exits 0, which the check refuses).
cat > "$T/standin.c" <<'EOF'
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  const char *n = strrchr(argv[0], '/'); n = n ? n + 1 : argv[0];
  if (strcmp(n, "heimdall-source") == 0) {
#ifdef UNREADY
    return 0;
#else
    fprintf(stderr, "heimdall-source: unexpected argument\nusage: heimdall-source ...\n");
    return 2;
#endif
  }
  printf("%s args:", n);
  for (int i = 1; i < argc; i++) printf(" %s", argv[i]);
  printf("\n"); fflush(stdout);
  usleep(300000);
#ifndef UNREADY
  if (!strcmp(n, "http-gateway")) printf("listening on 127.0.0.1:7710\n");
  else if (!strcmp(n, "forge-worker")) printf("{\"message\":\"job run\"}\n");
  else if (!strcmp(n, "midgard-replay")) printf("{\"message\":\"replay started\"}\n");
  else printf("{\"message\":\"serving\"}\n");
  fflush(stdout);
#endif
  for (int i = 0; i < 600; i++) sleep(1);
  return 0;
}
EOF
cc -O -o "$T/standin" "$T/standin.c"
cc -O -DUNREADY -o "$T/unready" "$T/standin.c"
BINS="http-gateway mimir-server heimdall-feed forge-worker midgard-replay heimdall-source"
mkdir -p "$SUITE/active" "$MIDGARD_HOME/rec"
for v in 1 2 3; do
  mkdir -p "$SUITE/releases/$v"
  for b in $BINS; do cp "$T/standin" "$SUITE/releases/$v/$b"; done
done
for b in $BINS; do cp "$T/unready" "$SUITE/releases/3/$b"; ln -s "../releases/1/$b" "$SUITE/active/$b"; done
echo '{}' > "$MIDGARD_HOME/rec/meta.json"
printf '#!/bin/sh\necho model ok\n' > "$MIDGARD_HOME/mimir-model.sh"
RUN="$MIDGARD_HOME/native-run"
relink() { ln -sfn "../releases/$2/$1" "$SUITE/active/.$1"; mv -f "$SUITE/active/.$1" "$SUITE/active/$1"; }

echo "--- managed start"
MIDGARD_REPLAY="$MIDGARD_HOME/rec" managed start > "$T/start.out" 2>&1 || { cat "$T/start.out"; exit 1; }
for s in gateway mimir heimdall forge replay; do
  expect "$s ready on 1" "ready 1" "$(managed status "$s")"
done
expect "launched from the slot, bound to its pid" "$(cat "$RUN/forge.pid") 1" "$(cat "$RUN/forge.version")"
case $(ps -p "$(cat "$RUN/forge.pid")" -o comm=) in
  "$SUITE/releases/1/forge-worker") ok "forge runs the resolved slot path" ;;
  *) bad "forge runs $(ps -p "$(cat "$RUN/forge.pid")" -o comm=)" ;;
esac
grep -q -- "--source-bin $SUITE/active/heimdall-source" "$RUN/replay.log" \
  && ok "replay gets --source-bin active/heimdall-source" || bad "replay args: $(head -1 "$RUN/replay.log")"
expect "suite root recorded" "$SUITE" "$(cat "$RUN/suite-root")"

echo "--- status is version-true and read-only"
relink forge-worker 2
expect "relinked, old process: unready on 1" "unready 1" "$(managed status forge-worker)"
managed restart forge-worker > /dev/null
expect "restarted: ready on 2" "ready 2" "$(managed status forge)"
[ -f "$RUN/forge.log.1" ] && ok "the previous log kept as forge.log.1" || bad "no forge.log.1"
echo 99999 > "$RUN/gone.pid"
cp "$RUN/gateway.pid" "$T/gateway.pid"
echo 99999 > "$RUN/gateway.pid"
expect "dead pid: stopped" "stopped" "$(managed status gateway)"
[ -f "$RUN/gateway.pid" ] && ok "status left the pid file alone" || bad "status removed the pid file"
cp "$T/gateway.pid" "$RUN/gateway.pid"

echo "--- a build that never becomes ready"
relink forge-worker 3
if managed restart forge > "$T/r3.out" 2>&1; then bad "restart of an unready build succeeded"; else
  ok "restart of an unready build fails ($(tail -1 "$T/r3.out" | cut -c1-40)...)"; fi
expect "it is reported unready on 3" "unready 3" "$(managed status forge)"
relink forge-worker 2
managed restart forge > /dev/null
expect "back on 2" "ready 2" "$(managed status forge)"

# A process that looks like a run-native.sh holding a lock for $1 seconds.
holder() { bash -c "sleep $1; :" run-native.sh > /dev/null 2>&1 & echo $!; }

echo "--- the version is bound to the pid"
cp "$RUN/forge.version" "$T/forge.version"
echo "99999 2" > "$RUN/forge.version"
expect "a version recorded for another pid is not trusted" "unready -" "$(managed status forge)"
cp "$T/forge.version" "$RUN/forge.version"
expect "the real record is" "ready 2" "$(managed status forge)"
kill "$(cat "$RUN/forge.pid")"; sleep 0.5
FAKE=1 nohup "$SUITE/releases/2/forge-worker" > /dev/null 2>&1 &
echo $! > "$RUN/forge.pid"; sleep 0.5
MIDGARD_REPLAY="$MIDGARD_HOME/rec" managed start > "$T/other.out" 2>&1
grep -q "was not launched by this script managed" "$T/other.out" \
  && ok "start warns about a service launched elsewhere" || bad "$(cat "$T/other.out")"
expect "and it is unready with an unknown version" "unready -" "$(managed status forge)"
managed restart forge > /dev/null
expect "until restarted" "ready 2" "$(managed status forge)"

echo "--- the per-service lock"
holder=$(holder 3)
mkdir "$RUN/forge.busy"; echo "$holder" > "$RUN/forge.busy/pid"
expect "locked: unready" "unready 2" "$(managed status forge)"
start_s=$(date +%s)
managed restart forge > /dev/null
waited=$(( $(date +%s) - start_s ))
[ "$waited" -ge 1 ] && ok "restart waited for the lock (${waited} s)" || bad "restart did not wait ($waited s)"
[ ! -d "$RUN/forge.busy" ] && ok "the lock is released" || bad "the lock is left"
mkdir "$RUN/forge.busy"; echo 99999 > "$RUN/forge.busy/pid"
managed restart forge > "$T/stale.out"
grep -q "removed the stale lock" "$T/stale.out" && ok "a dead owner's lock is removed" || bad "$(cat "$T/stale.out")"
sleep 30 & other=$!
mkdir "$RUN/forge.busy"; echo "$other" > "$RUN/forge.busy/pid"
managed restart forge > "$T/stale2.out"
grep -q "removed the stale lock" "$T/stale2.out" && ok "a live owner that is not run-native.sh is stale" || bad "$(cat "$T/stale2.out")"
kill "$other"
holder=$(holder 10)
mkdir "$RUN/forge.busy"; echo "$holder" > "$RUN/forge.busy/pid"
rc=0; MIDGARD_LOCK_TIMEOUT=2 managed restart forge > /dev/null || rc=$?
expect "a lock held too long: not attempted (exit 4)" "4" "$rc"
expect "and nothing changed" "unready 2" "$(managed status forge)"
kill "$holder"; drop() { rm -f "$RUN/forge.busy/pid"; rmdir "$RUN/forge.busy"; }; drop
expect "lock gone: ready again" "ready 2" "$(managed status forge)"

echo "--- paused while waiting for the lock"
holder=$(holder 3)
mkdir "$RUN/forge.busy"; echo "$holder" > "$RUN/forge.busy/pid"
pid_before=$(cat "$RUN/forge.pid")
( rc=0; managed restart forge > /dev/null || rc=$?; echo "$rc" > "$T/paused.rc" ) &
sleep 1; echo "test" > "$RUN/agent-paused"
wait $!
expect "restart re-checks the pause after the lock (exit 3)" "3" "$(cat "$T/paused.rc")"
expect "and left the service alone" "$pid_before" "$(cat "$RUN/forge.pid")"
rm -f "$RUN/agent-paused"

echo "--- Docker"
if command -v docker > /dev/null && ! docker ps -q > /dev/null 2>&1; then
  rc=0; MIDGARD_DOCKER_STOPPED= managed restart forge > /dev/null || rc=$?
  expect "Docker's daemon down, no override: not attempted (exit 4)" "4" "$rc"
  MIDGARD_DOCKER_STOPPED=auto managed restart forge > "$T/auto.out" \
    && grep -q "MIDGARD_DOCKER_STOPPED=auto" "$T/auto.out" && ok "auto: a daemon that is down is seen as down" \
    || bad "auto: $(cat "$T/auto.out")"
else
  echo "skip Docker checks (Docker's daemon is running, or no docker CLI)"
fi

echo "--- heimdall-source"
managed restart heimdall-source > /dev/null && ok "heimdall-source checked by running it"
expect "heimdall-source ready on 1" "ready 1" "$(managed status heimdall-source)"
relink heimdall-source 3
expect "a heimdall-source that does not behave: unready" "unready 1" "$(managed status heimdall-source)"
if managed restart heimdall-source > /dev/null 2>&1; then bad "a bad heimdall-source passed"; else ok "a bad heimdall-source fails its restart"; fi
relink heimdall-source 1

echo "--- an unmanaged start of a managed suite"
if bash "$RN" start > "$T/unmanaged.out" 2>&1; then bad "an unmanaged start ran"; else
  grep -q "managed by Bifrost's agent" "$T/unmanaged.out" && ok "an unmanaged start refuses" || bad "$(cat "$T/unmanaged.out")"
fi
if bash "$RN" restart forge > /dev/null 2>&1; then bad "an unmanaged restart ran"; else ok "an unmanaged restart refuses"; fi

echo "--- stop pauses"
managed stop > "$T/stop.out"
grep -q "note: this suite is managed by Bifrost's agent" "$T/stop.out" && ok "stop names the script to use" || bad "$(cat "$T/stop.out")"
[ -f "$RUN/agent-paused" ] && ok "stop writes agent-paused" || bad "no agent-paused"
expect "paused status" "paused" "$(managed status forge)"
rc=0; managed restart forge > /dev/null || rc=$?
expect "restart while paused leaves it stopped (exit 3)" "3" "$rc"
alive=no; [ -f "$RUN/forge.pid" ] && kill -0 "$(cat "$RUN/forge.pid")" 2> /dev/null && alive=yes
expect "nothing was started" "no" "$alive"
MIDGARD_REPLAY="$MIDGARD_HOME/rec" managed start > /dev/null
[ ! -f "$RUN/agent-paused" ] && ok "start removes agent-paused" || bad "agent-paused left"
expect "forge ready after start" "ready 2" "$(managed status forge)"

echo
if [ "$FAILS" = 0 ]; then echo "run-native managed mode: all passed"; else echo "run-native managed mode: $FAILS failed"; exit 1; fi
