#!/usr/bin/env python3
"""Capture the rain radar for the offline Mac app: RainViewer's past frames, as the Heimdall
console's radar overlay (products/heimdall/ui/src/app/radar.ts) requests them, into
<recording>/radar/, which the app's web server serves at /radar/.

    python3 tools/mac-app/capture-radar.py [RECORDING_DIR]

RECORDING_DIR defaults to the recorded hour (~/Library/Application Support/Midgard-dev/
recordings/hour-1). Reads https://api.rainviewer.com/public/weather-maps.json and, for every
past frame in it (about two hours, one every 10 minutes), the zoom-6 Web Mercator tiles that
cover radar.ts's RADAR_BOX (the UK and Ireland; the overlay draws only that box, whatever region
the console shows), 256 px, colour scheme 2, options 1_1: the same URLs the live overlay loads.

Writes:
    radar/manifest.json            what the console's offline build reads (see below)
    radar/frames/<n>/<x>/<y>.png   frame n's tiles (n = 0 is the oldest)

manifest.json: {"version": 1, "source", "attribution", "link", "captured" (unix s),
"firstTime" (unix s of frame 0), "step" (s between frames), "zoom", "tileSize", "box",
"tiles": {"x0","x1","y0","y1"}, "frames": [{"offset": s after frame 0, "path":
"frames/<n>", "missing": [[x, y], ...]}]}. Times are relative to the first frame: the console
shifts them so the newest frame is "now" and loops over them.

Polite to the API: WORKERS downloads at a time, at most one start every PACE_S, TIMEOUT_S
each, one retry (after Retry-After when rate limited), failures skipped and counted. Downloads are untrusted: a tile is kept only if it is a PNG (magic bytes) under
MAX_TILE bytes. Written to radar.tmp/ first and swapped in only when complete, so a failed
capture leaves the previous one in place. Needs no key.

RainViewer's terms (rainviewer.com/api.html): free for personal, educational and small-scale
community use; attribution ("Weather data by RainViewer" with a link to rainviewer.com) is
mandatory. The console shows it with the overlay.
"""
import concurrent.futures
import json
import math
import os
import shutil
import sys
import threading
import time
import urllib.error
import urllib.request

MANIFEST_URL = "https://api.rainviewer.com/public/weather-maps.json"
HOST = "https://tilecache.rainviewer.com"
# Must match radar.ts (RADAR_BOX, ZOOM, TILE, the colour scheme and options in its tile URL);
# the console refuses a manifest whose box or zoom differs.
BOX = {"west": -12, "east": 4, "south": 48, "north": 62}
ZOOM = 6
TILE = 256
COLOUR = 2
OPTIONS = "1_1"

WORKERS = 2
PACE_S = 0.25  # at most 4 requests a second in all
TIMEOUT_S = 20
RETRY_WAIT_S = 3
RATE_LIMIT_WAIT_S = 15
MAX_RETRY_AFTER_S = 60
MAX_TILE = 2 * 1024 * 1024
MAX_MANIFEST = 1024 * 1024
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
USER_AGENT = "midgard-mac-app-capture/1 (offline demo build)"
DEFAULT_RECORDING = os.path.expanduser(
    "~/Library/Application Support/Midgard-dev/recordings/hour-1")


def tile_x(lon, z):
    return (lon + 180) / 360 * 2 ** z


def tile_y(lat, z):
    r = math.radians(lat)
    return (1 - math.log(math.tan(r) + 1 / math.cos(r)) / math.pi) / 2 * 2 ** z


def tile_range(box=BOX, z=ZOOM):
    """(x0, x1, y0, y1), inclusive, as radar.ts computes them."""
    return (math.floor(tile_x(box["west"], z)), math.floor(tile_x(box["east"], z)),
            math.floor(tile_y(box["north"], z)), math.floor(tile_y(box["south"], z)))


def is_png(data):
    return len(data) > len(PNG_MAGIC) and data.startswith(PNG_MAGIC)


_pace_lock = threading.Lock()
_next_start = [0.0]


def _pace():
    """Spaces request starts PACE_S apart across all workers."""
    with _pace_lock:
        now = time.monotonic()
        wait = _next_start[0] - now
        _next_start[0] = max(now, _next_start[0]) + PACE_S
    if wait > 0:
        time.sleep(wait)


def _retry_wait(e):
    """Seconds to wait before the retry: Retry-After on a 429/503 (capped), else RETRY_WAIT_S."""
    if isinstance(e, urllib.error.HTTPError) and e.code in (429, 503):
        try:
            return min(MAX_RETRY_AFTER_S, max(1, int(e.headers.get("Retry-After", ""))))
        except (TypeError, ValueError, AttributeError):
            return RATE_LIMIT_WAIT_S
    return RETRY_WAIT_S


def fetch(url, limit):
    """The body of `url` (at most `limit` bytes), with one retry; raises on failure."""
    last = None
    for attempt in range(2):
        _pace()
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=TIMEOUT_S) as res:
                if res.status != 200:
                    raise urllib.error.HTTPError(url, res.status, "status", res.headers, None)
                data = res.read(limit + 1)
                if len(data) > limit:
                    raise ValueError("too large")
                return data
        except Exception as e:  # noqa: BLE001 - any failure is retried once, then reported
            last = e
            if attempt == 0:
                time.sleep(_retry_wait(e))
    raise last


def past_frames(maps):
    """The past frames of a weather-maps.json, oldest first, checked."""
    frames = []
    for f in (maps.get("radar") or {}).get("past") or []:
        t, path = f.get("time"), f.get("path")
        if not isinstance(t, int) or not isinstance(path, str):
            continue
        # A path is "/v2/radar/<id>": nothing that could leave the tile host.
        if not path.startswith("/") or ".." in path or "//" in path or "?" in path or "#" in path:
            continue
        frames.append({"time": t, "path": path})
    frames.sort(key=lambda f: f["time"])
    return frames


def build_manifest(frames, missing, captured, rng=None):
    """The local manifest for `frames` (oldest first, each with "time"); `missing[n]` is the
    list of [x, y] tiles frame n lacks. Frames with no tile at all are left out."""
    x0, x1, y0, y1 = rng or tile_range()
    total = (x1 - x0 + 1) * (y1 - y0 + 1)
    kept = [(n, f) for n, f in enumerate(frames) if len(missing[n]) < total]
    if not kept:
        raise ValueError("no frame has any tile")
    first = kept[0][1]["time"]
    offsets = [f["time"] - first for _, f in kept]
    gaps = [b - a for a, b in zip(offsets, offsets[1:]) if b > a]
    return {
        "version": 1,
        "source": "RainViewer",
        "attribution": "Weather data by RainViewer",
        "link": "https://www.rainviewer.com/",
        "captured": captured,
        "firstTime": first,
        "step": min(gaps) if gaps else 600,
        "zoom": ZOOM,
        "tileSize": TILE,
        "box": dict(BOX),
        "tiles": {"x0": x0, "x1": x1, "y0": y0, "y1": y1},
        "frames": [{"offset": f["time"] - first, "path": "frames/%d" % n,
                    "missing": sorted(missing[n])} for n, f in kept],
    }


def main(argv):
    if len(argv) > 2 or (len(argv) == 2 and argv[1].startswith("-")):
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    recording = os.path.abspath(argv[1] if len(argv) == 2 else DEFAULT_RECORDING)
    if not os.path.isfile(os.path.join(recording, "meta.json")):
        print("not a recording (no meta.json): %s" % recording, file=sys.stderr)
        return 1
    out = os.path.join(recording, "radar")
    tmp = out + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)

    maps = json.loads(fetch(MANIFEST_URL, MAX_MANIFEST))
    host = maps.get("host")
    if host != HOST:  # only the tile host radar.ts falls back to; nothing else is fetched
        print("unexpected tile host in weather-maps.json; using %s" % HOST)
    frames = past_frames(maps)
    if not frames:
        print("no past radar frames in weather-maps.json", file=sys.stderr)
        return 1
    x0, x1, y0, y1 = tile_range()
    jobs = [(n, x, y) for n in range(len(frames))
            for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)]
    print("radar: %d frames x %d tiles = %d downloads (zoom %d, x %d-%d, y %d-%d)" % (
        len(frames), (x1 - x0 + 1) * (y1 - y0 + 1), len(jobs), ZOOM, x0, x1, y0, y1))

    def one(job):
        n, x, y = job
        url = "%s%s/%d/%d/%d/%d/%d/%s.png" % (HOST, frames[n]["path"], TILE, ZOOM, x, y, COLOUR, OPTIONS)
        try:
            data = fetch(url, MAX_TILE)
        except urllib.error.HTTPError as e:
            return job, "failed (HTTP %d)" % e.code
        except Exception as e:  # noqa: BLE001
            return job, "failed (%s)" % type(e).__name__
        if not is_png(data):
            return job, "not a PNG"
        d = os.path.join(tmp, "frames", str(n), str(x))
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, "%d.png" % y), "wb") as f:
            f.write(data)
        return job, None

    missing = [[] for _ in frames]
    failures = {}
    with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
        for (n, x, y), err in pool.map(one, jobs):
            if err:
                missing[n].append([x, y])
                failures[err] = failures.get(err, 0) + 1
    ok = len(jobs) - sum(failures.values())
    print("tiles: %d saved, %d skipped%s" % (ok, len(jobs) - ok,
          "".join(" · %d %s" % (c, e) for e, c in sorted(failures.items()))))
    try:
        manifest = build_manifest(frames, missing, int(time.time()))
    except ValueError as e:
        shutil.rmtree(tmp, ignore_errors=True)
        print("capture failed: %s; the previous radar/ (if any) is kept" % e, file=sys.stderr)
        return 1
    os.makedirs(tmp, exist_ok=True)
    with open(os.path.join(tmp, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=1)
    old = out + ".old"
    shutil.rmtree(old, ignore_errors=True)
    if os.path.exists(out):
        os.rename(out, old)
    os.rename(tmp, out)
    shutil.rmtree(old, ignore_errors=True)
    size = sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(out) for f in fs)
    span = manifest["frames"][-1]["offset"]
    print("radar: %d frames over %d min, %.1f MB in %s" % (
        len(manifest["frames"]), span // 60, size / 1e6, out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
