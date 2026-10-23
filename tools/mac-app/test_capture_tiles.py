#!/usr/bin/env python3
"""Tests for capture-tiles.py (no network): python3 tools/mac-app/test_capture_tiles.py"""
import importlib.util
import os
import tempfile
import struct
import unittest
import urllib.request
import zlib

_spec = importlib.util.spec_from_file_location(
    "capture_tiles", os.path.join(os.path.dirname(os.path.abspath(__file__)), "capture-tiles.py"))
ct = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ct)


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


def png(pixel, w=256, h=256, colour=2, filt=0, land_at=None):
    """A PNG of one RGB `pixel` (land_at: (i, j) of one pixel 1 m above sea level)."""
    bpp = 3 if colour == 2 else 4
    rows = []
    for j in range(h):
        row = bytearray(pixel * w) if bpp == 3 else bytearray((pixel + b"\xff") * w)
        if land_at and land_at[1] == j:
            i = land_at[0] * bpp
            row[i:i + 3] = b"\x80\x01\x00"
        if filt == 1:  # Sub
            row = bytearray((row[k] - (row[k - bpp] if k >= bpp else 0)) & 0xFF for k in range(len(row)))
        rows.append(bytes([filt]) + bytes(row))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, colour, 0, 0, 0)
    return ct.PNG_MAGIC + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b"")


SEA = b"\x7f\xf0\x00"  # 127 * 256 + 240 - 32768 = -272 m


class CaptureTilesTests(unittest.TestCase):
    def test_ranges_are_the_ones_detail_patch_computes(self):
        # The box at zoom 7 is traffic.ts's overview range too (70 tiles).
        self.assertEqual(ct.tile_range(7), (59, 65, 35, 44))
        x0, x1, y0, y1 = ct.tile_range(10)
        self.assertEqual((x0, x1), (477, 523))
        self.assertTrue(ct.in_range((1, 2, 3, 4), 2, 3) and not ct.in_range((1, 2, 3, 4), 3, 3))

    def test_children_only_under_land_deduplicated(self):
        got = ct.children({(60, 40): {(0, 0), (1, 1)}, (61, 40): {(0, 1)}}, 7, (0, 999, 0, 999))
        self.assertEqual(got, [(120, 80), (121, 81), (122, 81)])
        # Clipped to the level's range.
        self.assertEqual(ct.children({(60, 40): {(0, 0), (1, 1)}}, 7, (121, 121, 0, 999)), [(121, 81)])

    def test_terrarium_tiles_are_decoded_and_land_found(self):
        rows, bpp = ct.decode_terrarium(png(SEA))
        self.assertEqual((len(rows), len(rows[0]), bpp), (256, 768, 3))
        self.assertEqual(ct.land_quadrants(rows, bpp), set())
        rows, bpp = ct.decode_terrarium(png(SEA, filt=1, land_at=(200, 10)))
        self.assertEqual(ct.land_quadrants(rows, bpp), {(1, 0)})
        rows, bpp = ct.decode_terrarium(png(SEA, colour=6, land_at=(128, 128)))
        self.assertEqual(ct.land_quadrants(rows, bpp), {(0, 0), (1, 0), (0, 1), (1, 1)})  # on the cross: all four (margin)

    def test_bad_downloads_are_refused(self):
        good = png(SEA)
        broken = bytearray(good)
        broken[40] ^= 0xFF  # inside IHDR/IDAT: the CRC no longer matches
        for bad in (b"", b"<html>rate limited</html>", good[:100], bytes(broken), png(SEA, w=128, h=128),
                    b"\xff\xd8\xff\xe0" + b"\0" * 100):
            with self.assertRaises((ValueError, zlib.error)):
                ct.decode_terrarium(bad)
        self.assertEqual(ct.imagery_kind(b"\xff\xd8\xff\xe0" + b"\0" * 100), "jpeg")
        self.assertEqual(ct.imagery_kind(png(b"\x18\x2c\x4e", 4, 4)), "sea")
        for bad in (b"", b"<html>", b"\xff\xd8\xff", b"GIF89a" + b"\0" * 100):
            with self.assertRaises(ValueError):
                ct.imagery_kind(bad)

    def test_levels_chosen_coarse_to_fine_under_the_cap(self):
        terrain = {7: 2, 8: 10, 9: 40, 10: 120, 11: 400, 12: 1400}
        imagery = {8: 1, 9: 5, 10: 18, 11: 60, 12: 200, 13: 700}
        self.assertEqual(ct.choose_levels(terrain, imagery, 300), ([7, 8, 9, 10], [8, 9, 10, 11], 256))
        # Finer imagery alone when it fits and the next pair does not.
        self.assertEqual(ct.choose_levels(terrain, imagery, 150), ([7, 8, 9], [8, 9, 10, 11], 136))
        self.assertEqual(ct.choose_levels(terrain, imagery, 2), ([], [], 0))
        self.assertEqual(ct.choose_levels(terrain, imagery, 10 ** 9)[0], [7, 8, 9, 10, 11, 12])

    def test_redirects_to_another_host_are_refused(self):
        h = ct.SameHostRedirects()
        req = urllib.request.Request("https://tiles.maps.eox.at/wmts/a.jpg")
        with self.assertRaises(ct.Failed):
            h.redirect_request(req, None, 302, "Found", {}, "https://elsewhere.example/a.jpg")
        with self.assertRaises(ct.Failed):
            h.redirect_request(req, None, 302, "Found", {}, "http://tiles.maps.eox.at/a.jpg")
        self.assertIsNotNone(h.redirect_request(req, None, 302, "Found", {}, "https://tiles.maps.eox.at/b.jpg"))

    def test_eox_layer_attribution_from_capabilities(self):
        caps = ("<Layer><ows:Title>x</ows:Title><ows:Abstract>other</ows:Abstract>"
                "<ows:Identifier>s2cloudless-2020_3857</ows:Identifier></Layer>"
                "<Layer><ows:Abstract>EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH "
                "(Contains modified Copernicus Sentinel data 2016) released under &lt;a rel=\"license\" "
                "href=\"https://creativecommons.org/licenses/by/4.0/\"&gt;Creative Commons&lt;/a&gt;."
                "</ows:Abstract><ows:Identifier>s2cloudless_3857</ows:Identifier></Layer>")
        self.assertEqual(ct.layer_attribution(caps), (ct.SOURCES["s2cloudless"]["attribution"], "CC BY 4.0"))
        self.assertEqual(ct.layer_attribution("<Layer></Layer>"), (None, None))

    def test_manifest_holds_only_integers_in_levels(self):
        levels = {name: [{"z": 7, "range": (59, 65, 35, 44), "tiles": [(63, 42), (60, 40)], "missing": [(61, 41)]}]
                  for name in ct.SOURCES}
        m = ct.build_manifest(levels, 1700000000, {})
        self.assertEqual(m["box"], ct.BOX)
        lv = m["sources"]["terrarium"]["levels"][0]
        self.assertEqual(lv, {"z": 7, "x0": 59, "x1": 65, "y0": 35, "y1": 44,
                              "tiles": [[60, 40], [63, 42]], "missing": [[61, 41]]})
        self.assertNotIn("http", str(m["sources"]["terrarium"]["levels"]))
        with self.assertRaises(ValueError):
            ct.build_manifest({name: [] for name in ct.SOURCES}, 0, {})

    def test_land_grid_estimate(self):
        grid = ct.read_land_grid()
        self.assertTrue(ct.grid_land(grid, 10, int(ct.tile_x(-0.12, 10)), int(ct.tile_y(51.5, 10))))  # London
        self.assertFalse(ct.grid_land(grid, 10, int(ct.tile_x(-10.5, 10)), int(ct.tile_y(49, 10))))  # Atlantic

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
