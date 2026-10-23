"""Tests for record.py: thinning, dropping camera analysis, camera media capture, packing a
raw capture, and the capture rule that decides when a changed file is copied.

    python3 -m unittest tools/mac-app/test_record.py -v
"""
import contextlib
import gzip
import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import record  # noqa: E402


def obs(entity, kind, at, **attrs):
    return {"entity": entity, "kind": kind, "observedAt": at, "attrs": attrs}


def batch(offset, feed, observations):
    return {"offset": offset, "feed": feed, "observations": observations}


class ThinTest(unittest.TestCase):
    def test_aircraft_kept_every_30s_quake_all_kept(self):
        t = 1_792_000_000
        batches = [
            batch(0, "adsb-fi", [obs("A1", "air-track", t), obs("Q1", "earthquake", t)]),
            batch(10, "adsb-fi", [obs("A1", "air-track", t + 10), obs("Q1", "earthquake", t + 10)]),
            batch(30, "adsb-fi", [obs("A1", "air-track", t + 30), obs("Q1", "earthquake", t + 30)]),
        ]
        out = record.thin(batches)
        aircraft = [o["observedAt"] - t for b in out for o in b["observations"] if o["entity"] == "A1"]
        quakes = [o["observedAt"] - t for b in out for o in b["observations"] if o["entity"] == "Q1"]
        self.assertEqual(aircraft, [0, 30])
        self.assertEqual(quakes, [0, 10, 30])

    def test_thin_processes_batches_in_offset_order(self):
        t = 1_792_000_000
        batches = [batch(30, "f", [obs("A1", "air-track", t + 30)]),
                   batch(0, "f", [obs("A1", "air-track", t)])]
        out = record.thin(batches)
        self.assertEqual([b["offset"] for b in out], [0, 30])
        self.assertEqual(sum(len(b["observations"]) for b in out), 2)


JPEG = b"\xff\xd8\xff\xe0" + b"x" * 100
MP4 = b"\x00\x00\x00\x18ftypmp42" + b"y" * 100


class CameraTest(unittest.TestCase):
    T = 1_792_000_000

    def test_camera_analysis_dropped_cameras_kept(self):
        batches = [
            batch(0, "cams", [obs("C1", "land-camera", self.T), obs("C2", "land-camera-ai", self.T)]),
            batch(1, "ai", [obs("C3", "land-camera-ai", self.T)]),
        ]
        out = record.drop_empty(record.drop_kinds(batches))
        self.assertEqual([b["feed"] for b in out], ["cams"])
        self.assertEqual([o["entity"] for o in out[0]["observations"]], ["C1"])

    def test_safe_stem(self):
        self.assertEqual(record.safe_stem("JamCams_00002.00865", set()), "JamCams_00002.00865")
        self.assertEqual(record.safe_stem("../a/b c", set()), "_a_b_c")
        self.assertEqual(record.safe_stem("...", set()), "camera")
        clash = record.safe_stem("a/b", {"a_b"})
        self.assertTrue(clash.startswith("a_b-") and len(clash) == 12)
        self.assertNotIn("/", record.safe_stem("x/" * 200, set()))

    def test_media_plan_uses_each_cameras_latest_urls(self):
        batches = [
            batch(10, "sg", [obs("SGCAM-1", "land-camera", self.T + 10, image_url="https://i/new.jpg")]),
            batch(0, "sg", [obs("SGCAM-1", "land-camera", self.T, image_url="https://i/old.jpg")]),
            batch(0, "tfl", [obs("JamCams_1", "land-camera", self.T, image_url="https://t/1.jpg",
                                 video_url="https://t/1.mp4"),
                             obs("Q1", "earthquake", self.T, image_url="https://q/1.jpg"),
                             obs("X", "land-camera", self.T, image_url="/media/X.jpg")]),
            batch(0, "bc", [obs("BCCAM-2", "land-camera", self.T, image_url="https://b/2?x=1")]),
        ]
        plan = record.media_plan(batches)
        self.assertEqual(plan, {
            ("BCCAM-2", "image_url"): ("BCCAM-2.jpg", "https://b/2?x=1"),
            ("JamCams_1", "image_url"): ("JamCams_1.jpg", "https://t/1.jpg"),
            ("JamCams_1", "video_url"): ("JamCams_1.mp4", "https://t/1.mp4"),
            ("SGCAM-1", "image_url"): ("SGCAM-1.jpg", "https://i/new.jpg"),
        })
        self.assertEqual(record.media_ext("https://a/b.PNG", "image_url"), ".png")
        self.assertEqual(record.media_ext("https://a/b.webm", "video_url"), ".webm")
        self.assertEqual(record.media_ext("https://a/b.mp4", "image_url"), ".jpg")

    def test_looks_like_media(self):
        self.assertTrue(record.looks_like_media(JPEG, "image"))
        self.assertTrue(record.looks_like_media(b"\x89PNG\r\n\x1a\nrest", "image"))
        self.assertTrue(record.looks_like_media(MP4, "video"))
        self.assertFalse(record.looks_like_media(b"<html>error</html>", "image"))
        self.assertFalse(record.looks_like_media(b"", "image"))
        self.assertFalse(record.looks_like_media(JPEG, "video"))

    def test_localise_media_rewrites_and_drops_internet_urls(self):
        batches = [batch(0, "tfl", [
            obs("JamCams_1", "land-camera", self.T, image_url="https://t/1.jpg", video_url="https://t/1.mp4"),
            obs("NZCAM-2", "land-camera", self.T, image_url="https://n/2.jpg", stream_url="https://s/live"),
            obs("S1", "sea-vessel", self.T, image_url="https://ship/photo.jpg"),
        ])]
        saved = {("JamCams_1", "image_url"): "JamCams_1.jpg"}
        out = record.localise_media(batches, saved)
        tfl, nz, ship = out[0]["observations"]
        self.assertEqual(tfl["attrs"]["image_url"], "/media/JamCams_1.jpg")
        self.assertNotIn("video_url", tfl["attrs"])  # its clip failed: no internet URL left
        self.assertNotIn("image_url", nz["attrs"])
        self.assertNotIn("stream_url", nz["attrs"])
        self.assertEqual(ship["attrs"]["image_url"], "https://ship/photo.jpg")  # not a camera
        self.assertEqual(batches[0]["observations"][0]["attrs"]["image_url"], "https://t/1.jpg")  # input untouched


class CaptureMediaTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.media = os.path.join(self.tmp.name, "media")
        self.calls = []

    def fetch(self, responses):
        def f(url):
            self.calls.append(url)
            r = responses[url]
            r = r.pop(0) if isinstance(r, list) else r
            if isinstance(r, Exception):
                raise r
            return r
        return f

    def test_saves_retries_once_and_skips_failures(self):
        plan = {("A", "image_url"): ("A.jpg", "u/a"), ("A", "video_url"): ("A.mp4", "u/a.mp4"),
                ("B", "image_url"): ("B.jpg", "u/b"), ("C", "image_url"): ("C.jpg", "u/c")}
        fetch = self.fetch({"u/a": [OSError("timeout"), JPEG], "u/a.mp4": MP4,
                            "u/b": b"<html>no</html>", "u/c": OSError("down")})
        saved, counts = record.capture_media(plan, self.media, fetch=fetch, retry_wait=0)
        self.assertEqual(saved, {("A", "image_url"): "A.jpg", ("A", "video_url"): "A.mp4"})
        self.assertEqual(counts, {"image_url": [1, 0, 2], "video_url": [1, 0, 0]})
        self.assertEqual(sorted(os.listdir(self.media)), ["A.jpg", "A.mp4"])
        with open(os.path.join(self.media, "A.jpg"), "rb") as f:
            self.assertEqual(f.read(), JPEG)
        self.assertEqual(self.calls.count("u/c"), 2)  # tried twice, not more
        self.assertEqual(self.calls.count("u/a.mp4"), 1)

    def test_reuses_stored_files_and_prunes_unplanned_ones(self):
        os.makedirs(self.media)
        for name in ("A.jpg", "OLD.jpg"):
            with open(os.path.join(self.media, name), "wb") as f:
                f.write(JPEG)
        plan = {("A", "image_url"): ("A.jpg", "u/a")}
        saved, counts = record.capture_media(plan, self.media, fetch=self.fetch({}), retry_wait=0)
        self.assertEqual(saved, {("A", "image_url"): "A.jpg"})
        self.assertEqual(counts["image_url"], [0, 1, 0])
        self.assertEqual(self.calls, [])
        self.assertEqual(os.listdir(self.media), ["A.jpg"])
        saved, counts = record.capture_media(plan, self.media, fetch=self.fetch({"u/a": OSError()}),
                                             refresh=True, retry_wait=0)
        self.assertEqual(saved, {})  # refresh failed: the stale copy is not kept
        self.assertEqual(os.listdir(self.media), [])

    def test_stops_past_the_size_cap(self):
        plan = {(f"E{i}", "image_url"): (f"E{i}.jpg", f"u/{i}") for i in range(3)}
        big = JPEG + b"z" * 600_000
        with self.assertRaises(SystemExit):
            record.capture_media(plan, self.media, fetch=lambda u: big, max_mb=1, workers=1, retry_wait=0)


class PackTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = self.tmp.name
        self.raw = os.path.join(root, "raw")
        self.out = os.path.join(root, "rec")
        os.makedirs(os.path.join(self.raw, "heimdall"))
        os.makedirs(os.path.join(self.raw, "forge"))
        t = 1_792_000_000
        with open(os.path.join(self.raw, "meta.json"), "w") as f:
            json.dump({"recordedFrom": t, "duration": 5}, f)

        def heimdall(name, observations):
            with open(os.path.join(self.raw, "heimdall", name), "w") as f:
                for o in observations:
                    f.write(json.dumps(o) + "\n")
                f.write("\n")

        heimdall("000007-usgs.jsonl", [obs("Q1", "earthquake", t + 7)])
        heimdall("000000-adsb-fi.jsonl", [obs("A1", "air-track", t)])
        heimdall("000000-adsb-fi~002.jsonl", [obs("A2", "air-track", t)])
        heimdall("000003-cams.jsonl", [obs("C1", "land-camera", t)])
        heimdall("000004-cams-ai.jsonl", [obs("AI-C1", "land-camera-ai", t)])
        heimdall("000002-adsb-lol.jsonl", [obs("A3", "air-track", t + 2), obs("A1", "air-track", t + 2)])
        for name, body in [("000000-aircraft.csv", "a,b\r\n1,2\r\n"),
                           ("000009-aircraft.csv", "a,b\r\n3,4\r\n"),
                           ("000000-ships.csv", "x\r\n"),
                           ("jobs.json", "[]")]:
            with open(os.path.join(self.raw, "forge", name), "w", newline="") as f:
                f.write(body)

    def test_pack_writes_a_loadable_layout(self):
        record.pack(self.raw, self.out, quiet=True)
        meta = record.load_json(os.path.join(self.out, "meta.json"))
        self.assertEqual(meta["version"], 1)
        self.assertEqual(meta["recordedFrom"], 1_792_000_000)
        self.assertEqual(meta["feeds"], ["adsb-fi", "adsb-lol", "cams", "usgs"])
        self.assertEqual(meta["forgeFiles"], ["aircraft.csv", "ships.csv"])
        self.assertGreater(meta["duration"], 9)

        with gzip.open(os.path.join(self.out, "heimdall.jsonl.gz"), "rt") as f:
            lines = [json.loads(l) for l in f if l.strip()]
        offsets = [b["offset"] for b in lines]
        self.assertEqual(offsets, sorted(offsets))
        self.assertEqual(offsets, [0, 0, 2, 3, 7])  # the camera kept, its analysis dropped
        lol = [b for b in lines if b["feed"] == "adsb-lol"][0]
        self.assertEqual([o["entity"] for o in lol["observations"]], ["A3"])  # A1 thinned
        for b in lines:
            self.assertEqual(set(b), {"offset", "feed", "observations"})
            self.assertTrue(b["observations"])
            self.assertLess(b["offset"], meta["duration"])

        manifest = record.load_json(os.path.join(self.out, "forge", "manifest.json"))
        self.assertEqual([m["offset"] for m in manifest], sorted(m["offset"] for m in manifest))
        self.assertEqual(len(manifest), 3)
        for m in manifest:
            self.assertFalse(os.path.isabs(m["path"]))
            self.assertTrue(os.path.isfile(os.path.join(self.out, m["path"])))
            self.assertLess(m["offset"], meta["duration"])
        nine = [m for m in manifest if m["offset"] == 9][0]
        self.assertEqual(nine["file"], "aircraft.csv")
        with open(os.path.join(self.out, nine["path"]), newline="") as f:
            self.assertEqual(f.read(), "a,b\r\n3,4\r\n")
        self.assertTrue(os.path.isfile(os.path.join(self.out, "forge", "jobs.json")))

    def test_pack_warns_about_missing_key_feeds(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            summary = record.pack(self.raw, self.out)
        self.assertEqual(summary["missing_key_feeds"], ["aisstream"])
        self.assertIn("WARNING", out.getvalue())
        self.assertIn("aisstream", out.getvalue())

    def test_parse_heimdall_name(self):
        self.assertEqual(record.parse_heimdall_name("000012-adsb-fi.jsonl"), (12, "adsb-fi"))
        self.assertEqual(record.parse_heimdall_name("000012-adsb-fi~003.jsonl"), (12, "adsb-fi"))
        self.assertIsNone(record.parse_heimdall_name("notes.txt"))


class PackDetailsTest(unittest.TestCase):
    T = 1_792_000_000

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.raw = os.path.join(self.tmp.name, "raw")
        self.out = os.path.join(self.tmp.name, "rec")
        os.makedirs(os.path.join(self.raw, "heimdall"))
        os.makedirs(os.path.join(self.raw, "forge"))
        with open(os.path.join(self.raw, "meta.json"), "w") as f:
            json.dump({"recordedFrom": self.T, "duration": 3}, f)

    def test_nan_and_infinity_observations_dropped_and_counted(self):
        lines = ['{"entity":"Q1","kind":"earthquake","observedAt":%d,"attrs":{"mag":NaN}}' % self.T,
                 '{"entity":"Q2","kind":"earthquake","observedAt":%d,"attrs":{"mag":-Infinity}}' % self.T,
                 '{"entity":"Q3","kind":"earthquake","observedAt":%d,"attrs":{"mag":4.1}}' % self.T]
        with open(os.path.join(self.raw, "heimdall", "000001-usgs.jsonl"), "w") as f:
            f.write("\n".join(lines) + "\n")
        summary = record.pack(self.raw, self.out, quiet=True)
        self.assertEqual(summary["nonfinite"], {"usgs": 2})
        self.assertEqual(summary["feeds"]["usgs"], (1, 1))

        def reject(c):
            raise ValueError(c)
        with gzip.open(os.path.join(self.out, "heimdall.jsonl.gz"), "rt") as f:
            got = [json.loads(l, parse_constant=reject) for l in f if l.strip()]
        self.assertEqual([o["entity"] for o in got[0]["observations"]], ["Q3"])

    def test_no_key_feeds_all_listed_and_none_missing_no_warning(self):
        with open(os.path.join(self.raw, "heimdall", "000001-usgs.jsonl"), "w") as f:
            f.write(json.dumps(obs("Q1", "earthquake", self.T)) + "\n")
        summary = record.pack(self.raw, self.out, quiet=True)
        self.assertEqual(summary["missing_key_feeds"], ["adsb-fi", "adsb-lol", "aisstream"])
        for feed in ["adsb-fi", "adsb-lol", "aisstream"]:
            with open(os.path.join(self.raw, "heimdall", f"000001-{feed}.jsonl"), "w") as f:
                f.write(json.dumps(obs("X" + feed, "air-track", self.T)) + "\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            summary = record.pack(self.raw, self.out)
        self.assertEqual(summary["missing_key_feeds"], [])
        self.assertNotIn("WARNING", out.getvalue())

    def test_pack_captures_camera_media_and_rewrites_urls(self):
        cams = [obs("JamCams_1", "land-camera", self.T, image_url="https://t/1.jpg", video_url="https://t/1.mp4"),
                obs("NZCAM-2", "land-camera", self.T, image_url="https://n/2.jpg")]
        with open(os.path.join(self.raw, "heimdall", "000001-tfl.jsonl"), "w") as f:
            f.write("\n".join(json.dumps(o) for o in cams) + "\n")
        bodies = {"https://t/1.jpg": JPEG, "https://t/1.mp4": MP4}

        def fetch(url):
            if url not in bodies:
                raise OSError("404")
            return bodies[url]
        summary = record.pack(self.raw, self.out, quiet=True, fetch=fetch)
        with gzip.open(os.path.join(self.out, "heimdall.jsonl.gz"), "rt") as f:
            text = f.read()
        self.assertNotIn("https://", text)
        got = {o["entity"]: o["attrs"] for l in text.splitlines() if l for o in json.loads(l)["observations"]}
        self.assertEqual(got["JamCams_1"]["image_url"], "/media/JamCams_1.jpg")
        self.assertEqual(got["JamCams_1"]["video_url"], "/media/JamCams_1.mp4")
        self.assertNotIn("image_url", got["NZCAM-2"])
        self.assertEqual(sorted(os.listdir(os.path.join(self.out, "media"))), ["JamCams_1.jpg", "JamCams_1.mp4"])
        self.assertEqual(summary["media"]["tfl"]["images"], 1)
        self.assertEqual(summary["media"]["tfl"]["clips"], 1)
        self.assertEqual(summary["media"]["nzta"]["failed"], 1)
        # Re-pack offline: stored files are used, nothing is fetched.
        summary = record.pack(self.raw, self.out, quiet=True, media=False, fetch=None)
        self.assertEqual(summary["media_counts"]["image_url"], [0, 1, 1])
        self.assertTrue(os.path.isfile(os.path.join(self.out, "media", "JamCams_1.mp4")))

    def test_forge_files_hard_linked(self):
        src = os.path.join(self.raw, "forge", "000000-aircraft.csv")
        with open(src, "w") as f:
            f.write("a\r\n")
        record.pack(self.raw, self.out, quiet=True)
        dst = os.path.join(self.out, "forge", "000000-aircraft.csv")
        self.assertEqual(os.stat(src).st_ino, os.stat(dst).st_ino)
        record.pack(self.raw, self.out, quiet=True)  # re-packing over the links works
        self.assertTrue(os.path.isfile(dst))


class IncomingTest(unittest.TestCase):
    T = 1_792_000_000

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_parse_incoming_name(self):
        ns = self.T * 10**9 + 123
        self.assertEqual(record.parse_incoming_name(f"{ns}-regional-extra.jsonl"), (ns, "regional-extra"))
        self.assertIsNone(record.parse_incoming_name(f".{ns}-regional-extra.jsonl"))
        self.assertIsNone(record.parse_incoming_name("notes.txt"))

    def test_move_incoming_keeps_every_batch_in_time_order(self):
        inc = os.path.join(self.tmp.name, "incoming")
        hdir = os.path.join(self.tmp.name, "heimdall")
        os.makedirs(inc)
        os.makedirs(hdir)
        base = (self.T + 5) * 10**9
        for i, ns in enumerate([base + 900_000_000, base + 100_000_000, base + 500_000_000]):
            with open(os.path.join(inc, f"{ns}-regional-extra.jsonl"), "w") as f:
                f.write('{"entity":"E%d"}\n' % i)
        with open(os.path.join(inc, f".{base}-regional-extra.jsonl"), "w") as f:
            f.write('{"entity":"half')  # still being written
        with open(os.path.join(inc, f"{(self.T - 2) * 10**9}-usgs.jsonl"), "w") as f:
            f.write('{"entity":"early"}\n')
        taken = set()
        moved = record.move_incoming(inc, hdir, self.T, taken)
        self.assertEqual(moved, 4)
        self.assertEqual(sorted(os.listdir(inc)), [f".{base}-regional-extra.jsonl"])
        self.assertEqual(sorted(os.listdir(hdir)), ["000000-usgs.jsonl", "000005-regional-extra.jsonl",
                                                     "000005-regional-extra~002.jsonl",
                                                     "000005-regional-extra~003.jsonl"])

        def entity(name):
            with open(os.path.join(hdir, name)) as f:
                return json.loads(f.readline())["entity"]
        self.assertEqual([entity(n) for n in sorted(os.listdir(hdir))[1:]], ["E1", "E2", "E0"])
        self.assertEqual(taken, set(os.listdir(hdir)))


class HookAndRunnerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_record_to_removed_even_on_error(self):
        hook = os.path.join(self.tmp.name, "record_to")
        target = os.path.join(self.tmp.name, "incoming")
        os.makedirs(target)
        with self.assertRaises(RuntimeError):
            with record.record_to(hook, target):
                with open(hook) as f:
                    self.assertEqual(f.read(), f"{os.path.abspath(target)}\npid={os.getpid()}\n")
                raise RuntimeError("boom")
        self.assertFalse(os.path.exists(hook))

    def test_prepare_writes_meta_at_the_start(self):
        out = os.path.join(self.tmp.name, "rec-x")
        raw = record.prepare(out, 1_792_000_000)
        self.assertEqual(raw, os.path.join(out, "raw"))
        self.assertEqual(record.load_json(os.path.join(raw, "meta.json")),
                         {"recordedFrom": 1_792_000_000, "duration": 1})
        for sub in ("heimdall", "forge", "incoming"):
            self.assertTrue(os.path.isdir(os.path.join(raw, sub)))
        with self.assertRaises(SystemExit):
            record.prepare(out, 1_792_000_000)  # the name is taken

    def test_sigterm_and_sighup_set_the_stopping_flag(self):
        before = {s: signal.getsignal(s) for s in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
        stopping = []
        with record.stop_on_signals(stopping):
            os.kill(os.getpid(), signal.SIGTERM)
            self.assertTrue(stopping)
            os.kill(os.getpid(), signal.SIGHUP)
            os.kill(os.getpid(), signal.SIGINT)
            self.assertEqual(len(stopping), 3)
        self.assertEqual({s: signal.getsignal(s) for s in before}, before)

    def test_pid_alive(self):
        pidfile = os.path.join(self.tmp.name, "feeds.pid")
        self.assertFalse(record.pid_alive(pidfile))
        with open(pidfile, "w") as f:
            f.write(f"{os.getpid()}\n")
        self.assertTrue(record.pid_alive(pidfile))
        child = subprocess.Popen(["/usr/bin/true"])
        child.wait()
        with open(pidfile, "w") as f:
            f.write(str(child.pid))
        self.assertFalse(record.pid_alive(pidfile))
        with open(pidfile, "w") as f:
            f.write("garbage")
        self.assertFalse(record.pid_alive(pidfile))


class CaptureRuleTest(unittest.TestCase):
    T0 = 1_792_000_000.0

    def test_copies_a_new_stable_file(self):
        self.assertTrue(record.should_copy(self.T0 + 5, self.T0 + 6.5, None, self.T0))

    def test_waits_until_stable_for_a_second(self):
        self.assertFalse(record.should_copy(self.T0 + 5, self.T0 + 5.5, None, self.T0))

    def test_skips_a_version_already_copied(self):
        self.assertFalse(record.should_copy(self.T0 + 5, self.T0 + 9, self.T0 + 5, self.T0))
        self.assertTrue(record.should_copy(self.T0 + 7, self.T0 + 9, self.T0 + 5, self.T0))

    def test_skips_files_older_than_the_recording(self):
        self.assertFalse(record.should_copy(self.T0 - 1, self.T0 + 9, None, self.T0))

    def test_valid_jsonl(self):
        self.assertTrue(record.valid_jsonl('{"entity":"A"}\n\n{"entity":"B"}\n'))
        self.assertTrue(record.valid_jsonl(""))
        self.assertFalse(record.valid_jsonl('{"entity":"A"}\n{"enti'))
        self.assertFalse(record.valid_jsonl('[1,2]\n'))

    def test_heimdall_copy_needs_valid_content(self):
        good, bad = '{"entity":"A"}\n', '{"entity":'
        self.assertTrue(record.should_copy(self.T0 + 5, self.T0 + 7, None, self.T0, text=good))
        self.assertFalse(record.should_copy(self.T0 + 5, self.T0 + 7, None, self.T0, text=bad))

    def test_offset_and_unique_names(self):
        self.assertEqual(record.offset_of(self.T0 + 12.9, self.T0), 12)
        self.assertEqual(record.offset_of(self.T0 - 3, self.T0), 0)
        self.assertEqual(record.heimdall_name(12, "adsb-fi", set()), "000012-adsb-fi.jsonl")
        taken = {"000012-adsb-fi.jsonl", "000012-adsb-fi~002.jsonl"}
        self.assertEqual(record.heimdall_name(12, "adsb-fi", taken), "000012-adsb-fi~003.jsonl")
        names = ["000012-adsb-fi.jsonl"] + [f"000012-adsb-fi~{n:03d}.jsonl" for n in range(2, 12)]
        self.assertEqual(sorted(names), names)


if __name__ == "__main__":
    unittest.main()
