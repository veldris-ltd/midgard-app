"""Tests for convert.py's data-protection screening.

    python3 -I -m unittest discover -s samples -p 'test_*.py'
    python3 -I samples/test_convert.py

(`python3 -I -m unittest samples/test_convert.py` cannot import the file: -I keeps the
current directory off sys.path, and `discover` adds the start directory itself.)

Standard library only. [VLD-ANB-MIDGARD-0074] [AC-25.1]
"""

import contextlib
import importlib.util
import io
import json
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("convert", os.path.join(HERE, "convert.py"))
convert = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(convert)

# Two made-up records, not real vessels: a 12 m craft with no IMO number and a 200 m ship
# with one. MMSIs in the 999 range are not assigned to any ship.
SMALL = {"callSign": "TEST1", "destination": "NOWHERE", "imo": 0, "mmsi": 999000002, "name": "TEST SMALL",
         "referencePointA": 8, "referencePointB": 4, "shipType": 37}
LARGE = {"callSign": "TEST2", "destination": "NOWHERE", "imo": 9000001, "mmsi": 999000001, "name": "TEST LARGE",
         "referencePointA": 160, "referencePointB": 40, "shipType": 70}


def fixture(folder):
    path = os.path.join(folder, convert.VESSELS)
    os.makedirs(os.path.dirname(path))
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"at": 0, "by_mmsi": {"999000001": dict(LARGE), "999000002": dict(SMALL)}}, f, indent=1)
    return path


def screen(folder, apply=False):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        convert.cmd_screen(type("Args", (), {"samples": folder, "apply": apply})())
    return out.getvalue()


class Screen(unittest.TestCase):
    def test_screen_counts_a_small_craft_without_imo_and_not_a_200_metre_vessel(self):
        # [VLD-ANB-MIDGARD-0074] [AC-25.1]
        with tempfile.TemporaryDirectory() as folder:
            path = fixture(folder)
            before = open(path, encoding="utf-8").read()
            report = screen(folder)
            self.assertIn("2 records, 1 small craft without an IMO number", report)
            self.assertEqual(before, open(path, encoding="utf-8").read(), "screen without --apply writes nothing")

    def test_apply_removes_name_and_call_sign_from_exactly_that_record_deterministically(self):
        # [VLD-ANB-MIDGARD-0074] [AC-25.1]
        with tempfile.TemporaryDirectory() as folder:
            path = fixture(folder)
            self.assertIn("pseudonymised the MMSI of, 1 record(s)", screen(folder, apply=True))
            first = open(path, encoding="utf-8").read()
            by_mmsi = json.loads(first)["by_mmsi"]
            self.assertEqual(["999000001", "small-craft-1"], list(by_mmsi))
            self.assertEqual(LARGE, by_mmsi["999000001"])
            small = by_mmsi["small-craft-1"]
            self.assertNotIn("name", small)
            self.assertNotIn("callSign", small)
            self.assertEqual("small-craft-1", small["mmsi"])
            self.assertEqual(SMALL["destination"], small["destination"])
            # A second run finds nothing left to change and leaves the file as it was.
            self.assertIn("of, 0 record(s)", screen(folder, apply=True))
            self.assertEqual(first, open(path, encoding="utf-8").read())

    def test_pseudonyms_follow_mmsi_order_after_those_already_given(self):
        # [VLD-ANB-MIDGARD-0074]
        doc = {"by_mmsi": {"999000009": dict(SMALL, mmsi=999000009), "small-craft-4": {"imo": 0, "mmsi": "small-craft-4"},
                           "999000003": dict(SMALL, mmsi=999000003)}}
        self.assertEqual(2, convert.pseudonymise(doc))
        self.assertEqual(["small-craft-6", "small-craft-4", "small-craft-5"], list(doc["by_mmsi"]))

    def test_check_refuses_a_published_entry_without_a_screening(self):
        # [VLD-ANB-MIDGARD-0074] [AC-25.1]
        good = {"personal_data": "none", "basis": "b", "screened_on": "2026-11-02", "screened_by": "r", "notes": "n"}
        cat = {"forge_datasets": {"a": {"publish": True, "data_protection": good},
                                  "b": {"publish": True},
                                  "c": {"publish": False}},
               "feed_snapshots": {"d": {"publish": True, "data_protection": dict(good, personal_data="maybe")}},
               "observations": {}}
        problems = convert.screening_problems(cat)
        self.assertEqual(2, len(problems), problems)
        self.assertIn("forge_datasets.b: published without", problems[0])
        self.assertIn("'maybe' is not one of none, possible, yes", problems[1])

    def test_the_committed_catalogue_screens_every_published_entry(self):
        # [VLD-ANB-MIDGARD-0074] [AC-25.1]
        self.assertEqual([], convert.screening_problems(convert.catalogue()))


if __name__ == "__main__":
    unittest.main()
