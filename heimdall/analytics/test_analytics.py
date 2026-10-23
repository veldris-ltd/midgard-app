"""Tests for analytics.py's offline layout (Midgard.app): stored media only, the cameras
from a recording, decoding through a frames helper, sending through heimdall-source, and
the HTTP service's token. Needs numpy (any Python with it: the dev .venv, or the app's
bundled runtime); onnxruntime is not needed, the detector is a stub. Nothing is fetched.

    python3 -m unittest heimdall/analytics/test_analytics.py -v
"""
import gzip
import http.client
import json
import os
import stat
import subprocess
import sys
import tempfile
import threading
import time
import types
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np  # noqa: E402

import analytics as A  # noqa: E402


def refuse(*_a, **_k):
    raise AssertionError("a test tried to reach the internet")


A.urllib.request.urlopen = refuse


def script(path, body):
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + body)
    os.chmod(path, 0o755)
    return path


class Layout(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name
        self.saved = {k: getattr(A, k) for k in ("FRAMES", "RECORDING", "MEDIA_DIR", "SOURCE", "LOG", "_RECORDED")}
        A.LOG = None

    def tearDown(self):
        for k, v in self.saved.items():
            setattr(A, k, v)
        self.tmp.cleanup()


class StoredMedia(Layout):
    def setUp(self):
        super().setUp()
        A.MEDIA_DIR = os.path.join(self.dir, "media")
        os.mkdir(A.MEDIA_DIR)
        with open(os.path.join(A.MEDIA_DIR, "JamCams_1.mp4"), "wb") as f:
            f.write(b"x" * 2000)
        with open(os.path.join(self.dir, "secret.mp4"), "wb") as f:
            f.write(b"y" * 2000)

    def test_reads_a_stored_file_in_place(self):
        self.assertEqual(A.stored_media("/media/JamCams_1.mp4"),
                         os.path.realpath(os.path.join(A.MEDIA_DIR, "JamCams_1.mp4")))
        self.assertEqual(A.stored_media("/media/JamCams%5F1.mp4?t=3"),
                         os.path.realpath(os.path.join(A.MEDIA_DIR, "JamCams_1.mp4")))

    def test_refuses_internet_urls_and_escapes(self):
        for url in ("https://s3-eu-west-1.amazonaws.com/jamcams.tfl.gov.uk/x.mp4", "http://127.0.0.1:1/media/a",
                    "/media/../secret.mp4", "/media/%2E%2E%2Fsecret.mp4", "/media/", "/media/.hidden", None, 3):
            with self.assertRaises((ValueError, FileNotFoundError), msg=url):
                A.stored_media(url)

    def test_refuses_a_symlink_out_and_a_missing_or_tiny_file(self):
        os.symlink(os.path.join(self.dir, "secret.mp4"), os.path.join(A.MEDIA_DIR, "link.mp4"))
        with self.assertRaises(FileNotFoundError):
            A.stored_media("/media/link.mp4")
        with self.assertRaises(FileNotFoundError):
            A.stored_media("/media/none.mp4")
        with open(os.path.join(A.MEDIA_DIR, "tiny.jpg"), "wb") as f:
            f.write(b"z")
        with self.assertRaises(ValueError):
            A.stored_media("/media/tiny.jpg")


class RecordedCameras(Layout):
    def setUp(self):
        super().setUp()
        A.RECORDING = self.dir
        A._RECORDED = None
        lines = [
            {"offset": 1, "feed": "tfl-jamcams", "observations": [
                {"entity": "JamCams_00001.1", "kind": "land-camera", "observedAt": 1,
                 "attrs": {"lat": 51.5, "lon": -0.1, "view": "West", "image_url": "/media/old.jpg"}}]},
            {"offset": 2, "feed": "adsb", "observations": [{"entity": "A1", "kind": "air-aircraft", "attrs": {}}]},
            {"offset": 3, "feed": "tfl-jamcams", "observations": [
                {"entity": "JamCams_00001.1", "kind": "land-camera", "observedAt": 2,
                 "attrs": {"lat": 51.5, "lon": -0.1, "view": "West", "image_url": "/media/JamCams_00001.1.jpg",
                           "video_url": "/media/JamCams_00001.1.mp4"}},
                {"entity": "JamCams_00001.2", "kind": "land-camera", "attrs": {"lat": 51.6, "lon": -0.2}}]},
            {"offset": 4, "feed": "drivebc", "observations": [
                {"entity": "BCCAM-7", "kind": "land-camera", "observedAt": 2,
                 "attrs": {"lat": 49, "lon": -123, "orientation": "W", "image_url": "/media/BCCAM-7.jpg"}}]},
        ]
        with gzip.open(os.path.join(self.dir, "heimdall.jsonl.gz"), "wt") as f:
            for line in lines:
                f.write(json.dumps(line) + "\n")

    def test_last_observation_per_camera_with_media(self):
        cams = {c["id"]: c for c in A.recorded_cams()}
        self.assertEqual(sorted(cams), ["BCCAM-7", "JamCams_00001.1"])  # .2 has no media
        self.assertEqual(cams["JamCams_00001.1"]["video"], "/media/JamCams_00001.1.mp4")
        self.assertEqual(cams["JamCams_00001.1"]["image"], "/media/JamCams_00001.1.jpg")
        self.assertEqual(cams["JamCams_00001.1"]["view"], "West")
        self.assertIsNone(cams["BCCAM-7"]["video"])
        self.assertEqual(cams["BCCAM-7"]["view"], "W")

    def test_pick_uses_the_recording(self):
        cfg = A.config()
        self.assertEqual([c["id"] for c in A.pick(cfg, ["JamCams_00001.1"])], ["JamCams_00001.1"])
        self.assertEqual([c["id"] for c in A.pick(cfg, ["BCCAM-7"])], ["BCCAM-7"])
        self.assertEqual(A.pick(cfg, ["SGCAM-1"]), [])  # no internet lookup


class HelperAndSource(Layout):
    def test_decode_through_the_frames_helper(self):
        log = os.path.join(self.dir, "args")
        # probe says 352x288; decode writes 3 frames of 640x523 BGR (the letterbox size)
        A.FRAMES = script(os.path.join(self.dir, "frames"), f"""
echo "$@" >> {log}
if [ "$1" = probe ]; then echo 352,288; exit 0; fi
head -c {3 * 640 * 523 * 3} /dev/zero
""")
        frames, ratio, size = A.decode("/x/clip.mp4", 640, 4, 40, False)
        self.assertEqual(frames.shape, (3, 523, 640, 3))
        self.assertEqual(size, (352, 288))
        self.assertAlmostEqual(ratio, 640 / 352)
        with open(log) as f:
            self.assertEqual(f.read().splitlines(), [
                "probe /x/clip.mp4",
                "decode /x/clip.mp4 --width 640 --height 523 --fps 4 --max-frames 40"])
        A.decode("/x/still.jpg", 640, 4, 40, True)
        with open(log) as f:
            self.assertEqual(f.read().splitlines()[-1], "decode /x/still.jpg --width 640 --height 523 --still")

    def test_a_failing_helper_is_reported(self):
        A.FRAMES = script(os.path.join(self.dir, "frames"), """
if [ "$1" = probe ]; then echo 352,288; exit 0; fi
echo "midgard-frames: clip.mp4: no video track" >&2; exit 1
""")
        with self.assertRaisesRegex(RuntimeError, "midgard-frames: .*no video track"):
            A.decode("/x/clip.mp4", 640, 4, 40, False)

    def test_send_pipes_the_batch_to_heimdall_source(self):
        got = os.path.join(self.dir, "got")
        src = script(os.path.join(self.dir, "heimdall-source"), f"""
echo "$@" > {got}.args
cat > {got}.stdin
echo "batch: 1 accepted, 0 refused, 0 unreadable in 0.1 s"
""")
        A.SOURCE = (src, "/home/airports-mesh/feeds", "127.0.0.1:5555")
        result = A.send([{"entity": "AI-X", "kind": "land-camera-ai", "observedAt": 1, "attrs": {"ai_vehicles": 2}}])
        self.assertEqual(result, "accepted=1 refused=0 unreadable=0")
        with open(got + ".args") as f:
            self.assertEqual(f.read().split(), ["--identity-dir", "/home/airports-mesh/feeds", "--addr",
                                                "127.0.0.1:5555", "--service", "camera-analytics", "--batch", "-"])
        with open(got + ".stdin") as f:
            self.assertEqual(json.loads(f.read())["entity"], "AI-X")


class Options(unittest.TestCase):
    def test_layout_options(self):
        saved = {k: getattr(A, k) for k in dir(A) if k.isupper()}
        try:
            with tempfile.TemporaryDirectory() as d:
                ns = types.SimpleNamespace(config=None, state_dir=os.path.join(d, "s"), log="-", model_dir="/m",
                                           port=4321, token_file=os.path.join(d, "tok"), frames_helper="/h/mf",
                                           recording="/r", media_dir=None, source_bin="/h/hs",
                                           source_identity_dir="/id", source_addr="127.0.0.1:9")
                A.layout(ns)
                self.assertTrue(os.path.isdir(os.path.join(d, "s")))
                self.assertEqual(stat.S_IMODE(os.stat(os.path.join(d, "s")).st_mode), 0o700)
                self.assertEqual((A.TREND_FILE, A.WATCH_FILE, A.LOG, A.MODELS, A.SERVE_PORT, A.SECRET),
                                 (os.path.join(d, "s", "trend.jsonl"), os.path.join(d, "s", "watches.json"), None,
                                  "/m", 4321, os.path.join(d, "tok")))
                self.assertEqual((A.FRAMES, A.RECORDING, A.MEDIA_DIR, A.SOURCE),
                                 ("/h/mf", "/r", "/r/media", ("/h/hs", "/id", "127.0.0.1:9")))
        finally:
            for k, v in saved.items():
                setattr(A, k, v)


class Serve(Layout):
    """The HTTP service end to end with a stub detector and helper: the token, a request,
    the trend."""

    def test_token_request_and_trend(self):
        d = self.dir
        media = os.path.join(d, "rec", "media")
        os.makedirs(media)
        with open(os.path.join(media, "JamCams_1.mp4"), "wb") as f:
            f.write(b"x" * 5000)
        with gzip.open(os.path.join(d, "rec", "heimdall.jsonl.gz"), "wt") as f:
            f.write(json.dumps({"observations": [{"entity": "JamCams_1", "kind": "land-camera", "attrs": {
                "lat": 51.5, "lon": -0.1, "view": "North", "video_url": "/media/JamCams_1.mp4"}}]}) + "\n")
        A.RECORDING, A.MEDIA_DIR, A._RECORDED = os.path.join(d, "rec"), media, None
        A.FRAMES = script(os.path.join(d, "frames"), f"""
if [ "$1" = probe ]; then echo 64,64; exit 0; fi
head -c {12 * 64 * 64 * 3} /dev/zero
""")
        A.SOURCE = (script(os.path.join(d, "hs"), 'cat >/dev/null; echo "batch: 1 accepted, 0 refused, 0 unreadable in 0 s"\n'),
                    d, "127.0.0.1:1")
        saved = (A.SECRET, A.TREND_FILE, A.WATCH_FILE, A.SERVE_PORT)
        A.SECRET, A.TREND_FILE, A.WATCH_FILE = (os.path.join(d, n) for n in ("tok", "trend.jsonl", "watches.json"))
        import socket
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        A.SERVE_PORT = s.getsockname()[1]
        s.close()

        class Det:
            size = 64

            def __call__(self, frame, ratio, conf, nms):  # one car in the middle, moving right
                return np.array([[20, 20, 30, 30, 0.9, 2]], np.float32)

            def name(self):
                return "stub"
        try:
            threading.Thread(target=A.serve, args=(Det(),), daemon=True).start()
            for _ in range(100):
                if os.path.exists(A.SECRET):
                    try:
                        socket.create_connection(("127.0.0.1", A.SERVE_PORT), 0.1).close()
                        break
                    except OSError:
                        pass
                time.sleep(0.05)
            self.assertEqual(stat.S_IMODE(os.stat(A.SECRET).st_mode), 0o600)
            with open(A.SECRET) as f:
                token = f.read().strip()
            self.assertEqual(len(token), 48)

            def post(path, body, tok=token):
                c = http.client.HTTPConnection("127.0.0.1", A.SERVE_PORT, timeout=10)
                c.request("POST", path, json.dumps(body), {"Authorization": f"Bearer {tok}"})
                r = c.getresponse()
                return r.status, json.loads(r.read())
            self.assertEqual(post("/analyse", {"camera": "JamCams_1"}, "wrong")[0], 401)
            code, body = post("/analyse", {"camera": "JamCams_1", "by": "analyst"})
            self.assertEqual(code, 200, body)
            self.assertEqual(body["attrs"]["camera_id"], "JamCams_1")
            self.assertEqual(body["attrs"]["ai_vehicles"], 1)
            self.assertEqual(body["sent"], "accepted=1 refused=0 unreadable=0")
            self.assertEqual(post("/analyse", {"camera": "JamCams_404"})[0], 404)
            code, trend = post("/trend", {"camera": "JamCams_1"})
            self.assertEqual((code, len(trend["points"]), trend["points"][0]["vehicles"]), (200, 1, 1))
        finally:
            A.SECRET, A.TREND_FILE, A.WATCH_FILE, A.SERVE_PORT = saved


if __name__ == "__main__":
    unittest.main()
