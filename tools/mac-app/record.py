#!/usr/bin/env python3
"""Record the live suite's feeds for the offline Mac app: every Heimdall batch the feeds
send and every Forge source file version, for a given time, packed as a recording that
midgard-replay plays back (format version 1, see tools/replay/src/recording.rs).

    python3 tools/mac-app/record.py record --minutes 60 [--name NAME]   # capture, then pack
    python3 tools/mac-app/record.py pack RAW_DIR OUT_DIR                 # re-pack a capture

Run `record` on the development Mac while the Docker suite and the live feeds run. It
writes to ~/Library/Application Support/Midgard-dev/recordings/<name> (raw capture in
<name>/raw), outside the suite folder and its storage budget. MIDGARD_HOME (default
~/Desktop/Midgard) is where the live feeds are. Ctrl-C stops early and still packs.
"""
import glob, gzip, json, os, re, shutil, signal, subprocess, sys, time

HOME = os.path.expanduser(os.environ.get("MIDGARD_HOME", "~/Desktop/Midgard"))
RECORDINGS = os.path.expanduser("~/Library/Application Support/Midgard-dev/recordings")
FEEDS = os.path.join(HOME, "heimdall-poc", "feeds")
ADSB_LOL = os.path.join(HOME, "heimdall-poc", "batch.jsonl")
FORGE_DATA = os.path.join(HOME, "live-feeds", "data")

POLL_S = 0.5
STABLE_S = 1.0
# Kinds whose camera images need the internet: not recorded.
CAMERA_KINDS = {"land-camera", "land-camera-ai"}
# Moving tracks kept at most once per THIN_S per entity; other kinds are all kept.
THINNED_KINDS = {"air-track", "sea-vessel", "air-balloon", "land-train"}
THIN_S = 30

HEIMDALL_NAME = re.compile(r"^(\d+)-(.+?)(?:~\d+)?\.jsonl$")
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
    """`<offset:06d>-<feed>.jsonl`, with `~N` added if a batch of that feed already has
    that second (each batch holds only new items, so none is overwritten)."""
    name, n = f"{offset:06d}-{feed}.jsonl", 1
    while name in taken:
        n += 1
        name = f"{offset:06d}-{feed}~{n}.jsonl"
    return name


def parse_heimdall_name(name):
    """(offset, feed) from a raw batch file name, or None."""
    m = HEIMDALL_NAME.match(name)
    return (int(m.group(1)), m.group(2)) if m else None


# ---- packing (pure) ----

def drop_cameras(batches):
    return [dict(b, observations=[o for o in b["observations"] if o.get("kind") not in CAMERA_KINDS])
            for b in batches]


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


def read_batches(heimdall_dir):
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
                    o = json.loads(line)
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


def pack(raw, out, quiet=False):
    """Pack a raw capture into a recording at `out`; returns the summary."""
    with open(os.path.join(raw, "meta.json")) as f:
        meta = json.load(f)
    recorded_from = int(meta["recordedFrom"])
    batches = drop_empty(thin(drop_cameras(read_batches(os.path.join(raw, "heimdall")))))
    forge = read_forge(os.path.join(raw, "forge"))
    offsets = [b["offset"] for b in batches] + [f[0] for f in forge]
    if not offsets:
        raise SystemExit("nothing captured: the recording would have no events")
    duration = max(int(meta.get("duration", 0)), max(offsets) + 1)

    os.makedirs(os.path.join(out, "forge"), exist_ok=True)
    with gzip.open(os.path.join(out, "heimdall.jsonl.gz"), "wt", encoding="utf-8") as f:
        for b in batches:
            f.write(json.dumps({"offset": b["offset"], "feed": b["feed"],
                                "observations": b["observations"]}, separators=(",", ":")) + "\n")
    manifest = []
    for offset, file, src in forge:
        rel = f"forge/{offset:06d}-{file}"
        shutil.copyfile(src, os.path.join(out, rel))
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

    summary = {
        "mb": dir_size(out) / 1e6 - (dir_size(raw) / 1e6 if is_inside(raw, out) else 0),
        "duration": duration,
        "feeds": {feed: (sum(1 for b in batches if b["feed"] == feed),
                         sum(len(b["observations"]) for b in batches if b["feed"] == feed))
                  for feed in feeds},
        "forge": {file: sum(1 for m in manifest if m["file"] == file) for file in forge_files},
    }
    if not quiet:
        print(f"packed {out}: {summary['mb']:.1f} MB, {duration} s")
        for feed, (n, o) in summary["feeds"].items():
            print(f"  {feed:<24} {n:>5} batches {o:>8} observations")
        for file, n in summary["forge"].items():
            print(f"  forge {file:<18} {n:>5} versions")
    return summary


def is_inside(path, parent):
    path, parent = os.path.realpath(path), os.path.realpath(parent)
    return os.path.commonpath([path, parent]) == parent


# ---- capture (live) ----

def heimdall_sources():
    """(feed, path) of every file the feeds write before sending."""
    found = [(os.path.splitext(os.path.basename(p))[0], p)
             for p in sorted(glob.glob(os.path.join(FEEDS, "out", "*.jsonl")))]
    if os.path.exists(ADSB_LOL):
        found.append(("adsb-lol", ADSB_LOL))
    return found


def restart_feeds():
    """Restart the feeds runner with its memory of what it sent cleared, so every source
    sends everything once."""
    subprocess.run(["bash", os.path.join(FEEDS, "stop.sh")], check=False)
    print("stopped the feeds runner")
    removed = 0
    for p in glob.glob(os.path.join(FEEDS, "state", "last_sent", "*.json")):
        os.remove(p)
        removed += 1
    print(f"cleared {removed} last-sent records")
    subprocess.run(["bash", os.path.join(FEEDS, "start.sh")], check=False)
    print("started the feeds runner: every source sends everything once")


def capture(raw, recorded_from, until, stop):
    """Poll the live files until `until` (or stop()), copying each stable new version."""
    hdir, fdir = os.path.join(raw, "heimdall"), os.path.join(raw, "forge")
    last = {p: os.path.getmtime(p) for p in glob.glob(os.path.join(FORGE_DATA, "*.csv"))}
    taken = set(os.listdir(hdir))
    batches, versions, next_progress = 0, 0, time.time() + 60
    while not stop() and time.time() < until:
        now = time.time()
        for feed, path in heimdall_sources():
            try:
                mtime = os.path.getmtime(path)
                if not should_copy(mtime, now, last.get(path), recorded_from):
                    continue
                with open(path, encoding="utf-8") as f:
                    text = f.read()
                if os.path.getmtime(path) != mtime or not valid_jsonl(text):
                    continue  # changed while read, or mid-write: next poll
            except (OSError, UnicodeDecodeError):
                continue
            last[path] = mtime
            if not text.strip():
                continue
            name = heimdall_name(offset_of(mtime, recorded_from), feed, taken)
            taken.add(name)
            with open(os.path.join(hdir, name), "w", encoding="utf-8") as f:
                f.write(text)
            batches += 1
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
            print(f"{int(now - recorded_from) // 60} min: {batches} batches, {versions} Forge versions")
            next_progress += 60
        time.sleep(POLL_S)
    return batches, versions


def record(minutes, name):
    out = os.path.join(RECORDINGS, name)
    if os.path.exists(out):
        raise SystemExit(f"{out} already exists: choose another --name")
    raw = os.path.join(out, "raw")
    os.makedirs(os.path.join(raw, "heimdall"))
    os.makedirs(os.path.join(raw, "forge"))

    recorded_from = int(time.time())
    csvs = sorted(glob.glob(os.path.join(FORGE_DATA, "*.csv")))
    for p in csvs:
        shutil.copyfile(p, os.path.join(raw, "forge", f"000000-{os.path.basename(p)}"))
    jobs = os.path.join(FORGE_DATA, "jobs.json")
    if os.path.isfile(jobs):
        shutil.copyfile(jobs, os.path.join(raw, "forge", "jobs.json"))
    print(f"recording into {out}: {len(csvs)} Forge files copied at the start")

    stopping = []
    signal.signal(signal.SIGINT, lambda *_: (stopping.append(1), print("\nstopping: packing what was captured")))
    restart_feeds()
    print(f"capturing for {minutes} min (Ctrl-C stops early)")
    batches, versions = capture(raw, recorded_from, recorded_from + minutes * 60, lambda: bool(stopping))
    signal.signal(signal.SIGINT, signal.SIG_DFL)

    duration = max(1, int(time.time() - recorded_from))
    with open(os.path.join(raw, "meta.json"), "w") as f:
        json.dump({"recordedFrom": recorded_from, "duration": duration}, f)
    print(f"captured {batches} batches and {versions} Forge versions in {duration} s")
    pack(raw, out)


def main(argv):
    if len(argv) >= 1 and argv[0] == "record":
        args = argv[1:]
        minutes, name = 60.0, time.strftime("rec-%Y%m%d-%H%M")
        try:
            if "--minutes" in args:
                minutes = float(args[args.index("--minutes") + 1])
            if "--name" in args:
                name = args[args.index("--name") + 1]
        except (IndexError, ValueError):
            raise SystemExit(__doc__)
        if minutes <= 0 or not plain(name):
            raise SystemExit("--minutes must be positive and --name a plain name")
        record(minutes, name)
    elif len(argv) == 3 and argv[0] == "pack":
        pack(argv[1], argv[2])
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
