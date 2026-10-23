#!/usr/bin/env python3
"""Smoke-test a running suite through its gateway: sign in, then check the fabric,
Heimdall's picture, Mimir's model plane and Forge's overview. Exit 1 on any failure.

    python3 tools/mac-app/smoke.py [--base http://127.0.0.1:7710/api/v1] [--passwords FILE]
                                   [--forge-jobs N] [--min-object-types N]

The analyst password is read from --passwords (default
~/Desktop/Midgard/airports-passwords.txt) and is never printed; neither is the session
token. --forge-jobs (default 11) is how many jobs Forge's overview must list and
--min-object-types (default 20) how many object types the fabric must have at least:
the enclosure home's; tools/win-app/run-suite.py passes its fixture's.
"""
import json, os, re, sys, urllib.error, urllib.request


def opt(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


BASE = opt("--base", "http://127.0.0.1:7710/api/v1")
PW_FILE = os.path.expanduser(opt("--passwords", "~/Desktop/Midgard/airports-passwords.txt"))
FORGE_JOBS = int(opt("--forge-jobs", "11"))
MIN_OBJECT_TYPES = int(opt("--min-object-types", "20"))


def call(path, body=None, token=None):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"content-type": "application/json",
                                          **({"authorization": f"Bearer {token}"} if token else {})})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def reason(e):
    """A short, secret-free reason for a failed call."""
    if isinstance(e, urllib.error.HTTPError):
        msg = ""
        try:
            raw = e.read().decode("utf-8", "replace")
            try:
                j = json.loads(raw)
                msg = j.get("error") or j.get("message") or j.get("detail") or "" if isinstance(j, dict) else ""
            except ValueError:
                msg = raw
        except Exception:
            pass
        return f"HTTP {e.code}" + (f": {str(msg).strip()[:120]}" if msg else "")
    if isinstance(e, urllib.error.URLError):
        return f"unreachable: {e.reason}"
    return f"{type(e).__name__}: {str(e)[:120]}"


failures = []


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}".rstrip())
    if not ok:
        failures.append(name)


def run(name, path, token, test):
    """GET path, then test(response) -> (ok, detail). Any error becomes a FAIL line."""
    try:
        ok, detail = test(call(path, token=token))
    except Exception as e:
        ok, detail = False, f"({reason(e)})"
    check(name, ok, detail)


# Sign in: nothing else can work without it.
try:
    with open(PW_FILE, encoding="utf-8") as f:
        m = re.search(r"^analyst \(internal\): (\S+)", f.read(), re.M)
    if not m:
        raise ValueError("no 'analyst (internal):' line in the passwords file")
    tok = call("/login", {"username": "analyst", "password": m.group(1)}).get("token")
    check("sign in", bool(tok), "" if tok else "(no token in response)")
except Exception as e:
    check("sign in", False, f"({reason(e)})")
if failures:
    sys.exit(1)


def object_types(r):
    return isinstance(r, list) and len(r) >= MIN_OBJECT_TYPES, f"({len(r) if isinstance(r, list) else 'not a list'})"


def picture(r):
    ents = r.get("entities", []) if isinstance(r, dict) else []
    return len(ents) > 0, f"({len(ents)} entities{', truncated' if isinstance(r, dict) and r.get('truncated') else ''})"


def mimir(r):
    model = r.get("model") if isinstance(r, dict) else None
    model = model if isinstance(model, dict) else {}
    return bool(model.get("id")), f"({model.get('id')})"


def forge(r):
    jobs = r.get("jobs", []) if isinstance(r, dict) else []
    return len(jobs) == FORGE_JOBS, f"({len(jobs)} jobs)"


run("fabric object types", "/object-types", tok, object_types)
run("heimdall picture", "/heimdall/picture", tok, picture)
run("mimir model plane", "/mimir/models", tok, mimir)
run("forge overview", "/forge/overview", tok, forge)
sys.exit(1 if failures else 0)
