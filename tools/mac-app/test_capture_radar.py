#!/usr/bin/env python3
"""Tests for capture-radar.py (no network): python3 tools/mac-app/test_capture_radar.py"""
import importlib.util
import os
import unittest

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


if __name__ == "__main__":
    unittest.main()
