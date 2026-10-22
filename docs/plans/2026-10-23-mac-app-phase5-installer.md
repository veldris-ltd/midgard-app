# Mac app phase 5: bundled model server, installer, install sheet — implementation plan

**Goal:** `Midgard-1.0.dmg`: a drag-to-Applications installer that runs on Intel i5 and Apple Silicon Macs (macOS 13.3+, 8 GB+) with no Homebrew, Docker, internet or Terminal; plus a one-page install sheet for non-technical presenters.

**Architecture:** Pin one official llama.cpp release (both macOS builds), verify it by SHA-256, and bundle `llama-server` with its libraries per architecture in `Contents/Helpers/llama/{arm64,x64}`. The shell picks the folder for the Mac it runs on (Apple Silicon: arm64 with Metal; Intel: x64, CPU only). `build-dmg.sh` makes a compressed DMG with the app, an Applications link and the install sheet (PDF). Tests: the app on this Mac, the app under Rosetta (exercises every Intel slice, the x64 model server included), and a quarantined round trip of the DMG.

**Tech Stack:** bash, `curl` + `shasum`, `hdiutil`, `codesign`, `spctl`, Swift (shell changes), headless Chrome (HTML → PDF).

Design: `docs/plans/2026-10-22-mac-presentation-app-design.md` §1, §4, §5 and "Phase 5 notes". Phase 3 and 4 plans in `docs/plans/`.

---

## Background

- Releases: `https://github.com/ggml-org/llama.cpp/releases/tag/b<N>` with `llama-b<N>-bin-macos-arm64.tar.gz` (~15 MB) and `llama-b<N>-bin-macos-x64.tar.gz` (~11 MB). The development Mac's Homebrew llama-server is build **11669**, which the suite has run with for days: pin **b11669**, unless its assets are missing, then the nearest older build with both macOS assets (record why).
- The shell (`apps/mac`, Swift 6) starts llama-server from `Contents/Helpers/llama-server` or falls back to Homebrew (`AppModel.swift`, `SuiteSpecs.make` in `ServiceSpec.swift`). The model is `Resources/models/granite/granite-3.3-2b-instruct-Q4_K_M.gguf`.
- `tools/mac-app/build-app.sh` assembles and ad-hoc signs the app (helpers signed one by one, then the bundle; verify before swap). Bundle output: `~/Library/Caches/midgard-build/app/Midgard.app`.
- Storage: the suite folder `~/Desktop/Midgard` stays ≤ 3.5 GB; downloads and build outputs go under `~/Library/Caches/midgard-build`; the finished DMG is copied to `~/Desktop/Midgard-1.0.dmg` (outside the suite folder) for the user.
- Commits signed, pushed to `md_mixedfeeds_3d`.

### Task 1: Fetch and pin the model server

- `tools/mac-app/llama.lock`: build number, the two asset URLs and their SHA-256 (computed once on download and committed).
- `tools/mac-app/fetch-llama.sh`: downloads both archives into `~/Library/Caches/midgard-build/llama/<build>/` (skips if present and matching), verifies SHA-256 against `llama.lock` (fails on mismatch), extracts, and lays out `llama/arm64/` and `llama/x64/` with `llama-server` and every dylib it needs (check with `otool -L`; rewrite any absolute paths to `@loader_path` with `install_name_tool` if the release doesn't already use `@rpath`/`@loader_path`). Prints sizes and `llama-server --version` for arm64 (and x64 under Rosetta).
- Verify both servers load the bundled model briefly (arm64 natively, x64 with `arch -x86_64`), `GET /health` 200, then stop them; use ports that are free.

### Task 2: The app uses the bundled server

- `build-app.sh`: runs `fetch-llama.sh` (unless `--skip-llama`), copies `llama/{arm64,x64}` into `Contents/Helpers/llama/`, signs every Mach-O there before the bundle.
- Shell: choose `Helpers/llama/arm64/llama-server` on Apple Silicon (including when the shell itself runs under Rosetta — check `sysctl.proc_translated`/`hw.optional.arm64`) and `Helpers/llama/x64/llama-server` on Intel; on Intel pass `--n-gpu-layers 0` (CPU) and a thread count suited to the Mac (e.g. physical cores). Keep the Homebrew fallback only for dev runs (no bundle). Reaping/sweeps already match `llama-server` by basename and the models path.
- Tests: unit tests for the choice logic (inject the arch facts) and the Intel arguments.

### Task 3: Installer and install sheet

- `tools/mac-app/install-sheet.html` (one A4 page, plain language, design-system look): what Midgard is (one line), system needs (macOS 13.3+, 8 GB+, ~3 GB free), steps: 1. double-click `Midgard-1.0.dmg`; 2. drag Midgard to Applications; 3. open Applications and double-click Midgard; 4. macOS says it can't verify the app — click **Done** (macOS 15+) or **Cancel**; 5. open **System Settings → Privacy & Security**, scroll to **Security**, click **Open Anyway** next to "Midgard was blocked…", enter your Mac password; 6. Midgard opens (the first launch may show "Verifying…" for a minute because of the 1.5 GB model). Then: "Starting Midgard…" takes up to 40 s on an Intel Mac; the home screen's Presenter's guide; Reset demo; Quit. Troubleshooting: "damaged" message → download the DMG again (don't use a different unzip tool); not enough memory/older macOS → the app says so. Rendered to `Install Midgard.pdf` with headless Chrome.
- `tools/mac-app/build-dmg.sh`: stages `Midgard.app` (from build-app.sh), an `Applications` symlink and `Install Midgard.pdf` in a temp folder; `hdiutil create -volname "Midgard" -fs HFS+ -format UDZO -imagekey zlib-level=9`; verifies the image (`hdiutil verify`), mounts it read-only and checks the app inside verifies with codesign; writes `~/Library/Caches/midgard-build/dist/Midgard-1.0.dmg` and copies it to `~/Desktop/Midgard-1.0.dmg`; prints sizes and SHA-256.

### Task 4: Tests

1. Fresh install simulation on this Mac: quit any Midgard; move the working copy aside (never delete); mount the DMG, copy the app to a temp "Applications" folder (not the real /Applications), open it: ready, Heimdall shows aircraft/ships from the recorded hour, a guide question and a guide alert answer instantly, Mimir answers a new question live, Reset demo, quit clean.
2. Intel simulation: run the same copy with `arch -x86_64` (Rosetta): the shell, every service and the x64 model server run as x86_64 (check with `ps -o arch` or `file`/`vmmap`), ready time and a live Mimir answer time recorded (expect minutes).
3. Quarantine: `xattr -w com.apple.quarantine "0081;$(printf %x $(date +%s));Safari;" Midgard-1.0.dmg` on a copy, mount, copy the app out, `spctl --assess -vv` and `xattr -l` on it; record the result (expected: rejected, "Open Anyway" route). Note the dialog text cannot be screenshotted here.
4. Record all results in this plan; restore the original working copy afterwards; the suite folder stays ≤ 3.5 GB.
