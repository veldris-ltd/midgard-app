#!/usr/bin/env python3
"""Record the live suite's feeds for the offline Mac app: every Heimdall batch the feeds
send and every Forge source file version, for a given time, packed as a recording that
midgard-replay plays back (format version 1, see tools/replay/src/recording.rs).

    python3 tools/mac-app/record.py record --minutes 60 [--name NAME] [--drop-raw]
    python3 tools/mac-app/record.py pack RAW_DIR OUT_DIR                 # re-pack a capture

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
"""
import argparse
import contextlib
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
# Kinds whose camera images need the internet: not recorded.
CAMERA_KINDS = {"land-camera", "land-camera-ai"}
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


def pack(raw, out, quiet=False):
    """Pack a raw capture into a recording at `out`; returns the summary.

    Thinning keys on (kind, entity) across all feeds: adsb-fi's and adsb-lol's reports of
    one aircraft are thinned together, deliberately (one track every 30 s is enough)."""
    with open(os.path.join(raw, "meta.json")) as f:
        meta = json.load(f)
    recorded_from = int(meta["recordedFrom"])
    nonfinite = {}
    batches = drop_empty(thin(drop_cameras(read_batches(os.path.join(raw, "heimdall"), nonfinite))))
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
    }
    if not quiet:
        print(f"packed {out}: {summary['mb']:.1f} MB (raw capture {raw_mb:.1f} MB), {duration} s")
        for feed, (n, o) in summary["feeds"].items():
            print(f"  {feed:<24} {n:>5} batches {o:>8} observations")
        for feed, n in sorted(nonfinite.items()):
            print(f"  {feed:<24} {n:>5} observations dropped for NaN/Infinity")
        for file, n in summary["forge"].items():
            print(f"  forge {file:<18} {n:>5} versions")
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
    args = ap.parse_args(argv)
    if args.command == "record":
        if args.minutes <= 0 or not plain(args.name):
            ap.error("--minutes must be positive and --name a plain name")
        record(args.minutes, args.name, args.drop_raw)
    else:
        pack(args.raw_dir, args.out_dir)


if __name__ == "__main__":
    main(sys.argv[1:])
