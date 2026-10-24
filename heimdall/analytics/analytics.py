"""Local camera analytics for the Heimdall proof of concept (feed identity `camera-analytics`).

    .venv/bin/python analytics.py                      run forever (see start.sh / stop.sh)
    .venv/bin/python analytics.py --once [ids..]       one cycle (or just the named cameras), then exit
    .venv/bin/python analytics.py --once --annotate ID also save one annotated frame in test/
    .venv/bin/python analytics.py --bench [ids..]      time the model/provider choices, send nothing
    .venv/bin/python analytics.py --serve              analyse only on request (what the gateway uses)

Two layouts (see README.md):
  * the development setup: this file in ~/Desktop/Midgard/heimdall-poc/analytics/ with its
    .venv, models/ and secret beside it, cameras from the feed runner's caches, clips fetched
    from the internet, ffmpeg/ffprobe from Homebrew, results sent with ../hsource.sh;
  * Midgard.app (offline): --serve with --recording (cameras from the recording's land-camera
    observations), --media-dir (their stored stills and clips, read from disk, nothing fetched),
    --frames-helper (the bundled AVFoundation decoder in place of ffmpeg), --source-bin and
    friends (heimdall-source run directly) and --state-dir/--token-file/--port/--model-dir.

What it does, per camera, one camera at a time:
  * TfL JamCams (default): download the ~10 s MP4 clip, decode ~4 fps with ffmpeg (<= 40 frames),
    detect objects with YOLOX (Apache-2.0, Megvii; official ONNX, see MODEL.md) on onnxruntime,
    track them across frames with a small IoU + centroid tracker (ByteTrack-style two-stage
    association, written here), then derive counts, moving/stationary, image-plane flow and an
    approximate compass heading.
  * Still-image cameras (DriveBC, NZTA, Singapore LTA): detection and counts only, no motion.
  * Everything for one cycle goes to Heimdall in one JSON Lines batch via ../hsource.sh, with the
    entity key "AI-<camera id>" and kind "land-camera-ai", with camera_id naming the camera, so
    the console can join them (Heimdall replaces attributes per report, so they must not share
    the camera's entity).

Privacy: people are only counted. No faces, no re-identification beyond the few seconds of one
clip, and no clip or frame is kept: media go to a temporary directory that is deleted after each
camera. (The only exception is the single test frame written by --annotate, with people pixelated.)

Movement and heading are APPROXIMATE and UNCALIBRATED: they are measured in the image plane
(normalised image units, 0-1 across the frame), with no lens model, no camera height/tilt and no
ground-plane homography. No speed in real-world units is claimed anywhere. "towards"/"away" just
mean down/up the image, which is only a proxy for towards/away from the camera.

Dependencies: numpy and onnxruntime (pinned in .venv; in Midgard.app and the Windows app, the
bundled runtime's), ffmpeg/ffprobe from Homebrew unless --frames-helper is given. Stdlib
otherwise. On Windows (the Windows app: midgard-frames.exe as --frames-helper) there is no
`resource` (CPU times count this process only) and no nice (--nice sets the below-normal
priority class), and a recording's batches may be plain heimdall.jsonl.
"""

import argparse
import gzip
import json
import math
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from collections import Counter
from datetime import datetime, timezone

import numpy as np

try:
    import resource  # not on Windows
except ImportError:
    resource = None

HERE = os.path.dirname(os.path.abspath(__file__))
POC = os.path.dirname(HERE)
FEEDS_STATE = os.path.join(POC, "feeds", "state")  # read only: the feed runner's camera caches
MODELS = os.path.join(HERE, "models")
STATE_DIR = HERE  # what this service writes: out/, state.json, trend.jsonl, watches.json
OUT = os.path.join(HERE, "out")
TEST = os.path.join(HERE, "test")
LOG = os.path.join(HERE, "analytics.log")  # None: stdout only
STATE_FILE = os.path.join(HERE, "state.json")  # rotation offsets only
CONFIG_FILE = os.path.join(HERE, "config.json")
HSOURCE = os.path.join(POC, "hsource.sh")
MID = os.path.expanduser("~/Desktop/Midgard")  # mounted at /mid in the toolchain container
FFMPEG = "/opt/homebrew/bin/ffmpeg"
FFPROBE = "/opt/homebrew/bin/ffprobe"
FRAMES = None  # midgard-frames (Midgard.app): decodes in place of ffmpeg/ffprobe
RECORDING = None  # a recording folder: the cameras are its land-camera observations
MEDIA_DIR = None  # stored stills and clips for "/media/<file>" URLs; nothing is fetched
SOURCE = None  # (heimdall-source, identity dir, Heimdall's mesh address): sent directly
UA = "VeldrisMidgardDemo/0.1 (local camera analytics test)"
FEED = "camera-analytics"

DEFAULTS = {
    "interval_s": 300,
    "tfl_cameras_per_cycle": 24,
    "always_include": [],
    "stills": {"drivebc": 0, "nz": 0, "sg": 0},
    "still_ids": [],
    "model": "yolox_s",
    "provider": "cpu",
    "threads": 2,
    "fps": 4,
    "max_frames": 40,
    "conf": 0.35,
    "low_conf": 0.15,
    "nms": 0.45,
}

# COCO-80 class order as used by YOLOX
COCO = ["person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat"]
KEEP = {0: "person", 1: "bicycle", 2: "car", 3: "motorcycle", 4: "airplane", 5: "bus", 6: "train", 7: "truck", 8: "boat"}
VEHICLES = {"car", "truck", "bus", "motorcycle"}  # what ai_vehicles / ai_queue / ai_flow count
# classes that may swap between frames for the same object, so the tracker associates within a group
GROUP = {"car": "v", "truck": "v", "bus": "v", "motorcycle": "2w", "bicycle": "2w", "person": "p",
         "airplane": "air", "train": "rail", "boat": "boat"}

GROUP_ID = {g: i for i, g in enumerate(sorted(set(GROUP.values())))}

STOP = False


def utc():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log_line(text):
    line = f"{utc()} {text}"
    print(line, flush=True)
    if LOG:
        with open(LOG, "a") as f:
            f.write(line + "\n")


def load_json(path, default):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def save_json(path, value):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(value, f, indent=1)
    os.replace(tmp, path)


def config():
    c = dict(DEFAULTS)
    c.update(load_json(CONFIG_FILE, {}))
    return c


def cpu_seconds():
    """CPU time of this process plus its finished children (ffmpeg, ffprobe, hsource); on
    Windows (no getrusage) this process's only."""
    if resource is None:
        return time.process_time()
    s = resource.getrusage(resource.RUSAGE_SELF)
    ch = resource.getrusage(resource.RUSAGE_CHILDREN)
    return s.ru_utime + s.ru_stime + ch.ru_utime + ch.ru_stime


# ---------------------------------------------------------------- compass handling

COMPASS = {"N": 0, "NE": 45, "E": 90, "SE": 135, "S": 180, "SW": 225, "W": 270, "NW": 315}
WORD = {"NORTH": "N", "SOUTH": "S", "EAST": "E", "WEST": "W"}


def parse_view(view):
    """First compass word in a camera's `view` text -> bearing the camera looks along, or None.
    "West", "WEST - Addington St", "South East", "SouthEast-Harleyford Rd", "N/WEST-A2", "NE zoom",
    "East Facing (Home)". Single letters with /B ("A23 N/B" = northbound) are travel directions,
    not the view, and are ignored."""
    if not view:
        return None
    v = view.replace("\\/", "/").upper()
    m = re.search(r"\b(NORTH|SOUTH)[\s\-/]*(EAST|WEST)\b", v) or re.search(r"\b(NORTH|SOUTH)(EAST|WEST)\b", v)
    first = re.search(r"\b(NORTH|SOUTH|EAST|WEST)", v)
    short = re.search(r"\b([NS])/(EAST|WEST|E|W)\b", v) or re.search(r"(?:^|[\s\-(])(NE|NW|SE|SW)\b", v)
    cands = []
    if m:
        cands.append((m.start(), WORD[m.group(1)] + WORD[m.group(2)]))
    if first:
        cands.append((first.start(), WORD[first.group(1)]))
    if short:
        if short.re.pattern.startswith(r"\b([NS])"):
            cands.append((short.start(), short.group(1) + short.group(2)[0]))
        else:
            cands.append((short.start(1), short.group(1)))
    if not cands:
        return None
    pos = min(p for p, _ in cands)
    # at the same position prefer the longer (two-word) reading
    best = max((c for c in cands if c[0] == pos), key=lambda c: len(c[1]))[1]
    return COMPASS[best]


def compass8(deg):
    return ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][int(((deg % 360) + 22.5) // 45) % 8]


# ---------------------------------------------------------------- media

def fetch(url, path, timeout=30):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout) as r, open(path, "wb") as f:
        shutil.copyfileobj(r, f)
    if os.path.getsize(path) < 1000:
        raise ValueError(f"media too small ({os.path.getsize(path)} bytes)")


def stored_media(url):
    """The stored file for an app-relative "/media/<file>" URL, inside MEDIA_DIR (symlinks
    resolved, so nothing outside it is read); anything else is refused: offline, nothing is
    fetched."""
    if not isinstance(url, str) or not url.startswith("/media/"):
        raise ValueError(f"not a stored media URL: {str(url)[:80]}")
    name = urllib.parse.unquote(url[len("/media/"):].split("?")[0])
    if not name or "/" in name or "\\" in name or name.startswith(".") or "\0" in name:
        raise ValueError(f"bad media name: {name[:80]}")
    root = os.path.realpath(MEDIA_DIR)
    path = os.path.realpath(os.path.join(root, name))
    if os.path.dirname(path) != root or not os.path.isfile(path):
        raise FileNotFoundError(f"no stored media {name}")
    if os.path.getsize(path) < 1000:
        raise ValueError(f"media too small ({os.path.getsize(path)} bytes)")
    return path


def probe_size(path):
    if FRAMES:
        p = subprocess.run([FRAMES, "probe", path], capture_output=True, text=True, timeout=30)
        if p.returncode != 0:
            raise RuntimeError(p.stderr.strip()[-200:] or f"midgard-frames probe failed ({p.returncode})")
    else:
        p = subprocess.run([FFPROBE, "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
                            "-of", "csv=p=0", path], capture_output=True, text=True, timeout=30)
    w, h = p.stdout.strip().split("\n")[0].split(",")[:2]
    return int(w), int(h)


def decode(path, size, fps, max_frames, still):
    """Decode to BGR frames already resized for the letterbox (ffmpeg does the resize, single
    thread; or midgard-frames, the same frames from AVFoundation). Returns (frames uint8
    [n,h,w,3], ratio, original (W,H))."""
    W, H = probe_size(path)
    r = min(size / W, size / H)
    nw, nh = max(1, int(W * r)), max(1, int(H * r))
    if FRAMES:
        cmd = [FRAMES, "decode", path, "--width", str(nw), "--height", str(nh)]
        cmd += ["--still"] if still else ["--fps", str(fps), "--max-frames", str(max_frames)]
    else:
        vf = f"scale={nw}:{nh}:flags=bilinear" if still else f"fps={fps},scale={nw}:{nh}:flags=bilinear"
        cmd = [FFMPEG, "-v", "error", "-threads", "1", "-filter_threads", "1", "-i", path, "-vf", vf,
               "-frames:v", "1" if still else str(max_frames), "-f", "rawvideo", "-pix_fmt", "bgr24", "pipe:1"]
    p = subprocess.run(cmd, capture_output=True, timeout=120)
    if p.returncode != 0:
        tool = "midgard-frames" if FRAMES else "ffmpeg"
        raise RuntimeError(f"{tool}: " + p.stderr.decode(errors="replace").strip()[-200:])
    n = len(p.stdout) // (nw * nh * 3)
    if n == 0:
        raise RuntimeError("no frames decoded")
    frames = np.frombuffer(p.stdout[: n * nw * nh * 3], np.uint8).reshape(n, nh, nw, 3)
    return frames, r, (W, H)


# ---------------------------------------------------------------- YOLOX detector

class Detector:
    """Official YOLOX ONNX (0.1.1rc0 and later exports): input 1x3xSxS float32, BGR, 0-255, NO
    mean/std normalisation, letterboxed top-left onto a 114-grey canvas. Output 1xNx85 with raw
    box regressions and already-sigmoided objectness and class scores; boxes need the grid/stride
    decode (decode_in_inference was False at export)."""

    def __init__(self, model="yolox_s", provider="cpu", threads=2):
        import onnxruntime as ort
        path = os.path.join(MODELS, f"{model}.onnx")
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        so.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        so.log_severity_level = 3
        if provider == "coreml":
            providers = [("CoreMLExecutionProvider", {"ModelFormat": "MLProgram", "MLComputeUnits": "ALL"}),
                         "CPUExecutionProvider"]
        else:
            providers = ["CPUExecutionProvider"]
        self.sess = ort.InferenceSession(path, so, providers=providers)
        self.input = self.sess.get_inputs()[0].name
        self.size = self.sess.get_inputs()[0].shape[2]
        self.model = model
        self.provider = provider
        grids, strides = [], []
        for s in (8, 16, 32):
            n = self.size // s
            yv, xv = np.meshgrid(np.arange(n), np.arange(n), indexing="ij")
            grids.append(np.stack((xv, yv), 2).reshape(-1, 2))
            strides.append(np.full((n * n, 1), s))
        self.grids = np.concatenate(grids).astype(np.float32)
        self.strides = np.concatenate(strides).astype(np.float32)

    def name(self):
        return {"yolox_s": "YOLOX-s", "yolox_tiny": "YOLOX-tiny", "yolox_nano": "YOLOX-nano",
                "yolox_m": "YOLOX-m"}.get(self.model, self.model) + " (Apache-2.0) · onnxruntime" + \
            (" CoreML" if self.provider == "coreml" else "")

    def __call__(self, frame, ratio, conf_min, nms_thr):
        """frame: already-resized BGR uint8 (h<=S, w<=S). Returns array [k,6] of
        x1,y1,x2,y2 (ORIGINAL image pixels), score, class id, for the kept classes only."""
        S = self.size
        canvas = np.full((S, S, 3), 114, np.uint8)
        h, w = frame.shape[:2]
        canvas[:h, :w] = frame
        x = np.ascontiguousarray(canvas.transpose(2, 0, 1)[None], dtype=np.float32)
        out = self.sess.run(None, {self.input: x})[0][0]
        xy = (out[:, :2] + self.grids) * self.strides
        wh = np.exp(np.minimum(out[:, 2:4], 10.0)) * self.strides
        cls_scores = out[:, 5:5 + len(KEEP)] * out[:, 4:5]  # obj * cls, kept classes are ids 0-8
        cls = cls_scores.argmax(1)
        score = cls_scores[np.arange(len(cls)), cls]
        m = score >= conf_min
        if not m.any():
            return np.zeros((0, 6), np.float32)
        xy, wh, score, cls = xy[m], wh[m], score[m], cls[m]
        boxes = np.concatenate([xy - wh / 2, xy + wh / 2], 1) / ratio
        keep = nms(boxes, score, cls, nms_thr)
        return np.concatenate([boxes[keep], score[keep, None], cls[keep, None].astype(np.float32)], 1)


def iou_matrix(a, b):
    if len(a) == 0 or len(b) == 0:
        return np.zeros((len(a), len(b)), np.float32)
    x1 = np.maximum(a[:, None, 0], b[None, :, 0])
    y1 = np.maximum(a[:, None, 1], b[None, :, 1])
    x2 = np.minimum(a[:, None, 2], b[None, :, 2])
    y2 = np.minimum(a[:, None, 3], b[None, :, 3])
    inter = np.clip(x2 - x1, 0, None) * np.clip(y2 - y1, 0, None)
    area_a = (a[:, 2] - a[:, 0]) * (a[:, 3] - a[:, 1])
    area_b = (b[:, 2] - b[:, 0]) * (b[:, 3] - b[:, 1])
    return inter / np.maximum(area_a[:, None] + area_b[None, :] - inter, 1e-9)


def nms(boxes, scores, cls, thr):
    """Greedy NMS per class GROUP (offset trick: boxes of different groups never overlap), so one
    vehicle scored as both car and truck is kept once, under its higher-scoring class."""
    gid = np.array([GROUP_ID[GROUP[KEEP[int(c)]]] for c in cls], np.float32)
    off = boxes + (gid[:, None] * 10000.0)
    order = scores.argsort()[::-1]
    keep = []
    while len(order):
        i = order[0]
        keep.append(i)
        if len(order) == 1:
            break
        ious = iou_matrix(off[i:i + 1], off[order[1:]])[0]
        order = order[1:][ious < thr]
    return np.array(keep, int)


# ---------------------------------------------------------------- tracker

class Track:
    __slots__ = ("id", "group", "obs", "votes", "vel", "lost")

    def __init__(self, tid, det, f):
        self.id = tid
        self.group = GROUP[KEEP[int(det[5])]]
        self.obs = [(f, det[:4].copy(), float(det[4]))]
        self.votes = Counter({KEEP[int(det[5])]: float(det[4])})
        self.vel = np.zeros(4, np.float32)  # box change per frame
        self.lost = 0

    def predict(self, f):
        lf, box, _ = self.obs[-1]
        return box + self.vel * (f - lf)

    def update(self, det, f):
        lf, box, _ = self.obs[-1]
        step = (det[:4] - box) / max(1, f - lf)
        self.vel = step if len(self.obs) == 1 else 0.6 * step + 0.4 * self.vel
        self.obs.append((f, det[:4].copy(), float(det[4])))
        self.votes[KEEP[int(det[5])]] += float(det[4])
        self.lost = 0

    def cls(self):
        return self.votes.most_common(1)[0][0]


def associate(tracks, dets, f, iou_min, centroid=True):
    """Greedy association of dets to tracks (same class group): IoU against the constant-velocity
    prediction first, then (optionally) centroid distance within 1.2 box sizes for small or fast
    objects whose boxes no longer overlap at 4 fps. Returns (pairs, unmatched det indices)."""
    if not tracks or len(dets) == 0:
        return [], list(range(len(dets)))
    pred = np.array([t.predict(f) for t in tracks], np.float32)
    ious = iou_matrix(pred, dets[:, :4])
    same = np.array([[t.group == GROUP[KEEP[int(d[5])]] for d in dets] for t in tracks])
    ious = np.where(same, ious, 0)
    pairs, used_t, used_d = [], set(), set()
    for ti, di in sorted(zip(*np.nonzero(ious >= iou_min)), key=lambda p: -ious[p]):
        if ti not in used_t and di not in used_d:
            pairs.append((ti, di)); used_t.add(ti); used_d.add(di)
    if centroid:
        pc = (pred[:, :2] + pred[:, 2:]) / 2
        dc = (dets[:, :2] + dets[:, 2:4]) / 2
        scale = np.maximum(pred[:, 2] - pred[:, 0], pred[:, 3] - pred[:, 1])[:, None]
        dist = np.linalg.norm(pc[:, None] - dc[None], axis=2) / np.maximum(scale, 1)
        cand = [(dist[ti, di], ti, di) for ti in range(len(tracks)) for di in range(len(dets))
                if ti not in used_t and di not in used_d and same[ti, di] and dist[ti, di] < 1.2]
        for _, ti, di in sorted(cand):
            if ti not in used_t and di not in used_d:
                pairs.append((ti, di)); used_t.add(ti); used_d.add(di)
    return pairs, [i for i in range(len(dets)) if i not in used_d]


def track(all_dets, conf, max_lost=4):
    """ByteTrack-style: high-confidence dets associate first (IoU, then centroid), leftover tracks
    then try the low-confidence dets (IoU only), unmatched high dets start new tracks."""
    active, done, nid = [], [], 0
    for f, dets in enumerate(all_dets):
        hi, lo = dets[dets[:, 4] >= conf], dets[dets[:, 4] < conf]
        pairs, new = associate(active, hi, f, 0.25)
        for ti, di in pairs:
            active[ti].update(hi[di], f)
        rest = [t for i, t in enumerate(active) if i not in {p[0] for p in pairs}]
        pairs2, _ = associate(rest, lo, f, 0.4, centroid=False)
        for ti, di in pairs2:
            rest[ti].update(lo[di], f)
        matched = {id(active[p[0]]) for p in pairs} | {id(rest[p[0]]) for p in pairs2}
        for t in active:
            if id(t) not in matched:
                t.lost += 1
        for di in new:
            active.append(Track(nid, hi[di], f)); nid += 1
        done += [t for t in active if t.lost > max_lost]
        active = [t for t in active if t.lost <= max_lost]
    return done + active


def motion(t, W, H, fps, n_frames):
    """Image-plane motion of one track (normalised units; x right, y down). Start and end
    positions average the first/last two observations to damp box jitter."""
    c = np.array([((b[0] + b[2]) / 2 / W, (b[1] + b[3]) / 2 / H) for _, b, _ in t.obs])
    k = 2 if len(c) >= 4 else 1
    d = c[-k:].mean(0) - c[:k].mean(0)
    span = (t.obs[-1][0] - t.obs[0][0]) / fps
    hnorm = np.mean([(b[3] - b[1]) / H for _, b, _ in t.obs])
    dist = float(np.hypot(*d))
    # moving: beyond a small absolute threshold AND beyond a fraction of the object's own size
    # (big, close objects jitter more in pixels)
    moving = dist > max(0.03, 0.2 * hnorm)
    whole = (t.obs[-1][0] - t.obs[0][0] + 1) >= 0.8 * n_frames
    per_s = d / span if span > 0 else np.zeros(2)
    return {"dx": float(d[0]), "dy": float(d[1]), "vx": float(per_s[0]), "vy": float(per_s[1]),
            "speed": dist / span if span > 0 else 0.0, "moving": bool(moving), "whole_clip": bool(whole)}


def direction(dx, dy):
    """Image-plane direction word. Down the image ~ towards the camera, up ~ away (a proxy only:
    it assumes a camera looking along the road with the horizon at the top)."""
    if abs(dy) >= abs(dx):
        return "towards" if dy > 0 else "away"
    return "right" if dx > 0 else "left"


def image_to_compass(dx, dy, view_deg):
    """APPROXIMATE, UNCALIBRATED: treat the image as a map seen from behind the camera, rotated so
    that up-the-image points along the camera's view bearing. Up = view bearing (away), down =
    view + 180 (towards), right = view + 90, left = view - 90. Perspective, tilt and lens
    distortion are ignored, so this is only good to the nearest octant or so on a straight road."""
    image_bearing = math.degrees(math.atan2(dx, -dy))  # 0 = up, 90 = right
    return (view_deg + image_bearing) % 360


# ---------------------------------------------------------------- incidents in a clip

CORRIDOR_GRID = (64, 48)  # cells across, down


def centres(t, W, H):
    """A track's box centres and widths, normalised: (frames, [[x, y]], mean width)."""
    fs = [f for f, _, _ in t.obs]
    c = np.array([((b[0] + b[2]) / 2 / W, (b[1] + b[3]) / 2 / H) for _, b, _ in t.obs])
    w = float(np.mean([(b[2] - b[0]) / W for _, b, _ in t.obs]))
    return fs, c, w


def corridor(moving, W, H):
    """Where moving vehicles drove: a grid of the image marking the lower half of every
    moving vehicle's box on every frame (the road surface under it)."""
    gx, gy = CORRIDOR_GRID
    grid = np.zeros((gy, gx), np.uint16)
    for t in moving:
        seen = np.zeros((gy, gx), bool)
        for _, b, _ in t.obs:
            x0, x1 = int(b[0] / W * gx), int(np.ceil(b[2] / W * gx))
            y0, y1 = int((b[1] + b[3]) / 2 / H * gy), int(np.ceil(b[3] / H * gy))
            seen[max(0, y0):min(gy, y1), max(0, x0):min(gx, x1)] = True
        grid += seen  # each vehicle counts once per cell
    return grid


def in_corridor(grid, x, y, need=2):
    gx, gy = CORRIDOR_GRID
    i, j = min(gy - 1, max(0, int(y * gy))), min(gx - 1, max(0, int(x * gx)))
    return int(grid[i, j]) >= need


def incidents(tracks, motions, W, H):
    """APPROXIMATE, UNCALIBRATED incident flags for one clip, from the image alone:
    - blocking: a vehicle stationary through the whole clip whose box lies on road two
      or more other vehicles drove over (not a car parked beside the flow);
    - wrong_way: a moving vehicle whose path runs along the paths of two or more vehicles
      going the opposite way, within a third of its own width (the same lane), and of
      more of them than vehicles going its way;
    - person_road: a person seen on at least two frames with their feet on road two or
      more vehicles drove over (includes people at crossings).
    Returns ({track id: flag}, counts)."""
    veh = [t for t in tracks if t.cls() in VEHICLES]
    moving = [t for t in veh if motions[t.id]["moving"]]
    grid = corridor(moving, W, H)
    flags = {}
    for t in veh:
        m = motions[t.id]
        if m["moving"] or not m["whole_clip"]:
            continue
        b = t.obs[-1][1]
        if in_corridor(grid, (b[0] + b[2]) / 2 / W, (b[1] + 3 * b[3]) / 4 / H):
            flags[t.id] = "stopped"
    paths = {t.id: centres(t, W, H) for t in moving}
    for t in moving:
        _, ca, wa = paths[t.id]
        da = np.array([motions[t.id]["dx"], motions[t.id]["dy"]])
        if np.linalg.norm(da) == 0 or len(ca) < 3:
            continue
        same = opposite = 0
        for u in moving:
            if u is t:
                continue
            _, cb, _ = paths[u.id]
            db = np.array([motions[u.id]["dx"], motions[u.id]["dy"]])
            if np.linalg.norm(db) == 0:
                continue
            # mean distance from this path's points to the other path
            d = float(np.mean(np.min(np.linalg.norm(ca[:, None] - cb[None], axis=2), axis=1)))
            if d > max(0.01, wa / 3):
                continue
            cos = float(da @ db / (np.linalg.norm(da) * np.linalg.norm(db)))
            if cos < -0.7:
                opposite += 1
            elif cos > 0.7:
                same += 1
        if opposite >= 2 and opposite > same:
            flags[t.id] = "wrong-way"
    for t in tracks:
        if t.cls() != "person" or len(t.obs) < 2:
            continue
        feet = sum(in_corridor(grid, (b[0] + b[2]) / 2 / W, b[3] / H) for _, b, _ in t.obs)
        if feet >= 2:
            flags[t.id] = "person-road"
    counts = Counter(flags.values())
    return flags, {"ai_blocking": counts["stopped"], "ai_wrong_way": counts["wrong-way"],
                   "ai_person_road": counts["person-road"]}


# ---------------------------------------------------------------- per camera analysis

def analyse(det, cam, cfg, annotate=None):
    """Analyse one camera; returns (observation, summary dict). Media live only in a temporary
    directory removed before returning."""
    still = cam.get("video") is None
    if MEDIA_DIR:  # offline: the stored copy, read in place
        try:
            path = stored_media(cam["video"] if not still else cam["image"])
        except FileNotFoundError:
            if still or not cam.get("image"):
                raise
            # The clip was not stored (or is gone) but the still is: analyse the still.
            still = True
            path = stored_media(cam["image"])
        frames, ratio, (W, H) = decode(path, det.size, cfg["fps"], cfg["max_frames"], still)
    else:
        tmp = tempfile.mkdtemp(prefix="hca-")
        try:
            path = os.path.join(tmp, "clip.mp4" if not still else "still.jpg")
            fetch(cam["video"] if not still else cam["image"], path)
            frames, ratio, (W, H) = decode(path, det.size, cfg["fps"], cfg["max_frames"], still)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)

    low = cfg["conf"] if still else cfg["low_conf"]
    all_dets = [det(fr, ratio, low, cfg["nms"]) for fr in frames]
    n = len(frames)
    now = int(time.time())
    attrs = {"ai_at": now, "ai_model": det.name(), "ai_frames": n}
    last = n - 1
    boxes = []
    playback = None  # per-frame tracks for the console's player; not sent to Heimdall

    if still:
        d = all_dets[0]
        names = [KEEP[int(x[5])] for x in d]
        counts = Counter(names)
        for x in d:
            boxes.append((float(x[4]), KEEP[int(x[5])], x[:4], 0.0, 0.0))
        moving_n = None
    else:
        tracks = [t for t in track(all_dets, cfg["conf"]) if len(t.obs) >= 3
                  and max(o[2] for o in t.obs) >= cfg["conf"]]
        counts = Counter(t.cls() for t in tracks)
        flow = Counter({"towards": 0, "away": 0, "left": 0, "right": 0})
        moving_n = stationary_n = stopped_n = 0
        headings = {}  # direction bucket -> list of (vx, vy)
        for t in tracks:
            m = motion(t, W, H, cfg["fps"], n)
            is_vehicle = t.cls() in VEHICLES
            if is_vehicle:
                if m["moving"]:
                    moving_n += 1
                    b = direction(m["dx"], m["dy"])
                    flow[b] += 1
                    headings.setdefault(b, []).append((m["dx"], m["dy"]))
                else:
                    stationary_n += 1
                    if m["whole_clip"]:
                        stopped_n += 1
            if t.obs[-1][0] == last:
                vx, vy = (m["vx"], m["vy"]) if m["moving"] else (0.0, 0.0)
                boxes.append((t.obs[-1][2], t.cls(), t.obs[-1][1], vx, vy))
        # last-frame detections that did not make a confirmed track still belong in the box list
        tracked_last = [b[2] for b in boxes]
        for x in all_dets[last]:
            if x[4] >= cfg["conf"] and not any(np.allclose(x[:4], tb) for tb in tracked_last):
                boxes.append((float(x[4]), KEEP[int(x[5])], x[:4], 0.0, 0.0))
        motions = {t.id: motion(t, W, H, cfg["fps"], n) for t in tracks}
        flags, flagged = incidents(tracks, motions, W, H)
        attrs.update(flagged)
        # Every confirmed track's box on each sampled frame, normalised, so the console can
        # play the boxes in step with the clip (frame f is at f / fps seconds).
        playback = {"fps": cfg["fps"], "frames": n, "tracks": [
            {"id": int(t.id), "cls": t.cls(), "moving": bool(motions[t.id]["moving"]), "flag": flags.get(t.id),
             "obs": [[int(f), round(max(0.0, float(b[0]) / W), 4), round(max(0.0, float(b[1]) / H), 4),
                      round(float(b[2] - b[0]) / W, 4), round(float(b[3] - b[1]) / H, 4)] for f, b, _ in t.obs]}
            for t in tracks[:80]]}
        vehicles = sum(v for k, v in counts.items() if k in VEHICLES)
        attrs.update({
            "ai_moving": moving_n, "ai_stationary": stationary_n, "ai_stopped": stopped_n,
            "ai_queue": bool(vehicles >= 8 and stationary_n >= 0.75 * vehicles),
            "ai_flow": ",".join(f"{k}:{flow[k]}" for k in ("towards", "away", "left", "right")),
        })
        view_deg = parse_view(cam.get("view"))
        if view_deg is not None and moving_n >= 3:
            dom = max(("towards", "away", "left", "right"), key=lambda k: flow[k])
            vs = headings[dom]
            # circular mean of the compass headings of the vehicles in the dominant flow
            hs = [math.radians(image_to_compass(dx, dy, view_deg)) for dx, dy in vs]
            hd = math.degrees(math.atan2(sum(map(math.sin, hs)), sum(map(math.cos, hs)))) % 360
            attrs["ai_heading"] = round(hd, 1)
            attrs["ai_heading_text"] = f"{dom} {compass8(hd)}"

    vehicles = sum(v for k, v in counts.items() if k in VEHICLES)
    attrs["ai_counts"] = ",".join(f"{k}:{v}" for k, v in counts.most_common())
    attrs["ai_vehicles"] = vehicles
    attrs["ai_people"] = counts.get("person", 0)
    boxes.sort(key=lambda b: -b[0])
    attrs["ai_boxes"] = ";".join(
        f"{c},{s:.3f},{max(0, b[0] / W):.3f},{max(0, b[1] / H):.3f},{(b[2] - b[0]) / W:.3f},{(b[3] - b[1]) / H:.3f},"
        f"{vx:.3f},{vy:.3f}" for s, c, b, vx, vy in boxes[:60])
    attrs.update({"lat": cam["lat"], "lon": cam["lon"], "source": FEED})
    if annotate:
        save_annotated(frames[last], ratio, boxes, attrs, annotate)
    # Its own entity, linked by camera_id: Heimdall's fusion replaces a track's attributes
    # with each newer report, so sharing the camera's entity would make the camera feed
    # and the analytics overwrite each other. The console joins them by camera_id.
    attrs["camera_id"] = cam["id"]
    obs = {"entity": f"AI-{cam['id']}", "kind": "land-camera-ai", "observedAt": now, "attrs": attrs}
    return obs, {"counts": attrs["ai_counts"] or "-", "moving": moving_n, "frames": n, "playback": playback}


# ---------------------------------------------------------------- annotated test frame

FONT = {  # 3x5 bitmap glyphs, rows top to bottom
    "A": "010101111101101", "B": "110101110101110", "C": "011100100100011", "D": "110101101101110",
    "E": "111100110100111", "G": "011100101101011", "H": "101101111101101", "I": "111010010010111",
    "K": "101101110101101", "L": "100100100100111", "M": "101111111101101", "N": "110101101101101",
    "O": "010101101101010", "P": "110101110100100", "R": "110101110101101", "S": "011100010001110",
    "T": "111010010010010", "U": "101101101101111", "W": "101101111111101", "Y": "101101010010010",
    "0": "111101101101111", "1": "010110010010111", "2": "110001010100111", "3": "110001010001110",
    "4": "101101111001001", "5": "111100110001110", "6": "011100111101111", "7": "111001010010010",
    "8": "111101111101111", "9": "111101111001110", " ": "000000000000000", ".": "000000000000010",
    ":": "000010000010000", "-": "000000111000000", "F": "111100110100100", "V": "101101101101010",
    "X": "101101010101101", "Z": "111001010100111", "J": "001001001101010", "Q": "010101101110011",
    ",": "000000000010100", "/": "001001010100100",
}
COLOURS = {"car": (0, 200, 255), "truck": (255, 120, 0), "bus": (255, 0, 200), "motorcycle": (0, 255, 0),
           "bicycle": (120, 255, 120), "person": (0, 0, 255), "train": (255, 255, 0), "boat": (200, 200, 200),
           "airplane": (255, 255, 255)}  # BGR


def text(img, s, x, y, col, k=2):
    for ch in s.upper():
        g = FONT.get(ch, FONT[" "])
        for i, bit in enumerate(g):
            if bit == "1":
                yy, xx = y + (i // 3) * k, x + (i % 3) * k
                img[max(0, yy):yy + k, max(0, xx):xx + k] = col
        x += 4 * k


def save_annotated(frame, ratio, boxes, attrs, path):
    img = frame.copy()
    H, W = img.shape[:2]
    for s, c, b, vx, vy in boxes:
        x1, y1, x2, y2 = [int(round(v * ratio)) for v in b]
        x1, y1, x2, y2 = max(0, x1), max(0, y1), min(W - 1, x2), min(H - 1, y2)
        if c == "person":  # privacy: pixelate people even in the test frame
            roi = img[y1:y2, x1:x2]
            if roi.size:
                small = roi[::8, ::8]
                img[y1:y2, x1:x2] = np.repeat(np.repeat(small, 8, 0), 8, 1)[: y2 - y1, : x2 - x1]
        col = COLOURS.get(c, (255, 255, 255))
        img[y1:y1 + 2, x1:x2] = col; img[y2 - 1:y2 + 1, x1:x2] = col
        img[y1:y2, x1:x1 + 2] = col; img[y1:y2, x2 - 1:x2 + 1] = col
        text(img, f"{c[:5]} {int(s * 100)}", x1 + 2, max(0, y1 - 12), col)
        if vx or vy:  # motion arrow: 2 s of image-plane motion
            cx, cy = (x1 + x2) / 2, (y1 + y2) / 2
            for t in np.linspace(0, 1, 40):
                px, py = int(cx + t * vx * 2 * W), int(cy + t * vy * 2 * H)
                if 0 <= px < W - 2 and 0 <= py < H - 2:
                    img[py:py + 3, px:px + 3] = (0, 255, 255)
    img[H - 16:H] = 0
    text(img, f"V{attrs['ai_vehicles']} P{attrs['ai_people']} MOV {attrs.get('ai_moving', '-')} "
              f"{attrs.get('ai_heading_text', '')}", 4, H - 13, (255, 255, 255))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    subprocess.run([FFMPEG, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{W}x{H}",
                    "-i", "pipe:0", "-frames:v", "1", path], input=img.tobytes(), check=True, timeout=30)


# ---------------------------------------------------------------- camera selection

_RECORDED = None


def recorded_cams():
    """The cameras of RECORDING: each land-camera entity's last observation in its
    heimdall.jsonl.gz (or heimdall.jsonl: recordings for Windows are shipped uncompressed,
    tools/win-app/make-fixture.py), with media only (read once)."""
    global _RECORDED
    if _RECORDED is None:
        last = {}
        packed = os.path.join(RECORDING, "heimdall.jsonl.gz")
        if os.path.exists(packed):
            f = gzip.open(packed, "rt", encoding="utf-8")
        else:
            f = open(os.path.join(RECORDING, "heimdall.jsonl"), encoding="utf-8")
        with f:
            for line in f:
                if '"land-camera"' not in line:
                    continue
                try:
                    batch = json.loads(line)
                except ValueError:
                    continue
                for o in batch.get("observations") or []:
                    if o.get("kind") == "land-camera" and isinstance(o.get("attrs"), dict):
                        last[str(o.get("entity"))] = o["attrs"]
        cams = []
        for cid, a in sorted(last.items()):
            if a.get("lat") is None or a.get("lon") is None or not (a.get("video_url") or a.get("image_url")):
                continue
            cams.append({"id": cid, "lat": a["lat"], "lon": a["lon"], "image": a.get("image_url"),
                         "video": a.get("video_url") or None,
                         "view": a.get("view") if cid.startswith("JamCams") else a.get("orientation") or a.get("view")})
        _RECORDED = cams
        log_line(f"recording: {len(cams)} cameras with stored media ({sum(1 for c in cams if c['video'])} with clips)")
    return _RECORDED


def jamcams():
    if RECORDING:
        return [c for c in recorded_cams() if c["id"].startswith("JamCams")]
    return load_json(os.path.join(FEEDS_STATE, "jamcams.json"), {"items": []})["items"]


def still_cams(kind):
    """Still-image cameras with the entity keys feeds.py uses for them."""
    if RECORDING:
        pre = {"drivebc": "BCCAM-", "nz": "NZCAM-", "sg": "SGCAM-"}.get(kind)
        return [dict(c, video=None) for c in recorded_cams() if pre and c["id"].startswith(pre) and c.get("image")]
    if kind == "drivebc":
        items = load_json(os.path.join(FEEDS_STATE, "drivebc_cams.json"), {"items": []})["items"]
        return [{"id": f"BCCAM-{c['id']}", "lat": c["lat"], "lon": c["lon"], "image": c["image"], "video": None,
                 "view": c.get("orientation") or c.get("view")} for c in items if c.get("image")]
    if kind == "nz":
        items = load_json(os.path.join(FEEDS_STATE, "nz_cams.json"), {"items": []})["items"]
        return [{"id": f"NZCAM-{c['id']}", "lat": c["lat"], "lon": c["lon"], "image": c["image"], "video": None,
                 "view": c.get("view")} for c in items if c.get("image")]
    if kind == "sg":  # image URLs change every few minutes, so ask the API (keyless) each cycle
        req = urllib.request.Request("https://api.data.gov.sg/v1/transport/traffic-images",
                                     headers={"User-Agent": UA, "Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=20) as r:
            d = json.load(r)
        out = []
        for c in ((d.get("items") or [{}])[0]).get("cameras") or []:
            loc = c.get("location") or {}
            if loc.get("latitude") is not None and c.get("image"):
                out.append({"id": f"SGCAM-{c['camera_id']}", "lat": float(loc["latitude"]),
                            "lon": float(loc["longitude"]), "image": c["image"], "video": None, "view": None})
        return out
    return []


def rotate(items, n, key, state):
    if n <= 0 or not items:
        return []
    items = sorted(items, key=lambda c: str(c["id"]))
    off = state.get(key, 0) % len(items)
    state[key] = (off + n) % len(items)
    return (items + items)[off:off + min(n, len(items))]


def pick(cfg, ids=None):
    tfl = jamcams()
    by_id = {c["id"]: c for c in tfl}
    if ids:
        cams = []
        for i in ids:
            if i in by_id:
                cams.append(by_id[i])
            else:
                for kind, pre in (("drivebc", "BCCAM-"), ("nz", "NZCAM-"), ("sg", "SGCAM-")):
                    if i.startswith(pre):
                        cams += [c for c in still_cams(kind) if c["id"] == i]
        return cams
    state = load_json(STATE_FILE, {})
    always = [by_id[i] for i in cfg["always_include"] if i in by_id]
    rest = [c for c in tfl if c["id"] not in set(cfg["always_include"]) and c.get("video")]
    cams = always + rotate(rest, cfg["tfl_cameras_per_cycle"] - len(always), "tfl", state)
    for kind, n in cfg["stills"].items():
        try:
            pool = still_cams(kind) if n or any(str(i).startswith(kind) for i in cfg["still_ids"]) else []
        except Exception as e:
            log_line(f"{kind:<22} still list failed: {e}")
            continue
        cams += [c for c in pool if c["id"] in cfg["still_ids"]]
        cams += rotate([c for c in pool if c["id"] not in cfg["still_ids"]], n, kind, state)
    save_json(STATE_FILE, state)
    return cams


# ---------------------------------------------------------------- send

def send(observations):
    if not observations:
        return "nothing to send"
    lines = "".join(json.dumps(o, separators=(",", ":")) + "\n" for o in observations)
    if SOURCE:  # heimdall-source itself, the batch on its stdin
        source_bin, identity_dir, addr = SOURCE
        p = subprocess.run([source_bin, "--identity-dir", identity_dir, "--addr", addr, "--service", FEED,
                            "--batch", "-"], input=lines, capture_output=True, text=True, timeout=300)
    else:
        os.makedirs(OUT, exist_ok=True)
        path = os.path.join(OUT, f"{FEED}.jsonl")
        with open(path, "w") as f:
            f.write(lines)
        in_container = "/mid/" + os.path.relpath(path, MID)
        p = subprocess.run([HSOURCE, "--service", FEED, "--batch", in_container], stdin=subprocess.DEVNULL,
                           capture_output=True, text=True, timeout=300)
    out = (p.stdout + p.stderr).strip()
    m = re.search(r"batch: (\d+) accepted, (\d+) refused, (\d+) unreadable", out)
    if m:
        return f"accepted={m.group(1)} refused={m.group(2)} unreadable={m.group(3)}"
    return f"hsource FAILED (exit {p.returncode}): {out.splitlines()[-1] if out else 'no output'}"


# ---------------------------------------------------------------- cycle / loop

def cycle(det, cfg, ids=None, annotate=None, dry=False):
    t0, c0 = time.time(), cpu_seconds()
    cams = pick(cfg, ids)
    observations, errors = [], 0
    for i, cam in enumerate(cams):
        if STOP:
            break
        a, ac = time.time(), cpu_seconds()
        try:
            ann = os.path.join(TEST, f"annotated-{cam['id']}.png") if annotate and (annotate == cam["id"]) else None
            obs, s = analyse(det, cam, cfg, ann)
            observations.append(obs)
            log_line(f"{cam['id']:<22} frames={s['frames']:<3} {s['counts']:<40} moving={s['moving'] if s['moving'] is not None else '-'}"
                     f" {obs['attrs'].get('ai_heading_text', '')}  ({(time.time() - a) * 1000:.0f} ms, cpu {(cpu_seconds() - ac):.2f} s)")
        except Exception as e:
            errors += 1
            log_line(f"{cam['id']:<22} ERROR {type(e).__name__}: {str(e)[:160]}")
    result = "dry run, not sent" if dry else send(observations)
    wall, cpu = time.time() - t0, cpu_seconds() - c0
    log_line(f"CYCLE cameras={len(cams)} ok={len(observations)} errors={errors} {result} wall={wall:.1f}s "
             f"cpu={cpu:.1f}s ({100 * cpu / max(cfg['interval_s'], wall):.1f}% of one core over the {cfg['interval_s']} s interval)")
    return observations


def layout(a):
    """Applies the layout options (see main)."""
    global CONFIG_FILE, STATE_DIR, OUT, TEST, LOG, STATE_FILE, TREND_FILE, WATCH_FILE, SECRET, MODELS
    global SERVE_PORT, FRAMES, RECORDING, MEDIA_DIR, SOURCE
    if a.config:
        CONFIG_FILE = os.path.abspath(a.config)
    if a.state_dir:
        STATE_DIR = os.path.abspath(a.state_dir)
        os.makedirs(STATE_DIR, mode=0o700, exist_ok=True)
        OUT, TEST = os.path.join(STATE_DIR, "out"), os.path.join(STATE_DIR, "test")
        STATE_FILE = os.path.join(STATE_DIR, "state.json")
        TREND_FILE = os.path.join(STATE_DIR, "trend.jsonl")
        WATCH_FILE = os.path.join(STATE_DIR, "watches.json")
        SECRET = os.path.join(STATE_DIR, "secret")
        LOG = os.path.join(STATE_DIR, "analytics.log")
    if a.log:
        LOG = None if a.log == "-" else os.path.abspath(a.log)
    if a.model_dir:
        MODELS = os.path.abspath(a.model_dir)
    if a.port:
        SERVE_PORT = a.port
    if a.token_file:
        SECRET = os.path.abspath(a.token_file)
    if a.frames_helper:
        FRAMES = os.path.abspath(a.frames_helper)
    if a.recording:
        RECORDING = os.path.abspath(a.recording)
        MEDIA_DIR = os.path.join(RECORDING, "media")
    if a.media_dir:
        MEDIA_DIR = os.path.abspath(a.media_dir)
    if a.source_bin:
        if not (a.source_identity_dir and a.source_addr):
            sys.exit("--source-bin needs --source-identity-dir and --source-addr")
        SOURCE = (os.path.abspath(a.source_bin), os.path.abspath(a.source_identity_dir), a.source_addr)


def lower_priority(n):
    """nice(n); on Windows (no nice) the below-normal priority class, whatever n."""
    if hasattr(os, "nice"):
        os.nice(n)
        return
    import ctypes
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.GetCurrentProcess.restype = ctypes.c_void_p
    k.SetPriorityClass.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
    if not k.SetPriorityClass(k.GetCurrentProcess(), 0x00004000):  # BELOW_NORMAL_PRIORITY_CLASS
        log_line(f"cannot lower the priority (error {ctypes.get_last_error()})")


def main():
    global STOP
    ap = argparse.ArgumentParser()
    ap.add_argument("--once", action="store_true")
    ap.add_argument("--dry", action="store_true", help="do not send to Heimdall")
    ap.add_argument("--annotate", help="camera id whose last frame is saved annotated in test/")
    ap.add_argument("--bench", action="store_true")
    ap.add_argument("--serve", action="store_true", help="analyse only on request (POST /analyse)")
    g = ap.add_argument_group("layout (default: the development setup beside this file)")
    g.add_argument("--config", help="config.json to read (default: beside this file)")
    g.add_argument("--state-dir", help="where trend.jsonl, watches.json, state.json and out/ go (default: beside this file)")
    g.add_argument("--log", help="log file; '-' for stdout only (default: analytics.log in the state dir)")
    g.add_argument("--model-dir", help="folder with <model>.onnx (default: models/ beside this file)")
    g.add_argument("--provider", choices=("coreml", "cpu"), help="execution provider (default: config.json's)")
    g.add_argument("--threads", type=int, help="CPU provider threads (default: config.json's)")
    g.add_argument("--port", type=int, help=f"--serve: port on 127.0.0.1 (default {SERVE_PORT})")
    g.add_argument("--token-file", help="--serve: the gateway's bearer token (default: secret in the state dir)")
    g.add_argument("--frames-helper", help="midgard-frames, decoding in place of ffmpeg/ffprobe")
    g.add_argument("--recording", help="a recording folder: its land-camera observations are the cameras")
    g.add_argument("--media-dir", help="stored stills and clips for /media/ URLs (default: the recording's media/); "
                                       "with it nothing is fetched")
    g.add_argument("--source-bin", help="heimdall-source, run directly in place of ../hsource.sh")
    g.add_argument("--source-identity-dir", help="with --source-bin: the feeds' identity folder")
    g.add_argument("--source-addr", help="with --source-bin: Heimdall's mesh address")
    g.add_argument("--nice", type=int, default=0, help="lower this process's priority by this much")
    ap.add_argument("ids", nargs="*")
    a = ap.parse_args()
    layout(a)
    cfg = config()
    if a.provider:
        cfg["provider"] = a.provider
    if a.threads:
        cfg["threads"] = max(1, a.threads)
    if a.nice:
        lower_priority(a.nice)
    if a.bench:
        return bench(cfg, a.ids)
    try:
        det = Detector(cfg["model"], cfg["provider"], cfg["threads"])
    except Exception as e:  # e.g. CoreML unavailable: fall back to CPU with a small thread pool
        log_line(f"provider {cfg['provider']} failed ({e}); using CPU, {cfg['threads']} threads")
        det = Detector(cfg["model"], "cpu", cfg["threads"])
    if a.once:
        cycle(det, cfg, a.ids or None, a.annotate, a.dry)
        return
    if a.serve:
        return serve(det, cfg)

    def stop(*_):
        global STOP
        STOP = True
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    log_line(f"START pid={os.getpid()} model={det.name()} interval={cfg['interval_s']}s "
             f"tfl={cfg['tfl_cameras_per_cycle']} stills={cfg['stills']}")
    while not STOP:
        start = time.time()
        cfg = config()  # pick up config.json edits each cycle (model changes need a restart)
        try:
            cycle(det, cfg)
        except Exception as e:
            log_line(f"CYCLE ERROR {type(e).__name__}: {e}")
        while not STOP and time.time() < start + cfg["interval_s"]:
            time.sleep(1)
    log_line("STOP")


# ---------------------------------------------------------------- on request

SECRET = os.path.join(HERE, "secret")  # shared with the gateway; never committed
SERVE_PORT = 7740
RECENT_S = 60  # a camera analysed this recently is answered from the last result


TREND_FILE = os.path.join(HERE, "trend.jsonl")  # results only (numbers), last TREND_S
WATCH_FILE = os.path.join(HERE, "watches.json")
TREND_S = 24 * 3600
WATCH_EVERY_S = 300
MAX_WATCHES = 20
TREND_KEYS = ("ai_vehicles", "ai_people", "ai_moving", "ai_stationary", "ai_stopped", "ai_queue",
              "ai_blocking", "ai_wrong_way", "ai_person_road", "ai_flow", "ai_heading")


def serve(det, cfg=None):
    """Analyse cameras only when asked: POST /analyse {"camera": id, "by": user} from the
    gateway (bearer token from analytics/secret), one at a time. The result is sent to
    Heimdall like any observation and returned to the gateway. Listens on 127.0.0.1 only.

    POST /watch {"camera", "hours", "by"} asks for a camera to be re-analysed every
    WATCH_EVERY_S for that many hours (0 stops it); POST /trend {"camera"} returns its
    results over the last day, kept in trend.jsonl as numbers only, never images."""
    import hmac
    import http.server
    import secrets
    import threading
    if not os.path.exists(SECRET):
        fd = os.open(SECRET, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(secrets.token_hex(24))
    with open(SECRET) as f:
        token = f.read().strip()
    if not token:
        sys.exit(f"empty token in {SECRET}")
    lock = threading.Lock()  # one analysis at a time
    recent = {}  # camera id -> (time, response)
    trend = {}  # camera id -> [point]
    cutoff = time.time() - TREND_S
    try:
        with open(TREND_FILE) as f:
            for line in f:
                try:
                    p = json.loads(line)
                except ValueError:
                    continue
                if p.get("at", 0) >= cutoff:
                    trend.setdefault(p.pop("camera"), []).append(p)
    except FileNotFoundError:
        pass
    # rewrite without the expired points
    with open(TREND_FILE, "w") as f:
        for cam, pts in trend.items():
            for p in pts:
                f.write(json.dumps(dict(p, camera=cam)) + "\n")
    watches = {k: v for k, v in load_json(WATCH_FILE, {}).items() if v.get("until", 0) > time.time()}
    save_json(WATCH_FILE, watches)

    def record(cam_id, attrs, by):
        p = {"at": attrs["ai_at"], "by": by}
        p.update({k[3:]: attrs[k] for k in TREND_KEYS if k in attrs})
        pts = trend.setdefault(cam_id, [])
        pts.append(p)
        old = time.time() - TREND_S
        while pts and pts[0]["at"] < old:
            pts.pop(0)
        with open(TREND_FILE, "a") as f:
            f.write(json.dumps(dict(p, camera=cam_id)) + "\n")

    def run(cam_id, by):
        """(status, body) for one analysis; caller holds the lock."""
        hit = recent.get(cam_id)
        if hit and time.time() - hit[0] < RECENT_S:
            return 200, dict(hit[1], cached=True)
        cams = pick(config(), [cam_id])
        if not cams:
            return 404, {"error": f"no camera {cam_id} with media"}
        t0 = time.time()
        try:
            obs, s = analyse(det, cams[0], config())
        except Exception as e:
            log_line(f"REQUEST {cam_id} by {by} ERROR {type(e).__name__}: {str(e)[:160]}")
            return 502, {"error": f"analysis failed: {type(e).__name__}"}
        result = send([obs])
        log_line(f"REQUEST {cam_id:<22} by {by:<12} {s['counts']:<40} moving={s['moving']} ({(time.time() - t0) * 1000:.0f} ms) {result}")
        record(cam_id, obs["attrs"], by)
        body = {"camera": cam_id, "attrs": obs["attrs"], "sent": result}
        if s.get("playback"):
            body["playback"] = s["playback"]
        recent[cam_id] = (time.time(), body)
        return 200, body

    def watcher():
        while True:
            time.sleep(15)
            now = time.time()
            for cam_id, w in list(watches.items()):
                if w["until"] <= now:
                    watches.pop(cam_id, None)
                    save_json(WATCH_FILE, watches)
                    log_line(f"WATCH {cam_id} ended")
                    continue
                if w.get("next", 0) <= now:
                    w["next"] = now + WATCH_EVERY_S
                    with lock:
                        run(cam_id, f"watch:{w['by']}")
                    save_json(WATCH_FILE, watches)

    threading.Thread(target=watcher, daemon=True).start()

    def watch_of(cam_id):
        w = watches.get(cam_id)
        return {"until": int(w["until"]), "by": w["by"], "every_s": WATCH_EVERY_S} if w else None

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, code, body):
            data = json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            if self.path not in ("/analyse", "/watch", "/trend"):
                return self.reply(404, {"error": "not found"})
            given = (self.headers.get("Authorization") or "").encode()
            if not hmac.compare_digest(given, f"Bearer {token}".encode()):
                return self.reply(401, {"error": "unauthorised"})
            try:
                req = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
                cam_id, by = str(req.get("camera") or ""), str(req.get("by") or "?")[:64]
            except ValueError:
                return self.reply(400, {"error": "bad request"})
            if not re.fullmatch(r"[A-Za-z0-9._-]{1,64}", cam_id):
                return self.reply(400, {"error": "bad camera id"})
            if self.path == "/trend":
                return self.reply(200, {"camera": cam_id, "points": trend.get(cam_id, []), "watch": watch_of(cam_id)})
            if self.path == "/watch":
                hours = req.get("hours")
                if not isinstance(hours, (int, float)) or not 0 <= hours <= 12:
                    return self.reply(400, {"error": "hours must be 0 to 12"})
                if hours == 0:
                    if watches.pop(cam_id, None):
                        log_line(f"WATCH {cam_id} stopped by {by}")
                    save_json(WATCH_FILE, watches)
                    return self.reply(200, {"camera": cam_id, "watch": None})
                if cam_id not in watches and len(watches) >= MAX_WATCHES:
                    return self.reply(400, {"error": f"at most {MAX_WATCHES} cameras can be watched at once"})
                if not pick(config(), [cam_id]):
                    return self.reply(404, {"error": f"no camera {cam_id} with media"})
                old = watches.get(cam_id, {})
                watches[cam_id] = {"until": time.time() + hours * 3600, "by": by, "next": old.get("next", 0)}
                save_json(WATCH_FILE, watches)
                log_line(f"WATCH {cam_id} for {hours} h by {by}")
                return self.reply(200, {"camera": cam_id, "watch": watch_of(cam_id)})
            with lock:
                code, body = run(cam_id, by)
            return self.reply(code, body)

    srv = http.server.ThreadingHTTPServer(("127.0.0.1", SERVE_PORT), Handler)
    if RECORDING:
        recorded_cams()  # read the recording's cameras before the first request
    # One inference before the first request (a CoreML session compiles on first use).
    t0 = time.time()
    det(np.zeros((det.size, det.size, 3), np.uint8), 1.0, 0.3, 0.45)
    log_line(f"warm-up inference {(time.time() - t0) * 1000:.0f} ms")
    log_line(f"SERVE pid={os.getpid()} model={det.name()} on 127.0.0.1:{SERVE_PORT} (on request only; {len(watches)} watches)")
    srv.serve_forever()


def bench(cfg, ids):
    cams = pick(cfg, ids or [c["id"] for c in jamcams()[:2]])
    for model, prov, th in (("yolox_tiny", "cpu", 2), ("yolox_s", "cpu", 2), ("yolox_s", "coreml", 2),
                            ("yolox_tiny", "coreml", 2)):
        try:
            det = Detector(model, prov, th)
        except Exception as e:
            print(model, prov, "failed:", e); continue
        c = dict(cfg, model=model, provider=prov)
        det(np.zeros((det.size, det.size, 3), np.uint8), 1.0, 0.3, 0.45)  # warm up
        for cam in cams:
            t, ct = time.time(), cpu_seconds()
            obs, s = analyse(det, cam, c)
            print(f"{model:<11} {prov:<6} {cam['id']} {s['frames']} frames {time.time() - t:.2f}s wall "
                  f"{cpu_seconds() - ct:.2f}s cpu  {s['counts']} moving={s['moving']}", flush=True)


if __name__ == "__main__":
    main()
