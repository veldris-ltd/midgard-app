#!/usr/bin/env python3
"""Capture real Mimir answers for the Mac app's prepared answers (phase 4, Task 4).

    python3 tools/mac-app/prepare-answers.py --base http://127.0.0.1:7710/api/v1 \\
        --out apps/mac/Resources/home/prepared-answers.json \\
        [--recording hour-1|DIR] [--alerts 5] [--timeout 300] [--no-recheck]

Run it against the native suite replaying the recorded hour, after at least ten
minutes of replay (so alerts exist). It signs in as analyst (password from
~/Desktop/Midgard/airports-passwords.txt; neither it nor the session token is ever
printed; a 401 mid-run signs in again once and retries), then asks Mimir exactly what
Heimdall's console asks:

- Map questions: each of AskCard.tsx's EXAMPLES (read from the source, so they stay in
  step) and two of ours, as `POST /heimdall/ask?question=…&bbox=…` with no focus (the
  2D view). Each is asked over the region it is about, with that region's box exactly
  as the console sends it (regions.ts, `+v.toFixed(3)`): a question that names the
  Gulf over the 'gulf' region (`bbox=44,12,62,32`), everything else over the default
  'uk' region (`bbox=-12,48,4,62`). Over the UK box Mimir would be shown no Gulf
  evidence at all. The shipped key ignores the area (`ask|*|…`), so the guide says
  which region to switch on: each `guide.questions` entry is
  `{"question", "region"}` (the region's label, e.g. "UK & Ireland", "Gulf").
- Decision support: up to --alerts open alerts, each as
  `POST /heimdall/alerts/{id}/advise` with the default (empty) question. An alert is
  eligible when it is open, was raised at least ten minutes ago, its subject has no
  other open alert (shipped advice is keyed by subject), and its subject has
  observations in both the first and the last ten minutes of the recording (by batch
  offset in `heimdall.jsonl.gz`), so it is in the picture all hour. Most severe first,
  then the longest open; one per kind (power cut, flood, weather warning, GPS
  interference, Gulf watch, airspace) before any second of a kind. With --recheck (the
  default) the open alerts are read again at the end and any subject no longer open is
  dropped. An alert that would share its row in the console's ALERTS IN VIEW panel
  (same severity and text; a click there selects only the newest) with another open
  alert is skipped; weather warnings are compared by severity, level, weather and
  valid-to time, and one whose details are not read (only eligible alerts' are)
  counts as possibly the same row. Each `guide.alerts` entry carries the `region` containing the alert (Gulf
  watch alerts are clickable only with the Gulf region on).
  Remaining limit: being observed all hour and open at the start and end of the
  capture does not prove the alert stays open through every replay loop (Heimdall may
  clear and re-raise it, under a new id; the shipped key is by subject, so that is
  harmless), and a subject Heimdall derives itself (not in the recording, e.g. a GPS
  interference cell computed from aircraft) is never picked. Panel rows are judged at
  capture time: alerts raised later may still come to share a picked alert's row.

`--recording` is the packed recording: a directory holding `heimdall.jsonl.gz` (and
`meta.json`), that file itself, or a name under
~/Library/Application Support/Midgard-dev/recordings; its folder name is written as
the file's `recording`.

Each answer gets up to two tries of --timeout seconds each (default 300 s per try);
a failure or an empty answer is skipped and reported, never stored. Values are stored
in the console's shapes (`HeimdallAnswer`, `HeimdallAdvice`, as
sdk/ts/src/products/heimdall-http.ts builds them from the wire, strings coerced as
JavaScript's `String` would), under the console's `shippedKey` keys (questions must be
ASCII; a start-up self-check compares the keys with preparedAnswers.test.ts's vectors).
The file (version 1, see products/heimdall/ui/src/app/preparedAnswers.ts) is
rewritten atomically (temp file, fsync, mode 0644, rename) after every answer, so an
interrupted run keeps what it had. Exit 1 if nothing could be captured (the output
file is then left as it was).
"""
import argparse
import contextlib
import gzip
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
REGIONS_TS = os.path.join(REPO, "products", "heimdall", "ui", "src", "app", "regions.ts")
PW_FILE = os.path.expanduser("~/Desktop/Midgard/airports-passwords.txt")
RECORDINGS = os.path.expanduser("~/Library/Application Support/Midgard-dev/recordings")

# Ours, besides AskCard's examples.
OUR_QUESTIONS = [
    "Is there anything unusual in the Gulf watch?",
    "Where are aircraft or ships behaving unusually?",
]
# The console's default region (regions.ts 'uk'), and the one Gulf questions are about.
DEFAULT_REGION, GULF_REGION = "uk", "gulf"
MIN_OPEN_S = 10 * 60
# The recording's first and last this many seconds: a subject must be seen in both.
WINDOW_S = 10 * 60
SEVERITY_RANK = {"critical": 0, "warning": 1, "info": 2}

# Alert label prefix → (kind for the guide, its name in words).
CATEGORIES = [
    ("power-cut", "power-cut", "Power cut"),
    ("flood-", "flood", "Flood"),
    ("weather-warning-", "weather-warning", "Weather warning"),
    ("gnss-", "gnss", "GPS interference"),
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


def regions(path=REGIONS_TS):
    """regions.ts REGION_LIST: [{id, label, box: [w, s, e, n]}], in its order."""
    with open(path, encoding="utf-8") as f:
        src = f.read()
    out = [{"id": i, "label": label, "box": [float(v) for v in box.split(",")]}
           for i, label, box in re.findall(r"\{\s*id:\s*'([^']+)',\s*label:\s*'([^']+)',\s*box:\s*\[([^\]]+)\]", src)]
    if not out:
        raise ValueError(f"no REGION_LIST in {path}")
    return out


def bbox_param(box):
    """heimdall-http.ts regionParams: `bbox=` + each `+v.toFixed(3)`, comma-joined."""
    return "bbox=" + ",".join(js_num(v) for v in box)


def region_of_question(q, regs):
    """The region a question is asked over: the Gulf for one that names it, else the
    console's default."""
    want = GULF_REGION if "gulf" in normalise(q) else DEFAULT_REGION
    return next(r for r in regs if r["id"] == want)


def region_at(regs, lat, lon):
    """The first region whose box holds the point (regions.ts inRegionBox), or None."""
    if not all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in (lat, lon)):
        return None
    for r in regs:
        w, s, e, n = r["box"]
        if s <= lat <= n and w <= lon <= e:
            return r
    return None


def recording_file(arg):
    """(name, path of heimdall.jsonl.gz) for --recording, or raise ValueError."""
    for cand in (arg, os.path.join(RECORDINGS, arg)):
        if os.path.isfile(cand) and cand.endswith(".jsonl.gz"):
            return os.path.basename(os.path.dirname(os.path.abspath(cand))), cand
        if os.path.isfile(os.path.join(cand, "heimdall.jsonl.gz")):
            return os.path.basename(os.path.abspath(cand).rstrip("/")), os.path.join(cand, "heimdall.jsonl.gz")
    raise ValueError(f"no packed recording (heimdall.jsonl.gz) at {arg!r} or in {RECORDINGS}")


def seen_all_hour(path):
    """Entities with observations in both the first and the last WINDOW_S seconds of
    the recording (batch offsets; the length from meta.json, else the last offset)."""
    first, last, batches = set(), set(), []
    with gzip.open(path, "rt", encoding="utf-8") as f:
        for line in f:
            if line.strip():
                b = json.loads(line)
                batches.append((int(b.get("offset", 0)), {o.get("entity") for o in b.get("observations", []) if isinstance(o, dict)}))
    duration = max((o for o, _ in batches), default=0) + 1
    try:
        with open(os.path.join(os.path.dirname(path), "meta.json"), encoding="utf-8") as f:
            duration = max(duration, int(json.load(f).get("duration", 0)))
    except (OSError, ValueError):
        pass
    for offset, ents in batches:
        if offset < WINDOW_S:
            first |= ents
        if offset >= duration - WINDOW_S:
            last |= ents
    return (first & last) - {None}


def strs(v):
    return [x for x in v if isinstance(x, str)] if isinstance(v, list) else []


def ts_str(v):
    """JavaScript's `String(v ?? '')` for JSON values."""
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, float):
        return js_num_full(v)
    if isinstance(v, (list, dict)):
        return json.dumps(v)
    return str(v)


def js_num_full(v):
    if not math.isfinite(v):
        return "NaN" if v != v else ("Infinity" if v > 0 else "-Infinity")
    return str(int(v)) if v == int(v) and abs(v) < 1e21 else repr(v)


def num(v):
    """`Number(v ?? 0)`, a non-number as 0 (JSON cannot hold NaN)."""
    if isinstance(v, bool):
        return int(v)
    if isinstance(v, (int, float)):
        return v if math.isfinite(v) else 0
    if isinstance(v, str):
        try:
            f = float(v.strip() or 0)
            return (int(f) if f == int(f) else f) if math.isfinite(f) else 0
        except ValueError:
            return 0
    return 0


def answer_of(b):
    """heimdall-http.ts `ask`: the wire reply as a HeimdallAnswer."""
    b = b if isinstance(b, dict) else {}
    r = b.get("refusal")
    return {
        "answer": b["answer"] if isinstance(b.get("answer"), str) else None,
        "citations": strs(b.get("citations")), "unresolved": strs(b.get("unresolved")),
        "grounded": b.get("grounded") is True,
        "inferences": strs(b.get("inferences")), "proposed": strs(b.get("proposed")),
        "refusal": {"code": r["code"], "detail": ts_str(r.get("detail"))} if isinstance(r, dict) and isinstance(r.get("code"), str) else None,
        "considered": num(b.get("considered")), "ms": num(b.get("ms")),
    }


def advice_of(b):
    """heimdall-http.ts `advise`: the wire reply as a HeimdallAdvice."""
    b = b if isinstance(b, dict) else {}
    opts = []
    for o in b.get("options") if isinstance(b.get("options"), list) else []:
        o = o if isinstance(o, dict) else {}
        opts.append({
            "rank": num(o.get("rank")), "title": ts_str(o.get("title")), "summary": ts_str(o.get("summary")),
            "expectedEffect": ts_str(o.get("expected_effect")), "risks": strs(o.get("risks")),
            "citations": strs(o.get("citations")), "grounded": o.get("grounded") is True, "unresolved": strs(o.get("unresolved")),
        })
    opts.sort(key=lambda o: o["rank"])
    return {
        "subject": ts_str(b.get("subject")), "decisionPoint": ts_str(b.get("decision_point")),
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


def alert_text(label):
    """alertMarks.ts alertText."""
    return re.sub(r"[-_]+", " ", label).upper()


def weather_of(entity):
    """A weather warning entity's (level, weather, valid-to minute), upper-cased as its
    panel row shows them (warningText), or None for any other entity."""
    if (entity or {}).get("kind") != "event-weather-warning":
        return None
    attrs = entity.get("attributes") or {}
    vt = attrs.get("valid_to")
    vt = int(vt // 60) if isinstance(vt, (int, float)) and not isinstance(vt, bool) and math.isfinite(vt) else None
    return (ts_str(attrs.get("level")).upper(), ts_str(attrs.get("weather")).upper(), vt)


def row_of(a, entity):
    """The ALERTS IN VIEW panel's row for an alert (AlertsPanel groups by severity and
    the mark's text). A weather warning (alert kind event-weather-warning) is
    ("warning", severity, label, (level, weather, valid-to minute)) — its row's FROM /
    IN FORCE wording is ignored, so rows that might merge later count as one — with
    None for the details when its entity is not known; anything else is
    ("text", severity, alertText(label))."""
    sev, label = a.get("severity"), str(a.get("label", ""))
    if a.get("kind") == "event-weather-warning" or weather_of(entity):
        return ("warning", sev, label, weather_of(entity))
    return ("text", sev, alert_text(label))


def same_row(r1, r2):
    """Whether two rows may be one in the panel: equal, or two weather warnings of one
    severity and label where either one's details are unknown (errs towards skipping)."""
    if r1 == r2:
        return True
    return r1[0] == r2[0] == "warning" and r1[1:3] == r2[1:3] and (r1[3] is None or r2[3] is None)


def eligible(a, now, all_hour=None):
    """A normalised alert that may be picked, before the subject and row checks: open,
    raised at least MIN_OPEN_S ago, its subject (if `all_hour` is given) seen all hour."""
    return (isinstance(a.get("id"), str) and isinstance(a.get("entity"), str) and bool(a["entity"])
            and a.get("clearedAt") is None and 0 < a["raisedAt"] <= now - MIN_OPEN_S
            and (all_hour is None or a["entity"] in all_hour))


def select_alerts(alerts, n, now, all_hour=None, row_key=None):
    """Up to n eligible alerts (see `eligible`): most severe first, then the longest
    open; one per kind before any second of a kind. Shipped advice is keyed by subject,
    so a subject with more than one open alert is skipped (its advice would stand for
    whichever alert is clicked). The alerts panel merges alerts of one severity and
    text into a row and a click selects only the newest subject, so an alert whose row
    (`row_key(a)`, default `row_of(a, None)`) may be shared with another open alert is
    skipped."""
    key = row_key or (lambda a: row_of(a, None))
    alerts = [a for a in map(normalise_alert, alerts) if isinstance(a, dict)]
    open_ = [a for a in alerts if a.get("clearedAt") is None]
    per_subject = {}
    for a in open_:
        if isinstance(a.get("entity"), str):
            per_subject[a["entity"]] = per_subject.get(a["entity"], 0) + 1
    ok = [a for a in alerts if eligible(a, now, all_hour) and per_subject.get(a["entity"]) == 1]
    rows = {id(a): key(a) for a in open_}
    ok = [a for a in ok if not any(b is not a and same_row(rows[id(a)], rows[id(b)]) for b in open_)]
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
        return v if isinstance(v, str) and v else ts_str(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else None

    w = weather_of(entity)
    if w:
        what = " ".join(x for x in w[:2] if x).lower()
        words = f"{what.capitalize()} warning" if what else "Weather warning"
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
        self._password = None  # kept for a second sign-in; never printed

    def _call(self, method, path, body, timeout):
        headers = {"content-type": "application/json"}
        if self.token:
            headers["authorization"] = f"Bearer {self.token}"
        req = urllib.request.Request(self.base + path, method=method, headers=headers,
                                     data=json.dumps(body).encode() if body is not None else None)
        with urllib.request.urlopen(req, timeout=timeout or self.timeout) as r:
            return json.load(r)

    def call(self, method, path, body=None, timeout=None):
        """One request; on 401 (session expired) sign in again once and repeat it."""
        try:
            return self._call(method, path, body, timeout)
        except urllib.error.HTTPError as e:
            if e.code != 401 or self._password is None:
                raise
        self._login()
        return self._call(method, path, body, timeout)

    def _login(self):
        tok = self._call("POST", "/login", {"username": "analyst", "password": self._password}, 60).get("token")
        if not tok:
            raise ValueError("no token in the sign-in response")
        self.token = tok

    def sign_in(self, pw_file):
        with open(pw_file, encoding="utf-8") as f:
            m = re.search(r"^analyst \(internal\): (\S+)", f.read(), re.M)
        if not m:
            raise ValueError("no 'analyst (internal):' line in the passwords file")
        self._password, self.token = m.group(1), None
        self._login()


def attempt(fn, done, tries=2):
    """fn() -> value, accepted when done(value); one retry. Returns (value, error)."""
    err = None
    for _ in range(tries):
        try:
            v = fn()
            if done(v):
                return v, None
            err = "empty answer"
        except Exception as e:  # noqa: BLE001 -- every failure is reported, never raised
            err = reason(e)
    return None, err


def write_atomic(out, doc):
    """Write doc as JSON to out: a temp file beside it, fsynced, mode 0644, renamed."""
    out = os.path.abspath(out)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".prepared-", suffix=".json", dir=os.path.dirname(out))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(doc, f, ensure_ascii=False, indent=1)
            f.write("\n")
            f.flush()
            os.fsync(f.fileno())
        os.chmod(tmp, 0o644)
        os.replace(tmp, out)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise


def open_subjects(gw):
    """Subjects of the open alerts now, normalised as the console does."""
    body = gw.call("GET", "/heimdall/alerts?state=open", timeout=60)
    alerts = body.get("alerts") if isinstance(body, dict) else None
    if not isinstance(alerts, list):
        raise ValueError("unexpected reply")
    return alerts


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--base", default="http://127.0.0.1:7710/api/v1")
    p.add_argument("--out", required=True)
    p.add_argument("--recording", default="hour-1", help="packed recording: name, folder or heimdall.jsonl.gz")
    p.add_argument("--alerts", type=int, default=5)
    p.add_argument("--timeout", type=float, default=300, help="seconds per try, two tries per answer (default 300)")
    p.add_argument("--recheck", action=argparse.BooleanOptionalAction, default=True,
                   help="re-read the open alerts at the end and drop subjects no longer open (default on)")
    p.add_argument("--passwords", default=PW_FILE, help=argparse.SUPPRESS)
    p.add_argument("--ask-card", default=ASK_CARD, help=argparse.SUPPRESS)
    p.add_argument("--regions-ts", default=REGIONS_TS, help=argparse.SUPPRESS)
    args = p.parse_args(argv)

    all_questions = examples(args.ask_card) + OUR_QUESTIONS
    self_check(all_questions)
    regs = regions(args.regions_ts)
    try:
        rec_name, rec_file = recording_file(args.recording)
        all_hour = seen_all_hour(rec_file)
    except (OSError, ValueError) as e:
        print(f"FAIL recording ({e})")
        return 1
    print(f"ok   recording {rec_name}: {len(all_hour)} entities seen in its first and last {WINDOW_S // 60} min")

    gw = Gateway(args.base, args.timeout)
    try:
        gw.sign_in(args.passwords)
    except Exception as e:
        print(f"FAIL sign in ({reason(e)})")
        return 1
    print("ok   sign in as analyst")

    entries, questions, guide_alerts, failed = {}, [], [], []
    now_ms = lambda: int(time.time() * 1000)  # noqa: E731
    created = now_ms()

    saved = []

    def save():
        if entries or saved:
            saved.append(True)
            write_atomic(args.out, {"version": 1, "recording": rec_name, "created": created, "entries": entries,
                                    "guide": {"questions": questions, "alerts": guide_alerts}})

    # Map questions, as AskCard asks them, each over the region it is about.
    for q in all_questions:
        region = region_of_question(q, regs)
        t0 = time.monotonic()
        path = f"/heimdall/ask?question={encode_component(q.strip())}&{bbox_param(region['box'])}"
        v, err = attempt(lambda: answer_of(gw.call("POST", path)), lambda a: bool((a["answer"] or "").strip()))
        secs = time.monotonic() - t0
        if v is None:
            print(f"FAIL ask    {q!r} [{region['label']}]: {err} ({secs:.0f} s)")
            failed.append(q)
            continue
        entries[shipped_key("ask", "*", q)] = {"value": v, "at": now_ms()}
        questions.append({"question": q, "region": region["label"]})
        save()
        print(f"ok   ask    {q!r} [{region['label']}]: {len(v['answer'])} chars, {secs:.0f} s")

    # Decision support on alerts that have stayed open.
    try:
        alerts = open_subjects(gw)
    except Exception as e:
        print(f"FAIL alerts ({reason(e)})")
        alerts = []
    found = {}

    def entity_of(a):
        """The alert's subject from a small picture read around it, or None (cached)."""
        if a.get("id") not in found:
            found[a.get("id")] = None
            if isinstance(a.get("lat"), (int, float)) and isinstance(a.get("lon"), (int, float)):
                box = (a["lon"] - 0.5, a["lat"] - 0.5, a["lon"] + 0.5, a["lat"] + 0.5)
                try:
                    pic = gw.call("GET", f"/heimdall/picture?{bbox_param(box)}", timeout=60)
                    found[a.get("id")] = next((e for e in pic.get("entities", []) if isinstance(e, dict) and e.get("id") == a.get("entity")), None)
                except Exception:  # noqa: BLE001 -- labelled from the alert alone
                    pass
        return found[a.get("id")]

    now = time.time()
    # Details are read only for eligible weather warnings (others count as unknown).
    def row(a):
        details = a.get("kind") == "event-weather-warning" and eligible(a, now, all_hour)
        return row_of(a, entity_of(a) if details else None)

    picked = select_alerts(alerts, args.alerts, now, all_hour, row)
    if len(picked) < args.alerts:
        print(f"note {len(picked)} of {args.alerts} alerts found open for at least {MIN_OPEN_S // 60} min,"
              f" alone on their subject, with the subject seen all through the recording")
    for a in picked:
        try:
            entity = entity_of(a)
            label = alert_label(a, entity)
            attrs = (entity or {}).get("attributes") or {}
            region = region_at(regs, attrs.get("lat", a.get("lat")), attrs.get("lon", a.get("lon")))
            t0 = time.monotonic()
            path = f"/heimdall/alerts/{encode_component(a['id'])}/advise"
            v, err = attempt(lambda: advice_of(gw.call("POST", path)), lambda d: bool(d["options"]))
            secs = time.monotonic() - t0
            if v is None:
                print(f"FAIL advise {label} [{a.get('severity')}]: {err} ({secs:.0f} s)")
                failed.append(label)
                continue
            key = shipped_key("advise", a["entity"], "")
            entries[key] = {"value": v, "at": now_ms()}
            guide_alerts.append({"key": key, "label": label, "subject": a["entity"],
                                 "kind": category(str(a.get("label", "")))[0], "region": region["label"] if region else None})
            save()
            size = sum(len(o["title"]) + len(o["summary"]) for o in v["options"])
            print(f"ok   advise {label} [{a.get('severity')}]: {len(v['options'])} options, {size} chars, {secs:.0f} s")
        except Exception as e:  # noqa: BLE001 -- one bad alert must not lose the run
            print(f"FAIL advise {a.get('id')!r}: {type(e).__name__}: {str(e)[:120]}")
            failed.append(str(a.get("id")))

    # Still open at the end? Drop any subject that is not.
    if args.recheck and guide_alerts:
        try:
            still = {x["entity"] for x in map(normalise_alert, open_subjects(gw))
                     if x.get("clearedAt") is None and isinstance(x.get("entity"), str)}
            for g in list(guide_alerts):
                if g["subject"] not in still:
                    print(f"drop advise {g['label']}: no longer open at the end")
                    guide_alerts.remove(g)
                    entries.pop(g["key"], None)
            save()
        except Exception as e:  # noqa: BLE001
            print(f"note recheck failed ({reason(e)}); alerts kept")

    if not entries:
        print("FAIL nothing captured" + ("" if saved else "; output left as it was"))
        return 1
    print(f"wrote {os.path.abspath(args.out)}: {len(questions)} questions, {len(guide_alerts)} alerts"
          + (f"; {len(failed)} skipped" if failed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
