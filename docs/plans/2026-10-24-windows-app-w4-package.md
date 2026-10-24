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
- *As built* (commits b98bdba, 163337c, 6a1b599; green run [40049563871](https://github.com/veldris-ltd/midgard-app/actions/runs/40049563871), every job):
  - **Shell artefact:** `windows-shell-build` writes `target/windows-shell-artifact/`: Midgard.exe, midgard-frames.exe, `vcruntime/` (below) and a BUILD.txt with commit, runtime, SDK (10.0.402), the runtime's toolset and the date. windows.yml uploads it as `midgard-shell-windows-x64-<sha>` (pinned upload-artifact, 30 days), and the GitLab job keeps it under the same name.
  - **`fetch-artifacts.sh <sha>`:** takes the latest successful run for the commit (a run succeeds only when every job is green). It downloads both artefacts into `~/Library/Caches/midgard-build/win/<sha>/{services,shell}/` and checks that each BUILD.txt names the commit. It also checks that the six services, the two shell exes and the four runtime DLLs are all there, then lists them. For 6a1b599, `file` reports PE32+ x86-64 for all 8 exes and the 4 DLLs. Midgard.exe is 117.6 MB, midgard-frames.exe 12.8 MB. An older fetch (`win/4be06ce…/`) is still in the earlier flat layout.
  - **llama.cpp:** b11669's release has `llama-b11669-bin-win-cpu-x64.zip` (19.5 MB), so the Mac's build is kept. Its SHA-256 is `37b3efcf962847ca869745fd4ccf7f5e7e8dacfeff9fa3523b4ca8cdc66d632b`, which matches GitHub's digest. It is pinned in `tools/mac-app/llama.lock` (`win_x64_url`, `win_x64_sha256`) beside the macOS archives, so `build=b11669` stays the one build number, and `fetch-llama.sh` checks that every URL names it. The ZIP carries no llama.cpp licence, only `LICENSE-LLVM-OpenMP` for libomp.dll. So the tag's `LICENSE` is pinned too (`license_url`, SHA-256 `94f29bbe…010d`), which is the same file as the macOS archives' licence.
  - **`tools/win-app/fetch-llama.sh`** runs on the Mac and in CI, and lays out `win/llama/server/`: 24 files, 42 MB, plus `manifest.txt` and `server.stamp`; downloads go in `win/llama/downloads/`.
    - It checks the ZIP's member names first: one flat folder of plain names, with no links.
    - `tools/win-app/pe-deps.py` (stdlib) walks the import tables from `llama-server.exe`. That exe is a 9 KB stub importing `llama-server-impl.dll`, which pulls in `llama-common`, `mtmd`, `llama`, `ggml`, `ggml-base` and `libomp`.
    - All 14 `ggml-cpu-*.dll` (x64, sse42 … zen4) are added. They are not imported: `ggml-base.dll` loads the best one for the CPU at run time.
    - Left out: the other tools, `ggml-rpc.dll` (ggml would otherwise load the network backend from the folder) and `ggml-rpc-server.exe`.
  - **windows-llama CI job** (both pipelines): `fetch-llama.sh` on the runner, which runs `--version` → `build 11669, commit 620d2225c, Clang 20.1.8 for Windows x86_64`. Then `tools/win-app/check-llama.py` starts the server with the shell's own arguments (`ModelServer.cs`; two threads), with the app-local runtime beside it. The model is llama.cpp's tiny test model stories260K.gguf: 1.2 MB, ggml-org/models at a pinned revision, SHA-256 in `run.sh`, CI only, never shipped.
    - `/health` was ready in 0.3 s.
    - `/props` reported the alias and 1 slot.
    - A streamed `/v1/completions` (mimir-server's call) returned 16 pieces opening with `{` (the JSON grammar applied), then `[DONE]`.
    - b11669 does not log which `ggml-cpu-*.dll` it chose at its default verbosity, so that line is informational. A completion needs a CPU backend that loaded and ran, so passing already shows one did.
  - **The Visual C++ runtime, app-local.** A clean Windows 11 has no Visual C++ Redistributable, and `pe-deps.py` showed the package's binaries import it:
    - the services: VCRUNTIME140;
    - llama.cpp: MSVCP140, VCRUNTIME140, VCRUNTIME140_1;
    - onnxruntime: MSVCP140 and MSVCP140_1. The analytics runtime brings only vcruntime140 and vcruntime140_1 (python-build-standalone, 14.44), and numpy its own renamed msvcp140. Without them the analytics would fail on a clean PC; the runners hide this, since they have the redistributable installed.
    - Midgard.exe and midgard-frames.exe import none of them.

    `windows-shell-build` copies those four x64 DLLs (`VCRUNTIME_DLLS` in `run.sh`) from the runner's Visual Studio into `vcruntime/`. Visual Studio is found with vswhere (18.11.1), and the source is its default redist version's `VC\Redist\MSVC\14.51.36231\x64\Microsoft.VC145.CRT`; all four are FileVersion 14.51.36247.0. `VERSIONS.txt` records the toolset, each file's version and SHA-256, and the notice: Visual Studio licence terms, Distributable Code, and the Redist list. The installation carries no licence text for them (only the Windows 8/8.1 SDK licences in `Licenses\`), so the notice cites the terms rather than copying them. They go into `helpers\`, `helpers\llama\` and `helpers\analytics\` (`VCRUNTIME_FOLDERS`, `windows_place_vcruntime`, for build-zip.sh), replacing python-build-standalone's older copies.
  - **`tools/win-app/check-imports.py PACKAGE`:** every import of every PE file must resolve in one of these ways:
    - in its folder, or in a folder the loader also searches: python.exe's, for extension modules, and numpy.libs, which numpy adds with `os.add_dll_directory`;
    - on an explicit clean-Windows-11 list: API sets, KnownDLLs of 24H2 and standard System32 DLLs. It has no MSVCP/VCRUNTIME, and no `mf*.dll`, since N editions lack them;
    - as a known optional delay-load (none today).

    Otherwise it fails with the list. CI runs it on both package layouts, before and after the runtime goes in. Before, it must fail, and on Visual C++ runtime DLLs only. After, it must pass:
    - windows-shell-e2e: 62 PE files, failing on msvcp140, msvcp140_1 and vcruntime140; then 68 files, PASS. The headless check then passed 36 of 36 on the app-local runtime.
    - windows-llama: 22 files, failing on msvcp140, vcruntime140 and vcruntime140_1; then PASS.
  - **Not tested:** a PC without the redistributable, since every runner has it. check-imports.py stands in for that.

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
