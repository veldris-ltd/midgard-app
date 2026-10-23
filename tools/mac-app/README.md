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
| `build-app.sh` | Builds `Midgard.app`, the presentation app (see [The app](#the-app)) |
| `fetch-llama.sh` | Downloads and lays out the pinned llama.cpp model server for the app (see [Model server](#model-server)) |
| `fetch-analytics.sh` | Downloads and lays out the pinned Python runtime, numpy, onnxruntime and model of the camera analytics for the app (see [Camera analytics](#camera-analytics)) |

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

## Live feeds without Docker

The feeds send to the native suite when Docker (or the `midgard-local-runtime:1` image) is not
usable: `heimdall-poc/hsource.sh` then runs `dist/mac/bin/heimdall-source` against
`127.0.0.1:7730`, rewriting `/mid/...` paths to the enclosure home (force it with
`MIDGARD_NATIVE=1`), and `live-feeds/start.sh` starts only the fetcher (forge-worker is part of
the native suite). From the enclosure home (`~/Desktop/Midgard`):

```bash
# start
MIDGARD_DOCKER_STOPPED=1 bash midgard-app/tools/mac-app/run-native.sh start
bash heimdall-poc/feeds/start.sh
(cd heimdall-poc && nohup ./live_loop.sh > live_loop.log 2>&1 & echo $! > live_loop.pid)
bash live-feeds/start.sh

# check: feeds.log shows accepted= per feed; live_loop.log shows adsb-lol
tail heimdall-poc/feeds/feeds.log heimdall-poc/live_loop.log
python3 midgard-app/tools/mac-app/smoke.py

# stop (feeds first, then the suite)
bash live-feeds/stop.sh
kill "$(cat heimdall-poc/live_loop.pid)"; rm -f heimdall-poc/live_loop.pid
bash heimdall-poc/feeds/stop.sh
bash midgard-app/tools/mac-app/run-native.sh stop
```

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
- Cameras are kept, and `pack` stores their media (fetched when `pack` runs, so the latest
  frame then, not the one current during the recording): each camera's latest still and (TfL) clip
  go into `OUT/media/` (about 180 MB for the recorded hour; 6 downloads at a time, one retry,
  stops past 400 MB) and the batches point at `/media/<file>`, which the app's web server
  serves. A camera whose media failed has no image URL rather than an internet one. Stored
  files are reused on a re-pack; `--refresh-media` downloads again (a failed download keeps
  the stored file), `--no-media` downloads nothing.

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

## The app

`Midgard.app` is the native presentation app (`apps/mac`, plan
`docs/plans/2026-10-22-mac-app-phase3-shell.md`): one window that starts the services
itself, signs the presenter in and shows the four products, with nothing reaching the
internet.

### Building

```bash
bash tools/mac-app/build-app.sh [--recording DIR] [--skip-services] [--skip-consoles] [--skip-llama] [--skip-analytics] [--out DIR]
```

- Needs the starting state from `make-starting-state.sh` (in
  `~/Library/Caches/midgard-build/starting-state`; build it with the suite stopped) and the
  model in `~/Desktop/Midgard/mimir-models`.
- Runs `build-services.sh` and `build-consoles.sh` (skip either to reuse `dist/mac` as it is;
  it warns when sources changed since), builds the shell universal with `swift build`, and
  assembles the bundle in `~/Library/Caches/midgard-build/app/Midgard.app` (`--out` to
  change). The service binaries go in `Contents/Helpers` (the shell runs them from there;
  in a development layout with `MIDGARD_APP_RESOURCES` it uses `$MIDGARD_APP_RESOURCES/bin`),
  the rest in `Contents/Resources`. Each helper and then the bundle are signed ad hoc, and
  the new bundle must pass `codesign --verify --deep --strict` before it replaces the old
  one; an interrupted build leaves the previous app in place.
- `--recording DIR` (or `MIDGARD_RECORDING`) bundles a recording (it must contain
  `meta.json`; its `raw/` is left out), which the app replays. Not given, it bundles the
  recorded hour (`~/Library/Application Support/Midgard-dev/recordings/hour-1`) if that has
  `meta.json`, and says so; `--recording ""` (or `MIDGARD_RECORDING=`) builds without one,
  and the app then starts with no feeds.
- Prepared Mimir answers: `apps/mac/Resources/home/prepared-answers.json` ships inside
  `home/`. The build checks it parses as version 1 and prints how many answers, guide
  questions and guide alerts it holds; without it every answer is asked live.
- The model (1.5 GB) and the starting state are copied as APFS clones, so the 1.6 GB bundle
  costs little extra disk while it stays on the same volume as its sources. With the Rust
  and Swift builds up to date, a build takes about 20 seconds.
- The model server: the build runs `fetch-llama.sh` (`--skip-llama` reuses
  `~/Library/Caches/midgard-build/llama/current` as it is) and copies the Mach-O files of
  both builds (`llama-server` and its dylibs), as real files, into
  `Contents/Helpers/llama/{arm64,x64}`; llama.cpp's LICENSE goes to
  `Contents/Resources/licenses/llama.cpp-LICENSE` (one copy when both are the same).
  codesign treats `Helpers` as nested code, so the build fails if anything there is not
  Mach-O (checked by the first four bytes). Every Mach-O there is signed ad hoc and verified
  before the bundle; `fetch-llama.sh` already signed the x64 build, and signing it again
  here is deliberate (every helper carries this build's signature). `Info.plist` asks for
  macOS 13.3 (`LSMinimumSystemVersion`), what both llama.cpp builds need; the app's system
  check says so on an older Mac.
- The app runs `Helpers/llama/arm64/llama-server` with `--n-gpu-layers 99` (Metal) on
  Apple Silicon, also when the shell itself runs under Rosetta (`hw.optional.arm64` or
  `sysctl.proc_translated`), and `Helpers/llama/x64/llama-server` with `--n-gpu-layers 0`
  and `--threads <physical cores>` (`hw.physicalcpu`) on Intel. An installed app uses only
  the bundled server: if it is missing it says "The model server is missing — install
  Midgard again." A development layout (no bundle, or `MIDGARD_APP_RESOURCES` set) falls back
  to `<helpers>/llama-server`, then Homebrew's (`/opt/homebrew/bin`, `/usr/local/bin`;
  `brew install llama.cpp`).
- Testing the Intel path on Apple Silicon: `MIDGARD_FORCE_INTEL_MODEL=1` makes the app run
  the x64 server on the CPU (logged as "forced Intel model server"), and
  `MIDGARD_MODEL_THREADS=N` sets its thread count, e.g.
  `MIDGARD_FORCE_INTEL_MODEL=1 MIDGARD_MODEL_THREADS=4 arch -x86_64 Midgard.app/Contents/MacOS/Midgard`.
- The camera analytics: the build runs `fetch-analytics.sh` (`--skip-analytics` reuses
  `~/Library/Caches/midgard-build/analytics` as it is), builds `midgard-frames` (universal,
  `apps/mac`) into `Contents/Helpers`, and copies `heimdall/analytics/{analytics.py,config.json}`
  and both runtimes into `Contents/Resources/analytics/` (`runtime/{arm64,x64}`), the model into
  `Contents/Resources/models/yolox/` and the licences (CPython, numpy, onnxruntime and its
  third-party notices, YOLOX, python-build-standalone's note) into `Contents/Resources/licenses/`.
  The runtimes hold Python sources, which codesign refuses in `Helpers`, so they live in
  Resources; every Mach-O in them (about 27 per runtime, inside site-packages too) is signed ad
  hoc and verified one by one before the bundle. The shell runs the `analytics` service after
  `heimdall` (see [Camera analytics](#camera-analytics)).
- Signing, notarisation and delivery for phase 5: "Phase 5 notes" in
  `docs/plans/2026-10-22-mac-presentation-app-design.md`.

### Model server

The app's model server is llama.cpp's official macOS release, pinned in `llama.lock` (build
b11669, the build the suite has run with through Homebrew; both asset URLs and their SHA-256).

```bash
bash tools/mac-app/fetch-llama.sh
```

downloads both archives (about 12 MB each, HTTPS only) into
`~/Library/Caches/midgard-build/llama/b11669/` (a download is kept only if its SHA-256 matches
the lock; a new one must match before it takes its final name), and lays out
`llama/current/{arm64,x64}` (`current` is a symlink to the build folder): in each,
`llama-server` and the dylibs it loads (ten arm64, nine x64), found with `otool -L`, as real
files under the names the loader asks for, plus llama.cpp's LICENSE (about 28 MB arm64,
25 MB x64). Only system libraries and `@rpath/<name>` / `@loader_path/<name>` files inside
the release are accepted; any other reference, or a library missing from the release, fails
the script (nothing is taken from this Mac). b11669 already loads them through `@rpath` with
an `@loader_path` rpath, and its Metal shaders are built into `libggml-metal`; the x64
release is unsigned and is signed ad hoc. A `<arch>.stamp` beside each folder (archive
SHA-256 and the script's hash) makes re-runs a no-op, so `build-app.sh` can call it on every
build. It prints each `--version` (x64 under Rosetta).

- Both builds need macOS 13.3 or later. The x64 build needs AVX2/FMA, which every Intel Mac
  able to run macOS 13 has; it runs under Rosetta on Apple Silicon (checked on macOS 26; Rosetta has AVX2 from macOS 15).
- Checked on an M4: arm64 with Metal answers `GET /health` in about 6 s with the Granite
  model and generates about 50 tokens/s; x64 under Rosetta, CPU only (`--n-gpu-layers 0`),
  is ready in about 3 s and generates about 7 tokens/s.
- To move to another build: change `llama.lock` (URLs and SHA-256 from the release, checked
  against GitHub's digests) and run the script again.

### Camera analytics

Analyse, Watch and Trend on a camera card run live in the app, offline (see
`heimdall/analytics/README.md`). `analytics.lock` pins CPython 3.13.16 (python-build-standalone,
`install_only`, both architectures), numpy 2.5.4 and onnxruntime 1.23.2 wheels per architecture
(1.23.2 is the newest onnxruntime with a macOS x86_64 wheel) and the YOLOX-s model, each by URL
and SHA-256.

```bash
bash tools/mac-app/fetch-analytics.sh
```

downloads them (HTTPS only, each checked before it takes its final name) into
`~/Library/Caches/midgard-build/analytics/downloads/`, and lays out `runtime/{arm64,x64}` (each
archive's member names checked, extracted into its own empty folder; the wheels installed by an
unpruned copy of this Mac's runtime, `tool/`, with pip `--no-index --no-deps --only-binary` for
the target platform; Tk, headers, pip, libpython, tests and onnxruntime's tools removed; the
standard library and packages compiled to unchecked-hash .pyc; no symlinks; Mach-O files signed
ad hoc where they did not verify), `models/yolox_s.onnx` and `licenses/`. Stamps make re-runs a
no-op. About 131 MB arm64, 156 MB x64, 36 MB for the model.

- The shell starts `analytics` after `heimdall` when the bundle has the analytics and a
  recording: `Resources/analytics/runtime/arm64/bin/python3.13` with the CoreML execution
  provider on Apple Silicon (also under Rosetta), the x64 runtime on the CPU (up to 4 threads)
  on Intel or with `MIDGARD_FORCE_INTEL_MODEL=1`. It runs `python3.13 -I -B analytics.py --serve`
  on a free loopback port with the recording's cameras and stored media, `Helpers/midgard-frames`
  to decode, and `Helpers/heimdall-source` to send its results to Heimdall as `camera-analytics`
  (the identity in the starting state's `airports-mesh/feeds`). Before anything starts the shell
  writes the gateway's token (`<home>/heimdall-poc/analytics/secret`, 0600, where run-native.sh
  keeps the development one) and the gateway gets `--analytics-addr`/`--analytics-token-file`.
  Its trend and watches live in `<home>/heimdall-poc/analytics/`, its log in `analytics.log`.
  Ready on its `SERVE` line (after a warm-up inference). Quitting ends it like the others, and
  the leftover sweep also covers `Resources/analytics/runtime`.
- Checked on an M4 (stored TfL clips, 40 frames): through the gateway in the app, about 1.3 s
  per clip with CoreML (ready 3.6 s after launch); with `MIDGARD_FORCE_INTEL_MODEL=1
  MIDGARD_MODEL_THREADS=4 arch -x86_64`, the x64 runtime (translated) takes about 10 s per clip
  (7.7 s alone, outside the app), with the same detections; the gateway allows 30 s.

### Prepared answers and the Presenter's guide

Mimir takes 15–60 seconds per answer on a laptop, so the app ships real answers captured
over the recorded hour. With the native suite replaying it (`MIDGARD_REPLAY=…
run-native.sh start`) for at least ten minutes, so alerts exist:

```bash
python3 tools/mac-app/prepare-answers.py --base http://127.0.0.1:7710/api/v1 \
    --out apps/mac/Resources/home/prepared-answers.json [--recording hour-1] [--alerts 5]
```

It asks the map questions (AskCard's examples plus two of ours, each over the region it is
about) and decision support for up to five alerts that stay open all hour, and writes the
answers plus a `guide` (format in the header of
`products/heimdall/ui/src/app/preparedAnswers.ts`). Heimdall shows a shipped answer at once
(keyed by question, and for alerts by the alert's subject); anything else is asked live.
Commit the file; rebuild the app.

The home page's Presenter's guide is built from the same file's `guide` when the page
loads, grouped by region (which to switch on under REGIONS; not in WALL mode, which hides
that list): the questions to click in the MIMIR box (or type exactly, then ASK), and the
alerts to click in ALERTS IN VIEW or the bell's ALERTS drawer before pressing ASK MIMIR.
Without the file it says "No prepared answers in this build."

### Running

```bash
open ~/Library/Caches/midgard-build/app/Midgard.app
```

It is ready in about 10 seconds on a 2026 Apple Silicon Mac. It picks free loopback ports, so
it runs beside the Docker suite or `run-native.sh`.

- Logs: `~/Library/Logs/Midgard/` (`shell.log` plus one per service).
- Working copy: `~/Library/Application Support/Midgard` (copied from the starting state on
  first launch); `~/Library/Application Support/Midgard.webport` remembers the web port.
- Quitting (Cmd-Q, or `osascript -e 'tell application id "app.midgard.presenter" to quit'`)
  stops every service.
- To reset by hand: quit the app, delete `~/Library/Application Support/Midgard`, open it
  again (Reset demo in the app does the same).

### The installer

```bash
bash tools/mac-app/build-dmg.sh [--no-desktop]
```

- Makes `Midgard-1.0.dmg` from the app `build-app.sh` built. It refuses an app built from
  another commit than `HEAD` (`MidgardGitCommit` in its Info.plist) or not asking for macOS
  13.3 (`LSMinimumSystemVersion`), and one that is running; rerun `build-app.sh` first.
- The image (HFS+, zlib-compressed) holds `Midgard.app` (copied with `ditto`), an
  `Applications` link to drag it onto, and `Install Midgard.pdf`, the one-page install sheet
  for presenters (`install-sheet.html`, rendered with headless Chrome; the build fails
  unless it is one A4 page with the design system's fonts embedded). The model barely
  compresses, so the image is about the size of the app (1.6 GB) and takes about a minute.
  Presenters need about 4 GB free: the app plus the image.
- Checked before it is published: `hdiutil verify`, then mounted read-only, the app inside
  must pass `codesign --verify --deep --strict`, and the sheet and the link must be there.
- Output: `~/Library/Caches/midgard-build/dist/Midgard-1.0.dmg`, copied to
  `~/Desktop/Midgard-1.0.dmg` (outside the suite folder; `--no-desktop` skips the copy). It
  prints the size and SHA-256; send that with the file so a damaged download shows.
- The app is signed ad hoc, not notarised, so on first launch macOS blocks it until the
  presenter clicks Open Anyway in System Settings › Privacy & Security; the install sheet
  walks through that ("Phase 5 notes" in the design doc). If that route is unavailable
  (a managed Mac, no Open Anyway button), someone comfortable with Terminal can clear the
  download flag once: `xattr -dr com.apple.quarantine /Applications/Midgard.app`, then open
  it normally; on a managed Mac ask IT first.
- To change the sheet, edit `install-sheet.html`, open it in Chrome to check, and rebuild
  the image.
