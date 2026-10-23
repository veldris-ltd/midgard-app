# Midgard presentation app for Windows 11 — design

Agreed with the user on 23 Oct 2026. The Windows counterpart of the Mac presentation app (`apps/mac`, installer `Midgard-1.0.dmg`, plans `docs/plans/2026-10-22-*` and `2026-10-23-mac-app-phase*`). Branch `md_windows-app`, from `md_mac-installer`.

## Decisions

- **Platform:** Windows 11 on Intel/AMD x64 only. ARM laptops run it emulated, which isn't supported.
- **Signing:** unsigned for now. Presenters click *More info → Run anyway* once.
- **Build and test:** on GitHub Actions `windows-latest` runners. The repo is public, so these runners are free. Only code goes to GitHub. The recording, media, radar, TomTom traffic, imagery tiles and the models never leave the dev Mac, since some of them can't be republished.
- **Shell language:** C# / .NET 8, WinForms with WebView2, published self-contained for win-x64.
- **Delivery:** one ZIP (`Midgard-1.0-Windows.zip`) with no installer. Extract it, then double-click `Midgard.exe`. No admin rights are needed. To remove it, delete the folder and `%LOCALAPPDATA%\Midgard`.
- **Test hardware:** none yet. The first presenter-ready build needs one person to try it on a real Windows laptop. If the user sets up SSH to a PC, that check runs over it.

## 1. The package and how it runs

`Midgard-1.0-Windows.zip` extracts to one `Midgard` folder:

- **`Midgard.exe`:** the C# shell. It does the same jobs as the Mac app:
  - checks the PC;
  - keeps the working copy in `%LOCALAPPDATA%\Midgard`, with Reset demo;
  - starts the services on free loopback ports and signs in the presenter;
  - shows the home page and consoles in WebView2;
  - quits cleanly.
- **`helpers\`:**
  - the Rust services built for Windows: http-gateway, heimdall-feed, heimdall-source, mimir-server, forge-worker, midgard-replay;
  - the official llama.cpp Windows x64 CPU build, pinned by SHA-256;
  - the analytics runtime: python-build-standalone for Windows, with onnxruntime and numpy wheels, pinned;
  - `midgard-frames.exe`.
- **`resources\`:** the same content as the Mac bundle. That's the consoles (offline build), home, starting state, the Granite and YOLOX models, the recorded hour with media, radar, traffic and tiles, licences and credits.

Behaviour:
- **Offline:** WebView2 refuses every request outside the app's own 127.0.0.1 origin, and refuses new windows and outside links. This is done with `WebResourceRequested`, `NavigationStarting` and `NewWindowRequested`.
- **Nothing left running:** every service runs in a Job Object with kill-on-close, so Windows ends them if Midgard.exe exits or crashes.
- **Clip decoding:** `midgard-frames.exe` (C#, Windows Media Foundation) replaces the Mac's AVFoundation helper, so ffmpeg and its licence stay out.
- **Mimir:** runs on the CPU (AVX2). Live answers will be slow on laptops, as on the Intel Mac, so prepared answers matter more. A Vulkan GPU build can come later.

## 2. Rust services on Windows

Nine files use Unix-only features. Each gets a `#[cfg(windows)]` branch beside the existing `#[cfg(unix)]` code, using the small `windows-sys` crate, vendored as the offline-build rule requires.

- **Directory locks** (`services/runtime/src/lock.rs`): use `LockFileEx`; liveness comes from the process handle and creation time.
- **Secret and key files** (`crypto/pqc-agility/src/secret.rs`, `crypto/attest/src/keystore.rs`): the ACL is limited to the current user, and a file anyone else can read is refused.
- **Durable writes** (`services/fabric/src/durable.rs`, `crypto/audit-writer/src/{file,journal}.rs`, `heimdall/store/src/{util,history_store}.rs`): use `FlushFileBuffers`, and `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` for rename-over. Close a file before replacing it.
- **Heimdall alerts** (`heimdall/feed-server/src/alerts.rs`): Windows equivalents, or skipped where Windows has no such concept.
- **Path, line-ending and file-lock hazards:** handle `\\?\` long paths where paths are built, CRLF line endings, and deletes that fail while a file is open.

Mac and Linux behaviour must not change. The existing Linux CI and the Mac tests keep running.

**New CI job (Windows):** on `windows-latest` it builds the services, runs the Rust tests, starts the suite with a tiny fixture recording, and runs the gateway smoke test against it. The smoke test covers sign-in, picture, ask (with a stub model server), cameras and replay. The job uploads the Windows binaries as an artefact.

## 3. The C# shell (`apps/windows/`)

- **SystemCheck:** Windows 11 build 22000 or later, x64, at least 8 GB of memory and 4 GB free disk, and the WebView2 runtime present. Anything missing is explained in plain words.
- **Paths and Home:** the working copy is `%LOCALAPPDATA%\Midgard`, created from `resources\starting-state`. It's checked with the `.midgard-home` marker, and a folder that isn't a working copy is refused. Reset demo is supported, and the version is checked.
- **Ports and Supervisor:**
  - **Ports:** free ports are picked on 127.0.0.1.
  - **Start order:** llama, mimir, gateway, heimdall, forge, replay, with analytics last and optional.
  - **Readiness and retry:** each service has a readiness check and a timeout, and is retried once.
  - **Shutdown and leftovers:** the Job Object ends every service when the app exits. Leftovers from a previous crash are swept by path.
  - **Logs:** they go to `%LOCALAPPDATA%\Midgard\Logs`.
- **WebServer:**
  - **Routes:** a loopback static server with the Swift server's rules, serving `/home`, the four consoles, `/media/`, `/radar/`, `/traffic/` and `/tiles/`.
  - **Safety:** Host check, no traversal, symlinks and junctions must resolve inside their folder, and byte ranges are supported.
- **Session:** signs in the presenter, then injects the gateway URL, the console URL, the prepared-answers URL and the session token with `AddScriptToExecuteOnDocumentCreatedAsync`, only for the app's origin.
- **Window:**
  - **Pages:** home with the Presenter's guide, and a toolbar with Home, Reset demo and Quit.
  - **Starting screen:** "Starting Midgard…" with each service's status.
  - **First run:** offers a Start-menu shortcut.
- **Frames helper:** `midgard-frames.exe` writes the same raw BGR frames `analytics.py` reads, in the same probe and decode shape as the Mac helper.
- **Tests:**
  - **xUnit:** for paths, ports, the web server's rules, the session script, and supervisor order and retry.
  - **Headless end-to-end on Windows CI:** the shell starts the suite, signs in and quits with nothing left running, without opening a window.

## 4. Packaging, instruction sheet, testing, risks

- **Packaging (on the dev Mac):**
  1. `tools/win-app/fetch-artifacts.sh` downloads the CI artefacts for a commit and checks the commit.
  2. `tools/win-app/build-zip.sh` adds the shared resources and the pinned Windows llama.cpp and Python runtimes.
  3. It writes `~/Desktop/Midgard-1.0-Windows.zip` (ZIP64) and checks every file and its size.
  4. It refuses mismatched commits, as `build-dmg.sh` does.
- **Instruction sheet:** `Install Midgard on Windows.pdf`, one A4 page. It covers:
  - Extract All, and the "Windows protected your PC" box;
  - the Start-menu shortcut offer, Reset demo and Quit;
  - removing it;
  - Windows N editions without the media features clips need;
  - the credits.
- **What automated testing can't cover:** how the window looks, SmartScreen, antivirus first-scan time, and real laptop CPU speed for Mimir. One human run on a Windows laptop (or SSH to a PC) is needed before the first presenter build.
- **Risks:**
  - slow live Mimir answers on laptop CPUs;
  - antivirus scanning 2.2 GB on first launch;
  - Windows 11 S mode can't run it;
  - long paths;
  - Windows Defender quarantining unsigned helpers. If that happens, it is reported to the user; detection is not evaded.
