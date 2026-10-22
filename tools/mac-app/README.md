# tools/mac-app — the suite as native macOS programs

Phase 1 of the macOS presentation app (`docs/plans/2026-10-22-mac-presentation-app-design.md`):
the gateway, Heimdall, Mimir and Forge built as universal macOS binaries (Intel and Apple
Silicon) and run from a folder, without Docker.

| Script | What it does |
|---|---|
| `build-services.sh` | Builds `http-gateway`, `heimdall-feed`, `heimdall-source`, `mimir-server` and `forge-worker` for `aarch64-apple-darwin` and `x86_64-apple-darwin`, and merges each pair with `lipo` into `dist/mac/bin` (gitignored) |
| `run-native.sh start \| stop \| status` | Runs the suite from `dist/mac/bin` against an enclosure home (default `~/Desktop/Midgard`, override with `MIDGARD_HOME`). Logs and pids in `$MIDGARD_HOME/native-run/` |
| `smoke.py [--base URL]` | Signs in through the gateway and checks the fabric, Heimdall's picture, Mimir's model plane and Forge's overview; exit 0 only if all pass |

## Prerequisites

- Xcode (provides `clang` and `lipo`).
- Rust 1.82.0 with both macOS targets, through Homebrew's rustup (keg-only; the build script
  finds it in `/opt/homebrew/opt/rustup/bin`):

  ```bash
  brew install rustup
  export PATH=/opt/homebrew/opt/rustup/bin:$PATH
  rustup toolchain install 1.82.0 --profile minimal --component rustfmt --component clippy \
    --target aarch64-apple-darwin --target x86_64-apple-darwin
  ```

- Mimir's model server, started by `~/Desktop/Midgard/mimir-model.sh` (Homebrew
  `llama-server`, 127.0.0.1:8080). `run-native.sh start` starts it if needed; `stop` leaves it
  running because the Docker setup shares it.

Builds use the vendored crates offline and write to `~/Library/Caches/midgard-build`
(`MIDGARD_BUILD_DIR`), outside the suite folder. Delete that folder when done: the suite folder
must stay within its 3.5 GB budget.

## Ports (loopback only)

| Service | Ports |
|---|---|
| gateway | 7710 (HTTP, same as Docker, so the consoles connect unchanged), 7711 (mesh) |
| heimdall-feed | 7730 (mesh), 9730 (health) |
| mimir-server | 7720 (mesh), 7721 (observer), 9720 (health) |
| llama-server | 8080 |

## Switching between Docker and native

A state directory is held by one process at a time (`services/runtime/src/lock.rs`). On macOS a
lock written on another system (a Linux container) is never taken over automatically, so:

1. **To run natively:** stop the feeds (`heimdall-poc/feeds`, `live_loop.sh`,
   `live-feeds/stop.sh`) and the containers (`docker stop midgard-gateway midgard-heimdall
   midgard-mimir-forward midgard-mimir`), then `run-native.sh start`. It refuses while any
   running container mounts the enclosure home, and refuses if Docker is installed but can't be
   queried; with Docker Desktop quit, run it with `MIDGARD_DOCKER_STOPPED=1`. Once Docker is
   confirmed clear, it removes the locks the stopped containers left behind.
2. **To go back to Docker:** `run-native.sh stop` (it removes each lock only after its process
   has exited), then `docker start midgard-mimir midgard-mimir-forward midgard-heimdall
   midgard-gateway` and `live-feeds/start.sh`.

Never start the containers while the native suite is running: containers built before commit
`c7d8054` do not honour macOS lock files. The gateway's ledger size cap
(`rotate-gateway-ledger.sh`) only acts on the container.

On a 2026 Apple Silicon Mac the native suite is ready in about 30 seconds.
