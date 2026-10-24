#!/usr/bin/env python3
"""Write a synthetic traffic camera clip for the camera analytics' tests on Windows.

    python -I tools/win-app/make-camera-clip.py --ffmpeg FFMPEG OUT.mp4 [--still OUT.jpg]

A 10 s, 25 fps, 352x288 H.264 MP4 (yuv420p, the size and format of the TfL JamCams clips
the analytics are tuned for): a drawn road scene, side on, with cartoon cars driving
across it in two lanes, the near lane left to right and the far lane right to left,
and one parked. --still also writes the first frame as a JPEG. Everything is drawn here
(numpy) and encoded by the given ffmpeg (libx264, one thread): no recorded or
third-party footage. The same ffmpeg gives the same bytes.

Run with a Python that has numpy: the analytics runtime's (helpers\\analytics\\python.exe)
in CI.
"""
import argparse
import subprocess
import sys

import numpy as np

W, H, FPS, SECONDS = 352, 288, 25, 10

# BGR colours
SKY_TOP, SKY_BOTTOM = (235, 200, 150), (250, 235, 215)
GRASS = (70, 140, 80)
ASPHALT = (95, 95, 100)
MARK = (230, 230, 230)
BODY = [(40, 40, 200), (200, 90, 30), (60, 170, 230), (210, 210, 210), (40, 40, 40), (60, 120, 40)]
GLASS = (90, 70, 50)
TYRE = (25, 25, 25)
HUB = (170, 170, 170)

# (lane top y of the wheels' bottom, start x, pixels per frame, car length, body colour index)
CARS = [
    (232, -140, 3.2, 120, 0),
    (232, -420, 3.2, 112, 1),
    (232, -700, 3.6, 124, 2),
    (176, 420, -2.4, 92, 3),
    (176, 700, -2.6, 88, 4),
    (176, 980, -2.4, 96, 2),
]
PARKED = (272, 250, 0.0, 104, 5)  # on the near verge, still for the whole clip


def background():
    img = np.zeros((H, W, 3), np.float32)
    t = np.linspace(0, 1, 120)[:, None]
    img[:120] = (np.array(SKY_TOP) * (1 - t) + np.array(SKY_BOTTOM) * t)[:, None, :]
    img[120:140] = GRASS
    img[140:250] = ASPHALT
    img[250:] = GRASS
    # lane markings: dashed centre line, solid edges
    img[142:145] = MARK
    img[246:249] = MARK
    for x in range(0, W, 40):
        img[193:196, x:x + 22] = MARK
    # a little texture, so the encoder and the decoders have something to agree on
    rng = np.random.default_rng(7)
    img += rng.normal(0, 4, img.shape)
    return img


def disc(img, cx, cy, r, colour):
    y0, y1 = max(0, int(cy - r)), min(H, int(cy + r) + 1)
    x0, x1 = max(0, int(cx - r)), min(W, int(cx + r) + 1)
    if y0 >= y1 or x0 >= x1:
        return
    yy, xx = np.mgrid[y0:y1, x0:x1]
    m = (yy - cy) ** 2 + (xx - cx) ** 2 <= r * r
    img[y0:y1, x0:x1][m] = colour


def box(img, x0, y0, x1, y1, colour):
    x0, x1 = max(0, int(x0)), min(W, int(x1))
    y0, y1 = max(0, int(y0)), min(H, int(y1))
    if x0 < x1 and y0 < y1:
        img[y0:y1, x0:x1] = colour


def car(img, x, ground, length, colour, facing):
    """A side-on car whose wheels touch `ground`, from x to x + length."""
    h = length * 0.42
    wheel = length * 0.11
    body_top, body_bottom = ground - h * 0.62, ground - wheel * 0.6
    box(img, x, body_top, x + length, body_bottom, colour)
    # the cabin, set back from the front, with two windows and a pillar
    front = 0.22 if facing > 0 else 0.30
    cx0, cx1 = x + length * (0.30 if facing > 0 else 0.22), x + length * (1 - front)
    roof = ground - h
    for row in range(int(roof), int(body_top) + 1):
        f = (row - roof) / max(1.0, body_top - roof)  # 0 at the roof, 1 at the waist
        slope = (1 - f) * length * 0.08
        box(img, cx0 + slope, row, cx1 - slope, row + 1, colour)
        if row > roof + 2 and row < body_top - 1:
            g0, g1 = cx0 + slope + 3, cx1 - slope - 3
            mid = (g0 + g1) / 2
            box(img, g0, row, mid - 2, row + 1, GLASS)
            box(img, mid + 2, row, g1, row + 1, GLASS)
    # lights, bumpers and a darker sill
    lamp_x = x + length - 5 if facing > 0 else x
    box(img, lamp_x, body_top + 3, lamp_x + 5, body_top + 8, (180, 245, 255))
    tail_x = x if facing > 0 else x + length - 4
    box(img, tail_x, body_top + 3, tail_x + 4, body_top + 8, (40, 40, 220))
    box(img, x, body_bottom - 4, x + length, body_bottom, tuple(int(c * 0.6) for c in colour))
    for wx in (x + length * 0.2, x + length * 0.8):
        disc(img, wx, ground - wheel, wheel, TYRE)
        disc(img, wx, ground - wheel, wheel * 0.45, HUB)


def frames():
    base = background()
    rng = np.random.default_rng(11)
    for f in range(FPS * SECONDS):
        img = base.copy()
        # far lane first, so the near lane's cars are drawn over it
        for ground, x0, v, length, c in sorted(CARS + [PARKED], key=lambda c: c[0]):
            x = x0 + v * f
            if x + length < -10 or x > W + 10:
                continue
            car(img, x, ground, length, BODY[c], 1 if v >= 0 else -1)
        img += rng.normal(0, 2, img.shape)  # sensor noise, a little different each frame
        yield np.clip(img, 0, 255).astype(np.uint8)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--ffmpeg", required=True)
    ap.add_argument("--still", help="also write the first frame here as a JPEG")
    ap.add_argument("out")
    a = ap.parse_args()
    enc = subprocess.Popen([a.ffmpeg, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24",
                            "-s", f"{W}x{H}", "-r", str(FPS), "-i", "pipe:0",
                            "-c:v", "libx264", "-threads", "1", "-preset", "medium", "-crf", "23",
                            "-pix_fmt", "yuv420p", "-movflags", "+faststart", a.out], stdin=subprocess.PIPE)
    first = None
    for img in frames():
        if first is None:
            first = img
        enc.stdin.write(img.tobytes())
    enc.stdin.close()
    if enc.wait() != 0:
        sys.exit(f"make-camera-clip: ffmpeg exited {enc.returncode}")
    if a.still:
        p = subprocess.run([a.ffmpeg, "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24",
                            "-s", f"{W}x{H}", "-i", "pipe:0", "-frames:v", "1", "-q:v", "3", a.still],
                           input=first.tobytes())
        if p.returncode != 0:
            sys.exit(f"make-camera-clip: ffmpeg exited {p.returncode} for the still")
    print(f"make-camera-clip: {a.out} ({W}x{H}, {FPS} fps, {SECONDS} s)" + (f", {a.still}" if a.still else ""))


if __name__ == "__main__":
    main()
