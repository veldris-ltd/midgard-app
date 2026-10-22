# Mac app phase 3: the app shell — implementation plan

**Goal:** `Midgard.app`: one native macOS window that starts the suite's services itself, signs the presenter in, shows a home screen with the four products, keeps the services running, works with no internet, and can reset the demo.

**Architecture:** A Swift package (`apps/mac`, no third-party dependencies) with a testable core library and a SwiftUI app. The core finds free loopback ports, copies a starting state into `~/Library/Application Support/Midgard` on first launch, supervises the native services (phase 1 binaries, `midgard-replay` from phase 2, `llama-server`), serves the four consoles' static builds and a home page from a small loopback HTTP server, and signs in through the gateway. The window is a WKWebView with a native toolbar; the session is injected before each page loads, and a content rule list blocks every request that is not to 127.0.0.1, so nothing reaches the internet.

**Tech Stack:** Swift 6 / SwiftUI / WKWebView / Network.framework (`NWListener`), Swift Package Manager with XCTest (`swift test`), TypeScript (small console changes), Rust (one gateway flag), bash build scripts.

Design: `docs/plans/2026-10-22-mac-presentation-app-design.md` §1, §4. Decision (22 Oct): native Swift instead of Tauri (no new third-party dependencies).

---

## Background the implementer needs

- **Native services** (built by `tools/mac-app/build-services.sh` into `dist/mac/bin`, universal): `http-gateway`, `heimdall-feed`, `heimdall-source`, `mimir-server`, `forge-worker`, `midgard-replay`. `tools/mac-app/run-native.sh` shows exactly how each is started (arguments, identity dirs, ready log lines) — the shell's supervisor does the same thing in Swift, with ports it chooses.
- **Model server:** `llama-server` (llama.cpp). Phase 3 uses `Resources/bin/llama-server` when present, else `/opt/homebrew/bin/llama-server` (phase 5 bundles pinned releases). Arguments as in `~/Desktop/Midgard/mimir-model.sh` with the chosen port.
- **Consoles:** `products/{midgard,heimdall,mimir,forge}/ui`, Vite + React, `base: './'`, hash routing. They read the gateway URL at build time (`VITE_GATEWAY_URL`, default `http://127.0.0.1:7710/api/v1`). Sign-in: `ui/shared/src/AuthGate.tsx` reads `sessionStorage['midgard.session.token']` (any non-empty value counts) and `['midgard.session.who']` (`{subject, clearance, roles}` JSON) once at mount. Login: `POST {gateway}/api/v1/login {"username","password"}` → `{token, subject, clearance, roles, expiresAt}`. Tokens last `TOKEN_TTL = 3600` s (services/http-gateway/src/lib.rs:63) and the consoles do not handle expiry.
- **Internet use** (Heimdall only): rain radar (RainViewer) and traffic (TomTom, key from `products/heimdall/ui/.env.local` — must never be built into the app) overlays, both off by default; close-up terrain and satellite detail on the 3D globe (`globe/detailPatch.ts`, AWS terrarium and EOX tiles, falls back to the bundled map). Fonts and the base maps are bundled.
- **Gateway CORS:** exact match of `Origin` against `--allow-origin` values.
- **State the services need:** see the "Starting state" task. Demo users: `analyst` (INTERNAL) and `controller` (RESTRICTED); their passwords are in `~/Desktop/Midgard/airports-passwords.txt` (`analyst (internal): …`, `controller (restricted): …` — check the exact second label). Never print them.
- **Storage:** the suite folder `~/Desktop/Midgard` must stay within 3.5 GB. Build outputs go outside it: Swift scratch `~/Library/Caches/midgard-build/swift`, the assembled app `~/Library/Caches/midgard-build/app/Midgard.app`. Console builds may go in `dist/mac/consoles` (gitignored, a few MB each); the starting state goes in `~/Library/Caches/midgard-build/starting-state`.
- **Swift toolchain:** Xcode 26 (`swift` 6.2). `swift test --package-path apps/mac --scratch-path ~/Library/Caches/midgard-build/swift`.
- **Commits:** signed (`git commit -S`), on `md_mixedfeeds_3d`. Push after each task.

## Runtime layout (what the app does)

```
Midgard.app/Contents/
  MacOS/Midgard                       the shell (universal)
  Resources/bin/                      http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay [llama-server]
  Resources/consoles/{midgard,heimdall,mimir,forge}/   static builds
  Resources/home/index.html           the home page
  Resources/starting-state/           see Task 8
  Resources/recording/                optional: a phase 2 recording (replayed when present)
  Resources/models/granite/           the model (a symlink in development builds)
~/Library/Application Support/Midgard/   the working copy of starting-state (H)
~/Library/Logs/Midgard/                  one log per service + shell.log
```

Launch: check macOS 13+ and 8 GB+ (clear message otherwise) → single instance (a second launch activates the first) → copy starting state to H if absent → choose free ports → start llama-server, mimir, gateway (`--allow-origin http://127.0.0.1:<web port>`, `--token-ttl 43200`), heimdall, forge, replay (if a recording is bundled) → start the web server → wait for ready (20–40 s on an i5, "Starting Midgard…" screen with per-service status) → sign in as analyst → home. Quit stops every service (replay first, as run-native.sh).

---

### Task 1: Gateway `--token-ttl`

**Files:** `services/http-gateway/src/main.rs`, `src/lib.rs` (replace the `TOKEN_TTL` constant use with a configured value, default 3600), tests beside the existing gateway tests.

Behaviour: `--token-ttl SECONDS` (60–86400; anything else is a usage error, exit 2); issued tokens expire after that many seconds; default unchanged. Tests: a token from a gateway built with a 120 s TTL has `expiresAt` = issue time + 120; out-of-range values refused by the argument parser. Build and test natively (`CARGO_TARGET_DIR=~/Library/Caches/midgard-build`, `--offline`). Commit: `http-gateway: --token-ttl for long presenter sessions`.

### Task 2: Consoles for the app

**Files:** the four `products/*/ui/src/main.tsx`; `products/heimdall/ui/src/app/{radar,traffic}.ts` and `globe/detailPatch.ts` (or their callers); `products/forge/ui/src/screens/ObjectProjectionScreen.tsx`; new `tools/mac-app/build-consoles.sh`.

1. Gateway URL at run time: `const baseUrl = (window as any).__MIDGARD_GATEWAY__ ?? import.meta.env.VITE_GATEWAY_URL ?? 'http://127.0.0.1:7710/api/v1';` in all four (the shell injects `__MIDGARD_GATEWAY__` before the page loads). Put the expression in one shared helper in `ui/shared` and use it from the four.
2. `VITE_OFFLINE=1` builds: Heimdall hides the RADAR and TRAFFIC chips and never starts their fetches, and the globe never requests terrain/satellite detail tiles (the bundled map is used). Default (dev) behaviour unchanged.
3. Forge's link to Midgard: when `__MIDGARD_CONSOLE__` is defined (injected by the shell, e.g. `../midgard/index.html#/`), use it and open in the same window instead of `_blank`.
4. `build-consoles.sh`: builds each console with `VITE_OFFLINE=1 VITE_TOMTOM_KEY= ` (empty, overriding `.env.local`) into `dist/mac/consoles/<product>`; then fails if any built file contains `api.tomtom.com`, a TomTom key value, `rainviewer`, `elevation-tiles-prod` or `tiles.maps.eox.at`, and lists every other `https?://` string found for review (attribution links are fine).

Tests: `tsc --noEmit` for all four; the build script's checks pass; in dev (`pnpm dev`) nothing changes. Commit: `consoles: run-time gateway URL and an offline build for the Mac app`.

### Task 3: Swift package and core utilities

**Files:** `apps/mac/Package.swift` (platform macOS 13; library `MidgardShellCore`; executable `Midgard`; test target `MidgardShellCoreTests`), `apps/mac/Sources/MidgardShellCore/{Ports.swift,Paths.swift,SystemCheck.swift}`, tests.

- `Ports.free(count:)`: n distinct free TCP ports on 127.0.0.1 (bind to port 0, read, close); test that they are distinct and bindable.
- `Paths`: resources dir (bundle or an override for tests/dev via env `MIDGARD_APP_RESOURCES`), home `~/Library/Application Support/Midgard` (override `MIDGARD_APP_HOME`), logs `~/Library/Logs/Midgard`.
- `SystemCheck`: macOS ≥ 13 and physical memory ≥ 8 GB → nil, else a plain-English message; test with injected values.

Commit: `mac app: Swift package and core utilities`.

### Task 4: Loopback web server

**Files:** `Sources/MidgardShellCore/WebServer.swift`, tests.

`WebServer(root: URL, port: UInt16)` on `NWListener`, 127.0.0.1 only: `GET`/`HEAD` of files under `root` (`/` → `/home/index.html`; `/midgard/…`, `/heimdall/…`, `/mimir/…`, `/forge/…` → `root/consoles/<product>/…`; a directory → its `index.html`); content types for html, js, css, json, svg, png, jpg, ttf, woff2, wasm, i16 (octet-stream); 404 otherwise; refuses `..` and encoded traversal (403); `Cache-Control: no-store`; one request per connection (`Connection: close`). Tests (URLSession against a temp root): files served with the right type, 404, traversal refused, HEAD has no body, a 5 MB file streams intact.

Commit: `mac app: loopback web server for the consoles`.

### Task 5: Service supervisor

**Files:** `Sources/MidgardShellCore/{ServiceSpec.swift,Supervisor.swift}`, tests.

- `ServiceSpec`: name, executable URL, arguments, ready log pattern, log file. `SuiteSpecs.make(resources:, home:, ports:, webOrigin:, recording:)` builds the specs exactly like `run-native.sh start` (same arguments, identity dirs, ready patterns) but with the chosen ports; plus `llama-server` (ready when `GET /health` on its port is 200 — the supervisor supports a health-URL readiness too) and `replay` only when a recording is present. Unit-test the argument lists against expected arrays.
- `Supervisor`: starts specs in order, waiting for each to be ready (timeout per service, 120 s for llama-server); output appended to its log file; publishes status per service (`starting`, `ready`, `restarting`, `failed`) for the UI; restarts a service that exits unexpectedly after 2 s, 4 s, 8 s … up to 60 s, logging each; `stopAll()` stops in reverse order with SIGTERM, waits 15 s, then SIGKILL, and for `replay` also terminates its child processes first (as run-native.sh `kill_replay`). No lock sweeping: the services' own locks reclaim a dead macOS holder.
- Tests with fake services (small shell scripts in a temp dir): ready by log line; ready by health URL (use the Task 4 server or a tiny listener); restart after an unexpected exit with backoff (inject a fast clock or short delays); `stopAll` leaves no processes (check with `kill(pid, 0)`); a service that never becomes ready → `failed` with the last log lines.

Commit: `mac app: service supervisor`.

### Task 6: Working copy and Reset demo

**Files:** `Sources/MidgardShellCore/Home.swift`, tests.

- `Home.prepare(starting:, home:)`: if `home` lacks `.midgard-home` (a marker with the starting-state version), copy `starting` to a temp dir beside `home` and rename it into place (so a half-finished copy never counts), then write the marker.
- `Home.reset(…)`: caller has stopped the services; move `home` aside, prepare afresh, then delete the old copy.
- The model directory is not copied: specs point at `Resources/models` directly.
- Tests: first prepare copies and marks; second prepare does nothing; an interrupted copy (temp dir left) is cleaned up and redone; reset restores a modified file and removes added files.

Commit: `mac app: working copy and Reset demo`.

### Task 7: Sign-in

**Files:** `Sources/MidgardShellCore/Session.swift`, tests.

- `DemoUsers` read from `starting-state/demo-users.json` (`[{"username","password","label"}]`, label e.g. "Analyst (INTERNAL)").
- `Session.login(gateway:, user:)` → `{token, subject, clearance, roles, expiresAt}`; `Session.refreshDate` schedules the next sign-in 10 minutes before `expiresAt`, or halfway through the remaining life if that is sooner, but never sooner than 30 s from now (so a short-lived or already-expired token cannot cause a tight sign-in loop).
- `Session.injectionScript(origin:, gatewayURL:, midgardConsoleURL:, session:)` → JavaScript run at document start that does nothing unless `location.origin` is the app's own web server; it sets `window.__MIDGARD_GATEWAY__`, `window.__MIDGARD_CONSOLE__`, and `sessionStorage` `midgard.session.token` / `midgard.session.who`.
- Tests with a URLProtocol stub: successful login parsed; 401 → error; the script contains properly escaped JSON (quotes and backslashes in values).

Commit: `mac app: presenter sign-in`.

### Task 8: Starting state

**Files:** `tools/mac-app/make-starting-state.sh`.

Builds `~/Library/Caches/midgard-build/starting-state` (outside the suite folder: `objects.store` alone is about 250 MB) from an enclosure home (default `~/Desktop/Midgard`, with the Docker suite stopped, so its stores are quiet — the script refuses while the containers or native suite run):
- `airports/`: `ontology.json`, `policy.json`, `identity.directory`, `trust/`, `keys/`, `objects.store`, `admin/` (empty journals as they are). Not: `governance.ledger*`, `*.bak*`, `*.before-live`, `midgard.lock`, damaged ledgers.
- `airports-mesh/` complete.
- `heimdall-poc/state/`: `policy.json`, `rules.json`, `ttl.json`, `alerts.json` only (picture, ledgers and history start empty; the replay fills them).
- `mimir-state/policy.json`; `live-feeds/data/jobs.json` and the current `*.csv`; `live-feeds/worker-state/` empty.
- `heimdall-poc/feeds/state/status.json` (an empty `{"sources":{}}` is fine).
- `demo-users.json` from `airports-passwords.txt` (mode 600), never echoed.
- Excludes `mimir-signing/`, `grid-sam-password.txt`, analytics, `.venv`.
- Prints the size of each part and the total. Verify by starting the native suite against a copy (`MIDGARD_HOME=<copy> run-native.sh start` with Docker stopped) and running `smoke.py` with `--base` — record the result. Commit: `mac app: starting state builder`.

### Task 9: The window

**Files:** `apps/mac/Sources/Midgard/{MidgardApp.swift,AppModel.swift,WebView.swift,StartingView.swift}`, `apps/mac/Resources/home/index.html`.

- `AppModel` ties the core together: system check → single instance (`NSRunningApplication` for the bundle id; activate the other and quit) → `Home.prepare` → ports → supervisor start → web server → login → ready. Publishes status for the starting screen; `resetDemo()` (confirmation sheet; stop all, `Home.reset`, start again), `switchUser(_:)`, quit handling (`applicationShouldTerminate` waits for `stopAll`).
- `StartingView`: "Starting Midgard…" with each service's status; on failure, which service failed, its last log lines, and "Try again".
- Main window: a toolbar with Home, Midgard, Heimdall, Mimir, Forge (segmented), the signed-in user with "Switch user", and "Reset demo"; below, one WKWebView (persistent website data store so UI preferences survive) loading `http://127.0.0.1:<web>/` and `/<product>/index.html#/`.
- WKWebView: the injection script as a `WKUserScript` at document start; a `WKContentRuleList` that blocks every URL not starting `http://127.0.0.1:` (offline guarantee); `target=_blank` links open in the same view; after a token refresh, update `sessionStorage` via `evaluateJavaScript` (the consoles keep their token until reload, and the 12 h TTL from Task 1 covers a presentation day; reload on Home/product switch picks up the new one).
- Home page (`index.html`, plain HTML/CSS/JS in the design system's colours; fonts copied from `ui/design-system/src/fonts`): four product tiles (name, one line on what it does), a status line (services ready, replay loop running), placeholder area for the Presenter's guide (phase 4). Tiles navigate to `/<product>/index.html#/`.
- Heimdall opens in 2D on Intel Macs, 3D on Apple Silicon: set its stored view preference key before first load (find the key in `OperatingPictureScreen.tsx`).

Commit: `mac app: the window, home screen and toolbar`.

### Task 10: Build the app

**Files:** `tools/mac-app/build-app.sh`, README section.

`build-app.sh`: runs build-services.sh and build-consoles.sh (skippable with flags when fresh), `swift build -c release --arch arm64 --arch x86_64 --scratch-path …`, then assembles `~/Library/Caches/midgard-build/app/Midgard.app`: `Info.plist` (bundle id `app.midgard.presenter`, name Midgard, version 1.0, `LSMinimumSystemVersion` 13.0, `NSHighResolutionCapable`), the shell binary, `Resources/` as in the runtime layout (bin copied; consoles; home; starting-state from Task 8; `recording/` from `MIDGARD_RECORDING` if set; `models/granite` as a symlink to `~/Desktop/Midgard/mimir-models/granite` plus `trust/`), ad-hoc code signature (`codesign --force --deep -s -`), and prints the bundle size. README: how to build and run it, where logs and the working copy are, how to reset by hand.

Commit: `mac app: build script`.

### Task 11: End-to-end check (manual, report results)

With the Docker suite still running (the app uses its own ports and home, so both can run; note memory): build with the phase 2 e2e recording, open the app, and check:
1. Starting screen, then home within 60 s on this Mac; `~/Library/Logs/Midgard/` has a log per service.
2. Each tile opens its console already signed in; Heimdall shows replayed data with recent times; Forge shows jobs run; Mimir answers a question.
3. No request leaves the machine: `nettop`/`lsof -i` for the app and its children shows only 127.0.0.1 connections; the content rule list blocks a test request to an external URL.
4. Kill `heimdall-feed` by hand → status "restarting", back within seconds.
5. Reset demo → confirmation → starting screen → home; a Midgard action taken before is gone.
6. Second launch focuses the first window. Quit → no services left (`pgrep`).
7. `du -sh ~/Desktop/Midgard` still within 3.5 GB.

Commit any fixes; update this plan's results section.
