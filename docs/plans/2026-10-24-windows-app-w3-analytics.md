# Windows app W3: camera analytics on Windows — implementation plan

**Goal:** camera analysis (Analyse, Watch, Trend) runs in the Windows app as it does on the Mac. That means the same `heimdall/analytics/analytics.py` and the YOLOX-s model, on a bundled, pinned Python runtime with onnxruntime and numpy for win-x64, decoding the stored clips with a Windows-native frames helper. It is supervised by the shell as an optional service started last.

**Architecture:**
- **Frames helper:** `midgard-frames.exe`, a small C# (.NET 10) console app in `apps/windows/MidgardFrames` using Media Foundation (`IMFSourceReader` with RGB32 output). It writes the same raw BGR frame stream and probe output as the Mac's `apps/mac/Sources/MidgardFrames`, so `analytics.py` needs no Windows branch beyond the helper path.
- **Runtime:** python-build-standalone `x86_64-pc-windows-msvc` `install_only` plus numpy and onnxruntime win_amd64 wheels, all pinned by SHA-256 in `tools/win-app/analytics.lock`. It is assembled by `tools/win-app/fetch-analytics.sh` on the Mac: Python wheels are pure archives, so they unzip anywhere. Nothing is installed by pip at run time, and the CPU execution provider is used (no CoreML on Windows; DirectML is out of scope).
- **Shell:** an `analytics` spec in `SuiteSpecs` (optional, last), the token file, and gateway `--analytics-addr` / `--analytics-token-file`, mirroring `apps/mac` commits 563bb55 and c739170.

**Tech Stack:** C# / .NET 10 and Media Foundation via P/Invoke/COM, Python 3.13 (python-build-standalone), onnxruntime 1.23.2 or newer with a win_amd64 cp313 wheel, numpy, bash.

Design: `docs/plans/2026-10-23-windows-app-design.md` §1, §3. For reference, the Mac Task 2 is in `docs/plans/2026-10-23-mac-app-phase6-local-parity.md` and commits 612ee4f, f85d66e, 563bb55, c739170 on md_mac-installer (now on this branch).

---

### Task 1: midgard-frames.exe (Media Foundation)

- **Project:** `apps/windows/MidgardFrames` (console, net10.0-windows, published self-contained single-file win-x64), with tests in `Midgard.Core.Tests` or its own test project.
- **Behaviour:** the same CLI, probe output and decode semantics as `apps/mac/Sources/midgard-frames` and `MidgardFrames/Frames.swift`. Read those and `analytics.py`'s `decode()` and probe call: fps sampling, max frames, letterbox resize to the target size, packed BGR, the size from the first decoded frame, autoreleasepool-like resource release per frame, and errors to stderr with a non-zero exit.
- **Decoding:** MFStartup, then `MFCreateSourceReaderFromURL`. Set the output media type to RGB32 with video processing enabled (`MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`), read samples, convert RGB32 to BGR, and resize bilinearly in managed code, matching the Mac's.
- **Stills:** JPEG via WIC (`IWICImagingFactory`), for stills-only cameras, as on the Mac.
- **Windows N:** on an N edition without Media Feature Pack, MF has no H.264 decoder. Detect it and exit with a clear message ("This Windows edition lacks the Media Feature Pack; camera clips can't be decoded"), so the app can show it.
- **Tests on CI:** a stored-style H.264 MP4 fixture, generated in CI or a tiny synthetic clip committed under 200 KB and made by our own script (no TfL data in git). The tests check the output shape (frames × w × h × 3) and the frame selection matching the Mac helper's rules. Then compare detections helper vs ffmpeg on the CI fixture (ffmpeg is available on runners) and record the agreement.
- *As built:*
  - **Project:** `apps/windows/MidgardFrames` (`midgard-frames.exe`; `Frames.cs` is the shared part of `Frames.swift`, `MediaFoundation.cs`, `Wic.cs`, `Com.cs`, `Cli.cs`). COM is called through vtable slots by function pointer (the slot numbers are the mingw-w64 IDL order), with no runtime callable wrappers. Each sample, buffer and media type is released as soon as it is used. The held frame is copied out of its sample, so no decoder buffer is held across reads. Published trimmed (`PublishTrimmed`, so `Microsoft.NET.ILLink.Tasks` is in its lock file), self-contained and single-file, beside `Midgard.exe` in `target/windows-shell`. It is **12.8 MB**; untrimmed it was 73.6 MB. `windows-shell-build` checks that the published exe exits 2 with its usage.
  - **Decoding:** an `IMFSourceReader` with `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING` and RGB32 output. Only the first video stream is selected. Samples are read through `IMF2DBuffer.Lock2D`, which gives the top row and pitch, so bottom-up frames work. If the 2D buffer isn't available, `Lock` with `MF_MT_DEFAULT_STRIDE` is used. The size is the current type's `MF_MT_MINIMUM_DISPLAY_APERTURE` (or `MF_MT_FRAME_SIZE`), re-read on `CURRENTMEDIATYPECHANGED`, and the frame is cropped to it. So a 1080p clip, coded as 1088 rows, probes as 1920,1080, as ffprobe does. Times are in 100 ns units for the selector. Probe sniffs JPEG, PNG, BMP, GIF and TIFF magic bytes and goes to WIC; anything else tries Media Foundation, then WIC. Stills come from WIC's first frame converted to 32bpp BGR.
  - **No decoder:** a missing `mfplat.dll` or `mfreadwrite.dll` (an N edition) gives *"This Windows edition lacks the Media Feature Pack; camera clips can't be decoded"*. So does `SetCurrentMediaType` failing with `MF_E_TOPO_CODEC_NOT_FOUND` or with no decoder MFT for H.264 (`MFTEnumEx`). Another codec without a decoder is named: an HEVC clip on the CI runner gives *"no Media Foundation decoder for hevc.mp4's video (HEVC)"*. The GitHub Windows Server 2025 runner decodes H.264 without the `Server-Media-Foundation` feature: run 40041709921 probed a clip before the feature was added, exit 0. So the feature is not installed. The N path itself (no `mfplat.dll`) is untested; no runner has one.
  - **Tests:** `apps/windows/MidgardFrames.Tests`, 32 tests, which compile the helper's sources in. They include every selection and resize case of `FramesTests.swift`, a negative stride, the CLI's usage and errors, and synthetic fixtures made in the test by ffmpeg 9.0.2. That ffmpeg is gyan.dev's essentials build, pinned by SHA-256 in `run.sh`, unpacked with 7-Zip, used for tests only, and no footage is in git. The fixtures are a grey-per-frame clip at 352×288 and 25 fps (4 s, and 0.32 s), testsrc2 at 352×288 (10.4 s) and at 1920×1080, a 640×480 JPEG and an HEVC clip.
    - **Frame selection:** input frames 3, 9, 15, 21, 28, … 96 (16 frames, 4 s) and 3 alone for 0.32 s, the same as ffmpeg's `fps=4`. Ours are identified by their grey level, ffmpeg's by an exact match among all its decoded frames.
  - **Pixel difference from ffmpeg** (`-vf fps=4,scale=W:H:flags=bilinear -pix_fmt bgr24`, as `analytics.py`), recorded by run 40042594691:

    | Fixture | Size | Mean \|diff\| | Worst frame | Largest pixel |
    |---|---|---|---|---|
    | testsrc2 352×288 → 640×523, 40 frames | as the TfL clips | **2.03** | 2.23 | 229 |
    | testsrc2 1920×1080 → 640×360, 4 frames, ffmpeg default (BT.601) | HD | 10.35 | 10.38 | 231 |
    | the same, ffmpeg told `in_color_matrix=bt709` | HD | **2.65** | 2.69 | 244 |
    | grey clip 352×288 → 640×523 | flat | 1.00 (worst frame) | | |
    | JPEG 640×480 (WIC) | still | **1.53** | | 158 |

    The SD difference is chroma upsampling and the order of scaling and colour conversion. ffmpeg scales in YUV and converts once; Media Foundation converts first, then we scale RGB. The largest pixel differences are on testsrc2's hard colour edges. Untagged HD differs by its matrix: Media Foundation takes BT.709, ffmpeg's swscale BT.601. The cameras' clips are 352×288, where both take BT.601. The test limit is a mean of 4 (testsrc2 SD and HD-as-709) and 4 for the still. The detection agreement on the same fixture is Task 2's, with the model.
  - **CI:** `windows-shell-build` publishes the helper. `windows-shell-test` fetches ffmpeg into `.ci-cache`, runs both test projects, and prints the comparison, which is also saved as `target/windows-shell-test/frames-comparison.txt`. It adds about 10 s: 3 s for the fetch, 8 s for the tests.
  - **Watch:** `Microsoft.NET.ILLink.Tasks` in the lock file follows the runner's .NET SDK patch (10.0.13 now). When the runners move to a new 10.0.4xx patch, the locked restore fails and prints the new entry for a commit, as for any lock change.

### Task 2: the pinned Windows analytics runtime

- **`tools/win-app/analytics.lock`:**
  - python-build-standalone `cpython-3.13.x+<release>-x86_64-pc-windows-msvc-install_only.tar.gz`, the same release as the Mac's lock if it has the Windows build;
  - numpy and onnxruntime `cp313-win_amd64` wheels;
  - SHA-256 for each.
  - Check onnxruntime 1.23.2 (the Mac's pin) has cp313 win_amd64. If not, use the newest that does and record why.
- **`tools/win-app/fetch-analytics.sh`:** modelled on `tools/mac-app/fetch-analytics.sh`. It downloads into `~/Library/Caches/midgard-build/win/analytics/`, verifies hashes, extracts safely (checking names), and unzips the wheels into the runtime's `Lib\site-packages`. It prunes pip, ensurepip, tests and `__pycache__`, then compiles .pyc for Windows paths, or ships .py only (justify). It copies licences (CPython, numpy and its OpenBLAS notice, onnxruntime plus ThirdPartyNotices, and the YOLOX licence) and writes a manifest.
- **CI check (Windows):** a `windows-analytics` run.sh job (mirrored in GitLab). It builds the runtime on Linux or Windows CI with the same script, or fetches with the same lock, then runs `python.exe -I -c "import numpy, onnxruntime; print(...)"` and runs `analytics.py` once on the fixture clip with `midgard-frames.exe` and the YOLOX model. The model is fetched by its pinned URL and SHA-256 in CI, as `fetch-analytics.sh` does. It must produce a `land-camera-ai` observation.
- *As built:*
  - **Lock:** `tools/win-app/analytics.lock` pins the Mac's versions. That is python-build-standalone 20261009's CPython 3.13.16 `x86_64-pc-windows-msvc-install_only`, numpy 2.5.4 and onnxruntime 1.23.2 `cp313-win_amd64`, the YOLOX-s model and its licence, each by SHA-256. onnxruntime 1.23.2 has a cp313 win_amd64 wheel, so nothing newer was needed. numpy's wheel carries OpenBLAS (scipy-openblas) and an msvcp140 in `numpy.libs`.
  - **`tools/win-app/fetch-analytics.sh`:** runs on a Mac or in Windows Git Bash (curl, tar, unzip, sha256sum or shasum). Before extracting, it checks every member's name (relative, under the expected top folder, no `..`, backslash, drive letter or control character) and type (files and folders only; the Mac build tool's same-folder links alone are allowed). Each archive goes into its own empty folder. The wheels are unzipped as pip installs them: neither has a `.data` folder or a link, which is checked. Pruned: headers, import libraries, `.pdb`, Tcl/Tk, the `_test*` modules, pythonw, `Scripts`, pip, ensurepip, venv, idlelib, pydoc_data, numpy.f2py, onnxruntime's training and quantisation tools, every `tests` folder and the shipped `__pycache__`.
  - **.pyc:** compiled, not .py only. They are `unchecked-hash` .pyc, valid whatever the file times, which a ZIP keeps to 2 s only. The compiler is a CPython 3.13 that runs on the build machine, so the bytecode magic is the same. On a Mac that is the Mac lock's 3.13.16 for that Mac; on Windows it is the runtime itself. The shell runs `-I -B`, so nothing is written into the package at run time, and imports don't recompile 690 modules at each start.
  - **Licences and manifest:** licences for CPython, numpy (its `LICENSE.txt` carries OpenBLAS's and the GCC runtime's notices, which is checked), onnxruntime with ThirdPartyNotices, and YOLOX. There is also a python-build-standalone note, with OpenSSL, SQLite, libffi and others, and the VC++ runtime. `manifest.txt` lists the pins, then every file with its SHA-256 and size.
  - **Output:** `$MIDGARD_BUILD_DIR/win/analytics/{runtime,models,licenses,manifest.txt}`. Re-runs do nothing while the stamp matches. On the Mac it takes 9 s with the downloads present.
  - **Sizes**, from CI run 40046709287: the runtime is **112.7 MB**, 38.6 MB deflated in a ZIP. site-packages is 72 MB of that: onnxruntime 34 MB, numpy 17 MB, numpy.libs 20 MB. The model is 35.9 MB (33.4 MB deflated), the licences 0.4 MB, 1691 files in all. The e2e package grew to 298 MB.
  - **analytics.py on Windows:** the plan expected no Windows branch beyond the helper path. Three were needed (commit b89ba14):
    - there is no `resource` module, so CPU times count the process only;
    - `--nice` sets the below-normal priority class (`SetPriorityClass`) where `os.nice` is missing;
    - a recording's plain `heimdall.jsonl` is read when there is no `.gz` (Windows recordings are uncompressed), as UTF-8 (the Windows code page otherwise).
  - **Fixture:** `tools/win-app/make-camera-clip.py` draws a 10 s, 25 fps, 352×288 road scene with numpy: cartoon cars in two lanes and one parked. The tests' ffmpeg encodes it with libx264 (yuv420p), about 110 KB, made in CI, with nothing in git. `make-fixture.py --camera-clip/--camera-still` adds it as camera `JamCams_00000.00001`'s stored clip; analytics.py takes clips only from `JamCams_` ids. `working_copy.py` issues the `camera-analytics` feed identity. YOLOX-s sees the cars mostly as trucks, with some "airplane" boxes: vehicles, not a realistic scene, but enough to check detection, tracking and agreement.
  - **CI, `windows-analytics`** (run.sh, GitLab, `windows.yml` job `analytics`, about 4 min): it publishes midgard-frames.exe, fetches ffmpeg and runs `fetch-analytics.sh`. Then `python.exe -I` imports numpy 2.5.4 and onnxruntime 1.23.2, with providers `['AzureExecutionProvider', 'CPUExecutionProvider']` (Azure is unused). After that it records the sizes and runs `tools/win-app/check-analytics.py`:
    - `analytics.py --once --dry` as a program, with the shell's layout arguments and `--nice 10`: 9.6 s including start and model load;
    - one analysis as a module: `land-camera-ai` `AI-JamCams_00000.00001`, 40 frames, truck:4, airplane:3, 4 moving, in **8.5 s** (213 ms a frame, 4 threads on the 2-core runner);
    - midgard-frames.exe against ffmpeg on the same clip.

    onnxruntime warns "Unsupported Windows version (2025server)" on the runner; that is harmless, and Windows 11 is supported.
  - **Detections, helper against ffmpeg** (the comparison deferred from Task 1; run 40046709287, YOLOX-s, CPU, conf 0.35):

    | | midgard-frames.exe | ffmpeg 9.0.2 |
    |---|---|---|
    | frames | 40, 640×523 from 352×288, in 1.23 s | 40, in 0.19 s |
    | mean \|pixel difference\| | 1.42 (worst frame 1.48, largest 87) | |
    | per-frame detections | 56 | 67 |
    | share matched by the other (same class, IoU ≥ 0.5) | 98.2 % | 82.1 % |
    | tracks (`ai_counts`, moving) | truck:4, airplane:3; 4 moving | the same |

    Nearly everything the helper detects, ffmpeg's frames detect too. ffmpeg's frames give 11 more low-margin boxes, around the 0.35 threshold, so the confirmed tracks and counts are identical. On the Mac, the Mac's AVFoundation helper on the same clip scored 85–87 % both ways, with the same tracks. The real TfL comparison (96 %) is in `heimdall/analytics/README.md`.

### Task 3: the shell runs analytics

- **`SuiteSpecs`:**
  - `analytics` is optional and last, as on the Mac after c739170;
  - executable `helpers\analytics\python.exe` with `-I -B resources\analytics\analytics.py --serve …`, arguments as in `ServiceSpec.swift`;
  - the frames helper path, the media folder from the recording, the model dir, the port and the token file;
  - readiness via `SERVE pid=`.
- **Token file:** written at start, private to the user (the working copy's folder ACL), never logged.
- **Gateway flags:** `--analytics-addr 127.0.0.1:<port> --analytics-token-file <file>`.
- **Sweep:** the orphan sweep covers `helpers\analytics\` (python.exe).
- **Unit tests:** spec order, arguments, an optional failure not blocking start, the token.
- **Headless check:** extend the W2 Task 7 `--headless-check` and the e2e CI job to include analytics. Analyse the fixture camera through the gateway and assert a `land-camera-ai` entity and a trend point. Record the timing on the CI CPU.
- *As built* (commit 2ba5a5f):
  - **Spec:** `CameraAnalytics` (ServiceSpec.cs) with `Locate`. It needs `helpers\analytics\python.exe`, `resources\analytics\{analytics.py,config.json}`, `resources\models\yolox\yolox_s.onnx`, `helpers\{midgard-frames,heimdall-source}.exe` and a recording; without them it is logged and left out. `SuiteSpecs.Make` adds `analytics` last and `Optional`, ready on `SERVE pid=`.
  - **Arguments:** the Mac's: `-I -B analytics.py --serve --port --token-file --state-dir Home\heimdall-poc\analytics --log - --config --model-dir`, `--provider cpu --threads N`, `--frames-helper --recording --source-bin --source-identity-dir --source-addr`, and `--nice 10`. N is min(4, physical cores), as on an Intel Mac.
  - **Gateway:** given `--analytics-addr`/`--analytics-token-file` only with the analytics.
  - **Token:** `AnalyticsToken.Ensure`, before the services start. A new token is created with `FileMode.CreateNew` and a protected DACL: inheritance off, one entry, this user with full control (0600). An existing token is kept and its DACL made private. An empty file is replaced; a symbolic link or junction is removed, never followed. A folder is refused. If the token can't be written, the analytics are left out (logged); the token itself is never logged.
  - **Sweep:** `helpers\analytics\` is inside the helpers, so the orphan sweep and the headless check's last look already cover it (tested).
  - **Tests:**
    - spec order, the optional flag and the arguments;
    - the gateway's flags only with the analytics;
    - the ports and paths wired between gateway, analytics, Heimdall and replay;
    - `Locate` with each part missing;
    - the thread cap;
    - five token cases (Windows);
    - an optional service whose python.exe is missing not blocking the start;
    - the sweep ending a python.exe in `helpers\analytics\`.
  - **Headless check and `windows-shell-e2e`:**
    - The package gains the runtime as `helpers\analytics`, `midgard-frames.exe`, `resources\analytics` and `resources\models\yolox`, and the fixture gains the clip camera.
    - After the picture check, the check waits for the analytics to be ready, signs in, and runs `POST /api/v1/heimdall/cameras/JamCams_00000.00001/analyse`. It needs 40 frames, a vehicle, and `sent accepted=1`.
    - It then polls the picture for `AI-JamCams_00000.00001` of kind `land-camera-ai`, and reads `…/trend` for a point by the analyst.
    - Run 40046709287 passed 36 steps. The analytics were ready about 6 s after the suite (0.5 s after launch), and the entity came from source `camera-analytics`.
  - **Analyse timing on the CI CPU:** **7.8 s through the gateway**, 194 ms a frame for 40 frames, with 2 threads (the runner reports 2 physical cores). The Mac takes about 1.3 s with CoreML and about 10 s under Rosetta. The gateway's 30 s timeout leaves room on a 4-core laptop.

### Task 4: record results

Results go in this plan and the design doc: sizes, the detection agreement with ffmpeg, Analyse timing on CI, and anything learnt about Windows N.
- *As built:* sizes, agreement and timing are under Tasks 2 and 3 above, and summarised in the design doc §1 and §3.
  - **Windows N:** nothing new could be tested, because no runner is an N edition. On one, midgard-frames.exe exits with the Media Feature Pack message (Task 1). analytics.py turns that into a failed analysis: the service log gets the message and the gateway answers 502 "analysis failed: RuntimeError". The console doesn't yet say why, so W4's instruction sheet must cover N editions (Media Feature Pack, or Analyse unavailable).
  - **Still to check on a real Windows 11 laptop:** onnxruntime's start without the Server warning, Analyse time on a laptop CPU, and antivirus first-scan time over the 690 .pyc and 34 MB of onnxruntime DLLs.
