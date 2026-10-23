"""Tests for prepare-answers.py against a stub gateway (login, ask, alerts, picture,
advise) on a free local port: keys, the file format, normalisation (the console's
vectors), alert selection, retries and timeouts, and that no secret is printed.

    python3 -m unittest tools/mac-app/test_prepare_answers.py -v
"""
import contextlib
import gzip
import importlib.util
import io
import json
import os
import tempfile
import threading
import time
import unittest
import urllib.parse
from unittest import mock
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("prepare_answers", os.path.join(HERE, "prepare-answers.py"))
pa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pa)

PASSWORD = "s3cret-Pa55word"
TOKEN = "tok-abcdef-123456"
NOW = time.time()


def alert(id, entity, label, severity="warning", age_s=3600, kind="x", cleared=None, **kw):
    return {"id": id, "entity": entity, "kind": kind, "label": label, "severity": severity, "rationale": "",
            "raisedAt": int(NOW - age_s), "clearedAt": cleared, "acknowledgedBy": None, "acknowledgedAt": None,
            "lat": kw.get("lat"), "lon": kw.get("lon")}


ASK_REPLY = {"answer": "Two power cuts.", "citations": ["power-cut:1"], "unresolved": [], "grounded": True,
             "inferences": [], "proposed": [], "refusal": None, "considered": 12, "ms": 15000}
ADVISE_REPLY = {"subject": "s", "decision_point": "dp", "considered": 4, "unsupported": None, "ms": 14000,
                "options": [{"rank": 2, "title": "B", "summary": "b", "expected_effect": "e2", "risks": [], "citations": [], "grounded": False, "unresolved": []},
                            {"rank": 1, "title": "A", "summary": "a", "expected_effect": "e1", "risks": ["r"], "citations": ["entity:x"], "grounded": True, "unresolved": []}]}


class Stub:
    """A gateway stub. `ask` and `advise` are lists of behaviours consumed per call
    (dict reply, ("sleep", s, reply), or an int HTTP status); once used up, the last
    repeats."""

    def __init__(self):
        self.calls = []
        self.alerts = []
        self.alerts_later = None  # what later reads of the open alerts return, if set
        self.alert_reads = 0
        self.expire = 0  # the next this-many signed-in requests get 401
        self.ask = [ASK_REPLY]
        self.advise = [ADVISE_REPLY]
        self.entities = []
        stub = self

        class H(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def reply(self, code, body):
                data = json.dumps(body).encode()
                try:
                    self.send_response(code)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def behave(self, seq):
                b = seq.pop(0) if len(seq) > 1 else seq[0]
                if isinstance(b, tuple):
                    time.sleep(b[1])
                    b = b[2]
                if isinstance(b, int):
                    return self.reply(b, {"error": "boom"})
                self.reply(200, b)

            def handle_any(self, method):
                n = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(n) if n else b""
                u = urllib.parse.urlsplit(self.path)
                stub.calls.append((method, u.path, urllib.parse.parse_qs(u.query), self.headers.get("authorization"), body))
                p = u.path.removeprefix("/api/v1")
                if p == "/login":
                    j = json.loads(body)
                    ok = j == {"username": "analyst", "password": PASSWORD}
                    return self.reply(200 if ok else 401, {"token": TOKEN} if ok else {"error": "no"})
                if self.headers.get("authorization") != f"Bearer {TOKEN}" or stub.expire > 0:
                    stub.expire = max(0, stub.expire - 1)
                    return self.reply(401, {"error": "auth"})
                if method == "POST" and p == "/heimdall/ask":
                    return self.behave(stub.ask)
                if method == "GET" and p == "/heimdall/alerts":
                    stub.alert_reads += 1
                    later = stub.alerts_later is not None and stub.alert_reads > 1
                    return self.reply(200, {"alerts": stub.alerts_later if later else stub.alerts})
                if method == "GET" and p == "/heimdall/picture":
                    return self.reply(200, {"layers": [], "entities": stub.entities})
                if method == "POST" and p.startswith("/heimdall/alerts/") and p.endswith("/advise"):
                    return self.behave(stub.advise)
                self.reply(404, {"error": "not found"})

            def do_GET(self):
                self.handle_any("GET")

            def do_POST(self):
                self.handle_any("POST")

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), H)
        self.server.daemon_threads = True
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}/api/v1"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class Base(unittest.TestCase):
    def setUp(self):
        self.stub = Stub()
        self.addCleanup(self.stub.close)
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.pw = os.path.join(self.dir.name, "pw.txt")
        with open(self.pw, "w") as f:
            f.write(f"admin (internal): other\nanalyst (internal): {PASSWORD}\n")
        self.out = os.path.join(self.dir.name, "home", "prepared-answers.json")
        self.rec = os.path.join(self.dir.name, "hour-x")
        self.recording(["power-cut:ukpn-1", "flood:1", "e1", "e2", "e3"])

    def recording(self, all_hour, first_only=(), duration=3900):
        """A packed recording: `all_hour` seen at offsets 10 and duration-100,
        `first_only` at 10 only."""
        os.makedirs(self.rec, exist_ok=True)
        with gzip.open(os.path.join(self.rec, "heimdall.jsonl.gz"), "wt") as f:
            for off, ents in ((10, list(all_hour) + list(first_only)), (1800, list(first_only)), (duration - 100, list(all_hour))):
                f.write(json.dumps({"offset": off, "feed": "x", "observations": [{"entity": e, "kind": "k", "observedAt": 1, "attrs": {}} for e in ents]}) + "\n")
        with open(os.path.join(self.rec, "meta.json"), "w") as f:
            json.dump({"version": 1, "duration": duration}, f)

    def run_main(self, *extra):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = pa.main(["--base", self.stub.base, "--out", self.out, "--passwords", self.pw, "--recording", self.rec, *extra])
        return code, buf.getvalue()

    def file(self):
        with open(self.out) as f:
            return json.load(f)


class KeysTest(unittest.TestCase):
    def test_console_vectors(self):
        # products/heimdall/ui/test/preparedAnswers.test.ts 'keys normalise the question'
        self.assertEqual(pa.prepared_key("ask", "UK", "  What   has\tchanged?\n"), "ask|UK|what has changed?")
        self.assertEqual(pa.shipped_key("ask", "anything", "What has  CHANGED?"), "ask|*|what has changed?")
        self.assertEqual(pa.shipped_key("advise", "power-cut:ukpn-123", ""), "advise|entity:power-cut:ukpn-123|")
        self.assertEqual(pa.shipped_key("advise", "e1", " Why? "), "advise|entity:e1|why?")

    def test_self_check_passes_for_examples_and_refuses_non_ascii(self):
        pa.self_check(pa.examples() + pa.OUR_QUESTIONS)
        with self.assertRaises(ValueError):
            pa.self_check(["Qu’est-ce qui a changé ?"])

    def test_examples_parsed_from_ask_card(self):
        with open(pa.ASK_CARD) as f:
            src = f.read()
        ex = pa.examples()
        self.assertGreaterEqual(len(ex), 1)
        for q in ex:
            self.assertIn(q, src)
        self.assertIn("What has changed in the Gulf watch?", ex)

    def test_examples_with_escaped_quotes(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "AskCard.tsx")
            with open(p, "w") as f:
                f.write("const EXAMPLES = ['What\\'s new?', \"Say \\\"hi\\\"\"];\n")
            self.assertEqual(pa.examples(p), ["What's new?", 'Say "hi"'])

    def test_alert_text_mirrors_console(self):
        # alertMarks.ts alertWords/alertText
        self.assertEqual(pa.alert_text("gnss-interference-high"), "GPS INTERFERENCE HIGH")
        self.assertEqual(pa.alert_text("-camera_stopped--vehicle-"), "CAMERA STOPPED VEHICLE")
        self.assertEqual(pa.alert_text("GNSS_jam"), "GPS JAM")
        self.assertEqual(pa.alert_text("gnssx-jam"), "GNSSX JAM")
        self.assertEqual(pa.alert_text("é-gnss"), "É GPS")

    def test_js_num_and_encoding(self):
        self.assertEqual([pa.js_num(v) for v in (-12, 48, 4, 62)], ["-12", "48", "4", "62"])
        self.assertEqual(pa.js_num(48.12345), "48.123")
        self.assertEqual(pa.encode_component("What's near here? a&b"), "What's%20near%20here%3F%20a%26b")


class SelectTest(unittest.TestCase):
    def ids(self, alerts, n=5):
        return [a["id"] for a in pa.select_alerts(alerts, n, NOW)]

    def test_severity_first_then_distinct_kinds(self):
        alerts = [
            alert("i1", "e-i1", "power-cut", "info"),
            alert("w1", "e-w1", "power-cut-major", "warning"),
            alert("w2", "e-w2", "power-cut-severe", "warning", age_s=7200),
            alert("c1", "e-c1", "flood-severe-warning", "critical"),
            alert("w3", "e-w3", "gnss-interference-high", "warning"),
            alert("w4", "e-w4", "weather-warning-amber", "warning"),
            alert("w5", "e-w5", "gulf-watch-elevated", "warning"),
            alert("w6", "e-w6", "conflict-zone-bulletin-new", "warning"),
        ]
        got = self.ids(alerts)
        self.assertEqual(got[0], "c1")
        self.assertEqual(got[1], "w2")  # the longer open of the two power cuts
        self.assertEqual(len(got), 5)
        kinds = [pa.category(a["label"])[0] for a in alerts if a["id"] in got]
        self.assertEqual(len(set(kinds)), 5)
        self.assertNotIn("i1", got)

    def test_fills_with_second_of_a_kind_when_short(self):
        got = self.ids([alert("a", "ea", "power-cut-major"), alert("b", "eb", "power-cut-severe")])
        self.assertEqual(sorted(got), ["a", "b"])

    def test_young_cleared_and_shared_subjects_skipped(self):
        alerts = [
            alert("young", "e1", "flood-warning", age_s=300),
            alert("cleared", "e2", "flood-alert", cleared=int(NOW)),
            alert("twin1", "e3", "flood-severe-warning"),
            alert("twin2", "e3", "weather-warning-amber"),
            alert("ok", "e4", "flood-alert"),
        ]
        self.assertEqual(self.ids(alerts), ["ok"])

    def test_ms_times_and_camera_alerts(self):
        a = alert("cam", "AI-cam7", "camera-wrong-way", kind="land-camera-ai")
        a["raisedAt"] = int((NOW - 3600) * 1000)
        picked = pa.select_alerts([a], 5, NOW)
        self.assertEqual(picked[0]["entity"], "cam7")

    def test_shared_panel_rows_skipped(self):
        # AlertsPanel merges alerts of the same severity and text into one row; a click
        # selects only the newest subject, so neither is picked.
        alerts = [
            alert("p1", "e1", "power-cut-major", "warning"),
            alert("p2", "e2", "power-cut-major", "warning", age_s=7200),
            alert("p3", "e3", "power-cut-major", "critical"),  # same text, other severity: own row
            alert("f1", "e4", "flood-warning", "warning"),
            alert("f2", "e5", "flood_warning", "warning", age_s=60),  # same text (alertText), too young: still shares
            alert("g1", "e6", "gnss-interference-high", "warning", cleared=int(NOW)),  # cleared: not in the panel
            alert("g2", "e7", "gnss-interference-high", "warning"),
        ]
        self.assertEqual(sorted(self.ids(alerts)), ["g2", "p3"])

    def test_row_keys_for_weather_warnings(self):
        def ww(id, ent, weather, valid_to, valid_from=None, label="weather-warning-yellow"):
            attrs = {"level": "Yellow", "weather": weather, "valid_to": valid_to}
            if valid_from is not None:
                attrs["valid_from"] = valid_from
            return alert(id, ent, label, "info", kind="event-weather-warning"), {"id": ent, "kind": "event-weather-warning", "attributes": attrs}
        (a1, e1), (a2, e2) = ww("w1", "x1", "wind", 2000000000), ww("w2", "x2", "rain", 2000000000)
        # Same minute, one not yet in force (FROM …): still the same row.
        a3, e3 = ww("w3", "x3", "wind", 2000000010, valid_from=NOW + 3600)
        ents = {e["id"]: e for e in (e1, e2, e3)}
        key = lambda a: pa.row_of(a, ents.get(a["entity"]))  # noqa: E731
        self.assertEqual(key(a1), ("warning", "info", "weather-warning-yellow", ("YELLOW", "WIND", None, 2000000000 // 60)))
        self.assertNotEqual(key(a1), key(a2))
        self.assertTrue(pa.same_row(key(a1), key(a3)))  # same end minute
        self.assertEqual([a["id"] for a in pa.select_alerts([a1, a2, a3], 5, NOW, row_key=key)], ["w2"])
        # Details unknown for one: it may be the same row as any of its severity and label.
        a4 = alert("w4", "x4", "weather-warning-yellow", "info", kind="event-weather-warning")
        self.assertEqual(pa.select_alerts([a2, a4], 5, NOW, row_key=key), [])
        a5 = alert("w5", "x5", "weather-warning-amber", "warning", kind="event-weather-warning")
        self.assertEqual(sorted(a["id"] for a in pa.select_alerts([a2, a5], 5, NOW, row_key=key)), ["w2", "w5"])
        self.assertEqual(pa.row_of(alert("z", "z", "power-cut-major"), None), ("text", "warning", "POWER CUT MAJOR"))
        # Issued ahead, the row shows only FROM <valid_from>: the same start minute is one
        # row even with different ends; neither matching is two rows.
        b1, f1 = ww("b1", "y1", "rain", 2000090000, valid_from=2000000000)
        b2, f2 = ww("b2", "y2", "rain", 2000070000, valid_from=2000000030)
        b3, f3 = ww("b3", "y3", "rain", 2000050000, valid_from=2000003600)
        r = {x["id"]: pa.row_of(x, f) for x, f in ((b1, f1), (b2, f2), (b3, f3))}
        self.assertTrue(pa.same_row(r["b1"], r["b2"]))
        self.assertFalse(pa.same_row(r["b1"], r["b3"]))
        self.assertFalse(pa.same_row(r["b1"], pa.row_of(*ww("b4", "y4", "wind", 2000090000, valid_from=2000000000))))
        self.assertEqual(pa.alert_label(a2, dict(e2, attributes=dict(e2["attributes"], name="Yellow warning of rain affecting Wales"))),
                         "Yellow rain warning · Wales")

    def test_limit(self):
        alerts = [alert(f"a{i}", f"e{i}", f"x-{i}") for i in range(9)]
        self.assertEqual(len(self.ids(alerts, 3)), 3)


class LabelTest(unittest.TestCase):
    def test_power_cut(self):
        e = {"id": "pc", "kind": "utility-power-outage", "attributes": {"area": "Yorkshire", "customers": 1639}}
        self.assertEqual(pa.alert_label(alert("1", "pc", "power-cut-major"), e), "Power cut · Yorkshire (1639 customers)")
        e = {"id": "pc", "kind": "utility-power-outage", "attributes": {"postcodes": "LS1 2AB; LS2 3CD; LS3", "operator": "NPg"}}
        self.assertEqual(pa.alert_label(alert("1", "pc", "power-cut"), e), "Power cut · LS1 2AB, LS2 3CD")

    def test_names_and_places(self):
        e = {"id": "w", "kind": "event-weather-warning", "attributes": {"name": "Yellow warning of wind affecting South West England"}}
        self.assertEqual(pa.alert_label(alert("1", "w", "weather-warning-yellow"), e), "Weather warning · South West England")
        g = {"id": "g", "kind": "event-indicator", "attributes": {"name": "Gulf watch · Civil aircraft over Iran"}}
        self.assertEqual(pa.alert_label(alert("1", "g", "gulf-watch-elevated"), g), "Gulf watch · Civil aircraft over Iran")
        self.assertEqual(pa.alert_label(alert("1", "cell", "gnss-interference-high", lat=54.25, lon=-3.04), None), "GPS interference · 54.2°N 3.0°W")


class RunTest(Base):
    def test_full_run(self):
        s = self.stub
        s.alerts = [alert("A-1", "power-cut:ukpn-1", "power-cut-major", lat=53.8, lon=-1.5),
                    alert("A-2", "flood:1", "flood-warning", "critical")]
        s.entities = [{"id": "power-cut:ukpn-1", "kind": "utility-power-outage", "attributes": {"area": "Yorkshire", "customers": 1639}}]
        code, out = self.run_main("--alerts", "5", "--timeout", "5")
        self.assertEqual(code, 0, out)
        f = self.file()
        self.assertEqual(set(f), {"version", "recording", "created", "entries", "guide"})
        self.assertEqual((f["version"], f["recording"]), (1, "hour-x"))
        self.assertEqual(os.stat(self.out).st_mode & 0o777, 0o644)
        self.assertIsInstance(f["created"], int)
        qs = pa.examples() + pa.OUR_QUESTIONS
        region = lambda q: "Gulf" if "gulf" in q.lower() else "UK & Ireland"  # noqa: E731
        self.assertEqual(f["guide"]["questions"], [{"question": q, "region": region(q)} for q in qs])
        self.assertEqual(sum(1 for q in qs if region(q) == "Gulf"), 1)
        for q in qs:
            e = f["entries"][pa.shipped_key("ask", "*", q)]
            self.assertIsInstance(e["at"], int)
            self.assertEqual(e["value"]["answer"], "Two power cuts.")
            self.assertEqual(set(e["value"]), {"answer", "citations", "unresolved", "grounded", "inferences", "refusal", "proposed", "considered", "ms"})
        self.assertIn("ask|*|what has changed in the gulf watch?", f["entries"])
        adv = f["entries"]["advise|entity:power-cut:ukpn-1|"]["value"]
        self.assertEqual([o["rank"] for o in adv["options"]], [1, 2])
        self.assertEqual(adv["decisionPoint"], "dp")
        self.assertEqual(adv["options"][0]["expectedEffect"], "e1")
        self.assertEqual(f["guide"]["alerts"][0], {"key": "advise|entity:flood:1|", "label": "Flood · flood:1", "subject": "flood:1", "kind": "flood", "region": None})
        self.assertEqual(f["guide"]["alerts"][1], {"key": "advise|entity:power-cut:ukpn-1|", "label": "Power cut · Yorkshire (1639 customers)",
                                                   "subject": "power-cut:ukpn-1", "kind": "power-cut", "region": "UK & Ireland"})
        # Requests exactly as the console sends them.
        asks = [c for c in s.calls if c[1].endswith("/heimdall/ask")]
        for c in asks:
            q = c[2]["question"][0]
            self.assertEqual(c[2], {"question": [q], "bbox": ["44,12,62,32" if "gulf" in q.lower() else "-12,48,4,62"]})
        self.assertEqual([c[2]["question"][0] for c in asks], qs)
        self.assertTrue(all(c[0] == "POST" and c[4] == b"" for c in asks))
        advises = [c for c in s.calls if c[1].endswith("/advise")]
        self.assertEqual(sorted(c[1] for c in advises), ["/api/v1/heimdall/alerts/A-1/advise", "/api/v1/heimdall/alerts/A-2/advise"])
        self.assertTrue(all(c[2] == {} and c[4] == b"" for c in advises))
        self.assertTrue(any(c[1].endswith("/heimdall/alerts") and c[2] == {"state": ["open"]} for c in s.calls))
        # Summary, and no secrets anywhere.
        self.assertIn("Power cut · Yorkshire (1639 customers)", out)
        self.assertIn("15 chars", out)
        for secret in (PASSWORD, TOKEN):
            self.assertNotIn(secret, out)
            self.assertNotIn(secret, json.dumps(f))
        self.assertEqual(os.listdir(os.path.dirname(self.out)), ["prepared-answers.json"])

    def test_retry_then_success_and_failures_skipped(self):
        s = self.stub
        # first question: 500 then ok; second: empty twice (skipped); the rest ok.
        empty = dict(ASK_REPLY, answer="  ")
        s.ask = [500, ASK_REPLY, empty, empty, ASK_REPLY]
        s.alerts = [alert("A-1", "e1", "flood-warning")]
        s.advise = [dict(ADVISE_REPLY, options=[])]
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        f = self.file()
        qs = pa.examples() + pa.OUR_QUESTIONS
        self.assertEqual([g["question"] for g in f["guide"]["questions"]], [qs[0]] + qs[2:])
        self.assertNotIn(pa.shipped_key("ask", "*", qs[1]), f["entries"])
        self.assertEqual(f["guide"]["alerts"], [])
        self.assertFalse(any(k.startswith("advise|") for k in f["entries"]))
        self.assertIn("empty answer", out)
        self.assertEqual(len([c for c in s.calls if c[1].endswith("/advise")]), 2)  # one retry

    def test_timeout_then_retry(self):
        s = self.stub
        s.ask = [("sleep", 1.5, ASK_REPLY), ASK_REPLY]
        code, out = self.run_main("--timeout", "0.5")
        self.assertEqual(code, 0, out)
        self.assertIn(pa.examples()[0], [g["question"] for g in self.file()["guide"]["questions"]])

    def test_timeout_twice_skips(self):
        s = self.stub
        s.ask = [("sleep", 1.0, ASK_REPLY), ("sleep", 1.0, ASK_REPLY), ASK_REPLY]
        code, out = self.run_main("--timeout", "0.3")
        self.assertEqual(code, 0, out)
        self.assertIn("timed out", out)
        self.assertNotIn(pa.examples()[0], [g["question"] for g in self.file()["guide"]["questions"]])

    def test_nothing_captured_leaves_file(self):
        os.makedirs(os.path.dirname(self.out))
        with open(self.out, "w") as f:
            f.write("old")
        self.stub.ask = [503]
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 1)
        with open(self.out) as f:
            self.assertEqual(f.read(), "old")

    def test_bad_password_fails_without_printing_it(self):
        with open(self.pw, "w") as f:
            f.write("analyst (internal): wrong-one-xyz\n")
        code, out = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("FAIL sign in", out)
        self.assertNotIn("wrong-one-xyz", out)
        self.assertFalse(os.path.exists(self.out))


    def test_401_signs_in_again_once(self):
        self.stub.expire = 1
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        logins = [c for c in self.stub.calls if c[1].endswith("/login")]
        self.assertEqual(len(logins), 2)
        self.assertEqual(len(self.file()["guide"]["questions"]), len(pa.examples()) + len(pa.OUR_QUESTIONS))
        self.assertNotIn(PASSWORD, out)
        self.assertNotIn(TOKEN, out)

    def test_401_twice_fails_that_try_only(self):
        self.stub.expire = 2  # first try: 401, re-login, 401 again → fails; retry succeeds
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        self.assertEqual(len(self.file()["guide"]["questions"]), len(pa.examples()) + len(pa.OUR_QUESTIONS))

    def test_shared_weather_rows_use_entity_details(self):
        s = self.stub
        self.recording(["x1", "x2", "x3"])
        s.alerts = [alert(i, e, "weather-warning-yellow", "info", kind="event-weather-warning", lat=51.0, lon=-1.0)
                    for i, e in (("w1", "x1"), ("w2", "x2"), ("w3", "x3"))]
        # Not eligible (too young): its details are never read.
        s.alerts.append(alert("w9", "x9", "weather-warning-amber", "warning", kind="event-weather-warning", age_s=60, lat=57.0, lon=-4.0))
        s.entities = [{"id": e, "kind": "event-weather-warning", "attributes": {"level": "Yellow", "weather": w, "valid_to": 2000000000,
                                                                               "name": f"Yellow warning of {w} affecting Wales"}}
                      for e, w in (("x1", "wind"), ("x2", "rain"), ("x3", "wind"))]
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        g = self.file()["guide"]["alerts"]
        self.assertEqual([(x["subject"], x["label"]) for x in g], [("x2", "Yellow rain warning · Wales")])
        boxes = [c[2]["bbox"][0] for c in s.calls if c[1].endswith("/heimdall/picture")]
        self.assertEqual(boxes, ["-1.5,50.5,-0.5,51.5"] * 3)  # one read per eligible alert, cached

    def test_only_subjects_seen_all_hour(self):
        self.recording(["e1"], first_only=["e2"])
        self.stub.alerts = [alert("A-1", "e1", "flood-warning"), alert("A-2", "e2", "power-cut-major", "critical"),
                            alert("A-3", "derived-cell", "gnss-interference-high", "critical")]
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        self.assertEqual([g["subject"] for g in self.file()["guide"]["alerts"]], ["e1"])

    def test_recheck_drops_closed_subjects(self):
        s = self.stub
        s.alerts = [alert("A-1", "e1", "flood-warning"), alert("A-2", "e2", "power-cut-major")]
        s.alerts_later = [alert("A-9", "e1", "flood-warning")]  # e1 re-raised under a new id; e2 closed
        code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        f = self.file()
        self.assertEqual([g["subject"] for g in f["guide"]["alerts"]], ["e1"])
        self.assertNotIn("advise|entity:e2|", f["entries"])
        self.assertIn("advise|entity:e1|", f["entries"])
        self.assertIn("no longer open", out)
        s.alert_reads = 0  # without the recheck the later list is never read
        code, out = self.run_main("--timeout", "5", "--no-recheck")
        self.assertEqual(len(self.file()["guide"]["alerts"]), 2)

    def test_one_bad_alert_does_not_lose_the_run(self):
        self.stub.alerts = [alert("A-1", "e1", "flood-warning"), alert("A-2", "e2", "power-cut-major")]
        real = pa.alert_label
        with mock.patch.object(pa, "alert_label", side_effect=lambda a, e: (_ for _ in ()).throw(RuntimeError("bad")) if a["id"] == "A-1" else real(a, e)):
            code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        self.assertIn("FAIL advise 'A-1'", out)
        self.assertEqual([g["subject"] for g in self.file()["guide"]["alerts"]], ["e2"])

    def test_written_as_it_goes(self):
        self.stub.alerts = [alert("A-1", "e1", "flood-warning")]
        sizes = []
        real = pa.write_atomic
        with mock.patch.object(pa, "write_atomic", side_effect=lambda out, doc: (sizes.append(len(doc["entries"])), real(out, doc))):
            code, out = self.run_main("--timeout", "5")
        self.assertEqual(code, 0, out)
        n = len(pa.examples()) + len(pa.OUR_QUESTIONS)
        self.assertEqual(sizes, list(range(1, n + 2)) + [n + 1])  # each answer, then the recheck
        self.assertEqual([x for x in os.listdir(os.path.dirname(self.out))], ["prepared-answers.json"])

    def test_missing_recording_fails_before_signing_in(self):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = pa.main(["--base", self.stub.base, "--out", self.out, "--passwords", self.pw,
                            "--recording", os.path.join(self.dir.name, "nope")])
        self.assertEqual(code, 1)
        self.assertIn("FAIL recording", buf.getvalue())
        self.assertEqual(self.stub.calls, [])


class HelpersTest(unittest.TestCase):
    def test_ts_str(self):
        self.assertEqual([pa.ts_str(v) for v in (None, True, False, 3, 2.0, 2.5, "x")], ["", "true", "false", "3", "2", "2.5", "x"])
        self.assertEqual(pa.advice_of({"subject": None, "decision_point": False, "options": [{"title": 7, "summary": None}]})["decisionPoint"], "false")
        self.assertEqual(pa.advice_of({"options": [{"title": 7.0}]})["options"][0]["title"], "7")

    def test_regions_from_source(self):
        regs = {r["id"]: r for r in pa.regions()}
        self.assertEqual((regs["uk"]["label"], pa.bbox_param(regs["uk"]["box"])), ("UK & Ireland", "bbox=-12,48,4,62"))
        self.assertEqual((regs["gulf"]["label"], pa.bbox_param(regs["gulf"]["box"])), ("Gulf", "bbox=44,12,62,32"))
        self.assertEqual(pa.bbox_param(regs["balkans"]["box"]), "bbox=13,34,30,48.5")
        self.assertEqual(pa.region_at(pa.regions(), 26.5, 56.2)["id"], "gulf")
        self.assertIsNone(pa.region_at(pa.regions(), None, 1))
        self.assertEqual(pa.region_of_question("What has changed in the GULF watch?", pa.regions())["id"], "gulf")

    def test_seen_all_hour_and_recording_lookup(self):
        with tempfile.TemporaryDirectory() as d:
            rec = os.path.join(d, "hour-2")
            os.makedirs(rec)
            with gzip.open(os.path.join(rec, "heimdall.jsonl.gz"), "wt") as f:
                for off, ents in ((0, ["a", "b", "c"]), (599, ["d"]), (600, ["a", "e"]), (3000, ["a", "b", "d"]), (3599, ["c"])):
                    f.write(json.dumps({"offset": off, "feed": "x", "observations": [{"entity": e} for e in ents]}) + "\n")
            with open(os.path.join(rec, "meta.json"), "w") as f:
                json.dump({"duration": 3600}, f)
            path = os.path.join(rec, "heimdall.jsonl.gz")
            self.assertEqual(pa.seen_all_hour(path), {"a", "b", "c", "d"})
            self.assertEqual(pa.recording_file(rec), ("hour-2", path))
            self.assertEqual(pa.recording_file(rec + "/"), ("hour-2", path))
            self.assertEqual(pa.recording_file(path), ("hour-2", path))
            with self.assertRaises(ValueError):
                pa.recording_file(os.path.join(d, "missing"))


if __name__ == "__main__":
    unittest.main()
