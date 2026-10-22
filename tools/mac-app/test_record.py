"""Tests for record.py: thinning, dropping camera kinds, packing a raw capture, and the
capture rule that decides when a changed file is copied.

    python3 -m unittest tools/mac-app/test_record.py -v
"""
import gzip, json, os, sys, tempfile, unittest

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


class CameraTest(unittest.TestCase):
    def test_camera_kinds_dropped_and_empty_batch_dropped(self):
        t = 1_792_000_000
        batches = [
            batch(0, "cams", [obs("C1", "land-camera", t), obs("C2", "land-camera-ai", t)]),
            batch(1, "mixed", [obs("C3", "land-camera", t), obs("S1", "sea-vessel", t)]),
        ]
        out = record.drop_empty(record.drop_cameras(batches))
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["feed"], "mixed")
        self.assertEqual([o["entity"] for o in out[0]["observations"]], ["S1"])


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
        heimdall("000000-adsb-fi~2.jsonl", [obs("A2", "air-track", t)])
        heimdall("000003-cams.jsonl", [obs("C1", "land-camera", t)])
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
        self.assertEqual(meta["feeds"], ["adsb-fi", "adsb-lol", "usgs"])
        self.assertEqual(meta["forgeFiles"], ["aircraft.csv", "ships.csv"])
        self.assertGreater(meta["duration"], 9)

        with gzip.open(os.path.join(self.out, "heimdall.jsonl.gz"), "rt") as f:
            lines = [json.loads(l) for l in f if l.strip()]
        offsets = [b["offset"] for b in lines]
        self.assertEqual(offsets, sorted(offsets))
        self.assertEqual(offsets, [0, 0, 2, 7])  # the camera batch is dropped
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

    def test_parse_heimdall_name(self):
        self.assertEqual(record.parse_heimdall_name("000012-adsb-fi.jsonl"), (12, "adsb-fi"))
        self.assertEqual(record.parse_heimdall_name("000012-adsb-fi~3.jsonl"), (12, "adsb-fi"))
        self.assertIsNone(record.parse_heimdall_name("notes.txt"))


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
        taken = {"000012-adsb-fi.jsonl", "000012-adsb-fi~2.jsonl"}
        self.assertEqual(record.heimdall_name(12, "adsb-fi", taken), "000012-adsb-fi~3.jsonl")


if __name__ == "__main__":
    unittest.main()
