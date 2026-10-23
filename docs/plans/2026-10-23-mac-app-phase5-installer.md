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

- `tools/mac-app/install-sheet.html` (one A4 page, plain language, design-system look): what Midgard is (one line), system needs (macOS 13.3+, 8 GB+, ~4 GB free), steps: 1. double-click `Midgard-1.0.dmg`; 2. drag Midgard to Applications; 3. open Applications and double-click Midgard; 4. macOS says it can't verify the app — click **Done** (macOS 15+) or **Cancel**; 5. open **System Settings → Privacy & Security**, scroll to **Security**, click **Open Anyway** next to "Midgard was blocked…", enter your Mac password; 6. Midgard opens (the first launch may show "Verifying…" for a minute because of the 1.5 GB model). Then: "Starting Midgard…" takes up to 40 s on an Intel Mac; the home screen's Presenter's guide; Reset demo; Quit. Troubleshooting: "damaged" message → download the DMG again (don't use a different unzip tool); not enough memory/older macOS → the app says so. Rendered to `Install Midgard.pdf` with headless Chrome.
- `tools/mac-app/build-dmg.sh`: stages `Midgard.app` (from build-app.sh), an `Applications` symlink and `Install Midgard.pdf` in a temp folder; `hdiutil create -volname "Midgard" -fs HFS+ -format UDZO -imagekey zlib-level=9`; verifies the image (`hdiutil verify`), mounts it read-only and checks the app inside verifies with codesign; writes `~/Library/Caches/midgard-build/dist/Midgard-1.0.dmg` and copies it to `~/Desktop/Midgard-1.0.dmg`; prints sizes and SHA-256.

### Task 4: Tests

1. Fresh install simulation on this Mac: quit any Midgard; move the working copy aside (never delete); mount the DMG, copy the app to a temp "Applications" folder (not the real /Applications), open it: ready, Heimdall shows aircraft/ships from the recorded hour, a guide question and a guide alert answer instantly, Mimir answers a new question live, Reset demo, quit clean.
2. Intel simulation: run the same copy as `MIDGARD_FORCE_INTEL_MODEL=1 MIDGARD_MODEL_THREADS=4 arch -x86_64 <copy>/Midgard.app/Contents/MacOS/Midgard` (Rosetta; a translated shell would otherwise still pick the arm64 server, because the hardware is Apple Silicon; the variables force the x64 server, CPU only, 4 threads as on an i5). shell.log says "forced Intel model server (MIDGARD_FORCE_INTEL_MODEL)"; the shell and the x64 model server run as x86_64 (`ps -o arch`). Whether the universal service helpers also run as x86_64 under a translated shell is an observation to record, not a requirement (`ps -o arch`, or `file`/`vmmap`). Ready time and a live Mimir answer time recorded (expect minutes).
3. Quarantine: `xattr -w com.apple.quarantine "0081;$(printf %x $(date +%s));Safari;" Midgard-1.0.dmg` on a copy, mount, copy the app out, `spctl --assess -vv` and `xattr -l` on it; record the result (expected: rejected, "Open Anyway" route). Note the dialog text cannot be screenshotted here.
4. Record all results in this plan; restore the original working copy afterwards; the suite folder stays ≤ 3.5 GB.

**Results (23 Oct, app 2029e41, DMG 1639 MB):**

1. Fresh install: working copy moved aside to `Midgard.pre-task4`; app copied from the mounted DMG into `~/Library/Caches/midgard-build/test-install/Applications`, verifies. First launch ready in 14.0 s (bundled arm64 llama-server, Metal). UK view: 141 aircraft, 246 ships, 90 power cuts after 90 s (162 / 1,805 / 103 later). Home page served the prepared answers (8 entries, 5 guide questions). A new question ("How many ships are near the Dover Strait?") answered live in 9 s, grounded. The three guide alerts' subjects were open from +0.0 min (Vietnam) and +4.1 min (wind, flood), so the guide now says "These alerts appear within about 5 minutes of starting Midgard." Quit: all helpers gone in 1 s, final sweep found nothing. The instant console answers and Reset demo were not clicked here (no screen access; covered by the console tests and the phase 3/4 hands-on checks).
2. Intel simulation: shell.log "forced Intel model server (MIDGARD_FORCE_INTEL_MODEL)", x64 llama-server, CPU, 4 threads; ready in 25.3 s. Every process ran as X86-64 (translated) per `vmmap`: shell, llama-server and all six service helpers, so every Intel slice ran. A live question failed with 502 at the gateway's 240 s `ADVICE_TIMEOUT`: under Rosetta the x64 server read the 3,165-token prompt at 12.7 tokens/s (248 s) and wrote at 4 tokens/s (~290 s in all). A real i5 runs native AVX2 and should be several times faster, but this was not measured; the home page now says live answers take "several minutes on an Intel Mac". Open: measure on a real Intel Mac; if needed raise the ask timeouts (gateway, Heimdall→Mimir, console) or shorten the A.1 prompt. Prepared answers are unaffected. Quit clean (0.4 s).
3. Quarantine: a quarantined copy of the DMG is `rejected` by `spctl` ("no usable signature"); the app copied out of it carries `com.apple.quarantine` and is `rejected` (ad-hoc signature, no Team ID), i.e. the install sheet's "Open Anyway" route. The dialog text could not be captured here.
