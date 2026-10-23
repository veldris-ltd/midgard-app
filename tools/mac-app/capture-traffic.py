#!/usr/bin/env python3
"""Capture road traffic flow for the offline Mac app: TomTom Traffic Flow vector tiles, as the
Heimdall console's traffic overlay (products/heimdall/ui/src/app/traffic.ts) requests them, into
<recording>/traffic/, which the app's web server serves at /traffic/.

    python3 tools/mac-app/capture-traffic.py [RECORDING_DIR]

RECORDING_DIR defaults to the recorded hour (~/Library/Application Support/Midgard-dev/
recordings/hour-1). One snapshot of the "relative" flow tiles over traffic.ts's box (RADAR_BOX:
the UK and Ireland, which is all the overview reads):
    zoom 7    the overview (useTraffic), every tile in the box, as traffic.ts reads it;
    zoom 9-10 the globe's close-in reads (GlobeView.updateTraffic: zoomFor(box, 1600), at
              least 9; at the globe's closest, 50 km, over the UK that is zoom 10), only
              under tiles that had roads one level up (the sea has none), so the snapshot
              stays small. The offline console reads the finest captured zoom at or below
              the one it wants.
A zoom is skipped when it would take the capture over MAX_TOTAL bytes.

Writes:
    traffic/manifest.json           what the console's offline build reads (see below)
    traffic/<z>/<x>/<y>.pbf         tiles with at least one road segment

manifest.json: {"version": 1, "source", "attribution" (TomTom's copyright caption, from their
Copyright API at capture time), "link", "captured" (unix s), "overviewZoom", "box", "levels":
[{"z", "x0", "x1", "y0", "y1", "tiles": [[x, y], ...], "missing": [[x, y], ...]}]}. A tile in
a level's range that is in neither list has no roads (empty, or under an empty parent).
Only z/x/y appear in it: no URL, and never the key.

The key is the dev setup's (TOMTOM_KEY or VITE_TOMTOM_KEY in the environment, else
VITE_TOMTOM_KEY in products/heimdall/ui/.env.local). It is sent to api.tomtom.com and nowhere
else: never printed, logged, or written to the recording, and errors name only z/x/y.

Polite to the API: WORKERS downloads at a time, at most one start every PACE_S, TIMEOUT_S
each, one retry (after Retry-After when rate limited), failures skipped and counted by code.
Downloads are untrusted: a tile is kept only if it parses as a Mapbox vector tile (protobuf
layers) under MAX_TILE bytes. Written to traffic.tmp/ first and swapped in only when complete,
so a failed capture leaves the previous one in place.

TomTom's developer terms (docs.tomtom.com/legal/terms-and-conditions; see tools/mac-app/README.md):
storing Results is prohibited beyond client caching within their cache headers (11.4), and
attribution must come from their Copyright API (17.3). This capture goes beyond that; the user
decided to include it (docs/plans/2026-10-23-mac-app-phase6-local-parity.md). The console shows
the copyright caption with the overlay.
"""
import concurrent.futures
import json
import math
import os
import re
import shutil
import sys
import threading
import time
import urllib.error
import urllib.request

API = "https://api.tomtom.com"
TILE_PATH = "/traffic/map/4/tile/flow/relative/%d/%d/%d.pbf"
COPYRIGHT_PATH = "/map/2/copyrights/caption.json"
LAYER = "Traffic flow"
# Must match traffic.ts (RADAR_BOX, OVERVIEW_ZOOM); the console refuses a manifest whose box
# or overview zoom differs.
BOX = {"west": -12, "east": 4, "south": 48, "north": 62}
OVERVIEW_ZOOM = 7
CLOSE_ZOOMS = (9, 10)  # finer would never be read (see above)

WORKERS = 3
PACE_S = 0.2  # at most 5 requests a second in all
TIMEOUT_S = 20
RETRY_WAIT_S = 3
RATE_LIMIT_WAIT_S = 15
MAX_RETRY_AFTER_S = 60
MAX_TILE = 4 * 1024 * 1024
MAX_TOTAL = 150 * 1000 * 1000
MAX_CAPTION = 64 * 1024
DEFAULT_CAPTION = "©TomTom"
USER_AGENT = "midgard-mac-app-capture/1 (offline demo build)"
DEFAULT_RECORDING = os.path.expanduser(
    "~/Library/Application Support/Midgard-dev/recordings/hour-1")
ENV_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        "..", "..", "products", "heimdall", "ui", ".env.local")


def tile_x(lon, z):
    return (lon + 180) / 360 * 2 ** z


def tile_y(lat, z):
    r = math.radians(lat)
    return (1 - math.log(math.tan(r) + 1 / math.cos(r)) / math.pi) / 2 * 2 ** z


def tile_range(z, box=BOX):
    """(x0, x1, y0, y1), inclusive, as traffic.ts computes them."""
    return (math.floor(tile_x(box["west"], z)), math.floor(tile_x(box["east"], z)),
            math.floor(tile_y(box["north"], z)), math.floor(tile_y(box["south"], z)))


def wanted(z, rng, parent):
    """The tiles to read at `z` in `rng`: all of them for the overview (parent None), else
    those whose ancestor at parent["z"] had roads or could not be read."""
    x0, x1, y0, y1 = rng
    if parent is None:
        return [(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)]
    shift = z - parent["z"]
    keep = {tuple(t) for t in parent["tiles"]} | {tuple(t) for t in parent["missing"]}
    return [(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)
            if (x >> shift, y >> shift) in keep]


def _varint(data, i):
    shift = value = 0
    while True:
        if i >= len(data) or shift > 63:
            raise ValueError("bad varint")
        b = data[i]
        i += 1
        value |= (b & 0x7F) << shift
        if not b & 0x80:
            return value, i
        shift += 7


def _fields(data):
    """(field number, wire type, value) of a protobuf message; raises if malformed."""
    i = 0
    while i < len(data):
        key, i = _varint(data, i)
        field, wire = key >> 3, key & 7
        if field == 0:
            raise ValueError("field 0")
        if wire == 0:
            value, i = _varint(data, i)
        elif wire == 1:
            value, i = data[i:i + 8], i + 8
        elif wire == 2:
            n, i = _varint(data, i)
            value, i = data[i:i + n], i + n
        elif wire == 5:
            value, i = data[i:i + 4], i + 4
        else:
            raise ValueError("wire type %d" % wire)
        if i > len(data):
            raise ValueError("truncated")
        yield field, wire, value


def road_segments(data):
    """The number of features in the tile's "Traffic flow" layer; raises ValueError if `data`
    is not a vector tile (only layers, field 3, at the top level, each named)."""
    if not data:
        raise ValueError("empty body")
    count = 0
    for field, wire, layer in _fields(data):
        if field != 3 or wire != 2:
            raise ValueError("not a vector tile")
        name, features = None, 0
        for f, w, v in _fields(layer):
            if f == 1 and w == 2:
                name = v.decode("utf-8")
            elif f == 2 and w == 2:
                features += 1
        if name is None:
            raise ValueError("layer without a name")
        if name == LAYER:
            count += features
    return count


def read_key(environ=os.environ, env_file=ENV_FILE):
    """The TomTom key from the environment or the console's .env.local, or None."""
    for name in ("TOMTOM_KEY", "VITE_TOMTOM_KEY"):
        if environ.get(name, "").strip():
            return environ[name].strip()
    try:
        with open(env_file, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line.startswith("export "):
                    line = line[7:].lstrip()
                if line.startswith("VITE_TOMTOM_KEY="):
                    v = line.split("=", 1)[1].strip().strip("'\"")
                    return v or None
    except OSError:
        pass
    return None


def clean_caption(raw):
    """TomTom's copyright caption from a caption.json body, as plain text, or the default."""
    try:
        text = json.loads(raw).get("copyrightsCaption")
    except (ValueError, AttributeError):
        return DEFAULT_CAPTION
    if not isinstance(text, str):
        return DEFAULT_CAPTION
    text = re.sub(r"<[^>]*>", "", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text[:120] if text and "tomtom" in text.lower() else DEFAULT_CAPTION


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


class Failed(Exception):
    """A download failure that names no URL (the URL holds the key)."""

    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


def fetch(path, key, limit):
    """The body of API + path (at most `limit` bytes), with one retry; raises Failed."""
    url = "%s%s?key=%s" % (API, path, urllib.request.quote(key, safe=""))
    reason = "unknown"
    for attempt in range(2):
        _pace()
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=TIMEOUT_S) as res:
                if res.status != 200:
                    raise urllib.error.HTTPError("", res.status, "status", res.headers, None)
                data = res.read(limit + 1)
                if len(data) > limit:
                    raise Failed("too large")
                return data
        except Failed:
            raise
        except urllib.error.HTTPError as e:
            reason = "HTTP %d" % e.code
            err = e
        except Exception as e:  # noqa: BLE001 - any failure is retried once, then reported
            reason = type(e).__name__
            err = e
        if attempt == 0:
            time.sleep(_retry_wait(err))
    raise Failed(reason)


def build_manifest(levels, caption, captured):
    """The local manifest for the captured levels (each {"z", "range", "tiles", "missing"})."""
    if not levels or levels[0]["z"] != OVERVIEW_ZOOM:
        raise ValueError("no overview level")
    if not levels[0]["tiles"]:
        raise ValueError("the overview has no roads (every tile empty or failed)")
    out = []
    for lv in levels:
        x0, x1, y0, y1 = lv["range"]
        out.append({"z": lv["z"], "x0": x0, "x1": x1, "y0": y0, "y1": y1,
                    "tiles": sorted([list(t) for t in lv["tiles"]]),
                    "missing": sorted([list(t) for t in lv["missing"]])})
    return {
        "version": 1,
        "source": "TomTom Traffic Flow (relative), one snapshot",
        "attribution": caption,
        "link": "https://www.tomtom.com/",
        "captured": captured,
        "overviewZoom": OVERVIEW_ZOOM,
        "box": dict(BOX),
        "levels": out,
    }


def tree_size(path):
    return sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(path) for f in fs)


def recover_previous(out):
    """A capture stopped between the two renames of swap_in leaves only <out>.old: put it
    back, so the previous capture is not lost."""
    old = out + ".old"
    if os.path.isdir(old) and not os.path.exists(out):
        os.rename(old, out)


def swap_in(tmp, out):
    """Replaces `out` with the finished `tmp`; if that fails, the previous `out` is restored."""
    old = out + ".old"
    shutil.rmtree(old, ignore_errors=True)
    if os.path.exists(out):
        os.rename(out, old)
    try:
        os.rename(tmp, out)
    except OSError:
        if os.path.exists(old) and not os.path.exists(out):
            os.rename(old, out)
        raise
    shutil.rmtree(old, ignore_errors=True)


def main(argv):
    if len(argv) > 2 or (len(argv) == 2 and argv[1].startswith("-")):
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    recording = os.path.abspath(argv[1] if len(argv) == 2 else DEFAULT_RECORDING)
    if not os.path.isfile(os.path.join(recording, "meta.json")):
        print("not a recording (no meta.json): %s" % recording, file=sys.stderr)
        return 1
    key = read_key()
    if not key:
        print("no TomTom key: set TOMTOM_KEY, or VITE_TOMTOM_KEY in products/heimdall/ui/.env.local",
              file=sys.stderr)
        return 1
    out = os.path.join(recording, "traffic")
    recover_previous(out)
    tmp = out + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)

    try:
        caption = clean_caption(fetch(COPYRIGHT_PATH, key, MAX_CAPTION))
    except Failed as e:
        print("copyright caption: failed (%s); using %s" % (e.reason, DEFAULT_CAPTION))
        caption = DEFAULT_CAPTION

    levels, total, parent = [], 0, None
    started = time.time()
    for z in (OVERVIEW_ZOOM,) + CLOSE_ZOOMS:
        rng = tile_range(z)
        jobs = wanted(z, rng, parent)
        if parent is not None:
            # A finer level holds about twice the bytes of the one above it (more, shorter
            # segments); skip it if that would pass the cap.
            estimate = 2.5 * parent["bytes"]
            if total + estimate > MAX_TOTAL:
                print("zoom %d: skipped (about %.0f MB more would pass the %.0f MB cap)" % (
                    z, estimate / 1e6, MAX_TOTAL / 1e6))
                break
        print("zoom %d: %d downloads (x %d-%d, y %d-%d)" % (z, len(jobs), *rng))

        def one(job, z=z):
            x, y = job
            try:
                data = fetch(TILE_PATH % (z, x, y), key, MAX_TILE)
                n = road_segments(data)
            except Failed as e:
                return job, "failed (%s)" % e.reason, 0
            except (ValueError, UnicodeDecodeError):
                return job, "not a vector tile", 0
            if n == 0:
                return job, None, 0
            d = os.path.join(tmp, str(z), str(x))
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, "%d.pbf" % y), "wb") as f:
                f.write(data)
            return job, None, len(data)

        tiles, missing, failures, size = [], [], {}, 0
        with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
            for job, err, n in pool.map(one, jobs):
                if err:
                    missing.append(job)
                    failures[err] = failures.get(err, 0) + 1
                elif n:
                    tiles.append(job)
                    size += n
        if total + size > MAX_TOTAL and parent is not None:
            shutil.rmtree(os.path.join(tmp, str(z)), ignore_errors=True)
            print("zoom %d: dropped (%.1f MB would pass the cap)" % (z, size / 1e6))
            break
        total += size
        print("zoom %d: %d with roads (%.1f MB), %d empty, %d skipped%s" % (
            z, len(tiles), size / 1e6, len(jobs) - len(tiles) - len(missing), len(missing),
            "".join(" · %d %s" % (c, e) for e, c in sorted(failures.items()))))
        parent = {"z": z, "range": rng, "tiles": tiles, "missing": missing, "bytes": size}
        levels.append(parent)
        if z == OVERVIEW_ZOOM and not tiles:
            break

    try:
        manifest = build_manifest(levels, caption, int(started))
    except ValueError as e:
        shutil.rmtree(tmp, ignore_errors=True)
        print("capture failed: %s; the previous traffic/ (if any) is kept" % e, file=sys.stderr)
        return 1
    with open(os.path.join(tmp, "manifest.json"), "w") as f:
        json.dump(manifest, f, separators=(",", ":"))
    swap_in(tmp, out)
    print("traffic: zooms %s, %d tiles, %.1f MB in %s (%d s)" % (
        ",".join(str(lv["z"]) for lv in manifest["levels"]),
        sum(len(lv["tiles"]) for lv in manifest["levels"]), tree_size(out) / 1e6, out,
        time.time() - started))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
