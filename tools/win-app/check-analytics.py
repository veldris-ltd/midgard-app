#!/usr/bin/env python3
"""The camera analytics on the Windows runtime, once, on a fixture camera's stored clip: the
windows-analytics CI job's check (docs/plans/2026-10-24-windows-app-w3-analytics.md, Task 2).

    python.exe -I -B tools/win-app/check-analytics.py --recording DIR --camera ID
        --frames-helper midgard-frames.exe --model-dir DIR [--ffmpeg ffmpeg.exe --ffprobe ffprobe.exe]
        [--threads N] [--report FILE]

Run by the runtime's own python.exe (helpers\\analytics\\), with a recording from
make-fixture.py --camera-clip. It checks, in order:

  1. analytics.py itself, as a program: --once --dry on the camera with --frames-helper,
     --recording, --model-dir and --nice (the shell's app-mode arguments, less --serve and
     the sending), which must analyse it with no error;
  2. analytics.py as a module, the same camera: a land-camera-ai observation for AI-<ID>,
     40 frames, at least one vehicle;
  3. with --ffmpeg: midgard-frames.exe against ffmpeg on the same clip, the frames (count
     and mean pixel difference) and the detections (YOLOX-s, CPU, at the service's
     confidence: the share of each decoder's per-frame detections that the other has, with
     the same class and IoU >= 0.5) and the tracks (ai_counts, ai_moving), recorded and
     printed. It fails only if the frame counts differ or under half the detections agree.

Times are wall-clock on this CPU. The report (--report) is the same text as printed.
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ANALYTICS = os.path.join(os.path.dirname(os.path.dirname(HERE)), "heimdall", "analytics")
sys.path.insert(0, ANALYTICS)
import numpy as np  # noqa: E402

import analytics as A  # noqa: E402

LINES = []


def say(text=""):
    print(text, flush=True)
    LINES.append(text)


def fail(text):
    say(f"FAIL: {text}")
    raise SystemExit(1)


def match(a, b, iou=0.5):
    """How many of a's detections b has (same class, IoU >= iou; greedy, one to one)."""
    if len(a) == 0 or len(b) == 0:
        return 0
    m = A.iou_matrix(a[:, :4], b[:, :4])
    m = np.where(a[:, None, 5] == b[None, :, 5], m, 0)
    used_a, used_b, n = set(), set(), 0
    for i, j in sorted(zip(*np.nonzero(m >= iou)), key=lambda p: -m[p]):
        if i not in used_a and j not in used_b:
            used_a.add(i)
            used_b.add(j)
            n += 1
    return n


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--recording", required=True)
    ap.add_argument("--camera", required=True)
    ap.add_argument("--frames-helper", required=True)
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--ffmpeg")
    ap.add_argument("--ffprobe")
    ap.add_argument("--threads", type=int, default=min(4, os.cpu_count() or 1))
    ap.add_argument("--report")
    a = ap.parse_args()
    config = os.path.join(ANALYTICS, "config.json")
    try:
        run(a, config)
    finally:
        if a.report:
            with open(a.report, "w", encoding="utf-8", newline="\n") as f:
                f.write("\n".join(LINES) + "\n")


def run(a, config):
    say(f"python {sys.version.split()[0]} on {sys.platform}; numpy {np.__version__}; "
        f"threads {a.threads}; camera {a.camera}")

    # 1. the program
    with tempfile.TemporaryDirectory(prefix="midgard-analytics-") as state:
        cmd = [sys.executable, "-I", "-B", os.path.join(ANALYTICS, "analytics.py"), "--once", "--dry",
               "--state-dir", state, "--log", "-", "--config", config, "--model-dir", a.model_dir,
               "--provider", "cpu", "--threads", str(a.threads), "--frames-helper", a.frames_helper,
               "--recording", a.recording, "--nice", "10", a.camera]
        t = time.time()
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
        took = time.time() - t
        for line in (p.stdout + p.stderr).strip().splitlines():
            say(f"  | {line}")
        if p.returncode != 0 or not re.search(r"CYCLE cameras=1 ok=1 errors=0", p.stdout):
            fail(f"analytics.py --once exited {p.returncode} without analysing {a.camera}")
        say(f"1. analytics.py --once --dry: ok in {took:.1f} s (start, model load, one analysis)")

    # 2. the module, with the helper
    A.LOG = None
    A.CONFIG_FILE = config
    A.MODELS = os.path.abspath(a.model_dir)
    A.RECORDING = os.path.abspath(a.recording)
    A.MEDIA_DIR = os.path.join(A.RECORDING, "media")
    A.FRAMES = os.path.abspath(a.frames_helper)
    cfg = A.config()
    t = time.time()
    det = A.Detector(cfg["model"], "cpu", a.threads)
    load = time.time() - t
    t = time.time()
    det(np.zeros((det.size, det.size, 3), np.uint8), 1.0, 0.3, 0.45)
    warm = time.time() - t
    cams = A.pick(cfg, [a.camera])
    if not cams or not cams[0].get("video"):
        fail(f"the recording has no camera {a.camera} with a clip")
    cam = cams[0]
    t = time.time()
    obs, s = A.analyse(det, cam, cfg)
    took = time.time() - t
    at = obs["attrs"]
    say(f"2. analyse({a.camera}) with midgard-frames: {obs['kind']} {obs['entity']}, {at['ai_frames']} frames, "
        f"counts {at['ai_counts'] or '-'}, vehicles {at['ai_vehicles']}, moving {at.get('ai_moving')}, "
        f"flow {at.get('ai_flow')}; {took:.2f} s ({took / max(1, at['ai_frames']) * 1000:.0f} ms a frame; "
        f"model load {load:.2f} s, warm-up {warm:.2f} s)")
    if obs["kind"] != "land-camera-ai" or obs["entity"] != f"AI-{a.camera}" or at.get("camera_id") != a.camera:
        fail(f"not a land-camera-ai observation for AI-{a.camera}: {obs['kind']} {obs['entity']}")
    if at["ai_frames"] != cfg["max_frames"] or at["ai_vehicles"] < 1:
        fail(f"expected {cfg['max_frames']} frames and a vehicle, got {at['ai_frames']} and {at['ai_vehicles']}")

    if not a.ffmpeg:
        return
    # 3. midgard-frames against ffmpeg
    path = A.stored_media(cam["video"])
    decoded = {}
    for name in ("midgard-frames", "ffmpeg"):
        if name == "ffmpeg":
            A.FRAMES, A.FFMPEG, A.FFPROBE = None, os.path.abspath(a.ffmpeg), os.path.abspath(a.ffprobe or a.ffmpeg)
        t = time.time()
        frames, ratio, size = A.decode(path, det.size, cfg["fps"], cfg["max_frames"], False)
        decoded[name] = (frames, ratio, size, time.time() - t)
    (fh, rh, sh, th), (ff, rf, sf, tf) = decoded["midgard-frames"], decoded["ffmpeg"]
    say(f"3. decoding: midgard-frames {len(fh)} frames {fh.shape[2]}x{fh.shape[1]} from {sh[0]}x{sh[1]} "
        f"in {th:.2f} s; ffmpeg {len(ff)} frames {ff.shape[2]}x{ff.shape[1]} from {sf[0]}x{sf[1]} in {tf:.2f} s")
    if fh.shape != ff.shape:
        fail(f"the decoders differ: {fh.shape} against {ff.shape}")
    diff = np.abs(fh.astype(np.int16) - ff.astype(np.int16))
    say(f"   pixels: mean |difference| {diff.mean():.2f}, worst frame {diff.reshape(len(fh), -1).mean(1).max():.2f}, "
        f"largest {int(diff.max())}")
    hits = {"midgard-frames": 0, "ffmpeg": 0}
    totals = {"midgard-frames": 0, "ffmpeg": 0}
    for x, y in zip(fh, ff):
        dx, dy = det(x, rh, cfg["conf"], cfg["nms"]), det(y, rf, cfg["conf"], cfg["nms"])
        totals["midgard-frames"] += len(dx)
        totals["ffmpeg"] += len(dy)
        hits["midgard-frames"] += match(dx, dy)
        hits["ffmpeg"] += match(dy, dx)
    share = {k: hits[k] / totals[k] if totals[k] else 1.0 for k in hits}
    say(f"   detections at conf {cfg['conf']}: midgard-frames {totals['midgard-frames']}, ffmpeg {totals['ffmpeg']}; "
        f"{share['midgard-frames'] * 100:.1f} % of the helper's are ffmpeg's and {share['ffmpeg'] * 100:.1f} % "
        f"of ffmpeg's the helper's (same class, IoU >= 0.5)")
    o2, _ = A.analyse(det, cam, cfg)  # ffmpeg decoding now
    b = o2["attrs"]
    say(f"   tracks: midgard-frames {at['ai_counts'] or '-'} moving {at.get('ai_moving')}; "
        f"ffmpeg {b['ai_counts'] or '-'} moving {b.get('ai_moving')}")
    if min(share.values()) < 0.5:
        fail("under half the detections agree")
    say("ok")


if __name__ == "__main__":
    main()
