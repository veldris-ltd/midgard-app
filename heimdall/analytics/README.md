# Camera analytics

Object detection and tracking on camera clips and stills, on request, for Heimdall's
operating picture (feed identity `camera-analytics`, entities `AI-<camera id>` of kind
`land-camera-ai`). The gateway forwards `POST /api/v1/heimdall/cameras/{id}/analyse`,
`…/watch?hours=N` and `GET …/trend` to it (`--analytics-addr`, `--analytics-token-file`;
see `services/http-gateway/src/camera_analytics.rs`). What it computes, and its privacy and
accuracy limits, are in the docstring of `analytics.py`; the model and its pre/post-processing
are in `MODEL.md`.

| File | |
|---|---|
| `analytics.py` | the service (stdlib, numpy, onnxruntime) |
| `config.json` | model, provider, frame rate and thresholds (and the development loop's camera rotation) |
| `MODEL.md` | YOLOX-s (Apache-2.0): source URL, SHA-256, pre/post-processing |
| `test_analytics.py` | tests of the offline layout (`python3 -m unittest heimdall/analytics/test_analytics.py`, with numpy) |

Not in git: the model file (35.9 MB; fetched by URL and checked by SHA-256, see below), the
Python environment, the token (`secret`), logs and state (`trend.jsonl`, `watches.json`,
`state.json`, `out/`).

## In Midgard.app (offline)

`tools/mac-app/fetch-analytics.sh` downloads, checks (`tools/mac-app/analytics.lock`) and lays
out a relocatable CPython (python-build-standalone) per architecture with pinned numpy and
onnxruntime wheels, plus the model; `tools/mac-app/build-app.sh` bundles them:

* `Contents/Resources/analytics/analytics.py`, `config.json`
* `Contents/Resources/analytics/runtime/{arm64,x64}/` the Python runtime and its site-packages
  (in Resources, not Helpers: codesign accepts only Mach-O files in Helpers; every Mach-O in the
  runtime is signed one by one)
* `Contents/Resources/models/yolox/yolox_s.onnx`
* `Contents/Helpers/midgard-frames` the clip and image decoder (AVFoundation, `apps/mac`), in
  place of ffmpeg and ffprobe

The shell runs it as the `analytics` service, last and optional (the app never waits for it, and
starts without it if it fails: Analyse then answers 502): the arm64 runtime with the
CoreML execution provider on Apple Silicon, the x64 runtime on the CPU on Intel (or with
`MIDGARD_FORCE_INTEL_MODEL=1`), as `python3.13 -I -B analytics.py --serve` with
`--recording` (the cameras are the recording's `land-camera` observations), `--media-dir`
(their stored stills and clips, read from disk: `/media/<file>` URLs only, never the internet),
`--frames-helper`, `--source-bin`/`--source-identity-dir`/`--source-addr` (results sent with
`heimdall-source` straight to Heimdall's mesh port), and `--state-dir`/`--token-file` in the
working copy (`heimdall-poc/analytics/`). The token is created by the shell before the gateway
starts (O_EXCL|O_NOFOLLOW, 0600; an existing regular file is kept and tightened, a symlink
replaced) and checked here with `hmac.compare_digest`. A camera whose stored clip is missing
is analysed from its stored still. The service is left out (logged) unless the runtime, code,
model and `Helpers/{midgard-frames,heimdall-source}` are all there.

## In the Windows app (offline)

The same `analytics.py`, model and versions (CPython 3.13.16, numpy 2.5.4, onnxruntime 1.23.2)
as the Mac app. `tools/win-app/fetch-analytics.sh` (on a Mac or on Windows) downloads, checks
(`tools/win-app/analytics.lock`) and lays out python-build-standalone's x86_64-pc-windows-msvc
runtime with the numpy and onnxruntime win_amd64 wheels unzipped into its `Lib\site-packages`,
pruned and compiled, plus the model and the licences. The package holds:

* `helpers\analytics\` the runtime (`python.exe`, `python313.dll`, `DLLs\`, `Lib\`)
* `helpers\midgard-frames.exe` the clip and image decoder (Media Foundation and WIC,
  `apps/windows/MidgardFrames`), and `helpers\heimdall-source.exe`
* `resources\analytics\analytics.py`, `config.json`
* `resources\models\yolox\yolox_s.onnx`

The shell (`apps/windows`, `SuiteSpecs`) runs it last and optional as on the Mac, with the same
arguments, on the CPU (`--provider cpu`, up to 4 threads), at the below-normal priority class
(`--nice` on Windows). The token (`Home\heimdall-poc\analytics\secret`) is created before the
gateway starts with a protected DACL (this user only), exclusively, a link replaced. On Windows
`analytics.py` has no `resource` module (CPU times are its own only) and reads a recording's
plain `heimdall.jsonl` (recordings for Windows are uncompressed). CI: the `windows-analytics`
job (`tools/win-app/check-analytics.py`) and `windows-shell-e2e` (Analyse through the gateway).

### midgard-frames against ffmpeg

`midgard-frames decode` writes the same stream analytics.py reads from ffmpeg (raw BGR,
already resized for the letterbox). Frame selection is ffmpeg's `fps` filter exactly (same
frames chosen on every clip checked). Pixels differ slightly: ffmpeg's default RGB output path
(half-resolution chroma) is about 2 levels darker than a plain bilinear resize; the helper
matches ffmpeg with `flags=bilinear+full_chroma_int` to 0.4 levels on average. Measured on five
stored TfL clips (23 Oct 2026, YOLOX-s, CoreML, conf 0.35): 96.0 % of per-frame detections
match ffmpeg's (same class, IoU >= 0.5), against 96.5 % between ffmpeg's two chroma modes; track
counts differ by 0 to 3 per clip, as much as between those two ffmpeg modes.

## Development setup

The live copy runs in `~/Desktop/Midgard/heimdall-poc/analytics/` (outside git) with its own
`.venv` (numpy, onnxruntime), `models/yolox_s.onnx`, Homebrew's ffmpeg/ffprobe and
`../hsource.sh`; with no layout options `analytics.py` uses the files beside it, so this file
can be copied there as it is. `tools/mac-app/run-native.sh` passes the gateway
`--analytics-addr 127.0.0.1:7740 --analytics-token-file $H/heimdall-poc/analytics/secret` when
that token exists. Model file:

    curl -fLo models/yolox_s.onnx https://github.com/Megvii-BaseDetection/YOLOX/releases/download/0.1.1rc0/yolox_s.onnx
    shasum -a 256 models/yolox_s.onnx   # c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063
