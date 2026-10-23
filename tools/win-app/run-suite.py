#!/usr/bin/env python3
"""Run the suite end to end on a fixture recording, check it, and stop it: the Windows and
CI counterpart of tools/mac-app/run-native.sh, runnable on Windows, macOS and Linux.

    python tools/win-app/run-suite.py --bin DIR --fixture DIR [--tools DIR]
                                      [--work DIR] [--logs DIR] [--keep]

--bin holds the services (http-gateway, heimdall-feed, heimdall-source, mimir-server,
forge-worker, midgard-replay; with .exe on Windows), --tools mg-admin and mg-model
(default: --bin). --fixture is a recording, normally one tools/win-app/make-fixture.py
wrote. Everything else is made in --work (default: a new temporary directory, removed
afterwards unless --keep): the enclosure's state, as run-native.sh's $MIDGARD_HOME lays
it out, with fresh keys and identities.

The working copy. tools/mac-app/make-starting-state.sh builds the Mac app's starting
state by copying a live enclosure home, which CI does not have, so this builds a minimal
one the way an enclosure is first made (deploy/compose/init.sh): `mg-admin init`, the
gateway's policy and an ontology for the fixture, one enrolled analyst (a random
password, kept in airports-passwords.txt as smoke.py reads it, never printed), a mesh
identity per service and per recorded feed (`mg-admin mesh issue`, the roles the Mac
home's identities carry), Heimdall's and Mimir's policies, one Forge job on the fixture's
CSV (its first version copied in, as the starting state carries the current CSVs), and a
model directory signed with a fresh key (`mg-model`): tiny GGUF-headed weights, since
the model is the stub.

The run. It picks free loopback ports, starts tools/win-app/stub_llm.py in place of
llama-server, then mimir-server (--engine llama-cpp, pointed at the stub), http-gateway,
heimdall-feed, forge-worker and midgard-replay on the fixture, with run-native.sh's
arguments, each ready when its log shows what run-native.sh waits for. Then the checks:
  - tools/mac-app/smoke.py against the gateway (sign in, object types, picture, Mimir's
    model plane, Forge's overview);
  - the picture holds the fixture's layers and its camera, whose still is /media/...;
  - an ask comes back grounded with the stub's answer;
  - the Forge job ran: the fabric holds the CSV's objects;
  - every process stops, and none is left running (on Windows each service runs in its
    own Job Object, so heimdall-source runs end with the replay; elsewhere in its own
    process group).
Logs go to --logs (default: <work>/logs). Exit 0 only when every check passed.
"""
import argparse
import json
import os
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
WINDOWS = os.name == "nt"
EXE = ".exe" if WINDOWS else ""
sys.path.insert(0, str(HERE))
import stub_llm  # noqa: E402  (the answer the ask must come back with)

SERVICES = ["http-gateway", "heimdall-feed", "heimdall-source", "mimir-server", "forge-worker",
            "midgard-replay"]
TOOLS = ["mg-admin", "mg-model"]
READY_TIMEOUT_S = 180
STOP_TIMEOUT_S = 15

# Mesh identities: directory under airports-mesh, service name, roles (as the Mac home's).
IDENTITIES = [
    ("gateway", "http-gateway", "fabric-gateway"),
    ("mimir", "mimir-server", "inference-server"),
    ("mimir-observer", "mimir-observer", "inference-observer"),
    ("heimdall", "heimdall-feed", "feed-gateway"),
    ("heimdall-advisor", "heimdall-advisor", "inference-consumer"),
    ("heimdall-fabric", "heimdall-fabric", "heimdall-sync"),
    ("forge", "forge-worker", "forge-worker"),
    ("feeds", "picture-analyst", "picture-reader"),
]


def rule(rid, role, action):
    return {"id": rid, "role": role, "action": action, "resource": "*", "effect": "permit"}


GATEWAY_POLICY = {"version": 1, "rules": [
    rule("operator-read", "operator", "Read"),
    rule("operator-apply", "operator", "Apply"),
    rule("worker-channel", "forge-worker", "OpenChannel"),
    rule("worker-sync", "forge-worker", "SyncRecords"),
    rule("heimdall-channel", "heimdall-sync", "OpenChannel"),
    rule("heimdall-sync", "heimdall-sync", "SyncRecords"),
]}
HEIMDALL_POLICY = {"version": 1, "rules": [
    rule("source-channel", "feed-source", "OpenChannel"),
    rule("source-ingest", "feed-source", "Ingest"),
] + [rule(f"analyst-{a.lower()}", "picture-reader", a) for a in (
    "OpenChannel", "ReadPicture", "ReadHistory", "ReadAlerts", "AcknowledgeAlert",
    "AdviseOptions", "AskPicture")]}
MIMIR_POLICY = {"version": 1, "rules": [
    rule("consumer-channel", "inference-consumer", "OpenChannel"),
    rule("consumer-infer", "inference-consumer", "Infer"),
    rule("observer-channel", "inference-observer", "OpenChannel"),
    rule("observer-models", "inference-observer", "DescribeModels"),
    rule("observer-sessions", "inference-observer", "ReadSessions"),
]}


def props(*spec):
    return [dict(zip(("name", "type", "required"), p)) if len(p) == 3 else
            {"name": p[0], "type": p[1]} for p in spec]


# The fixture's Forge type, and the two Heimdall writes back to the fabric
# (heimdall/feed-server/src/fabric_out.rs), as the Mac home's ontology declares them.
ONTOLOGY = {
    "version": 1,
    "objectTypes": [
        {"id": "site", "title": "Fixture site",
         "properties": props(("name", "text", True), ("status", "text"), ("level", "float"),
                             ("updated_at", "timestamp"))},
        {"id": "heimdall_track", "title": "Heimdall tracked item",
         "properties": props(("entity_key", "text", True), ("kind", "text", True),
                             ("layer", "text", True), ("name", "text"), ("classification", "text"),
                             ("classification_rationale", "text"), ("latitude", "float"),
                             ("longitude", "float"), ("altitude_m", "float"), ("network", "text"),
                             ("feeds", "text"), ("observations", "int"), ("first_seen", "timestamp"),
                             ("last_seen", "timestamp"), ("synced_at", "timestamp")),
         "retention": {"after": ["synced_at", "last_seen"], "hours": 12}},
        {"id": "heimdall_alert", "title": "Heimdall alert",
         "properties": props(("alert_id", "text", True), ("label", "text", True),
                             ("severity", "text", True), ("rationale", "text"), ("subject", "text"),
                             ("subject_kind", "text"), ("raised_at", "timestamp"),
                             ("cleared_at", "timestamp"), ("acknowledged_by", "text"),
                             ("acknowledged_at", "timestamp"), ("synced_at", "timestamp"),
                             ("triage", "text")),
         "retention": {"after": ["synced_at", "cleared_at", "raised_at"], "hours": 24}},
    ],
    "linkTypes": [{"id": "alert_on", "name": "alert on", "from": "heimdall_alert",
                   "to": "heimdall_track", "cardinality": "many-to-many"}],
    "actions": [],
}
FORGE_TYPE = "site"
FORGE_ROWS = 3  # make-fixture.py's sites.csv


def forge_jobs(csv_file):
    return {"version": 1, "jobs": [{
        "id": "sites", "every": 10,
        "source": {"file": csv_file, "header": True, "columns": [
            {"name": "key", "type": "text"}, {"name": "name", "type": "text"},
            {"name": "status", "type": "text", "nullable": True},
            {"name": "level", "type": "float", "nullable": True},
            {"name": "updated_at", "type": "int", "nullable": True}]},
        "objects": {"type": FORGE_TYPE, "key": "key", "marking": "internal",
                    "purposes": ["operations"],
                    "properties": {"name": "name", "status": "status", "level": "level",
                                   "updated_at": "updated_at"}}}]}


LAYERS = ["air", "sea", "land", "events", "utilities"]
CAMERA = ("CAM-TEST", "/media/CAM-TEST.jpg")
QUESTION = "What is happening in the fixture area?"


def log(msg):
    print(f"run-suite: {msg}", flush=True)


def write_json(path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8", newline="\n")


def run_tool(cmd, stdin=None):
    """Run an offline tool; its output is shown only when it fails (none of these print a
    secret: the password goes in on standard input)."""
    r = subprocess.run([str(c) for c in cmd], input=stdin, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        raise SystemExit(f"run-suite: {Path(cmd[0]).name} {cmd[1]} failed (exit {r.returncode})")
    return r.stdout


# --- the working copy --------------------------------------------------------------------

def make_state(w, tools, fixture):
    meta = json.loads((fixture / "meta.json").read_text(encoding="utf-8"))
    feeds = meta.get("feeds") or []
    manifest = json.loads((fixture / "forge" / "manifest.json").read_text(encoding="utf-8"))
    if len(manifest) < 1:
        raise SystemExit("run-suite: the fixture has no Forge file")
    csv_file = manifest[0]["file"]
    admin, model = tools / f"mg-admin{EXE}", tools / f"mg-model{EXE}"

    airports = w / "airports"
    run_tool([admin, "init", "--state-dir", airports])
    write_json(airports / "ontology.json", ONTOLOGY)
    write_json(airports / "policy.json", GATEWAY_POLICY)
    password = secrets.token_urlsafe(18)
    run_tool([admin, "user", "add", "--state-dir", airports, "--subject", "analyst",
              "--roles", "operator", "--clearance", "internal",
              "--purposes", "operations,situational-awareness,investigation"],
             stdin=password + "\n")
    pw = w / "airports-passwords.txt"
    fd = os.open(pw, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(f"analyst (internal): {password}\n")
    del password

    for d, service, roles in IDENTITIES + [("feeds", f, "feed-source") for f in feeds]:
        run_tool([admin, "mesh", "issue", "--state-dir", airports, "--service", service,
                  "--roles", roles, "--out", w / "airports-mesh" / d])

    write_json(w / "mimir-state" / "policy.json", MIMIR_POLICY)
    write_json(w / "heimdall-poc" / "state" / "policy.json", HEIMDALL_POLICY)
    write_json(w / "heimdall-poc" / "feeds" / "state" / "status.json",
               {"generatedAt": 0, "sources": {}})
    data = w / "live-feeds" / "data"
    write_json(data / "jobs.json", forge_jobs(csv_file))
    shutil.copyfile(fixture / Path(*manifest[0]["path"].split("/")), data / csv_file)
    (w / "live-feeds" / "worker-state").mkdir(parents=True)

    # A model directory that passes Mimir's load gate: weights with a GGUF header, a
    # manifest signed by a key made here, and that key's public half as the trust file.
    keys, granite = w / "model-signing", w / "mimir-models" / "granite"
    granite.mkdir(parents=True)
    run_tool([model, "keygen", "--out", keys])
    weights = granite / "granite-stub.gguf"
    weights.write_bytes(b"GGUF\x03\x00\x00\x00" + bytes(i % 251 for i in range(65536)))
    run_tool([model, "sign", "--weights", weights, "--id", "granite-stub", "--family", "Granite",
              "--version", "1", "--quantisation", "Q4_K_M", "--licence", "Apache-2.0",
              "--osi-approved", "--redistributable", "--keystore", keys / "model-signing.keystore"])
    trust = w / "mimir-models" / "trust"
    trust.mkdir(parents=True)
    shutil.copyfile(keys / "model-signing.pub", trust / "model-signing.pub")
    return pw


# --- processes ---------------------------------------------------------------------------

if WINDOWS:
    import ctypes
    from ctypes import wintypes

    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class _BasicLimit(ctypes.Structure):
        _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                    ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                    ("SchedulingClass", wintypes.DWORD)]

    class _ExtendedLimit(ctypes.Structure):
        _fields_ = [("BasicLimitInformation", _BasicLimit), ("IoInfo", ctypes.c_uint64 * 6),
                    ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                    ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]

    class _Accounting(ctypes.Structure):
        _fields_ = [("TotalUserTime", ctypes.c_int64), ("TotalKernelTime", ctypes.c_int64),
                    ("ThisPeriodTotalUserTime", ctypes.c_int64),
                    ("ThisPeriodTotalKernelTime", ctypes.c_int64),
                    ("TotalPageFaultCount", wintypes.DWORD), ("TotalProcesses", wintypes.DWORD),
                    ("ActiveProcesses", wintypes.DWORD), ("TotalTerminatedProcesses", wintypes.DWORD)]

    _k32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    _k32.CreateJobObjectW.restype = wintypes.HANDLE
    _k32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    _k32.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                               wintypes.DWORD, ctypes.c_void_p]
    _k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    _k32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
    _k32.CloseHandle.argtypes = [wintypes.HANDLE]

    def _win_check(ok, what):
        if not ok:
            raise OSError(ctypes.get_last_error(), f"{what} failed")

    class Group:
        """A Job Object that ends every process in it when terminated or closed."""

        def __init__(self, proc):
            self.job = _k32.CreateJobObjectW(None, None)
            _win_check(self.job, "CreateJobObjectW")
            info = _ExtendedLimit()
            info.BasicLimitInformation.LimitFlags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            _win_check(_k32.SetInformationJobObject(self.job, 9, ctypes.byref(info),
                                                    ctypes.sizeof(info)), "SetInformationJobObject")
            _win_check(_k32.AssignProcessToJobObject(self.job, int(proc._handle)),
                       "AssignProcessToJobObject")

        def stop(self):
            _k32.TerminateJobObject(self.job, 1)

        def alive(self):
            acct = _Accounting()
            _win_check(_k32.QueryInformationJobObject(self.job, 1, ctypes.byref(acct),
                                                      ctypes.sizeof(acct), None),
                       "QueryInformationJobObject")
            return acct.ActiveProcesses

        def close(self):
            _k32.CloseHandle(self.job)

    POPEN_FLAGS = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
else:
    class Group:
        """The process group the service leads (it was started in a new session)."""

        def __init__(self, proc):
            self.pgid = proc.pid

        def stop(self, sig=signal.SIGTERM):
            try:
                os.killpg(self.pgid, sig)
            except ProcessLookupError:
                pass

        def alive(self):
            try:
                os.killpg(self.pgid, 0)
                return 1
            except ProcessLookupError:
                return 0
            except PermissionError:  # a member we cannot signal is still a member
                return 1

        def close(self):
            pass

    POPEN_FLAGS = {"start_new_session": True}


class Service:
    def __init__(self, name, cmd, ready, logs):
        self.name, self.cmd, self.ready = name, [str(c) for c in cmd], ready
        self.log = logs / f"{name}.log"
        self.proc = self.group = None

    def start(self):
        with open(self.log, "wb") as out:
            self.proc = subprocess.Popen(self.cmd, stdin=subprocess.DEVNULL, stdout=out,
                                         stderr=subprocess.STDOUT, **POPEN_FLAGS)
        self.group = Group(self.proc)

    def tail(self, n=8):
        try:
            return "\n".join(self.log.read_text(encoding="utf-8", errors="replace").splitlines()[-n:])
        except OSError:
            return ""

    def wait_ready(self):
        deadline = time.monotonic() + READY_TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                if self.ready in self.log.read_text(encoding="utf-8", errors="replace"):
                    log(f"{self.name} ready")
                    return
            except OSError:
                pass
            if self.proc.poll() is not None:
                raise RuntimeError(f"{self.name} exited ({self.proc.returncode}); see {self.log}\n{self.tail()}")
            time.sleep(0.5)
        raise RuntimeError(f"{self.name} not ready after {READY_TIMEOUT_S} s; see {self.log}\n{self.tail()}")

    def stop(self):
        """Stop the service and everything it started; whether none of it is left."""
        if self.proc is None:
            return True
        self.group.stop()
        deadline = time.monotonic() + STOP_TIMEOUT_S
        while time.monotonic() < deadline and (self.proc.poll() is None or self.group.alive()):
            time.sleep(0.2)
        left = self.proc.poll() is None or self.group.alive()
        if left:
            log(f"{self.name} (pid {self.proc.pid}) or a process it started did not exit "
                f"within {STOP_TIMEOUT_S} s; killing")
            if WINDOWS:
                self.group.stop()
            else:
                self.group.stop(signal.SIGKILL)
            time.sleep(1)
        self.group.close()
        return not left


def free_ports(n):
    socks = []
    try:
        for _ in range(n):
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.bind(("127.0.0.1", 0))
            socks.append(s)
        return [s.getsockname()[1] for s in socks]
    finally:
        for s in socks:
            s.close()


# --- checks ------------------------------------------------------------------------------

class Gateway:
    def __init__(self, base):
        self.base, self.token = base, None

    def call(self, method, path, body=None, timeout=60):
        req = urllib.request.Request(
            self.base + path, method=method,
            data=json.dumps(body).encode() if body is not None else None,
            headers={"content-type": "application/json",
                     **({"authorization": f"Bearer {self.token}"} if self.token else {})})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.load(r)

    def sign_in(self, pw_file):
        line = next(l for l in pw_file.read_text(encoding="utf-8").splitlines()
                    if l.startswith("analyst (internal): "))
        self.token = self.call("POST", "/login", {"username": "analyst",
                                                  "password": line.split(": ", 1)[1]}).get("token")
        if not self.token:
            raise RuntimeError("no token in the login response")


def reason(e):
    if isinstance(e, urllib.error.HTTPError):
        try:
            detail = e.read().decode("utf-8", "replace")[:200]
        except Exception:
            detail = ""
        return f"HTTP {e.code} {detail}".strip()
    return f"{type(e).__name__}: {str(e)[:200]}"


def poll(what, test, timeout):
    """Call test() until it returns (True, detail) or `timeout` s pass; the last detail."""
    deadline, detail = time.monotonic() + timeout, ""
    while True:
        try:
            ok, detail = test()
        except Exception as e:
            ok, detail = False, reason(e)
        if ok or time.monotonic() >= deadline:
            return ok, detail
        time.sleep(2)


def checks(gw, pw_file, fixture, results):
    def check(name, ok, detail=""):
        print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}".rstrip(), flush=True)
        results.append((name, ok))

    try:
        gw.sign_in(pw_file)
        check("sign in", True)
    except Exception as e:
        check("sign in", False, f"({reason(e)})")
        return

    def picture():
        ents = gw.call("GET", "/heimdall/picture").get("entities", [])
        layers = sorted({e.get("layer") for e in ents if e.get("layer")})
        missing = [l for l in LAYERS if l not in layers]
        return not missing, f"({len(ents)} entities; layers {','.join(layers)}" + \
            (f"; missing {','.join(missing)})" if missing else ")")
    check("picture has the fixture's layers", *poll("picture", picture, 120))

    def camera():
        ents = gw.call("GET", "/heimdall/picture").get("entities", [])
        cam = next((e for e in ents if e.get("id") == CAMERA[0]), None)
        if cam is None:
            return False, f"(no {CAMERA[0]})"
        blob = json.dumps(cam)
        return cam.get("kind") == "land-camera" and CAMERA[1] in blob, \
            f"({cam.get('kind')}, {'with' if CAMERA[1] in blob else 'without'} {CAMERA[1]})"
    ok, detail = poll("camera", camera, 30)
    still = fixture / "media" / CAMERA[1].rsplit("/", 1)[1]
    check("camera entity with its /media/ still", ok and still.is_file(),
          detail + ("" if still.is_file() else f" ({still} is missing)"))

    def forge():
        o = gw.call("GET", "/forge/overview")
        n = (o.get("objectCounts") or {}).get(FORGE_TYPE, 0)
        return n >= FORGE_ROWS, f"({n} {FORGE_TYPE} objects in the fabric)"
    check("forge job ran on the CSV", *poll("forge", forge, 90))

    smoke = subprocess.run([sys.executable, str(REPO / "tools" / "mac-app" / "smoke.py"),
                            "--base", gw.base, "--passwords", str(pw_file),
                            "--forge-jobs", "1", "--min-object-types", str(len(ONTOLOGY["objectTypes"]))],
                           capture_output=True, text=True, timeout=600)
    for line in (smoke.stdout + smoke.stderr).strip().splitlines():
        print(f"  smoke.py: {line}", flush=True)
    check("tools/mac-app/smoke.py", smoke.returncode == 0, f"(exit {smoke.returncode})")

    try:
        q = urllib.parse.quote(QUESTION)
        a = gw.call("POST", f"/heimdall/ask?question={q}", timeout=300)
        ok = a.get("grounded") is True and a.get("answer") == stub_llm.ANSWER_TEXT
        check("ask is grounded with the stub's answer", ok,
              f"(grounded={a.get('grounded')}, citations={a.get('citations')}, "
              f"considered={a.get('considered')})" + ("" if ok else f" answer={a.get('answer')!r}"))
    except Exception as e:
        check("ask is grounded with the stub's answer", False, f"({reason(e)})")


# --- main --------------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin", required=True, type=Path)
    ap.add_argument("--tools", type=Path)
    ap.add_argument("--fixture", required=True, type=Path)
    ap.add_argument("--work", type=Path)
    ap.add_argument("--logs", type=Path)
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    bin_dir, tools = a.bin.resolve(), (a.tools or a.bin).resolve()
    fixture = a.fixture.resolve()
    missing = [str(bin_dir / f"{b}{EXE}") for b in SERVICES if not (bin_dir / f"{b}{EXE}").is_file()]
    missing += [str(tools / f"{t}{EXE}") for t in TOOLS if not (tools / f"{t}{EXE}").is_file()]
    if missing:
        raise SystemExit("run-suite: missing " + ", ".join(missing))
    if not (fixture / "meta.json").is_file():
        raise SystemExit(f"run-suite: {fixture} has no meta.json; not a recording")

    made_work = a.work is None
    w = Path(tempfile.mkdtemp(prefix="midgard-suite-")) if made_work else a.work.resolve()
    if not made_work:
        w.mkdir(parents=True, exist_ok=True)
        if any(w.iterdir()):
            raise SystemExit(f"run-suite: {w} is not empty")
    logs = (a.logs.resolve() if a.logs else w / "logs")
    logs.mkdir(parents=True, exist_ok=True)
    log(f"working copy {w}; logs {logs}")
    results, services = [], []
    try:
        pw_file = make_state(w, tools, fixture)
        log("working copy made: keys, identities, policies, Forge job, signed stub model")
        (llm, mimir, mimir_health, mimir_obs, gw_port, mesh, heim, heim_health) = free_ports(8)
        b = lambda name: bin_dir / f"{name}{EXE}"  # noqa: E731
        mesh_dir = w / "airports-mesh"
        loop = lambda p: f"127.0.0.1:{p}"  # noqa: E731
        services = [
            Service("stub-llm", [sys.executable, "-u", HERE / "stub_llm.py", "--port", llm],
                    "listening on", logs),
            Service("mimir", [b("mimir-server"), "--identity-dir", mesh_dir / "mimir",
                              "--state-dir", w / "mimir-state", "--purposes", "investigation",
                              "--bind", loop(mimir), "--health-bind", loop(mimir_health),
                              "--observe-bind", loop(mimir_obs), "--engine", "llama-cpp",
                              "--model-dir", w / "mimir-models" / "granite",
                              "--model-trust", w / "mimir-models" / "trust" / "model-signing.pub",
                              "--sidecar", loop(llm)], '"serving"', logs),
            Service("gateway", [b("http-gateway"), "--state-dir", w / "airports", "--bind", loop(gw_port),
                                "--mesh-bind", loop(mesh), "--mesh-identity-dir", mesh_dir / "gateway",
                                "--heimdall-addr", loop(heim), "--heimdall-identity-dir", mesh_dir / "feeds",
                                "--mimir-observe-addr", loop(mimir_obs),
                                "--mimir-identity-dir", mesh_dir / "mimir-observer",
                                "--forge-jobs", w / "live-feeds" / "data" / "jobs.json",
                                "--forge-status", w / "live-feeds" / "worker-state" / "status.json",
                                "--heimdall-feeds-status",
                                w / "heimdall-poc" / "feeds" / "state" / "status.json"],
                    "listening on", logs),
            Service("heimdall", [b("heimdall-feed"), "--identity-dir", mesh_dir / "heimdall",
                                 "--state-dir", w / "heimdall-poc" / "state", "--bind", loop(heim),
                                 "--health-bind", loop(heim_health), "--track-ttl", "300",
                                 "--mimir-addr", loop(mimir),
                                 "--mimir-identity-dir", mesh_dir / "heimdall-advisor",
                                 "--fabric-addr", loop(mesh),
                                 "--fabric-identity-dir", mesh_dir / "heimdall-fabric"],
                    '"serving"', logs),
            # Ready on the first successful job run, as run-native.sh.
            Service("forge", [b("forge-worker"), "--identity-dir", mesh_dir / "forge",
                              "--state-dir", w / "live-feeds" / "worker-state",
                              "--jobs", w / "live-feeds" / "data" / "jobs.json",
                              "--gateway", loop(mesh)], '"message":"job run"', logs),
            Service("replay", [b("midgard-replay"), "--recording", fixture,
                               "--identity-dir", mesh_dir / "feeds",
                               "--forge-dir", w / "live-feeds" / "data", "--addr", loop(heim),
                               "--source-bin", b("heimdall-source")], '"replay started"', logs),
        ]
        for s in services:
            s.start()
            s.wait_ready()
        checks(Gateway(f"http://{loop(gw_port)}/api/v1"), pw_file, fixture, results)
    except (RuntimeError, OSError) as e:
        print(f"FAIL start: {e}", flush=True)
        results.append(("start", False))
    finally:
        clean = True
        for s in reversed(services):
            if s.proc is not None:
                stopped = s.stop()
                clean &= stopped
                log(f"{s.name} {'stopped' if stopped else 'had to be killed'}")
        if services:
            print(f"{'ok  ' if clean else 'FAIL'} every process stopped, none left running", flush=True)
            results.append(("clean stop", clean))
        if made_work and not a.keep:
            if a.logs is None:
                log(f"logs are removed with {w}; pass --logs DIR to keep them")
            shutil.rmtree(w, ignore_errors=True)
    failed = [n for n, ok in results if not ok]
    log(f"{len(results) - len(failed)} of {len(results)} checks passed"
        + (f"; failed: {', '.join(failed)}" if failed else ""))
    return 1 if failed or not results else 0


if __name__ == "__main__":
    sys.exit(main())
