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

### Task 2: the pinned Windows analytics runtime

- **`tools/win-app/analytics.lock`:**
  - python-build-standalone `cpython-3.13.x+<release>-x86_64-pc-windows-msvc-install_only.tar.gz`, the same release as the Mac's lock if it has the Windows build;
  - numpy and onnxruntime `cp313-win_amd64` wheels;
  - SHA-256 for each.
  - Check onnxruntime 1.23.2 (the Mac's pin) has cp313 win_amd64. If not, use the newest that does and record why.
- **`tools/win-app/fetch-analytics.sh`:** modelled on `tools/mac-app/fetch-analytics.sh`. It downloads into `~/Library/Caches/midgard-build/win/analytics/`, verifies hashes, extracts safely (checking names), and unzips the wheels into the runtime's `Lib\site-packages`. It prunes pip, ensurepip, tests and `__pycache__`, then compiles .pyc for Windows paths, or ships .py only (justify). It copies licences (CPython, numpy and its OpenBLAS notice, onnxruntime plus ThirdPartyNotices, and the YOLOX licence) and writes a manifest.
- **CI check (Windows):** a `windows-analytics` run.sh job (mirrored in GitLab). It builds the runtime on Linux or Windows CI with the same script, or fetches with the same lock, then runs `python.exe -I -c "import numpy, onnxruntime; print(...)"` and runs `analytics.py` once on the fixture clip with `midgard-frames.exe` and the YOLOX model. The model is fetched by its pinned URL and SHA-256 in CI, as `fetch-analytics.sh` does. It must produce a `land-camera-ai` observation.

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

### Task 4: record results

Results go in this plan and the design doc: sizes, the detection agreement with ffmpeg, Analyse timing on CI, and anything learnt about Windows N.
