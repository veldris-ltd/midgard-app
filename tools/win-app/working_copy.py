"""The minimal working copy the Windows app's end-to-end checks run the suite on, built the
way an enclosure is first made (deploy/compose/init.sh), since CI has no live enclosure
home to copy as tools/mac-app/make-starting-state.sh does: `mg-admin init`, the gateway's
policy and an ontology for the fixture, one enrolled analyst (a random password, never
printed), a mesh identity per service, per recorded feed and for the camera analytics
(`mg-admin mesh issue`, the roles the Mac home's identities carry), Heimdall's and Mimir's policies, one Forge job on
the fixture's CSV (its first version copied in, as the starting state carries the current
CSVs), and a model directory signed with a fresh key (`mg-model`): tiny GGUF-headed
weights, since the model is the stub (stub_llm.py).

run-suite.py builds it as an enclosure home for the services; make-starting-state.py as
the shell's resources\\starting-state (with demo-users.json and VERSION) and
resources\\models. Python standard library only.
"""
import json
import os
import secrets
import shutil
import subprocess
import sys
import time
from pathlib import Path

EXE = ".exe" if os.name == "nt" else ""

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


def write_json(path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8", newline="\n")


def run_tool(cmd, stdin=None):
    """Run an offline tool; its output is shown only when it fails (none of these print a
    secret: the password goes in on standard input)."""
    r = subprocess.run([str(c) for c in cmd], input=stdin, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        raise SystemExit(f"{Path(cmd[0]).name} {cmd[1]} failed (exit {r.returncode})")
    return r.stdout


# --- the working copy --------------------------------------------------------------------

def make_state(w, tools, fixture, models=None, keys=None, starting_state=False):
    """Build the working copy in `w` (see the module's docstring). The signed stub model
    goes in `models` (default: <w>/mimir-models; granite/ and trust/), its signing key in
    `keys` (default: <w>/model-signing). The analyst's password goes, never printed, in
    <w>/airports-passwords.txt as smoke.py reads it, or with `starting_state` in
    <w>/demo-users.json as the shells read it (with a VERSION file). Returns that file."""
    meta = json.loads((fixture / "meta.json").read_text(encoding="utf-8"))
    feeds = meta.get("feeds") or []
    manifest = json.loads((fixture / "forge" / "manifest.json").read_text(encoding="utf-8"))
    if len(manifest) < 1:
        raise SystemExit("the fixture has no Forge file")
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
    if starting_state:
        pw = w / "demo-users.json"
        text = json.dumps([{"username": "analyst", "password": password, "label": "Analyst (INTERNAL)"}],
                          indent=2) + "\n"
    else:
        pw = w / "airports-passwords.txt"
        text = f"analyst (internal): {password}\n"
    fd = os.open(pw, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    del password, text

    # The feeds' identities, and the camera analytics' (they send as camera-analytics with
    # heimdall-source from the same folder, as on the Mac).
    sources = feeds + [f for f in ("camera-analytics",) if f not in feeds]
    for d, service, roles in IDENTITIES + [("feeds", f, "feed-source") for f in sources]:
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
    models = models if models is not None else w / "mimir-models"
    keys = keys if keys is not None else w / "model-signing"
    granite = models / "granite"
    granite.mkdir(parents=True)
    run_tool([model, "keygen", "--out", keys])
    weights = granite / "granite-stub.gguf"
    weights.write_bytes(b"GGUF\x03\x00\x00\x00" + bytes(i % 251 for i in range(65536)))
    run_tool([model, "sign", "--weights", weights, "--id", "granite-stub", "--family", "Granite",
              "--version", "1", "--quantisation", "Q4_K_M", "--licence", "Apache-2.0",
              "--osi-approved", "--redistributable", "--keystore", keys / "model-signing.keystore"])
    trust = models / "trust"
    trust.mkdir(parents=True)
    shutil.copyfile(keys / "model-signing.pub", trust / "model-signing.pub")
    if starting_state:
        (w / "VERSION").write_text(f"starting-state {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())} fixture\n",
                                   encoding="utf-8", newline="\n")
    return pw
