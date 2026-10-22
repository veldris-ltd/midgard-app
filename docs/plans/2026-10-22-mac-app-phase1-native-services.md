# Mac app phase 1: native macOS services — Implementation Plan

**Goal:** Build `http-gateway`, `heimdall-feed`, `heimdall-source`, `mimir-server` and `forge-worker`
as universal macOS binaries (Intel + Apple Silicon) and run the whole suite natively from a
folder, without Docker, against the existing enclosure state.

**Architecture:** A build script compiles each binary for `aarch64-apple-darwin` and
`x86_64-apple-darwin` with the pinned Rust 1.82.0 from the vendored crates, and merges them with
`lipo` into `dist/mac/bin`. A run script starts the services on loopback ports, with the same
arguments the Docker containers use today but host paths and `127.0.0.1` addresses; Mimir talks
straight to the native `llama-server` on 127.0.0.1:8080. A smoke test signs in and checks every
service through the gateway. This is phase 1 of
`docs/plans/2026-10-22-mac-presentation-app-design.md`; the app shell, replay and packaging come
later.

**Tech Stack:** Rust 1.82.0 (rustup, two macOS targets), Xcode toolchain (`clang`, `lipo`),
bash, Python 3 (smoke test only, development Mac), existing `mimir-model.sh`.

**Constraints (read first):**
- Commits are SSH-signed (`git commit -S`) and pushed to `md_mixedfeeds_3d`; no PRs.
- The suite folder `~/Desktop/Midgard` must stay at or under 3.5 GB: build output goes to
  `~/Library/Caches/midgard-build` (outside it) and is deleted at the end; `dist/` is gitignored.
- Only one process may hold a state directory (`DirLock`): the Docker containers must be stopped
  before the native services use the same state, and restarted afterwards.
- The Docker setup must keep working unchanged.

---

### Task 1: Install the Rust toolchain on the Mac

**Files:** none.

**Step 1: Install rustup (Homebrew) and the pinned toolchain with both macOS targets**

```bash
brew install rustup
rustup-init -y --no-modify-path --default-toolchain none
source "$HOME/.cargo/env"
rustup toolchain install 1.82.0 --profile minimal \
  --target aarch64-apple-darwin --target x86_64-apple-darwin
```

**Step 2: Verify**

Run: `cd ~/Desktop/Midgard/midgard-app && cargo --version && rustup target list --installed`
Expected: `cargo 1.82.0 …` (picked from `rust-toolchain.toml`), and both
`aarch64-apple-darwin` and `x86_64-apple-darwin` listed.

No commit.

---

### Task 2: Run the services' tests natively on macOS

Catches platform differences (file locks, paths, permissions) before building releases.

**Files:** none unless a test fails (then fix in place; see Step 3).

**Step 1: Run the tests for the five binaries' crates**

```bash
cd ~/Desktop/Midgard/midgard-app
export CARGO_TARGET_DIR="$HOME/Library/Caches/midgard-build/test"
cargo test --offline -p service-runtime -p fabric -p http-gateway -p heimdall-feed-server \
  -p mimir-server -p forge-worker --all-features 2>&1 | grep -E "test result|FAILED|panicked|^error"
```
Expected: every `test result: ok`.

**Step 2: If a build error mentions a missing crate in `vendor/`**, the vendored set lacks a
macOS-only dependency. Do not re-vendor silently: stop and report the crate name (re-vendoring
changes `Cargo.lock` and `vendor/` and needs the procedure in `docs/dev/BUILD_ENV.md`).

**Step 3: If a test fails only on macOS**, fix the code with a `#[cfg(target_os = …)]` branch
next to the existing Linux one (see `services/runtime/src/lock.rs:90-110` for the pattern), re-run
Step 1, then commit:

```bash
git add <files> && git commit -S -m "<crate>: <what> on macOS"
```

---

### Task 3: Build script for universal binaries

**Files:**
- Create: `tools/mac-app/build-services.sh`
- Modify: `.gitignore` (add `/dist/`)

**Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Build the suite's services as universal macOS binaries (Intel + Apple Silicon) into
# dist/mac/bin. Uses the pinned toolchain (rust-toolchain.toml) and the vendored crates;
# build output lives outside the repo, in $MIDGARD_BUILD_DIR (default
# ~/Library/Caches/midgard-build), so the suite folder stays small.
#
#   bash tools/mac-app/build-services.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
OUT=dist/mac/bin
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
PACKAGES=(-p http-gateway -p heimdall-feed-server -p mimir-server -p forge-worker)
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker)

for t in "${TARGETS[@]}"; do
  echo "== building for $t"
  CARGO_TARGET_DIR="$BUILD" cargo build --offline --locked --release --target "$t" "${PACKAGES[@]}" --bins
done

mkdir -p "$OUT"
for b in "${BINARIES[@]}"; do
  lipo -create -output "$OUT/$b" \
    "$BUILD/aarch64-apple-darwin/release/$b" "$BUILD/x86_64-apple-darwin/release/$b"
  echo "$b: $(lipo -archs "$OUT/$b")"
done
```

**Step 2: Ignore the output and make the script executable**

```bash
printf '\n# Built app artefacts (tools/mac-app)\n/dist/\n' >> .gitignore
chmod +x tools/mac-app/build-services.sh
```

**Step 3: Run it**

Run: `bash tools/mac-app/build-services.sh`
Expected: ends with five lines like `http-gateway: x86_64 arm64`.

**Step 4: Check each binary starts on this Mac**

Run: `for b in dist/mac/bin/*; do $b --help 2>&1 | head -1; done`
Expected: a `usage:` line from each (no "bad CPU type" or dyld errors).

**Step 5: Commit**

```bash
git add tools/mac-app/build-services.sh .gitignore
git commit -S -m "tools/mac-app: build the services as universal macOS binaries"
```

---

### Task 4: Run script (start, stop, status) for the native suite

**Files:**
- Create: `tools/mac-app/run-native.sh`

Ports (loopback only; the gateway keeps 7710 so the existing consoles connect unchanged):

| Service | Ports |
|---|---|
| gateway | 7710 (HTTP), 7711 (mesh) |
| heimdall-feed | 7730 (mesh), 9730 (health) |
| mimir-server | 7720 (mesh), 7721 (observer), 9720 (health) |
| llama-server | 8080 (started by `~/Desktop/Midgard/mimir-model.sh`) |

**Step 1: Write the script**

```bash
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
  for s in forge heimdall gateway mimir; do
    if running "$s"; then kill "$(cat "$RUN/$s.pid")"; echo "$s stopped"; fi
    rm -f "$RUN/$s.pid"
  done
}

status() {
  for s in mimir gateway heimdall forge; do
    if running "$s"; then echo "$s running (pid $(cat "$RUN/$s.pid"))"; else echo "$s stopped"; fi
  done
}

case "${1:-status}" in start) start ;; stop) stop ;; status) status ;; *) echo "start|stop|status"; exit 2 ;; esac
```

Note: `forge-worker`'s ready pattern is its first `job run` log line; the Forge identity dir
must be the one the `midgard-forge` container uses — check with
`docker inspect midgard-forge --format '{{json .Config.Cmd}}'` (or `live-feeds/start.sh`) and
correct `--identity-dir`/`--service` if it differs.

**Step 2: Make it executable and commit**

```bash
chmod +x tools/mac-app/run-native.sh
git add tools/mac-app/run-native.sh
git commit -S -m "tools/mac-app: run the suite natively without Docker"
```

---

### Task 5: Smoke test

**Files:**
- Create: `tools/mac-app/smoke.py`

**Step 1: Write the smoke test** (reads the analyst password from
`~/Desktop/Midgard/airports-passwords.txt`; never prints it)

```python
#!/usr/bin/env python3
"""Smoke-test a running suite through its gateway: sign in, then check the fabric,
Heimdall's picture, Mimir's model plane and Forge's overview. Exit 1 on any failure.

    python3 tools/mac-app/smoke.py [--base http://127.0.0.1:7710/api/v1]
"""
import json, os, re, sys, urllib.request

BASE = sys.argv[sys.argv.index("--base") + 1] if "--base" in sys.argv else "http://127.0.0.1:7710/api/v1"
PW = re.search(r"^analyst \(internal\): (\S+)",
               open(os.path.expanduser("~/Desktop/Midgard/airports-passwords.txt")).read(), re.M).group(1)

def call(path, body=None, token=None):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"content-type": "application/json",
                                          **({"authorization": f"Bearer {token}"} if token else {})})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)

failures = []
def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}")
    if not ok:
        failures.append(name)

tok = call("/login", {"username": "analyst", "password": PW})["token"]
check("sign in", bool(tok))
types = call("/object-types", token=tok)
check("fabric object types", len(types) >= 20, f"({len(types)})")
pic = call("/heimdall/picture", token=tok)
ents = pic.get("entities", []) if isinstance(pic, dict) else []
check("heimdall picture", len(ents) > 0, f"({len(ents)} entities)")
models = call("/mimir/models", token=tok)
check("mimir model plane", bool(models.get("model")), f"({models.get('model', {}).get('id')})")
forge = call("/forge/overview", token=tok)
check("forge overview", len(forge.get("jobs", [])) == 11, f"({len(forge.get('jobs', []))} jobs)")
sys.exit(1 if failures else 0)
```

**Step 2: Run it against the Docker setup first (it must pass there too)**

Run: `python3 tools/mac-app/smoke.py`
Expected: five `ok` lines, exit 0. (If `heimdall picture` shows 0 entities, the picture is
empty because feeds are stopped and tracks expired; start the feeds and re-run.)

**Step 3: Commit**

```bash
chmod +x tools/mac-app/smoke.py
git add tools/mac-app/smoke.py
git commit -S -m "tools/mac-app: smoke test through the gateway"
```

---

### Task 6: Acceptance — the suite runs natively, then Docker again

**Files:** none.

**Step 1: Stop the Docker services (keep their containers) and the Forge container**

```bash
cd ~/Desktop/Midgard
bash live-feeds/stop.sh            # removes midgard-forge (recreated by start.sh)
docker stop midgard-gateway midgard-heimdall midgard-mimir-forward midgard-mimir
```

**Step 2: Start natively and smoke-test**

```bash
cd ~/Desktop/Midgard/midgard-app
bash tools/mac-app/run-native.sh start
python3 tools/mac-app/smoke.py
```
Expected: all five services report ready; five `ok` lines. Also open
http://localhost:5173 (Midgard console), sign in, and confirm objects load.

**Step 3: Record the result** (time to ready for each service, from `native-run/*.log`).

**Step 4: Stop native, restore Docker, smoke-test again**

```bash
bash tools/mac-app/run-native.sh stop
docker start midgard-mimir midgard-mimir-forward midgard-heimdall midgard-gateway
bash ~/Desktop/Midgard/live-feeds/start.sh
python3 tools/mac-app/smoke.py
```
Expected: five `ok` lines again.

---

### Task 7: README and clean-up

**Files:**
- Create: `tools/mac-app/README.md`

**Step 1: Write the README** — what each script does, prerequisites (Task 1), the port table,
"stop Docker first", and that build output lives in `~/Library/Caches/midgard-build`.

**Step 2: Remove the build caches and check the storage budget**

```bash
rm -rf "$HOME/Library/Caches/midgard-build"
du -sh ~/Desktop/Midgard     # must be <= 3.5G
du -sh ~/Desktop/Midgard/midgard-app/dist
```

**Step 3: Commit and push**

```bash
git add tools/mac-app/README.md
git commit -S -m "tools/mac-app: README for native builds and runs"
git push origin md_mixedfeeds_3d
```

---

## Done when

- `dist/mac/bin` holds five universal (x86_64 + arm64) binaries built offline from the
  vendored crates with the pinned toolchain.
- `run-native.sh start` brings up Mimir, the gateway, Heimdall and Forge without Docker, and
  `smoke.py` passes against it; the consoles work against it unchanged.
- Docker runs again afterwards and `smoke.py` passes there too.
- The suite folder is still at or under 3.5 GB.
