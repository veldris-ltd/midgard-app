#!/usr/bin/env bash
# End-to-end test of tools/bifrost/release.sh against a throwaway bifrost-server: its own
# BIFROST_HOME in a temporary folder (a private enclosure and identities, made by
# run-local.sh setup) on 127.0.0.1:17760 (health :19760), so neither the running
# bifrost-local instance nor the suite's enclosure is touched.
#
#   bash tools/bifrost/test-release.sh [<commit>]      # default HEAD
#
# Checks, in order:
#   - a release before the server trusts the key is stopped by the preflight;
#   - release.sh --init-key installs the key; after a restart <commit> releases as 0.1.0
#     and the registry lists all six services verified;
#   - a copy of forge-worker signed as 0.1.0-tampered and changed by one byte after
#     signing is refused (digest-mismatch) and listed as rejected;
#   - --bad-build as 0.1.1 registers six verified artefacts, its forge-worker is the
#     stand-in: alive on /healthz, 503 on /readyz;
#   - the temporary build worktree is gone afterwards.
# The build is the real one (both architectures), so a first run takes several minutes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMMIT="${1:-HEAD}"
T="$(mktemp -d "${TMPDIR:-/tmp}/bifrost-release-test.XXXXXX")"
export BIFROST_HOME="$T" BIFROST_MESH=127.0.0.1:17760 BIFROST_HEALTH=127.0.0.1:19760
TOOLS="${CARGO_TARGET_DIR:-$HOME/Library/Caches/midgard-build/bifrost-target}"
export CARGO_TARGET_DIR="$TOOLS"
command -v cargo >/dev/null 2>&1 || PATH="/opt/homebrew/opt/rustup/bin:$PATH"
LOCAL="$ROOT/tools/bifrost/run-local.sh"
RELEASE_SH="$ROOT/tools/bifrost/release.sh"
STUB_PID=""

pass=0
ok() { echo "PASS: $*"; pass=$((pass + 1)); }
fail() { echo "FAIL: $*" >&2; exit 1; }
finish() {
    if [ -n "$STUB_PID" ]; then kill "$STUB_PID" 2>/dev/null || true; wait "$STUB_PID" 2>/dev/null || true; fi
    bash "$LOCAL" stop >/dev/null 2>&1 || true
    rm -rf "$T"
}
trap finish EXIT

# Fresh binaries: run-local builds only what is missing.
(cd "$ROOT" && cargo build --offline --locked --release -q -p bifrost-fleet-server -p mg-admin)
BIN="$TOOLS/release"
list() {
    "$BIN/bifrost-release" list --identity-dir "$T/fleet-clients" --addr "$BIFROST_MESH" "$@"
}

bash "$LOCAL" setup >/dev/null
bash "$LOCAL" start

# 1. No trusted key yet (the release makes one and installs its .pub, but the running
#    server has not read it): the preflight stops the release before it builds anything.
if out="$(bash "$RELEASE_SH" "$COMMIT" --version 0.1.0 2>&1)"; then
    fail "a release without a trusted key went ahead: $out"
fi
grep -q "does not trust the release key" <<<"$out" || fail "unexpected refusal: $out"
ok "the preflight refuses a key the registry does not trust"

# 2. The key, trusted after a restart; then a real release.
bash "$RELEASE_SH" --init-key
[ -f "$T/release-key/release.keystore" ] && [ -f "$T/fleet-state/trust/release-signers/bifrost-release-1.pub" ] \
    || fail "--init-key did not make and install the key"
bash "$LOCAL" stop >/dev/null
bash "$LOCAL" start
t0=$(date +%s)
bash "$RELEASE_SH" "$COMMIT" --version 0.1.0 --out "$T/out-0.1.0"
echo "(release 0.1.0 took $(( $(date +%s) - t0 )) s)"
n="$(list --version 0.1.0 | grep -c ' 0.1.0 verified ' || true)"
[ "$n" = 6 ] || { list; fail "expected 6 verified artefacts at 0.1.0, got $n"; }
for b in http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay; do
    list --version 0.1.0 | grep -q "^$b 0.1.0 verified " || fail "$b 0.1.0 is not verified"
    archs="$(lipo -archs "$T/out-0.1.0/$b")"
    [[ "$archs" == *x86_64* && "$archs" == *arm64* ]] || fail "$b is not universal ($archs)"
done
ok "release 0.1.0: six universal services, all verified"

# 3. Signed, then changed by one byte: refused at the gate and listed as rejected.
TAMP="$T/tampered"
mkdir -p "$TAMP"
cp "$T/out-0.1.0/forge-worker" "$TAMP/forge-worker"
"$BIN/bifrost-release" sign --keystore "$T/release-key/release.keystore" --id forge-worker \
    --version 0.1.0-tampered --payload "$TAMP/forge-worker" \
    --sbom "$T/out-0.1.0/forge-worker.sbom.cdx.json" --out "$TAMP/forge-worker.bifrost.json" >/dev/null
python3 -I -c 'import sys
p = sys.argv[1]
b = bytearray(open(p, "rb").read())
b[len(b) // 2] ^= 0x01
open(p, "wb").write(b)' "$TAMP/forge-worker"
if out="$("$BIN/bifrost-release" upload --identity-dir "$T/fleet-clients" --addr "$BIFROST_MESH" \
        --signed "$TAMP/forge-worker.bifrost.json" --payload "$TAMP/forge-worker" \
        --sbom "$T/out-0.1.0/forge-worker.sbom.cdx.json" 2>&1)"; then
    fail "the tampered copy was accepted: $out"
fi
grep -q "digest-mismatch" <<<"$out" || fail "the tampered copy was refused for the wrong reason: $out"
list --version 0.1.0-tampered | grep -q "^forge-worker 0.1.0-tampered rejected (digest-mismatch" \
    || { list; fail "the refusal is not listed"; }
ok "a tampered forge-worker is refused (digest-mismatch) and listed as rejected"

# 4. The bad build: registered like any other, but its forge-worker is never ready.
bash "$RELEASE_SH" "$COMMIT" --version 0.1.1 --bad-build --out "$T/out-0.1.1"
n="$(list --version 0.1.1 | grep -c ' 0.1.1 verified ' || true)"
[ "$n" = 6 ] || { list; fail "expected 6 verified artefacts at 0.1.1, got $n"; }
cmp -s "$T/out-0.1.0/forge-worker" "$T/out-0.1.1/forge-worker" && fail "0.1.1's forge-worker is the real one"
cmp -s "$T/out-0.1.0/mimir-server" "$T/out-0.1.1/mimir-server" || echo "note: mimir-server differs between two builds of one commit"
grep -q '"name": "unready-forge-worker"' "$T/out-0.1.1/forge-worker.sbom.cdx.json" || fail "the bad build's SBOM does not name the stand-in"
"$T/out-0.1.1/forge-worker" --identity-dir /nonexistent --state-dir /nonexistent \
    --jobs /nonexistent --gateway 127.0.0.1:7711 --health-bind 127.0.0.1:19761 >"$T/stub.log" 2>&1 &
STUB_PID=$!
for _ in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:19761/healthz && break; sleep 0.1; done
[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:19761/healthz)" = 200 ] || fail "the stand-in is not alive"
[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:19761/readyz)" = 503 ] || fail "the stand-in says it is ready"
kill -0 "$STUB_PID" || fail "the stand-in exited"
grep -q '"message":"job run"' "$T/stub.log" && fail "the stand-in logged a job run"
ok "the bad build 0.1.1 registers, and its forge-worker stays alive and unready"

# 5. Nothing left behind in the build cache's worktree slot.
[ ! -e "$HOME/Library/Caches/midgard-build/bifrost-release/worktree" ] || fail "the build worktree was left behind"
git -C "$ROOT" worktree list | grep -q "bifrost-release/worktree" && fail "git still lists the build worktree"
ok "the temporary worktree is removed"

echo "--- the registry"
list
echo "all $pass checks passed"
