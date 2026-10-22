"""Tests for prepare-answers.py against a stub gateway (login, ask, alerts, picture,
advise) on a free local port: keys, the file format, normalisation (the console's
vectors), alert selection, retries and timeouts, and that no secret is printed.

    python3 -m unittest tools/mac-app/test_prepare_answers.py -v
"""
import contextlib
import importlib.util
import io
import json
import os
import tempfile
import threading
import time
import unittest
import urllib.parse
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
                if self.headers.get("authorization") != f"Bearer {TOKEN}":
                    return self.reply(401, {"error": "auth"})
                if method == "POST" and p == "/heimdall/ask":
                    return self.behave(stub.ask)
                if method == "GET" and p == "/heimdall/alerts":
                    return self.reply(200, {"alerts": stub.alerts})
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

    def run_main(self, *extra):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = pa.main(["--base", self.stub.base, "--out", self.out, "--passwords", self.pw, *extra])
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

    def test_js_num_and_encoding(self):
        self.assertEqual([pa.js_num(v) for v in pa.UK_BOX], ["-12", "48", "4", "62"])
        self.assertEqual(pa.js_num(48.12345), "48.123")
        self.assertEqual(pa.encode_component("What's near here? a&b"), "What's%20near%20here%3F%20a%26b")


class SelectTest(unittest.TestCase):
    def ids(self, alerts, n=5):
        return [a["id"] for a in pa.select_alerts(alerts, n, NOW)]

    def test_severity_first_then_distinct_kinds(self):
        alerts = [
            alert("i1", "e-i1", "power-cut", "info"),
            alert("w1", "e-w1", "power-cut-major", "warning"),
            alert("w2", "e-w2", "power-cut-major", "warning", age_s=7200),
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
        got = self.ids([alert("a", "ea", "power-cut-major"), alert("b", "eb", "power-cut-major")])
        self.assertEqual(sorted(got), ["a", "b"])

    def test_young_cleared_and_shared_subjects_skipped(self):
        alerts = [
            alert("young", "e1", "flood-warning", age_s=300),
            alert("cleared", "e2", "flood-warning", cleared=int(NOW)),
            alert("twin1", "e3", "flood-warning"),
            alert("twin2", "e3", "weather-warning-amber"),
            alert("ok", "e4", "flood-warning"),
        ]
        self.assertEqual(self.ids(alerts), ["ok"])

    def test_ms_times_and_camera_alerts(self):
        a = alert("cam", "AI-cam7", "camera-wrong-way", kind="land-camera-ai")
        a["raisedAt"] = int((NOW - 3600) * 1000)
        picked = pa.select_alerts([a], 5, NOW)
        self.assertEqual(picked[0]["entity"], "cam7")

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
        self.assertEqual(pa.alert_label(alert("1", "cell", "gnss-interference-high", lat=54.25, lon=-3.04), None), "GNSS interference · 54.2°N 3.0°W")


class RunTest(Base):
    def test_full_run(self):
        s = self.stub
        s.alerts = [alert("A-1", "power-cut:ukpn-1", "power-cut-major", lat=53.8, lon=-1.5),
                    alert("A-2", "flood:1", "flood-warning", "critical")]
        s.entities = [{"id": "power-cut:ukpn-1", "kind": "utility-power-outage", "attributes": {"area": "Yorkshire", "customers": 1639}}]
        code, out = self.run_main("--recording", "hour-9", "--alerts", "5", "--timeout", "5")
        self.assertEqual(code, 0, out)
        f = self.file()
        self.assertEqual(set(f), {"version", "recording", "created", "entries", "guide"})
        self.assertEqual((f["version"], f["recording"]), (1, "hour-9"))
        self.assertIsInstance(f["created"], int)
        qs = pa.examples() + pa.OUR_QUESTIONS
        self.assertEqual(f["guide"]["questions"], qs)
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
        self.assertEqual(f["guide"]["alerts"][0], {"key": "advise|entity:flood:1|", "label": "Flood · flood:1", "subject": "flood:1", "kind": "flood"})
        self.assertEqual(f["guide"]["alerts"][1], {"key": "advise|entity:power-cut:ukpn-1|", "label": "Power cut · Yorkshire (1639 customers)",
                                                   "subject": "power-cut:ukpn-1", "kind": "power-cut"})
        # Requests exactly as the console sends them.
        asks = [c for c in s.calls if c[1].endswith("/heimdall/ask")]
        self.assertEqual(asks[0][2], {"question": [qs[0]], "bbox": ["-12,48,4,62"]})
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
        self.assertEqual(f["guide"]["questions"], [qs[0]] + qs[2:])
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
        self.assertIn(pa.examples()[0], self.file()["guide"]["questions"])

    def test_timeout_twice_skips(self):
        s = self.stub
        s.ask = [("sleep", 1.0, ASK_REPLY), ("sleep", 1.0, ASK_REPLY), ASK_REPLY]
        code, out = self.run_main("--timeout", "0.3")
        self.assertEqual(code, 0, out)
        self.assertIn("timed out", out)
        self.assertNotIn(pa.examples()[0], self.file()["guide"]["questions"])

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


if __name__ == "__main__":
    unittest.main()
