#!/usr/bin/env python3
"""Tests for capture-radar.py (no network): python3 tools/mac-app/test_capture_radar.py"""
import importlib.util
import os
import tempfile
import unittest
import urllib.request

_spec = importlib.util.spec_from_file_location(
    "capture_radar", os.path.join(os.path.dirname(os.path.abspath(__file__)), "capture-radar.py"))
cr = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cr)


class CaptureRadarTests(unittest.TestCase):
    def test_tiles_are_the_ones_radar_ts_reads(self):
        # RADAR_BOX at zoom 6: x 29-32, y 17-22 (as radar.ts computes them).
        self.assertEqual(cr.tile_range(), (29, 32, 17, 22))

    def test_only_png_is_kept(self):
        self.assertTrue(cr.is_png(b"\x89PNG\r\n\x1a\n" + b"x"))
        self.assertFalse(cr.is_png(b"<html>error</html>"))
        self.assertFalse(cr.is_png(b"\x89PNG\r\n\x1a\n"))

    def test_past_frames_are_checked_and_sorted(self):
        maps = {"radar": {"past": [
            {"time": 1200, "path": "/v2/radar/b"},
            {"time": 600, "path": "/v2/radar/a"},
            {"time": 1800, "path": "//evil.example/x"},
            {"time": 2400, "path": "/v2/../x"},
            {"time": "3000", "path": "/v2/radar/c"},
            {"time": 3600, "path": "/v2/radar/d?x=1"},
        ]}}
        self.assertEqual(cr.past_frames(maps), [{"time": 600, "path": "/v2/radar/a"},
                                                {"time": 1200, "path": "/v2/radar/b"}])
        self.assertEqual(cr.past_frames({}), [])

    def test_past_frames_are_one_per_time(self):
        maps = {"radar": {"past": [
            {"time": 600, "path": "/v2/radar/a"},
            {"time": 1200, "path": "/v2/radar/b"},
            {"time": 600, "path": "/v2/radar/a2"},
            {"time": True, "path": "/v2/radar/t"},
            "junk",
        ]}}
        self.assertEqual(cr.past_frames(maps), [{"time": 600, "path": "/v2/radar/a"},
                                                {"time": 1200, "path": "/v2/radar/b"}])

    def test_redirects_to_another_host_are_refused(self):
        h = cr.SameHostRedirects()
        req = urllib.request.Request("https://tilecache.rainviewer.com/v2/radar/a/1.png")
        with self.assertRaises(cr.Refused):
            h.redirect_request(req, None, 302, "Found", {}, "https://elsewhere.example/a.png")
        with self.assertRaises(cr.Refused):
            h.redirect_request(req, None, 302, "Found", {}, "http://tilecache.rainviewer.com/a.png")
        self.assertIsNotNone(h.redirect_request(
            req, None, 302, "Found", {}, "https://tilecache.rainviewer.com/v2/radar/b/1.png"))
        self.assertTrue(cr.same_origin(cr.MANIFEST_URL, "https://api.rainviewer.com/x"))
        self.assertFalse(cr.same_origin(cr.MANIFEST_URL, "https://api.rainviewer.com.evil.example/x"))

    def test_manifest_times_are_relative_and_empty_frames_dropped(self):
        frames = [{"time": 1000}, {"time": 1600}, {"time": 2200}, {"time": 3400}]
        all_tiles = [[x, y] for x in range(29, 33) for y in range(17, 23)]
        m = cr.build_manifest(frames, [[], all_tiles, [[31, 17]], []], 5000)
        self.assertEqual(m["firstTime"], 1000)
        self.assertEqual([f["offset"] for f in m["frames"]], [0, 1200, 2400])
        self.assertEqual([f["path"] for f in m["frames"]], ["frames/0", "frames/2", "frames/3"])
        self.assertEqual(m["frames"][1]["missing"], [[31, 17]])
        self.assertEqual(m["step"], 1200)
        self.assertEqual((m["zoom"], m["tileSize"], m["box"]), (6, 256, cr.BOX))
        self.assertEqual(m["attribution"], "Weather data by RainViewer")
        with self.assertRaises(ValueError):
            cr.build_manifest([{"time": 1}], [all_tiles], 5)

    def test_swap_is_recoverable(self):
        with tempfile.TemporaryDirectory() as d:
            out, tmp = os.path.join(d, "x"), os.path.join(d, "x.tmp")
            def put(path, text):
                os.makedirs(path, exist_ok=True)
                with open(os.path.join(path, "manifest.json"), "w") as f:
                    f.write(text)
            def read(path):
                with open(os.path.join(path, "manifest.json")) as f:
                    return f.read()
            # Stopped between the renames: only x.old is left, and it is put back.
            put(out + ".old", "previous")
            cr.recover_previous(out)
            self.assertEqual(read(out), "previous")
            self.assertFalse(os.path.exists(out + ".old"))
            cr.recover_previous(out)  # nothing to do
            # The second rename fails (no tmp): the previous capture is restored.
            with self.assertRaises(OSError):
                cr.swap_in(tmp, out)
            self.assertEqual(read(out), "previous")
            # A good swap replaces it and leaves no .old.
            put(tmp, "new")
            cr.swap_in(tmp, out)
            self.assertEqual(read(out), "new")
            self.assertFalse(os.path.exists(out + ".old") or os.path.exists(tmp))


if __name__ == "__main__":
    unittest.main()
