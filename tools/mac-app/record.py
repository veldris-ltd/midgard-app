#!/usr/bin/env python3
"""Record the live suite's feeds for the offline Mac app: every Heimdall batch the feeds
send and every Forge source file version, for a given time, packed as a recording that
midgard-replay plays back (format version 1, see tools/replay/src/recording.rs).

    python3 tools/mac-app/record.py record --minutes 60 [--name NAME] [--drop-raw]
    python3 tools/mac-app/record.py pack RAW_DIR OUT_DIR [--no-media] [--refresh-media]
                                                                         # re-pack a capture

Run `record` on the development Mac while the Docker suite and the live feeds run. It
writes to ~/Library/Application Support/Midgard-dev/recordings/<name> (raw capture in
<name>/raw, kept unless --drop-raw), outside the suite folder and its storage budget.
MIDGARD_HOME (default ~/Desktop/Midgard) is where the live feeds are; MIDGARD_RECORDINGS
overrides the recordings folder. Ctrl-C (or SIGTERM, or closing the terminal) stops early and
still packs. raw/meta.json is written at the start, so `pack` also works on a capture that
crashed.

Heimdall batches come from feeds.py's recording hook (feeds.py is not in git; this is its
contract):
  - File: $MIDGARD_HOME/heimdall-poc/feeds/state/record_to
  - Format: "<absolute directory>\npid=<recorder pid>\n" (this script writes its own pid and
    removes the file when capture ends, also on Ctrl-C, SIGTERM, SIGHUP and errors).
  - While the file names an existing directory and that pid is alive, feeds.py also copies every
    batch it sends there as <time_ns>-<feed>.jsonl: written as ".<time_ns>-<feed>.jsonl", then
    renamed, so a file without the leading dot is complete.
  - If the pid is dead (or the directory is missing), feeds.py removes the hook itself, so a
    killed recorder cannot leave copies piling up; a failed copy never stops the live send.
This keeps every batch even when several sources share one feed file (regional-extra).
adsb.lol (heimdall-poc/batch.jsonl, one writer, not sent by feeds.py) and the Forge CSVs are
polled for changes.

Cameras (land-camera) are kept. Their images and clips come from the internet, which the app
never reaches, so packing also captures media: each camera's latest still (image_url) and, where
it has one (TfL), its clip (video_url) are downloaded once into <out>/media/<entity>.<ext>
(MEDIA_WORKERS at a time, MEDIA_TIMEOUT_S each, one retry; a file already there is reused unless
--refresh-media) and those attributes are rewritten to the app-relative /media/<file>, which the
app's web server serves. A camera whose media could not be fetched keeps no internet URL: the
attribute is dropped. --no-media skips the downloads (and so drops every camera media URL).
Camera analysis (land-camera-ai) is not recorded: the app runs its own.
"""
import argparse
import concurrent.futures
import contextlib
import hashlib
import glob
import gzip
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HOME = os.path.expanduser(os.environ.get("MIDGARD_HOME", "~/Desktop/Midgard"))
RECORDINGS = os.path.expanduser(os.environ.get("MIDGARD_RECORDINGS",
                                             "~/Library/Application Support/Midgard-dev/recordings"))
FEEDS = os.path.join(HOME, "heimdall-poc", "feeds")
ADSB_LOL = os.path.join(HOME, "heimdall-poc", "batch.jsonl")
FORGE_DATA = os.path.join(HOME, "live-feeds", "data")
RECORD_TO = os.path.join(FEEDS, "state", "record_to")
FEEDS_PID = os.path.join(FEEDS, "feeds.pid")

POLL_S = 0.5
STABLE_S = 1.0
START_WAIT_S = 3
NO_BATCH_WARN_S = 120
# Camera analysis is not recorded (the app analyses its own stored clips).
DROPPED_KINDS = {"land-camera-ai"}
CAMERA_KIND = "land-camera"
# Camera attributes that hold a URL: the first two are captured into media/, any other
# (stream_url: a live stream cannot be stored) is dropped, so no internet URL is replayed.
MEDIA_ATTRS = {"image_url": ("image", ".jpg"), "video_url": ("video", ".mp4")}
URL_ATTRS = ("image_url", "video_url", "stream_url")
MEDIA_PREFIX = "/media/"
MEDIA_WORKERS = 6
MEDIA_TIMEOUT_S = 20
MEDIA_FILE_MAX = 8 * 1024 * 1024
MEDIA_MAX_MB = 400
MEDIA_UA = {"User-Agent": "VeldrisMidgardDemo/0.1 (offline recording; one fetch per camera)"}
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".gif", ".webp"}
VIDEO_EXTS = {".mp4", ".webm"}
# Moving tracks kept at most once per THIN_S per entity; other kinds are all kept.
THINNED_KINDS = {"air-track", "sea-vessel", "air-balloon", "land-train"}
THIN_S = 30

HEIMDALL_NAME = re.compile(r"^(\d+)-(.+?)(?:~\d+)?\.jsonl$")
INCOMING_NAME = re.compile(r"^(\d+)-(.+)\.jsonl$")
FORGE_NAME = re.compile(r"^(\d+)-(.+)$")


# ---- capture rules (pure) ----

def valid_jsonl(text):
    """Every non-blank line is a JSON object (false for a file caught mid-write)."""
    for line in text.splitlines():
        if not line.strip():
            continue
        try:
            if not isinstance(json.loads(line), dict):
                return False
        except ValueError:
            return False
    return True


def should_copy(mtime, now, last_copied, recorded_from, text=None):
    """Copy a file whose mtime is newer than the version last copied (None: none yet) and
    than the recording's start, unchanged for STABLE_S, and (Heimdall files, given as
    `text`) made of complete JSON object lines."""
    if mtime <= recorded_from or (last_copied is not None and mtime <= last_copied):
        return False
    if now - mtime < STABLE_S:
        return False
    return text is None or valid_jsonl(text)


def offset_of(mtime, recorded_from):
    return max(0, int(mtime - recorded_from))


def heimdall_name(offset, feed, taken):
    """`<offset:06d>-<feed>.jsonl`, with `~002`, `~003`... added if a batch of that feed
    already has that second (each batch holds only new items, so none is overwritten; the
    padding keeps name order equal to arrival order)."""
    name, n = f"{offset:06d}-{feed}.jsonl", 1
    while name in taken:
        n += 1
        name = f"{offset:06d}-{feed}~{n:03d}.jsonl"
    return name


def parse_heimdall_name(name):
    """(offset, feed) from a raw batch file name, or None."""
    m = HEIMDALL_NAME.match(name)
    return (int(m.group(1)), m.group(2)) if m else None


def parse_incoming_name(name):
    """(time_ns, feed) from a file feeds.py's hook wrote, or None (dot-files are still
    being written)."""
    m = INCOMING_NAME.match(name)
    return (int(m.group(1)), m.group(2)) if m and plain(m.group(2)) else None


def move_incoming(incoming, hdir, recorded_from, taken):
    """Move every complete hook file from `incoming` into `hdir` as a raw batch, oldest
    first; returns how many were moved. `taken` (names already in hdir) is updated."""
    found = []
    for name in os.listdir(incoming):
        parsed = parse_incoming_name(name)
        if parsed:
            found.append((parsed, name))
    moved = 0
    for (ns, feed), name in sorted(found):
        dest = heimdall_name(offset_of(ns / 1e9, recorded_from), feed, taken)
        os.replace(os.path.join(incoming, name), os.path.join(hdir, dest))
        taken.add(dest)
        moved += 1
    return moved


@contextlib.contextmanager
def record_to(hook, target):
    """Point feeds.py's recording hook at `target` (with this process's pid, so feeds.py can
    drop a hook whose recorder has gone) for the duration of the block; the hook file is
    always removed afterwards (also on Ctrl-C and errors)."""
    with open(hook, "w") as f:
        f.write(f"{os.path.abspath(target)}\npid={os.getpid()}\n")
    try:
        yield
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.remove(hook)


@contextlib.contextmanager
def stop_on_signals(stopping):
    """For the duration of the block, SIGINT, SIGTERM and SIGHUP append to `stopping` (the
    capture loop then stops and the capture is packed); the previous handlers are restored
    afterwards."""
    def handler(signum, _frame):
        if not stopping:
            print(f"\nstopping ({signal.Signals(signum).name}): packing what was captured")
        stopping.append(signum)

    signums = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
    previous = {s: signal.signal(s, handler) for s in signums}
    try:
        yield
    finally:
        for s, h in previous.items():
            signal.signal(s, h)


def write_meta(raw, recorded_from, duration):
    with open(os.path.join(raw, "meta.json"), "w") as f:
        json.dump({"recordedFrom": recorded_from, "duration": duration}, f)


def prepare(out, recorded_from):
    """Create `<out>/raw` with its folders and a meta.json (duration 1 until the end), so a
    crashed capture can still be packed; returns the raw path. Refuses an existing `out`."""
    if os.path.exists(out):
        raise SystemExit(f"{out} already exists: choose another --name")
    raw = os.path.join(out, "raw")
    for sub in ("heimdall", "forge", "incoming"):
        os.makedirs(os.path.join(raw, sub))
    write_meta(raw, recorded_from, 1)
    return raw


def pid_alive(pidfile):
    """True if `pidfile` holds the pid of a running process."""
    try:
        with open(pidfile) as f:
            pid = int(f.read().strip())
        os.kill(pid, 0)
    except PermissionError:
        return True  # exists, owned by someone else
    except (OSError, ValueError):
        return False
    return True


# ---- packing (pure) ----

def drop_kinds(batches):
    return [dict(b, observations=[o for o in b["observations"] if o.get("kind") not in DROPPED_KINDS])
            for b in batches]


# ---- camera media (pure, except capture_media's downloads) ----

def safe_stem(entity, taken):
    """A file name stem for `entity`: letters, digits, `.`, `_` and `-` only, never starting
    with a dot; a stem already in `taken` (two entities that clean to the same name) gets a
    short hash of the entity added."""
    stem = re.sub(r"[^A-Za-z0-9._-]", "_", str(entity)).lstrip(".")[:100] or "camera"
    if stem in taken:
        stem = f"{stem}-{hashlib.sha1(str(entity).encode()).hexdigest()[:8]}"
    return stem


def media_ext(url, attr):
    """The file extension for a media URL: its own if it is one served for that attribute,
    else the attribute's default (.jpg for stills, .mp4 for clips)."""
    kind, default = MEDIA_ATTRS[attr]
    ext = os.path.splitext(urllib.parse.urlparse(url).path)[1].lower()
    ok = IMAGE_EXTS if kind == "image" else VIDEO_EXTS
    return ext if ext in ok else default


def is_web_url(v):
    return isinstance(v, str) and v.lower().startswith(("http://", "https://"))


def media_plan(batches):
    """{(entity, attr): (file, url)} for every camera's latest image and clip URL (the last
    observation of the entity, in offset order, that has that attribute)."""
    latest = {}
    for b in sorted(batches, key=lambda b: b["offset"]):
        for o in b["observations"]:
            if o.get("kind") != CAMERA_KIND or not isinstance(o.get("attrs"), dict):
                continue
            for attr in MEDIA_ATTRS:
                if is_web_url(o["attrs"].get(attr)):
                    latest[(o.get("entity"), attr)] = o["attrs"][attr]
    plan, stems = {}, {}
    for entity, attr in sorted(latest, key=lambda k: (str(k[0]), k[1])):
        if entity not in stems:
            stems[entity] = safe_stem(entity, set(stems.values()))
        url = latest[(entity, attr)]
        plan[(entity, attr)] = (stems[entity] + media_ext(url, attr), url)
    return plan


def looks_like_media(data, kind):
    """True if `data` starts like an image (JPEG, PNG, GIF, WebP) or a clip (MP4, WebM): an
    error page or an empty body is not stored."""
    if kind == "image":
        return (data[:3] == b"\xff\xd8\xff" or data[:8] == b"\x89PNG\r\n\x1a\n"
                or data[:4] == b"GIF8" or (data[:4] == b"RIFF" and data[8:12] == b"WEBP"))
    return data[4:8] == b"ftyp" or data[:4] == b"\x1a\x45\xdf\xa3"


def fetch_url(url, timeout=MEDIA_TIMEOUT_S):
    """The body of `url` (at most MEDIA_FILE_MAX bytes); raises on HTTP errors and timeouts."""
    req = urllib.request.Request(url, headers=MEDIA_UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        data = r.read(MEDIA_FILE_MAX + 1)
    if len(data) > MEDIA_FILE_MAX:
        raise ValueError(f"larger than {MEDIA_FILE_MAX} bytes")
    return data


def capture_media(plan, media_dir, fetch=fetch_url, refresh=False, workers=MEDIA_WORKERS,
                  max_mb=MEDIA_MAX_MB, retry_wait=1.0):
    """Download every planned file into `media_dir` (a file already there is kept unless
    `refresh`); each download is tried twice, and one that fails or is not an image/clip is
    skipped. Files no longer planned are removed. Raises SystemExit if the media would pass
    `max_mb`. Returns {(entity, attr): file} for the files present afterwards, and
    {attr: [saved, reused, failed]}."""
    os.makedirs(media_dir, exist_ok=True)
    counts = {attr: [0, 0, 0] for attr in MEDIA_ATTRS}
    saved, todo = {}, []
    for key, (file, url) in plan.items():
        path = os.path.join(media_dir, file)
        if not refresh and os.path.isfile(path) and os.path.getsize(path) > 0:
            saved[key] = file
            counts[key[1]][1] += 1
        else:
            todo.append((key, file, url))

    def one(job):
        key, file, url = job
        for attempt in (1, 2):
            try:
                data = fetch(url)
                if looks_like_media(data, MEDIA_ATTRS[key[1]][0]):
                    return job, data
                if attempt == 2:
                    return job, None
            except Exception:  # noqa: BLE001 - any failure: retry once, then skip
                if attempt == 2:
                    return job, None
            time.sleep(retry_wait)
        return job, None

    total = sum(os.path.getsize(os.path.join(media_dir, f)) for f in saved.values())
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        for (key, file, _url), data in pool.map(one, todo):
            if data is None:
                counts[key[1]][2] += 1
                with contextlib.suppress(FileNotFoundError):
                    os.remove(os.path.join(media_dir, file))  # a stale copy is not kept
                continue
            total += len(data)
            if total > max_mb * 1e6:
                pool.shutdown(wait=False, cancel_futures=True)
                raise SystemExit(f"camera media passed {max_mb} MB: stopped ({len(saved)} files "
                                 f"in {media_dir}); nothing was re-packed")
            part = os.path.join(media_dir, f".{file}.part")
            with open(part, "wb") as f:
                f.write(data)
            os.replace(part, os.path.join(media_dir, file))
            saved[key] = file
            counts[key[1]][0] += 1
    keep = set(saved.values())
    for name in os.listdir(media_dir):
        if name not in keep:
            with contextlib.suppress(OSError):
                os.remove(os.path.join(media_dir, name))
    return saved, counts


def localise_media(batches, saved):
    """Point every camera observation's image_url/video_url at its stored file (/media/...),
    and drop every camera URL attribute that is still an internet address (media not
    captured, or a live stream)."""
    out = []
    for b in batches:
        observations = []
        for o in b["observations"]:
            if o.get("kind") == CAMERA_KIND and isinstance(o.get("attrs"), dict):
                attrs = dict(o["attrs"])
                for attr in URL_ATTRS:
                    if attr not in attrs:
                        continue
                    file = saved.get((o.get("entity"), attr))
                    if file:
                        attrs[attr] = MEDIA_PREFIX + file
                    elif not (isinstance(attrs[attr], str) and attrs[attr].startswith(MEDIA_PREFIX)):
                        del attrs[attr]
                o = dict(o, attrs=attrs)
            observations.append(o)
        out.append(dict(b, observations=observations))
    return out


def camera_source(entity):
    """A camera's source for the summary, from its entity name."""
    e = str(entity)
    for prefix, name in (("JamCams_", "tfl"), ("NZCAM-", "nzta"), ("BCCAM-", "drivebc"),
                         ("SGCAM-", "data.gov.sg"), ("CAM-", "custom")):
        if e.startswith(prefix):
            return name
    return "other"


def thin(batches):
    """Keep a moving track's observation only THIN_S or more after the last one kept for
    that entity (by observedAt); batches are taken in offset order."""
    last = {}
    out = []
    for b in sorted(batches, key=lambda b: b["offset"]):
        kept = []
        for o in b["observations"]:
            at = o.get("observedAt")
            if o.get("kind") in THINNED_KINDS and isinstance(at, (int, float)):
                key = (o.get("kind"), o.get("entity"))
                if key in last and at - last[key] < THIN_S:
                    continue
                last[key] = at
            kept.append(o)
        out.append(dict(b, observations=kept))
    return out


def drop_empty(batches):
    return [b for b in batches if b["observations"]]


def plain(name):
    return bool(name) and "/" not in name and "\\" not in name and not name.startswith(".")


class NonFinite(ValueError):
    pass


def reject_constant(name):
    raise NonFinite(name)


def read_batches(heimdall_dir, nonfinite=None):
    """Raw batches in name order (offset, then feed, then arrival). Observations with NaN or
    Infinity are dropped and counted per feed in `nonfinite`; unreadable lines are dropped."""
    nonfinite = {} if nonfinite is None else nonfinite
    batches = []
    for path in sorted(glob.glob(os.path.join(heimdall_dir, "*.jsonl"))):
        parsed = parse_heimdall_name(os.path.basename(path))
        if not parsed or not plain(parsed[1]):
            print(f"skipping {path}: not <offset>-<feed>.jsonl", file=sys.stderr)
            continue
        observations = []
        with open(path, encoding="utf-8") as f:
            for line in f:
                if not line.strip():
                    continue
                try:
                    o = json.loads(line, parse_constant=reject_constant)
                except NonFinite:
                    nonfinite[parsed[1]] = nonfinite.get(parsed[1], 0) + 1
                    continue
                except ValueError:
                    continue
                if isinstance(o, dict):
                    observations.append(o)
        batches.append({"offset": parsed[0], "feed": parsed[1], "observations": observations})
    return batches  # sorted by name: offset, then feed, then ~N


def read_forge(forge_dir):
    """[(offset, file, source path)] for every raw Forge version, in offset order."""
    out = []
    for path in sorted(glob.glob(os.path.join(forge_dir, "*"))):
        m = FORGE_NAME.match(os.path.basename(path))
        if m and os.path.isfile(path) and plain(m.group(2)):
            out.append((int(m.group(1)), m.group(2), path))
    return out


def load_json(path):
    with open(path) as f:
        return json.load(f)


def dir_size(path):
    return sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(path) for f in fs)


def link_or_copy(src, dst):
    """Hard-link `src` at `dst` (no second copy on disk); copy if linking is not possible."""
    with contextlib.suppress(FileNotFoundError):
        os.remove(dst)
    try:
        os.link(src, dst)
    except OSError:
        shutil.copyfile(src, dst)


# The feeds the presentation needs (aircraft and ships); pack warns when one is absent.
KEY_FEEDS = ["adsb-fi", "adsb-lol", "aisstream"]


def no_fetch(url):
    raise OSError("downloads off (--no-media)")


def media_summary(plan, saved, media_dir):
    """{source: {"cameras", "images", "clips", "failed", "mb"}} for the summary."""
    out = {}
    for (entity, attr), (file, _url) in plan.items():
        s = out.setdefault(camera_source(entity), {"cameras": set(), "images": 0, "clips": 0,
                                                   "failed": 0, "mb": 0.0})
        s["cameras"].add(entity)
        if (entity, attr) in saved:
            s["images" if attr == "image_url" else "clips"] += 1
            s["mb"] += os.path.getsize(os.path.join(media_dir, file)) / 1e6
        else:
            s["failed"] += 1
    return {k: dict(v, cameras=len(v["cameras"])) for k, v in sorted(out.items())}


def pack(raw, out, quiet=False, media=True, refresh_media=False, fetch=fetch_url,
         max_media_mb=MEDIA_MAX_MB):
    """Pack a raw capture into a recording at `out`; returns the summary.

    Thinning keys on (kind, entity) across all feeds: adsb-fi's and adsb-lol's reports of
    one aircraft are thinned together, deliberately (one track every 30 s is enough).
    Camera media is captured into `out`/media first (see the module notes); with
    media=False nothing is downloaded and only files already there are used."""
    with open(os.path.join(raw, "meta.json")) as f:
        meta = json.load(f)
    recorded_from = int(meta["recordedFrom"])
    nonfinite = {}
    batches = drop_empty(thin(drop_kinds(read_batches(os.path.join(raw, "heimdall"), nonfinite))))
    plan = media_plan(batches)
    media_dir = os.path.join(out, "media")
    saved, media_counts = {}, {}
    if plan or os.path.isdir(media_dir):
        if not quiet and media and plan:
            print(f"capturing media for {len({e for e, _ in plan})} cameras into {media_dir}")
        saved, media_counts = capture_media(plan, media_dir, fetch=fetch if media else no_fetch,
                                            refresh=refresh_media, max_mb=max_media_mb,
                                            retry_wait=1.0 if media else 0)
    batches = localise_media(batches, saved)
    forge = read_forge(os.path.join(raw, "forge"))
    offsets = [b["offset"] for b in batches] + [f[0] for f in forge]
    if not offsets:
        raise SystemExit("nothing captured: the recording would have no events")
    duration = max(int(meta.get("duration", 0)), max(offsets) + 1)

    os.makedirs(os.path.join(out, "forge"), exist_ok=True)
    with gzip.open(os.path.join(out, "heimdall.jsonl.gz"), "wt", encoding="utf-8") as f:
        for b in batches:
            f.write(json.dumps({"offset": b["offset"], "feed": b["feed"],
                                "observations": b["observations"]}, separators=(",", ":"),
                               allow_nan=False) + "\n")
    manifest = []
    for offset, file, src in forge:
        rel = f"forge/{offset:06d}-{file}"
        link_or_copy(src, os.path.join(out, rel))
        manifest.append({"offset": offset, "file": file, "path": rel})
    with open(os.path.join(out, "forge", "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=1)
    jobs = os.path.join(raw, "forge", "jobs.json")
    if os.path.isfile(jobs):
        shutil.copyfile(jobs, os.path.join(out, "forge", "jobs.json"))
    feeds = sorted({b["feed"] for b in batches})
    forge_files = sorted({m["file"] for m in manifest})
    with open(os.path.join(out, "meta.json"), "w") as f:
        json.dump({"version": 1, "recordedFrom": recorded_from, "duration": duration,
                   "feeds": feeds, "forgeFiles": forge_files}, f, indent=1)

    raw_mb = dir_size(raw) / 1e6
    summary = {
        "mb": dir_size(out) / 1e6 - (raw_mb if is_inside(raw, out) else 0),
        "raw_mb": raw_mb,
        "nonfinite": nonfinite,
        "duration": duration,
        "feeds": {feed: (sum(1 for b in batches if b["feed"] == feed),
                         sum(len(b["observations"]) for b in batches if b["feed"] == feed))
                  for feed in feeds},
        "forge": {file: sum(1 for m in manifest if m["file"] == file) for file in forge_files},
        "missing_key_feeds": [f for f in KEY_FEEDS if f not in feeds],
        "media": media_summary(plan, saved, media_dir),
        "media_counts": media_counts,
        "media_mb": dir_size(media_dir) / 1e6 if os.path.isdir(media_dir) else 0.0,
    }
    if not quiet:
        print(f"packed {out}: {summary['mb']:.1f} MB (raw capture {raw_mb:.1f} MB), {duration} s")
        for feed, (n, o) in summary["feeds"].items():
            print(f"  {feed:<24} {n:>5} batches {o:>8} observations")
        for feed, n in sorted(nonfinite.items()):
            print(f"  {feed:<24} {n:>5} observations dropped for NaN/Infinity")
        for file, n in summary["forge"].items():
            print(f"  forge {file:<18} {n:>5} versions")
        if summary["media"]:
            print(f"  camera media {summary['media_mb']:.1f} MB in {media_dir}")
            for src, m in summary["media"].items():
                print(f"    {src:<12} {m['cameras']:>5} cameras {m['images']:>5} images "
                      f"{m['clips']:>5} clips {m['failed']:>4} failed {m['mb']:>7.1f} MB")
            for attr, (new_n, reused, failed) in summary["media_counts"].items():
                print(f"    {attr:<12} {new_n:>5} downloaded {reused:>5} reused {failed:>4} failed")
        if summary["missing_key_feeds"]:
            print(f"WARNING: no batches from {', '.join(summary['missing_key_feeds'])}: the "
                  "replay will have no aircraft or ships from them (rate-limited, or not "
                  "run yet: record for longer)", file=sys.stderr)
    return summary


def is_inside(path, parent):
    path, parent = os.path.realpath(path), os.path.realpath(parent)
    return os.path.commonpath([path, parent]) == parent


# ---- capture (live) ----

def run_script(name):
    """Run a feeds script in its own session (Ctrl-C does not reach it); True if it worked."""
    r = subprocess.run(["bash", os.path.join(FEEDS, name)], start_new_session=True)
    if r.returncode != 0:
        print(f"{name} exited with {r.returncode}", file=sys.stderr)
    return r.returncode == 0


def restart_feeds():
    """Restart the feeds runner with its memory of what it sent cleared, so every source
    sends everything once. True if the runner is running afterwards."""
    if not run_script("stop.sh"):
        return False
    print("stopped the feeds runner")
    removed = 0
    for p in glob.glob(os.path.join(FEEDS, "state", "last_sent", "*.json")):
        os.remove(p)
        removed += 1
    print(f"cleared {removed} last-sent records")
    if not run_script("start.sh"):
        return False
    time.sleep(START_WAIT_S)
    if not pid_alive(FEEDS_PID):
        return False
    print("started the feeds runner: every source sends everything once")
    return True


def capture(raw, recorded_from, until, stop, forge_last):
    """Until `until` (or stop()), move the hook's batches in and copy each stable new
    version of adsb.lol's batch and of the Forge CSVs. `forge_last` holds each CSV's mtime
    at its offset-0 copy."""
    incoming = os.path.join(raw, "incoming")
    hdir, fdir = os.path.join(raw, "heimdall"), os.path.join(raw, "forge")
    last = dict(forge_last)
    taken = set(os.listdir(hdir))
    batches, versions, next_progress = 0, 0, time.time() + 60
    while not stop() and time.time() < until:
        now = time.time()
        batches += move_incoming(incoming, hdir, recorded_from, taken)
        batches += copy_adsb_lol(hdir, recorded_from, now, last, taken)
        for path in glob.glob(os.path.join(FORGE_DATA, "*.csv")):
            try:
                mtime = os.path.getmtime(path)
                if not should_copy(mtime, now, last.get(path), recorded_from):
                    continue
                dest = os.path.join(fdir, f"{offset_of(mtime, recorded_from):06d}-{os.path.basename(path)}")
                shutil.copyfile(path, dest)  # same file twice in a second: the latest wins
            except OSError:
                continue
            last[path] = mtime
            versions += 1
        if now >= next_progress:
            line = f"{int(now - recorded_from) // 60} min: {batches} batches, {versions} Forge versions"
            if batches == 0 and now - recorded_from >= NO_BATCH_WARN_S:
                line += " -- WARNING: no Heimdall batch yet; is the feeds runner sending?"
            print(line)
            next_progress += 60
        time.sleep(POLL_S)
    return batches, versions


def copy_adsb_lol(hdir, recorded_from, now, last, taken):
    """Copy a new, stable, complete heimdall-poc/batch.jsonl; returns 1 if copied."""
    try:
        mtime = os.path.getmtime(ADSB_LOL)
        if not should_copy(mtime, now, last.get(ADSB_LOL), recorded_from):
            return 0
        with open(ADSB_LOL, encoding="utf-8") as f:
            text = f.read()
        if os.path.getmtime(ADSB_LOL) != mtime or not valid_jsonl(text):
            return 0  # changed while read, or mid-write: next poll
    except (OSError, UnicodeDecodeError):
        return 0
    last[ADSB_LOL] = mtime
    if not text.strip():
        return 0
    name = heimdall_name(offset_of(mtime, recorded_from), "adsb-lol", taken)
    taken.add(name)
    with open(os.path.join(hdir, name), "w", encoding="utf-8") as f:
        f.write(text)
    return 1


def record(minutes, name, drop_raw=False):
    out = os.path.join(RECORDINGS, name)
    recorded_from = int(time.time())
    raw = prepare(out, recorded_from)
    forge_last = {}
    for p in sorted(glob.glob(os.path.join(FORGE_DATA, "*.csv"))):
        forge_last[p] = os.path.getmtime(p)
        shutil.copyfile(p, os.path.join(raw, "forge", f"000000-{os.path.basename(p)}"))
    jobs = os.path.join(FORGE_DATA, "jobs.json")
    if os.path.isfile(jobs):
        shutil.copyfile(jobs, os.path.join(raw, "forge", "jobs.json"))
    print(f"recording into {out}: {len(forge_last)} Forge files copied at the start")

    stopping = []
    incoming = os.path.join(raw, "incoming")
    hdir = os.path.join(raw, "heimdall")
    with stop_on_signals(stopping):
        with record_to(RECORD_TO, incoming):
            started = restart_feeds()
            if started:
                print(f"capturing for {minutes:g} min (Ctrl-C stops early)")
                batches, versions = capture(raw, recorded_from, recorded_from + minutes * 60,
                                            lambda: bool(stopping), forge_last)
        if not started:
            shutil.rmtree(out)  # nothing captured: free the name for the next try
            raise SystemExit("feeds runner did not start: run heimdall-poc/feeds/start.sh")
        time.sleep(STABLE_S)  # a batch the hook was writing as it was removed
        batches += move_incoming(incoming, hdir, recorded_from, set(os.listdir(hdir)))
        duration = max(1, int(time.time() - recorded_from))
        write_meta(raw, recorded_from, duration)
    print(f"captured {batches} batches and {versions} Forge versions in {duration} s")
    pack(raw, out)
    if drop_raw:
        shutil.rmtree(raw)
        print(f"removed the raw capture {raw}")


def main(argv):
    ap = argparse.ArgumentParser(description="Record the live feeds for midgard-replay, or re-pack a capture.")
    sub = ap.add_subparsers(dest="command", required=True)
    rec = sub.add_parser("record", help="capture the live feeds, then pack")
    rec.add_argument("--minutes", type=float, default=60.0)
    rec.add_argument("--name", default=time.strftime("rec-%Y%m%d-%H%M"))
    rec.add_argument("--drop-raw", action="store_true", help="delete raw/ after a successful pack")
    pk = sub.add_parser("pack", help="pack a raw capture into a recording")
    pk.add_argument("raw_dir")
    pk.add_argument("out_dir")
    pk.add_argument("--no-media", action="store_true",
                    help="download no camera media (files already in OUT_DIR/media are used)")
    pk.add_argument("--refresh-media", action="store_true",
                    help="download every camera's media again, even if already stored")
    args = ap.parse_args(argv)
    if args.command == "record":
        if args.minutes <= 0 or not plain(args.name):
            ap.error("--minutes must be positive and --name a plain name")
        record(args.minutes, args.name, args.drop_raw)
    else:
        pack(args.raw_dir, args.out_dir, media=not args.no_media, refresh_media=args.refresh_media)


if __name__ == "__main__":
    main(sys.argv[1:])
