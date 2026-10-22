# tools/mac-app — the suite as native macOS programs

Phase 1 of the macOS presentation app (`docs/plans/2026-10-22-mac-presentation-app-design.md`):
the gateway, Heimdall, Mimir and Forge built as universal macOS binaries (Intel and Apple
Silicon) and run from a folder, without Docker.

| Script | What it does |
|---|---|
| `build-services.sh` | Builds `http-gateway`, `heimdall-feed`, `heimdall-source`, `mimir-server`, `forge-worker` and `midgard-replay` for `aarch64-apple-darwin` and `x86_64-apple-darwin`, and merges each pair with `lipo` into `dist/mac/bin` (gitignored) |
| `run-native.sh start \| stop \| status` | Runs the suite from `dist/mac/bin` against an enclosure home (default `~/Desktop/Midgard`, override with `MIDGARD_HOME`). Logs and pids in `$MIDGARD_HOME/native-run/` |
| `record.py record \| pack` | Records the live feeds for offline replay (see [Recording and replay](#recording-and-replay)) |
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
   has exited), then start the containers and everything that runs beside them, from the
   enclosure home:

   ```bash
   docker start midgard-mimir midgard-mimir-forward midgard-heimdall midgard-gateway
   bash live-feeds/start.sh
   bash heimdall-poc/feeds/start.sh
   (cd heimdall-poc && nohup ./live_loop.sh > live_loop.log 2>&1 & echo $! > live_loop.pid)
   bash heimdall-poc/rotate-loop.sh start
   bash rotate-gateway-ledger.sh loop start
   ```

Never start the containers while the native suite is running: containers built before commit
`c7d8054` do not honour macOS lock files. The gateway's ledger size cap
(`rotate-gateway-ledger.sh`) only acts on the container.

On a 2026 Apple Silicon Mac the native suite is ready in about 30 seconds.

## Recording and replay

The offline app plays a recording of the live feeds instead of fetching them:
`midgard-replay` (`tools/replay`) sends the recorded Heimdall batches through
`heimdall-source` and rewrites the recorded Forge source files, as the live feeds would.

### Recording (development Mac)

With the Docker suite and the live feeds running:

```bash
python3 tools/mac-app/record.py record --minutes 60 [--name NAME] [--drop-raw]
```

- Recordings are kept in `~/Library/Application Support/Midgard-dev/recordings/<name>`
  (`MIDGARD_RECORDINGS` overrides), outside the suite folder because of its 3.5 GB budget.
  The raw capture stays in `<name>/raw` unless `--drop-raw`. Delete old recordings when done.
- It restarts the feeds runner (`heimdall-poc/feeds`) with its last-sent memory cleared, so
  every source is sent, and so recorded, from the start. The runner's first full pass takes
  about 5 minutes after that restart, so record at least 15 minutes (an hour for the app).
  `pack` prints a WARNING if `adsb-fi`, `adsb-lol` or `aisstream` sent no batches (a
  rate-limited source, or one not reached yet): such a recording has no aircraft or ships
  from that feed.
- Heimdall batches come from a recording hook in `feeds.py` (which is outside git):
  `state/record_to` holds `"<dir>\npid=<n>"`; while that pid lives, `feeds.py` copies each batch
  it sends there as `<time_ns>-<feed>.jsonl`, and removes a stale hook itself. The full contract
  is in `record.py`'s docstring.
- Ctrl-C stops early and still packs. After a crash, re-pack the raw capture with
  `python3 tools/mac-app/record.py pack RAW OUT`.

### Replaying

1. Build with `build-services.sh` (it includes `midgard-replay`).
2. Stop the Docker containers and the feeds as in "To run natively" above.
3. Start the suite with the recording:

   ```bash
   MIDGARD_REPLAY="$HOME/Library/Application Support/Midgard-dev/recordings/NAME" \
     bash tools/mac-app/run-native.sh start
   ```

   `start` checks the recording (it must contain `meta.json`) and the binary, then runs the
   replay after Forge is ready. `stop` stops the replay first, with its `heimdall-source`
   runs; `status` lists it. Without `MIDGARD_REPLAY`, nothing changes.

`run-native.sh` plays the recording on a loop until stopped. Run by hand, `midgard-replay
--loops N` stops after N loops (`midgard-replay` without options prints its usage).

The replay logs JSON lines to `$MIDGARD_HOME/native-run/replay.log`:

| Line | Meaning |
|---|---|
| `replay started` | The recording is loaded and playback has begun |
| `loop started` | A pass over the recording begins (`loop` counts from 0) |
| `batch sent` | One Heimdall batch played (`feed`, `offset`, `lag` = how late it went out); `result` is `heimdall-source`'s summary, or `FAILED <error>` for the first failure of a kind |
| `batches failing` | The same error keeps repeating; `dropped` counts the repeats not logged one by one |
| `batches recovered` | Sends work again after failures (`failed`, `dropped`) |
| `forge file failed` | A recorded Forge file could not be read or written into `live-feeds/data` (`file`, `error`) |
| `behind; restarting the loop` | Playback fell too far behind (e.g. after the Mac slept): the rest of this loop is dropped and the next starts now |

The recording loops; each loop shifts its times to now, so the picture always looks current.
The replay overwrites the Forge CSVs in `live-feeds/data`; the live fetcher rewrites them the
next time it runs.
