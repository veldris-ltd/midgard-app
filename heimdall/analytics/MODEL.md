# Detection model

| | |
|---|---|
| Model | YOLOX-s, COCO-80 pretrained, official ONNX export (input 1x3x640x640, output 1x8400x85) |
| File | `models/yolox_s.onnx` (35,858,002 bytes) |
| SHA-256 | `c5c2d13e59ae883e6af3b45daea64af4833a4951c92d116ec270d9ddbe998063` |
| URL | https://github.com/Megvii-BaseDetection/YOLOX/releases/download/0.1.1rc0/yolox_s.onnx |
| Release page | https://github.com/Megvii-BaseDetection/YOLOX/releases/tag/0.1.1rc0 (published 2021-08-18) |
| Licence | Apache-2.0 (Megvii), https://github.com/Megvii-BaseDetection/YOLOX/blob/main/LICENSE (GitHub API reports `Apache-2.0` for the repository) |
| Alternate (not used by default) | `models/yolox_tiny.onnx`, same release, SHA-256 `427cc366d34e27ff7a03e2899b5e3671425c262ea2291f88bb942bc1cc70b0f7` (416x416) |

Not used: Ultralytics YOLOv5/v8/11 (AGPL-3.0).

## Runtime

onnxruntime 1.24.4 and numpy 2.5.4 (pinned in `.venv`, Python 3.14). Execution provider: CoreML
(`MLProgram`, compute units ALL) — measured ~0.3 CPU-seconds per 40-frame clip versus ~5.7 CPU-seconds
on the CPU provider with 2 threads, with identical detections. If CoreML fails to load, the code falls
back to the CPU provider with `threads` (default 2) intra-op threads.

## Pre/post-processing (as in YOLOX `demo/ONNXRuntime`)

* BGR, 0-255 float32, **no mean/std normalisation** (removed in 0.1.1rc0; older `--legacy` weights differ).
* Letterbox: resize by `r = min(640/W, 640/H)` (ffmpeg bilinear), paste top-left on a 114-grey canvas.
* Output: `xy = (raw_xy + grid) * stride`, `wh = exp(raw_wh) * stride` for strides 8/16/32;
  objectness and class scores are already sigmoided; score = obj x cls. Boxes divided by `r`.
* NMS 0.45 per class group (car/truck/bus share a group so one vehicle is not counted twice).
* Check: the YOLOX sample `assets/dog.jpg` gives bicycle 0.96 and truck 0.62 with YOLOX-s, matching
  the upstream demo (dog is not a kept class).
