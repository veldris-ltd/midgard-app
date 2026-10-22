#!/usr/bin/env python3
"""Capture real Mimir answers for the Mac app's prepared answers (phase 4, Task 4).

    python3 tools/mac-app/prepare-answers.py --base http://127.0.0.1:7710/api/v1 \\
        --out apps/mac/Resources/home/prepared-answers.json \\
        [--recording hour-1] [--alerts 5] [--timeout 300]

Run it against the native suite replaying the recorded hour, after at least ten
minutes of replay (so alerts exist). It signs in as analyst (password from
~/Desktop/Midgard/airports-passwords.txt; neither it nor the session token is ever
printed), then asks Mimir exactly what Heimdall's console asks by default:

- Map questions: each of AskCard.tsx's EXAMPLES (read from the source, so they stay in
  step) and two of ours, as `POST /heimdall/ask?question=…&bbox=-12,48,4,62` (the
  console's default region, UK & Ireland, in the 2D view: no focus).
- Decision support: up to --alerts open alerts raised at least ten minutes ago, on a
  subject with no other open alert (shipped advice is keyed by subject), most
  severe first, one per kind (power cut, flood, weather warning, GNSS interference,
  Gulf watch, airspace) before any second of a kind, each as
  `POST /heimdall/alerts/{id}/advise` with the default (empty) question.

Each answer gets --timeout seconds and one retry; a failure or an empty answer is
skipped and reported, never stored. Values are stored in the console's shapes
(`HeimdallAnswer`, `HeimdallAdvice`, as sdk/ts/src/products/heimdall-http.ts builds
them from the wire), under the console's `shippedKey` keys (questions must be ASCII; a start-up self-check
compares the keys with preparedAnswers.test.ts's vectors). The file (version 1, see
products/heimdall/ui/src/app/preparedAnswers.ts) is written atomically. Exit 1 if
nothing could be captured (the output file is then left as it was).
"""
import argparse
import json
import math
import os
import re
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
ASK_CARD = os.path.join(REPO, "products", "heimdall", "ui", "src", "app", "AskCard.tsx")
PW_FILE = os.path.expanduser("~/Desktop/Midgard/airports-passwords.txt")

# Ours, besides AskCard's examples.
OUR_QUESTIONS = [
    "Is there anything unusual in the Gulf watch?",
    "Where are aircraft or ships behaving unusually?",
]
# The console's default region (regions.ts: 'uk', UK & Ireland), [west, south, east, north].
UK_BOX = [-12, 48, 4, 62]
MIN_OPEN_S = 10 * 60
SEVERITY_RANK = {"critical": 0, "warning": 1, "info": 2}

# Alert label prefix → (kind for the guide, its name in words).
CATEGORIES = [
    ("power-cut", "power-cut", "Power cut"),
    ("flood-", "flood", "Flood"),
    ("weather-warning-", "weather-warning", "Weather warning"),
    ("gnss-", "gnss", "GNSS interference"),
    ("gulf-", "gulf-watch", "Gulf watch"),
    ("conflict-zone-", "airspace", "Airspace"),
    ("faa-restriction", "airspace", "Airspace"),
    ("sigmet-", "airspace", "Airspace"),
]


# ---- the console's rules, mirrored -------------------------------------------------

def normalise(question):
    """preparedAnswers.ts `normalise`: trimmed, lower-cased, whitespace runs one space."""
    return re.sub(r"\s+", " ", question.strip().lower())


# preparedAnswers.test.ts's vectors: (kind, about, question) → key. Checked at start.
TS_VECTORS = [
    ("prepared", "ask", "UK", "  What   has\tchanged?\n", "ask|UK|what has changed?"),
    ("shipped", "ask", "anything", "What has  CHANGED?", "ask|*|what has changed?"),
    ("shipped", "advise", "power-cut:ukpn-123", "", "advise|entity:power-cut:ukpn-123|"),
    ("shipped", "advise", "e1", " Why? ", "advise|entity:e1|why?"),
]


def self_check(questions):
    """The keys must equal the console's: the TS test vectors give the TS keys, and
    every question is ASCII (where Python's and JavaScript's trim, lower-case and
    whitespace rules agree exactly) and already as ASCII-normalised as TS would."""
    for which, kind, about, q, want in TS_VECTORS:
        got = (prepared_key if which == "prepared" else shipped_key)(kind, about, q)
        if got != want:
            raise AssertionError(f"key normalisation differs from the console: {got!r} != {want!r}")
    for q in questions:
        if not q.isascii():
            raise ValueError(f"question is not ASCII (keys might differ from the console's): {q!r}")
        # The TS rule spelled out for ASCII: trim " \t\n\v\f\r", A-Z → a-z, runs → " ".
        ts = re.sub(r"[ \t\n\v\f\r]+", " ", q.strip(" \t\n\v\f\r").translate(str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")))
        if normalise(q) != ts:
            raise AssertionError(f"key normalisation differs from the console for {q!r}")


def prepared_key(kind, about, question):
    return f"{kind}|{about}|{normalise(question)}"


def shipped_key(kind, about, question):
    """preparedAnswers.ts `shippedKey`."""
    return prepared_key("ask", "*", question) if kind == "ask" else prepared_key("advise", f"entity:{about}", question)


def js_num(v):
    """`+v.toFixed(3)` as JavaScript prints it: -12 → "-12", 48.5 → "48.5"."""
    r = round(float(v), 3)
    return str(int(r)) if r == int(r) else repr(r)


def encode_component(s):
    """encodeURIComponent."""
    return urllib.parse.quote(s, safe="-_.!~*'()")


def examples(path=ASK_CARD):
    """The EXAMPLES array of AskCard.tsx, as written there."""
    with open(path, encoding="utf-8") as f:
        src = f.read()
    m = re.search(r"const EXAMPLES\s*=\s*\[(.*?)\];", src, re.S)
    if not m:
        raise ValueError(f"no EXAMPLES array in {path}")
    out = []
    for q, body in re.findall(r"""(['"])((?:\\.|(?!\1).)*)\1""", m.group(1)):
        out.append(re.sub(r"\\(.)", r"\1", body))
    if not out:
        raise ValueError(f"EXAMPLES in {path} is empty")
    return out


def strs(v):
    return [x for x in v if isinstance(x, str)] if isinstance(v, list) else []


def num(v):
    return v if isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) else 0


def answer_of(b):
    """heimdall-http.ts `ask`: the wire reply as a HeimdallAnswer."""
    b = b if isinstance(b, dict) else {}
    r = b.get("refusal")
    return {
        "answer": b["answer"] if isinstance(b.get("answer"), str) else None,
        "citations": strs(b.get("citations")), "unresolved": strs(b.get("unresolved")),
        "grounded": b.get("grounded") is True,
        "inferences": strs(b.get("inferences")), "proposed": strs(b.get("proposed")),
        "refusal": {"code": r["code"], "detail": str(r.get("detail") or "")} if isinstance(r, dict) and isinstance(r.get("code"), str) else None,
        "considered": num(b.get("considered")), "ms": num(b.get("ms")),
    }


def advice_of(b):
    """heimdall-http.ts `advise`: the wire reply as a HeimdallAdvice."""
    b = b if isinstance(b, dict) else {}
    opts = []
    for o in b.get("options") if isinstance(b.get("options"), list) else []:
        o = o if isinstance(o, dict) else {}
        opts.append({
            "rank": num(o.get("rank")), "title": str(o.get("title") or ""), "summary": str(o.get("summary") or ""),
            "expectedEffect": str(o.get("expected_effect") or ""), "risks": strs(o.get("risks")),
            "citations": strs(o.get("citations")), "grounded": o.get("grounded") is True, "unresolved": strs(o.get("unresolved")),
        })
    opts.sort(key=lambda o: o["rank"])
    return {
        "subject": str(b.get("subject") or ""), "decisionPoint": str(b.get("decision_point") or ""),
        "considered": num(b.get("considered")), "options": opts,
        "unsupported": b["unsupported"] if isinstance(b.get("unsupported"), str) else None,
        "ms": num(b.get("ms")),
    }


def normalise_alert(a):
    """heimdall-http.ts `normaliseAlert`: seconds, and camera analytics on the camera."""
    def secs(v):
        if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
            return None
        return round(v / 1000) if v > 1e12 else v
    a = dict(a)
    if a.get("kind") == "land-camera-ai" and isinstance(a.get("entity"), str) and a["entity"].startswith("AI-"):
        a["entity"], a["kind"] = a["entity"][3:], "land-camera"
    a["raisedAt"] = secs(a.get("raisedAt")) or 0
    a["clearedAt"] = secs(a.get("clearedAt"))
    return a


def category(label):
    for prefix, kind, words in CATEGORIES:
        if label.startswith(prefix):
            return kind, words
    return label, label.replace("-", " ").replace("_", " ").capitalize()


def select_alerts(alerts, n, now):
    """Up to n open alerts raised at least MIN_OPEN_S ago: most severe first, then the
    longest open; one per kind before any second of a kind. Shipped advice is keyed by
    subject, so a subject with more than one open alert is skipped (its advice would
    stand for whichever alert is clicked)."""
    alerts = [a for a in map(normalise_alert, alerts) if isinstance(a, dict)]
    per_subject = {}
    for a in alerts:
        if a.get("clearedAt") is None and isinstance(a.get("entity"), str):
            per_subject[a["entity"]] = per_subject.get(a["entity"], 0) + 1
    ok = [a for a in alerts
          if isinstance(a.get("id"), str) and isinstance(a.get("entity"), str) and a["entity"]
          and a.get("clearedAt") is None and 0 < a["raisedAt"] <= now - MIN_OPEN_S
          and per_subject.get(a["entity"]) == 1]
    ok.sort(key=lambda a: (SEVERITY_RANK.get(a.get("severity"), 3), a["raisedAt"], a["id"]))
    picked, kinds, subjects = [], set(), set()
    for distinct in (True, False):
        for a in ok:
            if len(picked) >= n:
                return picked
            k = category(str(a.get("label", "")))[0]
            if a["entity"] in subjects or (distinct and k in kinds):
                continue
            picked.append(a)
            kinds.add(k)
            subjects.add(a["entity"])
    return picked


def ll(lat, lon):
    return f"{abs(lat):.1f}°{'N' if lat >= 0 else 'S'} {abs(lon):.1f}°{'E' if lon >= 0 else 'W'}"


def alert_label(a, entity):
    """What the console's alerts panel shows: the kind in words and where (alertName,
    AlertsPanel.placeOf), e.g. "Power cut · Yorkshire (1639 customers)"."""
    _, words = category(str(a.get("label", "")))
    attrs = (entity or {}).get("attributes") or {}

    def text(k):
        v = attrs.get(k)
        return v if isinstance(v, str) and v else str(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else None

    if (entity or {}).get("kind") == "utility-power-outage":
        postcodes = text("postcodes")
        where = text("area") or (", ".join(re.split(r"[;,]\s*", postcodes)[:2]) if postcodes else None) or text("operator")
        if where:
            c = text("customers")
            return f"{words} · {where}" + (f" ({c} customers)" if c else "")
    name = None
    if entity:
        name = text("name") or text("place") or text("area") or text("title") or text("location")
    if name:
        name = re.sub(r"^Gulf watch · ", "", name)
        m = re.search(r"affecting (.+)$", name, re.I)
        place = (m.group(1) if m else name).strip()
    elif isinstance(a.get("lat"), (int, float)) and isinstance(a.get("lon"), (int, float)):
        place = ll(a["lat"], a["lon"])
    else:
        place = a["entity"]
    return f"{words} · {place}"


# ---- the gateway -------------------------------------------------------------------

def reason(e):
    """A short, secret-free reason for a failed call."""
    if isinstance(e, urllib.error.HTTPError):
        msg = ""
        try:
            j = json.loads(e.read().decode("utf-8", "replace"))
            msg = (j.get("error") or "") if isinstance(j, dict) else ""
        except Exception:
            pass
        return f"HTTP {e.code}" + (f": {str(msg).strip()[:120]}" if msg else "")
    if isinstance(e, urllib.error.URLError):
        return f"unreachable: {e.reason}"
    if isinstance(e, (TimeoutError, OSError)) and "timed out" in str(e):
        return "timed out"
    return f"{type(e).__name__}: {str(e)[:120]}"


class Gateway:
    def __init__(self, base, timeout):
        self.base, self.timeout, self.token = base.rstrip("/"), timeout, None

    def call(self, method, path, body=None, timeout=None):
        headers = {"content-type": "application/json"}
        if self.token:
            headers["authorization"] = f"Bearer {self.token}"
        req = urllib.request.Request(self.base + path, method=method, headers=headers,
                                     data=json.dumps(body).encode() if body is not None else None)
        with urllib.request.urlopen(req, timeout=timeout or self.timeout) as r:
            return json.load(r)

    def sign_in(self, pw_file):
        with open(pw_file, encoding="utf-8") as f:
            m = re.search(r"^analyst \(internal\): (\S+)", f.read(), re.M)
        if not m:
            raise ValueError("no 'analyst (internal):' line in the passwords file")
        tok = self.call("POST", "/login", {"username": "analyst", "password": m.group(1)}, timeout=60).get("token")
        if not tok:
            raise ValueError("no token in the sign-in response")
        self.token = tok


def attempt(fn, done, tries=2):
    """fn() → value, accepted when done(value); one retry. Returns (value, error)."""
    err = None
    for _ in range(tries):
        try:
            v = fn()
            if done(v):
                return v, None
            err = "empty answer"
        except Exception as e:  # noqa: BLE001 — every failure is reported, never raised
            err = reason(e)
    return None, err


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--base", default="http://127.0.0.1:7710/api/v1")
    p.add_argument("--out", required=True)
    p.add_argument("--recording", default="hour-1")
    p.add_argument("--alerts", type=int, default=5)
    p.add_argument("--timeout", type=float, default=300, help="seconds per answer (default 300)")
    p.add_argument("--passwords", default=PW_FILE, help=argparse.SUPPRESS)
    p.add_argument("--ask-card", default=ASK_CARD, help=argparse.SUPPRESS)
    args = p.parse_args(argv)

    all_questions = examples(args.ask_card) + OUR_QUESTIONS
    self_check(all_questions)
    gw = Gateway(args.base, args.timeout)
    try:
        gw.sign_in(args.passwords)
    except Exception as e:
        print(f"FAIL sign in ({reason(e)})")
        return 1
    print("ok   sign in as analyst")

    entries, questions, guide_alerts, failed = {}, [], [], []
    now_ms = lambda: int(time.time() * 1000)  # noqa: E731
    bbox = "bbox=" + ",".join(js_num(v) for v in UK_BOX)

    # Map questions, as AskCard asks them with the default region on.
    for q in all_questions:
        t0 = time.monotonic()
        path = f"/heimdall/ask?question={encode_component(q.strip())}&{bbox}"
        v, err = attempt(lambda: answer_of(gw.call("POST", path)), lambda a: bool((a["answer"] or "").strip()))
        secs = time.monotonic() - t0
        if v is None:
            print(f"FAIL ask    {q!r}: {err} ({secs:.0f} s)")
            failed.append(q)
            continue
        entries[shipped_key("ask", "*", q)] = {"value": v, "at": now_ms()}
        questions.append(q)
        print(f"ok   ask    {q!r}: {len(v['answer'])} chars, {secs:.0f} s")

    # Decision support on alerts that have stayed open.
    try:
        body = gw.call("GET", "/heimdall/alerts?state=open", timeout=60)
        alerts = body.get("alerts") if isinstance(body, dict) and isinstance(body.get("alerts"), list) else None
        if alerts is None:
            raise ValueError("unexpected reply")
    except Exception as e:
        print(f"FAIL alerts ({reason(e)})")
        alerts = []
    picked = select_alerts(alerts, args.alerts, time.time())
    if len(picked) < args.alerts:
        print(f"note {len(picked)} of {args.alerts} alerts found open for at least {MIN_OPEN_S // 60} min")
    for a in picked:
        entity = None
        if isinstance(a.get("lat"), (int, float)) and isinstance(a.get("lon"), (int, float)):
            box = ",".join(js_num(x) for x in (a["lon"] - 0.5, a["lat"] - 0.5, a["lon"] + 0.5, a["lat"] + 0.5))
            try:
                pic = gw.call("GET", f"/heimdall/picture?bbox={box}", timeout=60)
                entity = next((e for e in pic.get("entities", []) if isinstance(e, dict) and e.get("id") == a["entity"]), None)
            except Exception:
                entity = None  # labelled from the alert alone
        label = alert_label(a, entity)
        t0 = time.monotonic()
        path = f"/heimdall/alerts/{encode_component(a['id'])}/advise"
        v, err = attempt(lambda: advice_of(gw.call("POST", path)), lambda d: bool(d["options"]))
        secs = time.monotonic() - t0
        if v is None:
            print(f"FAIL advise {label} [{a['severity']}]: {err} ({secs:.0f} s)")
            failed.append(label)
            continue
        key = shipped_key("advise", a["entity"], "")
        entries[key] = {"value": v, "at": now_ms()}
        guide_alerts.append({"key": key, "label": label, "subject": a["entity"], "kind": category(str(a.get("label", "")))[0]})
        size = sum(len(o["title"]) + len(o["summary"]) for o in v["options"])
        print(f"ok   advise {label} [{a['severity']}]: {len(v['options'])} options, {size} chars, {secs:.0f} s")

    if not entries:
        print("FAIL nothing captured; output left as it was")
        return 1
    doc = {"version": 1, "recording": args.recording, "created": now_ms(), "entries": entries,
           "guide": {"questions": questions, "alerts": guide_alerts}}
    out = os.path.abspath(args.out)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".prepared-", suffix=".json", dir=os.path.dirname(out))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(doc, f, ensure_ascii=False, indent=1)
            f.write("\n")
        os.replace(tmp, out)
    except BaseException:
        os.unlink(tmp)
        raise
    print(f"wrote {out}: {len(questions)} questions, {len(guide_alerts)} alerts"
          + (f"; {len(failed)} skipped" if failed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
