#!/usr/bin/env python3
"""Tests for capture-traffic.py (no network): python3 tools/mac-app/test_capture_traffic.py"""
import importlib.util
import json
import os
import tempfile
import unittest

_spec = importlib.util.spec_from_file_location(
    "capture_traffic", os.path.join(os.path.dirname(os.path.abspath(__file__)), "capture-traffic.py"))
ct = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ct)


def _len(b):
    out = bytearray()
    n = len(b)
    while True:
        if n < 0x80:
            out.append(n)
            return bytes(out) + b
        out.append((n & 0x7F) | 0x80)
        n >>= 7


def layer(name, features):
    body = b"\x0a" + _len(name.encode()) + b"".join(b"\x12" + _len(f) for f in features) + b"\x78\x02"
    return b"\x1a" + _len(body)


class CaptureTrafficTests(unittest.TestCase):
    def test_overview_tiles_are_the_ones_traffic_ts_reads(self):
        # RADAR_BOX at zoom 7: x 59-65, y 35-44 (traffic.ts's overview, 70 tiles, under MAX_TILES 80).
        self.assertEqual(ct.tile_range(7), (59, 65, 35, 44))

    def test_close_levels_only_under_roads(self):
        parent = {"z": 7, "tiles": [(63, 42)], "missing": [(60, 40)]}
        got = ct.wanted(9, ct.tile_range(9), parent)
        self.assertEqual(len(got), 32)  # 16 children of each kept tile
        self.assertTrue(all((x >> 2, y >> 2) in {(63, 42), (60, 40)} for x, y in got))
        self.assertEqual(len(ct.wanted(7, (0, 1, 0, 2), None)), 6)

    def test_vector_tiles_are_validated(self):
        tile = layer("Traffic flow", [b"\x08\x01", b"\x08\x02"]) + layer("other", [b"\x08\x03"])
        self.assertEqual(ct.road_segments(tile), 2)
        self.assertEqual(ct.road_segments(layer("empty", [])), 0)
        for bad in (b"", b"<html>error</html>", b"\x1a\x7f\x0a", b"\x0a\x01x", layer("Traffic flow", [])[:-1]):
            with self.assertRaises(ValueError):
                ct.road_segments(bad)

    def test_key_from_environment_or_env_file(self):
        self.assertEqual(ct.read_key({"TOMTOM_KEY": " k1 "}, "/nonexistent"), "k1")
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, ".env.local")
            with open(p, "w") as f:
                f.write("OTHER=1\nVITE_TOMTOM_KEY='k2'\n")
            self.assertEqual(ct.read_key({}, p), "k2")
        self.assertIsNone(ct.read_key({}, "/nonexistent"))

    def test_caption_is_plain_text(self):
        self.assertEqual(ct.clean_caption(b'{"copyrightsCaption":"\\u00a9TomTom"}'), "©TomTom")
        self.assertEqual(ct.clean_caption(b'{"copyrightsCaption":"<b>\\u00a9 TomTom</b>"}'), "© TomTom")
        self.assertEqual(ct.clean_caption(b'{"copyrightsCaption":"evil"}'), ct.DEFAULT_CAPTION)
        self.assertEqual(ct.clean_caption(b"not json"), ct.DEFAULT_CAPTION)

    def test_manifest_has_paths_not_urls(self):
        levels = [{"z": 7, "range": (59, 65, 35, 44), "tiles": [(63, 42), (62, 41)], "missing": [(60, 40)]},
                  {"z": 9, "range": (238, 261, 142, 177), "tiles": [(253, 170)], "missing": []}]
        m = ct.build_manifest(levels, "©TomTom", 5000)
        self.assertEqual(m["overviewZoom"], 7)
        self.assertEqual(m["box"], ct.BOX)
        self.assertEqual(m["levels"][0]["tiles"], [[62, 41], [63, 42]])
        self.assertEqual(m["levels"][0]["missing"], [[60, 40]])
        self.assertEqual((m["levels"][1]["z"], m["levels"][1]["x0"], m["levels"][1]["y1"]), (9, 238, 177))
        text = json.dumps(m)
        self.assertNotIn("key", text)
        self.assertNotIn("api.tomtom.com", text)
        with self.assertRaises(ValueError):
            ct.build_manifest([{"z": 7, "range": (0, 0, 0, 0), "tiles": [], "missing": [(0, 0)]}], "c", 1)
        with self.assertRaises(ValueError):
            ct.build_manifest([], "c", 1)

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
            ct.recover_previous(out)
            self.assertEqual(read(out), "previous")
            self.assertFalse(os.path.exists(out + ".old"))
            ct.recover_previous(out)  # nothing to do
            # The second rename fails (no tmp): the previous capture is restored.
            with self.assertRaises(OSError):
                ct.swap_in(tmp, out)
            self.assertEqual(read(out), "previous")
            # A good swap replaces it and leaves no .old.
            put(tmp, "new")
            ct.swap_in(tmp, out)
            self.assertEqual(read(out), "new")
            self.assertFalse(os.path.exists(out + ".old") or os.path.exists(tmp))


if __name__ == "__main__":
    unittest.main()
