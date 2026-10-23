#!/usr/bin/env python3
"""Capture the globe's close-up map tiles for the offline Mac app: the terrain (Terrain Tiles,
terrarium PNG) and satellite imagery (EOX Sentinel-2 cloudless 2016) tiles the Heimdall
console's close-up detail (products/heimdall/ui/src/app/globe/detailPatch.ts) reads, into
<recording>/tiles/, which the app's web server serves at /tiles/.

    python3 tools/mac-app/capture-tiles.py [--plan] [RECORDING_DIR]

RECORDING_DIR defaults to the recorded hour (~/Library/Application Support/Midgard-dev/
recordings/hour-1). --plan prints the plan (tiles and estimated size per zoom) and stops.

What the console reads (GlobeView.updateDetail, detailPatch.zoomFor): within 700 km of the
ground, a box about the view (0.2-8 degrees of latitude) at terrain zoom 7-12 (12 at the
closest, 50 km) and imagery one zoom finer (8-13). This captures them over the UK and Ireland
box (regions.ts 'uk', the default region and the bundled map's extent), as zoom pairs
(terrain z, imagery z+1) from coarse to fine, while the estimate stays under MAX_TOTAL; the
finest pairs are dropped. Offline the console uses the finest captured zoom at or below the
one it wants, and clips the close-up to the captured box (the bundled map outside it).
Only tiles with land are stored: zoom 7 terrain is read whole; a finer tile (terrain z+1, and
imagery) is read only where its parent terrain tile has ground above sea level (h > 0, which
is all detailPatch draws as land), so the open sea costs nothing. A tile in a level's range
that is not stored is sea, and the console draws it as sea (EOX's sea colour, height 0).
The other regions do not fit the cap beside the UK and Ireland, so they are not captured.

Writes:
    tiles/manifest.json                     what the console's offline build reads (below)
    tiles/terrarium/<z>/<x>/<y>.png         terrain, the same path shape as the source
    tiles/s2cloudless/<z>/<y>/<x>.jpg       imagery, the same path shape as the source

manifest.json: {"version": 1, "captured" (unix s), "box", "regions": ["uk"], "sources":
{"terrarium" | "s2cloudless": {"attribution", "link", "licence", "levels": [{"z", "x0",
"x1", "y0", "y1", "tiles": [[x, y], ...], "missing": [[x, y], ...]}]}}}. Only integers in the
levels: no URL. "missing" are tiles that could not be captured (the console then uses a
coarser zoom there).

Polite: per host WORKERS downloads at a time and at most one start every PACE_S, TIMEOUT_S
each, one retry (after Retry-After when rate limited), redirects to another host refused,
each tile requested once. Downloads are untrusted: a terrain tile is kept only if it decodes
as a 256 x 256 8-bit RGB(A) PNG, an imagery tile only if it is a JPEG (EOX sends a one-colour
PNG for the sea, which is dropped as sea), each under MAX_TILE bytes. Written to tiles.tmp/
and swapped in only when complete, so a failed capture leaves the previous one in place.

Licences (see tools/mac-app/README.md): Sentinel-2 cloudless 2016 (layer s2cloudless_3857) is
CC BY 4.0, attribution as EOX's capabilities give it for the layer (recorded in the manifest
and checked against the console's); Terrain Tiles need the attribution list in
github.com/tilezen/joerd/blob/master/docs/attribution.md. The console shows both with the
close-up.
"""
import array
import concurrent.futures
import html
import json
import math
import os
import re
import shutil
import struct
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
# The UK and Ireland, regions.ts 'uk' (and the bundled map's box).
BOX = {"west": -12, "east": 4, "south": 48, "north": 62}
REGIONS = ["uk"]
TERRAIN_ZOOMS = range(7, 13)  # detailPatch.zoomFor: 7-12; imagery z+1, at most 13

SOURCES = {
    "terrarium": {
        "host": "s3.amazonaws.com",
        "path": "/elevation-tiles-prod/terrarium/%(z)d/%(x)d/%(y)d.png",
        "file": "terrarium/%(z)d/%(x)d/%(y)d.png",
        "link": "https://github.com/tilezen/joerd/blob/master/docs/attribution.md",
        "licence": "Terrain Tiles attribution list (Mapzen, AWS Open Data)",
        "attribution": "Mapzen Terrain Tiles (AWS Open Data): United Kingdom terrain data "
                       "© Environment Agency copyright and/or database right 2015. All rights "
                       "reserved; Europe terrain data produced using Copernicus data and "
                       "information funded by the European Union - EU-DEM layers; SRTM and "
                       "GMTED2010 courtesy of the U.S. Geological Survey; ETOPO1 U.S. National "
                       "Oceanic and Atmospheric Administration",
        "pace": 0.12,
    },
    "s2cloudless": {
        "host": "tiles.maps.eox.at",
        "path": "/wmts/1.0.0/s2cloudless_3857/default/g/%(z)d/%(y)d/%(x)d.jpg",
        "file": "s2cloudless/%(z)d/%(y)d/%(x)d.jpg",
        "link": "https://cloudless.eox.at",
        "licence": "CC BY 4.0",
        # Must match detailPatch.ts IMAGERY_CREDIT; checked against EOX's capabilities.
        "attribution": "EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH "
                       "(Contains modified Copernicus Sentinel data 2016)",
        "pace": 0.25,
    },
}
CAPABILITIES = "/wmts/1.0.0/WMTSCapabilities.xml"
LAYER = "s2cloudless_3857"

SAMPLES = 12  # land tiles a zoom read to estimate the size (kept out of the capture)
BLOCK = 4096  # files take whole blocks on disk

WORKERS = 4
TIMEOUT_S = 20
RETRY_WAIT_S = 3
RATE_LIMIT_WAIT_S = 15
MAX_RETRY_AFTER_S = 60
MAX_TILE = 2 * 1024 * 1024
MAX_CAPS = 4 * 1024 * 1024
MAX_TOTAL = 300 * 1000 * 1000
USER_AGENT = "midgard-mac-app-capture/1 (offline demo build)"
DEFAULT_RECORDING = os.path.expanduser(
    "~/Library/Application Support/Midgard-dev/recordings/hour-1")
LAND_GRID = os.path.join(HERE, "..", "..", "products", "heimdall", "ui", "public", "imagery",
                         "terrain-uk-0025.i16")
GRID = {"west": -12, "north": 62, "step": 0.025, "nx": 641, "ny": 561}


# ---- tiles --------------------------------------------------------------------------------

def tile_x(lon, z):
    return (lon + 180) / 360 * 2 ** z


def tile_y(lat, z):
    r = math.radians(lat)
    return (1 - math.log(math.tan(r) + 1 / math.cos(r)) / math.pi) / 2 * 2 ** z


def lon_of(x, z):
    return x / 2 ** z * 360 - 180


def lat_of(y, z):
    return math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * y / 2 ** z))))


def tile_range(z, box=BOX):
    """(x0, x1, y0, y1), inclusive: the tiles the box touches, as detailPatch computes them."""
    return (math.floor(tile_x(box["west"], z)), math.floor(tile_x(box["east"], z)),
            math.floor(tile_y(box["north"], z)), math.floor(tile_y(box["south"], z)))


def in_range(rng, x, y):
    x0, x1, y0, y1 = rng
    return x0 <= x <= x1 and y0 <= y <= y1


def children(land_quads, z, rng):
    """The tiles at z + 1 in `rng` under the quadrants of zoom-z tiles that have land
    (land_quads: {(x, y): set of (dx, dy)}, dx/dy 0 or 1), deduplicated and sorted."""
    out = set()
    for (x, y), quads in land_quads.items():
        for dx, dy in quads:
            c = (2 * x + dx, 2 * y + dy)
            if in_range(rng, *c):
                out.add(c)
    return sorted(out)


# ---- terrarium PNG --------------------------------------------------------------------------

PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
JPEG_MAGIC = b"\xff\xd8\xff"


def _unfilter(raw, width, bpp, height):
    """The PNG scanlines of `raw` (filter byte + row each), unfiltered; raises ValueError."""
    stride = width * bpp
    if len(raw) != height * (stride + 1):
        raise ValueError("bad image data length")
    rows = []
    prev = bytearray(stride)
    for j in range(height):
        f = raw[j * (stride + 1)]
        cur = bytearray(raw[j * (stride + 1) + 1:(j + 1) * (stride + 1)])
        if f == 0:
            pass
        elif f == 1:
            for i in range(bpp, stride):
                cur[i] = (cur[i] + cur[i - bpp]) & 0xFF
        elif f == 2:
            for i in range(stride):
                cur[i] = (cur[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                a = cur[i - bpp] if i >= bpp else 0
                cur[i] = (cur[i] + ((a + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                if i >= bpp:
                    a, c = cur[i - bpp], prev[i - bpp]
                else:
                    a = c = 0
                b = prev[i]
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                cur[i] = (cur[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
        else:
            raise ValueError("bad filter %d" % f)
        rows.append(cur)
        prev = cur
    return rows


def decode_terrarium(data):
    """(rows, bpp) of a terrarium tile: 256 x 256, 8-bit RGB or RGBA, CRCs checked; raises
    ValueError if it is anything else."""
    if not data.startswith(PNG_MAGIC):
        raise ValueError("not a PNG")
    i, ihdr, idat = 8, None, []
    while True:
        if i + 8 > len(data):
            raise ValueError("truncated")
        n, kind = struct.unpack(">I4s", data[i:i + 8])
        body = data[i + 8:i + 8 + n]
        if len(body) != n or i + 12 + n > len(data):
            raise ValueError("truncated")
        (crc,) = struct.unpack(">I", data[i + 8 + n:i + 12 + n])
        if zlib.crc32(kind + body) & 0xFFFFFFFF != crc:
            raise ValueError("bad CRC")
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
        i += 12 + n
    if ihdr is None:
        raise ValueError("no header")
    w, h, depth, colour, _, _, interlace = ihdr
    if (w, h, depth, interlace) != (256, 256, 8, 0) or colour not in (2, 6):
        raise ValueError("not a 256 x 256 RGB tile")
    bpp = 3 if colour == 2 else 4
    d = zlib.decompressobj()
    raw = d.decompress(b"".join(idat), 256 * (256 * bpp + 1) + 1)
    if d.unconsumed_tail:
        raise ValueError("image data too long")
    return _unfilter(raw, 256, bpp, 256), bpp


def land_quadrants(rows, bpp, margin=2):
    """The quadrants (dx, dy) of a decoded tile with ground above sea level: terrarium
    h = R * 256 + G + B / 256 - 32768 > 0, within `margin` pixels of the quadrant."""
    out = set()
    for dy, (j0, j1) in enumerate(((0, 128 + margin), (128 - margin, 256))):
        for dx, (i0, i1) in enumerate(((0, 128 + margin), (128 - margin, 256))):
            hit = False
            for j in range(j0, j1):
                row = rows[j]
                for i in range(i0 * bpp, i1 * bpp, bpp):
                    r = row[i]
                    if r > 128 or (r == 128 and (row[i + 1] or row[i + 2])):
                        hit = True
                        break
                if hit:
                    break
            if hit:
                out.add((dx, dy))
    return out


def imagery_kind(data):
    """'jpeg' (a picture, kept), 'sea' (EOX's one-colour PNG for no land), or raises."""
    if data.startswith(JPEG_MAGIC) and len(data) > 64:
        return "jpeg"
    if data.startswith(PNG_MAGIC) and len(data) < 4096:
        return "sea"
    raise ValueError("not a JPEG")


# ---- the plan (estimates before downloading) -------------------------------------------------

def read_land_grid(path=LAND_GRID):
    """The bundled 2.5 km height grid of the box (int16, rows from the north), for estimates."""
    a = array.array("h")
    with open(path, "rb") as f:
        a.frombytes(f.read())
    if sys.byteorder != "little":
        a.byteswap()
    if len(a) != GRID["nx"] * GRID["ny"]:
        raise ValueError("unexpected land grid size")
    return a


def grid_land(grid, z, x, y):
    """Whether the grid has ground above sea level in tile z/x/y (one cell of margin)."""
    nx, ny, step = GRID["nx"], GRID["ny"], GRID["step"]
    w, e = lon_of(x, z), lon_of(x + 1, z)
    n, s = lat_of(y, z), lat_of(y + 1, z)
    i0 = max(0, int((w - GRID["west"]) / step) - 1)
    i1 = min(nx - 1, int((e - GRID["west"]) / step) + 1)
    j0 = max(0, int((GRID["north"] - n) / step) - 1)
    j1 = min(ny - 1, int((GRID["north"] - s) / step) + 1)
    if i0 > i1 or j0 > j1:
        return False
    return any(max(grid[j * nx + i0:j * nx + i1 + 1]) > 0 for j in range(j0, j1 + 1))


def on_disk(n):
    return (n + BLOCK - 1) // BLOCK * BLOCK


def choose_levels(terrain, imagery, cap=MAX_TOTAL):
    """The zooms to capture: terrain z with imagery z + 1, coarse to fine, while the running
    estimate stays under `cap`; then finer imagery alone while it still fits (the picture
    matters more close up than the hillshade). terrain {z: bytes}, imagery {z: bytes}.
    Returns (terrain zooms, imagery zooms, estimated bytes)."""
    total, tz, iz = 0, [], []
    for z in sorted(terrain):
        b = terrain[z] + imagery[min(13, z + 1)]
        if total + b > cap:
            break
        total += b
        tz.append(z)
        iz.append(min(13, z + 1))
    if tz:
        for z in range(iz[-1] + 1, max(imagery) + 1):
            if total + imagery[z] > cap:
                break
            total += imagery[z]
            iz.append(z)
    return tz, iz, total


# ---- fetching -------------------------------------------------------------------------------

class Failed(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


class SameHostRedirects(urllib.request.HTTPRedirectHandler):
    """Follows a redirect only to the same scheme and host; anything else is refused."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        old, new = urllib.parse.urlsplit(req.full_url), urllib.parse.urlsplit(newurl)
        if (new.scheme, new.hostname) != (old.scheme, old.hostname):
            raise Failed("redirect to another host")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


_opener = urllib.request.build_opener(SameHostRedirects())


class Pacer:
    """Spaces request starts `pace` seconds apart across the workers of one host."""

    def __init__(self, pace):
        self.pace, self.lock, self.next = pace, threading.Lock(), 0.0

    def wait(self):
        with self.lock:
            now = time.monotonic()
            wait = self.next - now
            self.next = max(now, self.next) + self.pace
        if wait > 0:
            time.sleep(wait)


def _retry_wait(e):
    if isinstance(e, urllib.error.HTTPError) and e.code in (429, 503):
        try:
            return min(MAX_RETRY_AFTER_S, max(1, int(e.headers.get("Retry-After", ""))))
        except (TypeError, ValueError, AttributeError):
            return RATE_LIMIT_WAIT_S
    return RETRY_WAIT_S


def fetch(host, path, pacer, limit=MAX_TILE):
    """The body of https://host/path (at most `limit` bytes), one retry; raises Failed."""
    url = "https://%s%s" % (host, path)
    reason, err = "unknown", None
    for attempt in range(2):
        pacer.wait()
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with _opener.open(req, timeout=TIMEOUT_S) as res:
                if res.status != 200:
                    raise urllib.error.HTTPError(url, res.status, "status", res.headers, None)
                if urllib.parse.urlsplit(res.geturl()).hostname != host:
                    raise Failed("redirect to another host")
                data = res.read(limit + 1)
                if len(data) > limit:
                    raise Failed("too large")
                return data
        except Failed:
            raise
        except urllib.error.HTTPError as e:
            if e.code == 404:
                raise Failed("HTTP 404")
            reason, err = "HTTP %d" % e.code, e
        except Exception as e:  # noqa: BLE001 - retried once, then reported
            reason, err = type(e).__name__, e
        if attempt == 0:
            time.sleep(_retry_wait(err))
    raise Failed(reason)


def clean_text(s, limit=300):
    s = re.sub(r"<[^>]*>", "", html.unescape(s))
    return re.sub(r"\s+", " ", s).strip()[:limit]


def layer_attribution(caps, layer=LAYER):
    """(attribution, licence) from EOX's WMTS capabilities for `layer`, or (None, None)."""
    for m in re.finditer(r"<Layer>(.*?)</Layer>", caps, re.S):
        block = m.group(1)
        ident = re.search(r"<ows:Identifier>(.*?)</ows:Identifier>", block)
        if not ident or ident.group(1).strip() != layer:
            continue
        ab = re.search(r"<ows:Abstract>(.*?)</ows:Abstract>", block, re.S)
        if not ab:
            return None, None
        text = clean_text(ab.group(1), 1000)
        lic = re.search(r"licenses/([a-z-]+)/([0-9.]+)", html.unescape(ab.group(1)))
        licence = "CC %s %s" % (lic.group(1).upper(), lic.group(2)) if lic else None
        attribution = re.split(r"\s+released under", text)[0].strip()
        return attribution, licence
    return None, None


def tree_size(path):
    return sum(on_disk(os.path.getsize(os.path.join(dp, f)))
               for dp, _, fs in os.walk(path) for f in fs)


def build_manifest(levels, captured, notes):
    out = {}
    for name, src in SOURCES.items():
        lv = []
        for l in levels[name]:
            x0, x1, y0, y1 = l["range"]
            lv.append({"z": l["z"], "x0": x0, "x1": x1, "y0": y0, "y1": y1,
                       "tiles": sorted([list(t) for t in l["tiles"]]),
                       "missing": sorted([list(t) for t in l["missing"]])})
        if not lv or not lv[0]["tiles"]:
            raise ValueError("no %s tiles captured" % name)
        out[name] = {"attribution": notes.get(name, src["attribution"]), "link": src["link"],
                     "licence": src["licence"], "levels": lv}
    return {"version": 1, "captured": captured, "box": dict(BOX), "regions": list(REGIONS),
            "sources": out}


# ---- main -----------------------------------------------------------------------------------

def land_tiles(grid, z):
    x0, x1, y0, y1 = tile_range(z)
    return [(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1) if grid_land(grid, z, x, y)]


def sample_sizes(grid, pacers, cache, n=SAMPLES):
    """Mean bytes on disk of a land tile, per source and zoom, from `n` land tiles spread
    evenly through each zoom's list (bodies kept in `cache`, so the capture does not ask
    again), and the land tile counts. Raises Failed."""
    sizes, counts = {"terrarium": {}, "s2cloudless": {}}, {}
    wanted = [("terrarium", z) for z in TERRAIN_ZOOMS] + \
             [("s2cloudless", z) for z in sorted({min(13, z + 1) for z in TERRAIN_ZOOMS})]
    for name, z in wanted:
        if z not in counts:
            counts[z] = land_tiles(grid, z)
        tiles = counts[z]
        picks = [tiles[int((k + 0.5) * len(tiles) / n)] for k in range(min(n, len(tiles)))]
        src, got = SOURCES[name], []
        for x, y in picks:
            try:
                data = fetch(src["host"], src["path"] % {"z": z, "x": x, "y": y}, pacers[name])
            except Failed:
                continue
            cache[(name, z, x, y)] = data
            if name == "terrarium" or data.startswith(JPEG_MAGIC):
                got.append(on_disk(len(data)))
            else:
                got.append(0)  # sea: not stored
        if not got:
            raise Failed("%s zoom %d: no sample could be read" % (name, z))
        sizes[name][z] = int(sum(got) / len(got) * 1.05)
    return sizes, {z: len(t) for z, t in counts.items()}


def main(argv):
    args = argv[1:]
    plan_only = "--plan" in args
    args = [a for a in args if a != "--plan"]
    if len(args) > 1 or (args and args[0].startswith("-")):
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    recording = os.path.abspath(args[0] if args else DEFAULT_RECORDING)
    if not os.path.isfile(os.path.join(recording, "meta.json")):
        print("not a recording (no meta.json): %s" % recording, file=sys.stderr)
        return 1
    pacers = {n: Pacer(s["pace"]) for n, s in SOURCES.items()}
    started = time.time()

    # EOX's own attribution and licence for the layer, checked against the console's.
    notes = {}
    try:
        caps = fetch(SOURCES["s2cloudless"]["host"], CAPABILITIES, pacers["s2cloudless"], MAX_CAPS)
        attribution, licence = layer_attribution(caps.decode("utf-8", "replace"))
    except Failed as e:
        attribution, licence = None, None
        print("EOX capabilities: failed (%s)" % e.reason)
    want = SOURCES["s2cloudless"]
    if attribution and attribution != want["attribution"]:
        print("NOTE: EOX now gives the attribution for %s as %r; detailPatch.ts IMAGERY_CREDIT "
              "says %r (the manifest records EOX's)" % (LAYER, attribution, want["attribution"]))
        notes["s2cloudless"] = attribution
    if licence and licence != want["licence"]:
        print("NOTE: EOX gives the licence of %s as %s, not %s" % (LAYER, licence, want["licence"]))
    print("imagery: %s, %s" % (attribution or want["attribution"], licence or "licence not read"))

    # The plan: land tiles per zoom (from the bundled height grid) x sampled tile sizes.
    grid = read_land_grid()
    cache = {}
    try:
        sizes, counts = sample_sizes(grid, pacers, cache)
    except Failed as e:
        print("size samples failed: %s" % e.reason, file=sys.stderr)
        return 1
    tb = {z: counts[z] * sizes["terrarium"][z] for z in TERRAIN_ZOOMS}
    ib = {z: counts[z] * sizes["s2cloudless"][z] for z in sizes["s2cloudless"]}
    keep, keep_i, total = choose_levels(tb, ib)
    print("plan, UK and Ireland %s, land tiles only, sizes on disk from %d land tiles a zoom:" % (
        BOX, SAMPLES))
    for z in TERRAIN_ZOOMS:
        iz = min(13, z + 1)
        print("  terrain z%-2d %6d tiles ~%5.0f MB %-9s imagery z%-2d %6d tiles ~%5.0f MB %s" % (
            z, counts[z], tb[z] / 1e6, "capture" if z in keep else "dropped",
            iz, counts[iz], ib[iz] / 1e6, "capture" if iz in keep_i else "dropped"))
    print("  all zooms: ~%.0f MB; cap %.0f MB; capturing terrain z%s, imagery z%s, ~%.0f MB" % (
        (sum(tb.values()) + sum(ib.values())) / 1e6, MAX_TOTAL / 1e6,
        ",".join(map(str, keep)), ",".join(map(str, keep_i)), total / 1e6))
    print("  other regions (regions.ts): not captured, the UK and Ireland fill the cap")
    if plan_only or not keep:
        return 0 if keep else 1

    out = os.path.join(recording, "tiles")
    tmp = out + ".tmp"
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    levels = {"terrarium": [], "s2cloudless": []}
    written = [0]
    lock = threading.Lock()

    def run_level(name, z, jobs, keep_fn):
        """Fetches `jobs` at z; keep_fn(data) -> (store?, extra) or raises ValueError."""
        src = SOURCES[name]
        tiles, missing, failures, extra = [], [], {}, {}

        def one(job):
            x, y = job
            with lock:
                if written[0] > MAX_TOTAL:
                    return job, "over the cap", None, 0
            try:
                data = cache.pop((name, z, x, y), None)
                if data is None:
                    data = fetch(src["host"], src["path"] % {"z": z, "x": x, "y": y}, pacers[name])
                store, info = keep_fn(data)
            except Failed as e:
                return job, e.reason, None, 0
            except (ValueError, zlib.error):
                return job, "invalid", None, 0
            if not store:
                return job, None, info, 0
            path = os.path.join(tmp, src["file"] % {"z": z, "x": x, "y": y})
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(data)
            with lock:
                written[0] += on_disk(len(data))
            return job, None, info, len(data)

        with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
            for job, err, info, n in pool.map(one, jobs):
                if err == "HTTP 404":
                    err = None  # outside the source's coverage: nothing there
                if err:
                    missing.append(job)
                    failures[err] = failures.get(err, 0) + 1
                    continue
                if n:
                    tiles.append(job)
                if info is not None:
                    extra[job] = info
        print("  %s z%d: %d requested, %d stored, %d sea, %d missing%s · total %.1f MB" % (
            name, z, len(jobs), len(tiles), len(jobs) - len(tiles) - len(missing), len(missing),
            "".join(" · %d %s" % (c, e) for e, c in sorted(failures.items())), written[0] / 1e6))
        return {"z": z, "range": tile_range(z), "tiles": tiles, "missing": missing}, extra

    def terrain_keep(data):
        rows, bpp = decode_terrarium(data)
        return True, land_quadrants(rows, bpp)

    def imagery_keep(data):
        return imagery_kind(data) == "jpeg", None

    # Terrain, coarse to fine: zoom 7 whole, then only under land. Each level's land
    # quadrants also choose the imagery one zoom finer.
    quads_by_z = {}
    parent = None
    for z in keep:
        rng = tile_range(z)
        if parent is None:
            jobs = [(x, y) for x in range(rng[0], rng[1] + 1) for y in range(rng[2], rng[3] + 1)]
        else:
            jobs = children(parent, z - 1, rng)
        level, quads = run_level("terrarium", z, jobs, terrain_keep)
        # A tile that could not be read may have land: keep its children.
        for t in level["missing"]:
            quads[t] = {(0, 0), (0, 1), (1, 0), (1, 1)}
        quads = {t: q for t, q in quads.items() if q}
        levels["terrarium"].append(level)
        quads_by_z[z] = quads
        parent = quads
        if written[0] > MAX_TOTAL:
            break
    # Imagery: under the land of the terrain one zoom coarser, or (finer than the terrain)
    # under the imagery tiles one zoom coarser that showed land.
    prev = None
    for iz in keep_i:
        if iz - 1 in quads_by_z:
            jobs = children(quads_by_z[iz - 1], iz - 1, tile_range(iz))
        elif prev is not None and prev["z"] == iz - 1:
            all4 = {(0, 0), (0, 1), (1, 0), (1, 1)}
            jobs = children({t: all4 for t in prev["tiles"] + prev["missing"]}, iz - 1, tile_range(iz))
        else:
            break
        prev, _ = run_level("s2cloudless", iz, jobs, imagery_keep)
        levels["s2cloudless"].append(prev)
        if written[0] > MAX_TOTAL:
            break

    # Over the cap (the estimate was short): drop the finest level (imagery first when it is
    # finer than the terrain's pair, else the pair) until it fits.
    while written[0] > MAX_TOTAL and len(levels["terrarium"]) > 1:
        tz = levels["terrarium"][-1]["z"]
        if levels["s2cloudless"][-1]["z"] > min(13, tz + 1):
            drops = [("s2cloudless", levels["s2cloudless"].pop()["z"])]
        else:
            levels["terrarium"].pop()
            drops = [("terrarium", tz)]
            if levels["s2cloudless"][-1]["z"] == min(13, tz + 1):
                drops.append(("s2cloudless", levels["s2cloudless"].pop()["z"]))
        for name, z in drops:
            shutil.rmtree(os.path.join(tmp, name, str(z)), ignore_errors=True)
            print("  over the cap: dropped %s z%d" % (name, z))
        written[0] = tree_size(tmp)

    try:
        manifest = build_manifest(levels, int(started), notes)
    except ValueError as e:
        shutil.rmtree(tmp, ignore_errors=True)
        print("capture failed: %s; the previous tiles/ (if any) is kept" % e, file=sys.stderr)
        return 1
    with open(os.path.join(tmp, "manifest.json"), "w") as f:
        json.dump(manifest, f, separators=(",", ":"))
    old = out + ".old"
    shutil.rmtree(old, ignore_errors=True)
    if os.path.exists(out):
        os.rename(out, old)
    os.rename(tmp, out)
    shutil.rmtree(old, ignore_errors=True)
    for name, src in manifest["sources"].items():
        print("tiles %s: zooms %s, %d tiles, %.1f MB" % (
            name, ",".join(str(l["z"]) for l in src["levels"]),
            sum(len(l["tiles"]) for l in src["levels"]), tree_size(os.path.join(out, name)) / 1e6))
    print("tiles: %.1f MB on disk in %s (%d s)" % (tree_size(out) / 1e6, out, time.time() - started))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
