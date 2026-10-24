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
- *As built* (commits 35277f1, 8d30bf8, 08f3ea7, 7ef6bf5, f7b096e):
  - **Shared resources:** `tools/mac-app/resources.sh` (sourced) holds `build-app.sh`'s resource steps: the prepared-answers check, consoles, home (its `fonts` link resolved; on a Windows checkout, which makes no links, copied from `ui/design-system/src/fonts`), credits, starting state, models (`granite`, `trust`) and the recording copy (minus `raw/`, `*.tmp`, `*.old`; optionally unpacking `heimdall.jsonl.gz`). `build-app.sh` now calls them. Checked by building the Mac app before and after into separate folders (`--skip-*`, same inputs): the same log lines, the same 16 405 paths with the same types and modes, and the same SHA-256 for all 15 498 files except `Info.plist` (the `-dirty` commit stamp) and `MacOS/Midgard` (whose signature covers the Info.plist).
  - **`tools/win-app/layout.sh`** (sourced): `win_layout PKG` lays out the package from named inputs (`WL_SHELL`, `WL_SERVICES`, `WL_LLAMA`, `WL_ANALYTICS`, `WL_CONSOLES`, `WL_STATE`, `WL_MODELS`, `WL_RECORDING`, `WL_NOTICES`, `WL_VCRUNTIME`, optional `WL_SHEET`), using `resources.sh` for the shared parts; `win_place_vcruntime` puts the Visual C++ runtime into `VCRUNTIME_FOLDERS` (moved here from `run.sh`, which sources this file). llama.cpp's `LICENSE` and `LICENSE-LLVM-OpenMP` go to `resources\licenses\` as `.txt`, so `helpers\llama\` holds only the exe and DLLs.
  - **Licences, `resources\licenses\` (16 files):** CREDITS.txt (the path the sheet names); llama.cpp and LLVM OpenMP; CPython, python-build-standalone, numpy, onnxruntime (+ ThirdPartyNotices), YOLOX; the Visual C++ runtime notice (the artefact's `VERSIONS.txt`); and the .NET runtime packs' and WebView2's licences. Those come from a new `notices/` folder in the shell artefact (`windows_shell_notices` in `run.sh`): the `*.Runtime.win-x64` download dependencies in Midgard's `obj/project.assets.json` of the shared frameworks it references (Microsoft.NETCore.App and Microsoft.WindowsDesktop.App 10.0.13; ASP.NET Core's pack is listed but unused and not downloaded, so it is left out), and Microsoft.Web.WebView2 1.0.4258.31's `LICENSE.txt` and its third-party notices file, from the NuGet global packages folder, with a `VERSIONS.txt`. `fetch-artifacts.sh` checks they are there.
  - **`tools/win-app/check-package.py`** (stdlib): the parts the shell needs are there (`Paths.cs`, `ServiceSpec.cs`, `ModelServer.cs`); every `.exe`/`.dll`/`.pyd` is PE32+ x86-64 by its headers; no links; no path longer than 200 characters written `Midgard\...`; no forbidden characters, trailing dot or space, or reserved device name; no names in one folder differing only by case; no `.DS_Store`/`._*`; a plain `heimdall.jsonl` and no `.gz`; prepared answers version 1. It prints sizes per top-level part, the total and the longest path. A negative test (a colon, `CON.txt`, a trailing dot, a non-PE `.dll`, a link, a `.DS_Store`) flags each.
  - **`tools/win-app/build-zip.sh [--binaries SHA] [--recording DIR] [--out ZIP] [--skip-consoles] [--keep-package] [--no-desktop]`:**
    - refuses uncommitted changes. The binaries are HEAD's, or with `--binaries SHA` an ancestor's when `git diff --quiet SHA HEAD -- . ':(exclude)docs' ':(exclude)*.md'` holds (only docs changed since);
    - `fetch-artifacts.sh` (the latest all-green run for that commit), `fetch-llama.sh`, `fetch-analytics.sh`, `build-consoles.sh` and `render-sheet.sh`; checks the starting state, model and recording as `build-app.sh` does, and that `zip`/`unzip` have Zip64 support;
    - checks free disk: the package (mostly clones) plus the ZIP, keeping 5 GB free;
    - lays out `~/Library/Caches/midgard-build/win/package/Midgard`, places the runtime, writes `resources\BUILD.txt` (commit, binaries' commit and date, starting state, recording), clears extended attributes, runs `check-imports.py` and `check-package.py` (reports in `win/dist/reports/`);
    - `zip -r -X` (deflate, level 6), `unzip -t`, and the ZIP's file list against the folder's; then `win/dist/Midgard-1.0-Windows.zip` (+ `.sha256`), an APFS clone on the Desktop and the sheet beside it. The package folder is removed afterwards unless `--keep-package`.

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
- *As built:*
  - **Package-shaped CI run:** `windows-shell-e2e` now builds its package with `win_layout`, the same code `build-zip.sh` runs. Inputs differ only where they must: the services and the shell are built in the job rather than fetched; the recording is the fixture (`make-fixture.py`, with its synthetic camera clip), and the starting state and the signed stub model are `make-starting-state.py`'s, made in `fixture/` and then copied in like the real ones. The job also runs `fetch-llama.sh`, so `helpers\llama\` is in the package (the check still runs `stub_llm.py` as the model). The install sheet is the only part left out: it is a PDF rendered on the Mac. The job then runs `check-imports.py` (before the runtime: 84 PE files, FAIL on the four Visual C++ DLLs only; after: PASS, 94 PE files) and `check-package.py` (PASS, 332.6 MB, 1 861 files), then the headless check: 36 of 36 passed (run [40054823071](https://github.com/veldris-ltd/midgard-app/actions/runs/40054823071)).
  - **CI:** run [40054823071](https://github.com/veldris-ltd/midgard-app/actions/runs/40054823071) for f7b096e, all five jobs green. Its services job passed on a re-run: the first attempt failed `heimdall-store`'s `durable_restart` test, which asserts that the 2-byte string `v1` never appears in the encrypted file, so random ciphertext matches it now and then. An earlier run failed `scheduler`'s `rollout_real` once (`report.all_healthy`). Neither touches this work; both passed on re-run.
  - **The ZIP** (f7b096e, binaries from its own run, 111 s): `~/Desktop/Midgard-1.0-Windows.zip`, 2 242 335 335 bytes (2.09 GiB), SHA-256 `f0af3913479152da086b60d866797de9f7469f2c73927eb3369554b31be8aab5`, with `~/Desktop/Install Midgard on Windows.pdf`. It holds 13 885 files and 710 folders, 2.4 GB unpacked. `unzip -t` reports no errors, and its file list equals the folder's. At 2.1 GB and 14 596 entries it needs no Zip64 records (zip adds them only past 4 GB or 65 535 entries), so it is a plain ZIP that Explorer's Extract All reads either way.
  - **Checks:** check-imports PASS (94 PE files, 0 imports not found); check-package PASS (0 problems; every PE file PE32+ x86-64; no links; longest path 124 characters, `Midgard\helpers\analytics\Lib\site-packages\numpy\random\_examples\numba\__pycache__\extending_distributions.cpython-313.pyc`); prepared answers 8 answers, 5 guide questions, 3 alerts. Recording `hour-1`: `heimdall.jsonl.gz` 20 MB unpacked to 121 MB; media 2 946 files (176 MB), radar 312, traffic 1 295 and tiles 7 080.
  - **Sizes (unpacked):** Midgard.exe 112.1 MB; helpers\ 174.8 MB (services, midgard-frames.exe and runtime 23.5 MB, analytics\ 108.2 MB, llama\ 43.1 MB); resources\ 2.1 GB (models 1.5 GB, recording 550 MB, starting-state 110 MB, consoles 9.7 MB, licenses 515 KB, home 404 KB, analytics 57 KB); the sheet 373 KB.
  - **Disk:** 13.4 GB free before, 11.2 GB after (the ZIP in `win/dist/`; the Desktop copy is a clone).
  - **Starting state:** the one `build-app.sh` ships, `starting-state 2026-10-22T20:03:55Z 8ed8b73`, made by `make-starting-state.sh` before W4. It has no Mac paths in its text files and no names Windows refuses, but it has never been started on Windows with the real data.
  - **Not tested, since no Windows PC is available:**
    - the window on a laptop screen (size, scaling, the starting screen);
    - SmartScreen's "Windows protected your PC" box and Run anyway on the unsigned exe;
    - antivirus: first-scan time on 2.4 GB, and whether Defender quarantines an unsigned helper;
    - laptop CPU speed for live Mimir answers (Granite on llama.cpp's CPU build) and for Analyse (CI: 194 ms a frame on 2 threads);
    - the real 2.2 GB data on Windows: the recorded hour, the real starting state, Granite and the camera clips through Media Foundation. CI runs only the fixture and the stub model;
    - Windows N without the Media Feature Pack (the message is unit-tested, not seen);
    - a PC without the Visual C++ Redistributable: every runner has it, so `check-imports.py` stands in;
    - Extract All itself, and long extract paths beyond the 124-character check.

    The first presenter build needs one person to try it on a Windows laptop, or SSH access to a PC.
