# Windows app W4: the ZIP, the instruction sheet, the final checks — implementation plan

**Goal:** `~/Desktop/Midgard-1.0-Windows.zip` holds everything the Mac DMG has. It is built on the dev Mac from CI-built Windows binaries (services, `Midgard.exe`, `midgard-frames.exe`), the pinned Windows llama.cpp and analytics runtime, and the same local resources the Mac bundle uses. It comes with a one-page `Install Midgard on Windows.pdf`.

**Architecture:** `tools/win-app/build-zip.sh` assembles `Midgard\` (`Midgard.exe`, `helpers\`, `resources\`) in `~/Library/Caches/midgard-build/win/package/`, checks it, and writes a ZIP64 archive.
- **Binaries:** the same commit's CI artefacts, fetched with `fetch-artifacts.sh`, extended to the shell artefact.
- **Resources:** produced by the Mac build's own steps (consoles offline build, starting state, recording, prepared answers, models, licences, credits), so the two platforms ship the same data.
- **Recording:** shipped uncompressed (`heimdall.jsonl`).

Nothing licensed goes to GitHub.

**Tech Stack:** bash, `gh`, Info-ZIP `zip`/`unzip` (ZIP64), Python 3 (stdlib checks), headless Chrome (sheet PDF), the existing `tools/mac-app` build steps.

Design: `docs/plans/2026-10-23-windows-app-design.md` §4. The Mac counterparts are `tools/mac-app/build-app.sh`, `build-dmg.sh`, `install-sheet.html` and `CREDITS.txt`.

---

### Task 1: the shell artefact and the Windows llama.cpp

- **Shell artefact:** windows.yml uploads `midgard-shell-windows-x64-<sha>` (Midgard.exe, midgard-frames.exe, BUILD.txt) like the services artefact. `fetch-artifacts.sh` fetches both and checks that each BUILD.txt names the commit.
- **llama.cpp:** add the Windows CPU x64 asset of the pinned build to a lock (`tools/win-app/llama.lock`, or `tools/mac-app/llama.lock` extended), b11669 as on the Mac (`llama-b11669-bin-win-cpu-x64.zip`). Check it exists; if not, use the nearest build with both and record why. Record its SHA-256.
- **`tools/win-app/fetch-llama.sh`:** downloads and verifies, extracts `llama-server.exe` and its DLLs into `~/Library/Caches/midgard-build/win/llama/`, and copies the licence.
- **CI smoke:** a Windows CI step runs `llama-server.exe --version` and, with a tiny model if one is easily made, else `--help`, to prove the binaries start on Windows.

### Task 2: `build-zip.sh`

- **Inputs:** the git commit (default HEAD, which must be clean; refuse if CI artefacts for it are missing or not green), the recording (default hour-1), and the output path.
- **Shared resources:** reuse the Mac build steps, factored out of `build-app.sh` into a shared script if needed rather than duplicated: consoles (offline build, `build-consoles.sh`), home with prepared answers and credits, starting state (`make-starting-state.sh`), models (Granite and YOLOX), licences plus `CREDITS.txt`.
- **Recording:** copy it minus `raw/` and `*.tmp`/`*.old`, unpacking `heimdall.jsonl.gz` to `heimdall.jsonl`.
- **Windows extras:**
  - `helpers\`: the six services, `llama\`, `analytics\` (the W3 runtime) and `midgard-frames.exe`.
  - `resources\analytics\analytics.py`.
  - The WebView2 licence, plus the onnxruntime, numpy and CPython licences from W3.
- **Checks:**
  - every .exe is PE32+ x86-64;
  - no symbolic links;
  - no path longer than 200 characters below `Midgard\`, so it fits under typical extract folders without hitting the 260-character limit;
  - no file name with characters Windows forbids, or a reserved name;
  - no two names differing only by case;
  - total size reported, with the zip free-space check (keep ≥5 GB free).
- **Output:** `zip -r -X` with ZIP64 into `~/Library/Caches/midgard-build/win/dist/`, then `unzip -t`. Copy to `~/Desktop/Midgard-1.0-Windows.zip` and print size and SHA-256.

### Task 3: the instruction sheet

`tools/win-app/install-sheet.html` is rendered to `Install Midgard on Windows.pdf`, one A4 page, the same look as the Mac sheet. The page check is like `build-dmg.sh`'s. It goes inside the ZIP and on the Desktop.

The sheet covers:
- **System needs:** Windows 11 64-bit Intel or AMD, 8 GB+ of memory, about 6 GB free.
- **Install steps:**
  1. Right-click the ZIP and choose Extract All. Pick a short path such as Documents. Don't run it from inside the ZIP.
  2. Open the Midgard folder and double-click Midgard.
  3. At "Windows protected your PC", click More info, then Run anyway.
  4. Optionally accept the Start-menu shortcut.
- **First start:** it can take a few minutes while antivirus scans it.
- **Using it:** Presenter's guide, Reset demo, Quit.
- **Removing it:** delete the folder and `%LOCALAPPDATA%\Midgard`.
- **Troubleshooting:**
  - "cannot run on this PC" messages;
  - N editions and Media Feature Pack (camera clips only);
  - S mode not supported;
  - live answers being slow on laptops.
- **Credits:** the same block as the Mac sheet.

### Task 4: final checks and handover

- **Package-shaped CI run:** `windows-shell-e2e` runs on the package layout built by the same `build-zip.sh` code path with the fixture (no licensed data), so CI tests exactly the layout presenters get. Share the layout function between the e2e job and `build-zip.sh`.
- **Build the real ZIP** at a clean HEAD and run all Task 2 checks.
- **Record results** in this plan and the design doc. Note plainly what has not been tested on real hardware, since no Windows PC is available:
  - the window on a laptop screen;
  - SmartScreen;
  - antivirus;
  - laptop CPU Mimir speed;
  - the real 2.2 GB data set on Windows.

  The first presenter build needs one person to try it on a Windows laptop, or SSH access to a PC.
