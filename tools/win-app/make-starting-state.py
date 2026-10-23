#!/usr/bin/env python3
"""Write the Windows shell's resources\\starting-state and resources\\models for tests, from
a fixture recording: the minimal working copy (working_copy.py) that run-suite.py runs the
services on, laid out as the shell's package carries it.

    python tools/win-app/make-starting-state.py --tools DIR --fixture DIR --out DIR --models DIR

--tools holds mg-admin and mg-model (.exe on Windows), --fixture is a recording
(make-fixture.py). --out (the starting state) and --models must not exist or be empty.
The starting state gets airports/, airports-mesh/, mimir-state/, heimdall-poc/,
live-feeds/, demo-users.json (the analyst, with a fresh random password that is never
printed) and VERSION; --models gets granite/ (the signed stub weights) and trust/. The
model's signing key is made in a temporary folder and deleted: it goes in neither.

For the windows-shell-e2e CI job (Midgard.exe --headless-check); the real starting state
is tools/mac-app/make-starting-state.sh's.
"""
import argparse
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from working_copy import EXE, make_state  # noqa: E402


def empty_or_missing(path):
    return not path.exists() or (path.is_dir() and not any(path.iterdir()))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--tools", required=True, type=Path)
    ap.add_argument("--fixture", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--models", required=True, type=Path)
    a = ap.parse_args()
    tools, fixture, out, models = (p.resolve() for p in (a.tools, a.fixture, a.out, a.models))
    missing = [str(tools / f"{t}{EXE}") for t in ("mg-admin", "mg-model") if not (tools / f"{t}{EXE}").is_file()]
    if missing:
        raise SystemExit("make-starting-state: missing " + ", ".join(missing))
    if not (fixture / "meta.json").is_file():
        raise SystemExit(f"make-starting-state: {fixture} has no meta.json; not a recording")
    for p in (out, models):
        if not empty_or_missing(p):
            raise SystemExit(f"make-starting-state: {p} is not empty")
    out.mkdir(parents=True, exist_ok=True)
    models.mkdir(parents=True, exist_ok=True)
    keys = Path(tempfile.mkdtemp(prefix="midgard-model-signing-"))
    try:
        make_state(out, tools, fixture, models=models, keys=keys / "keys", starting_state=True)
    finally:
        shutil.rmtree(keys, ignore_errors=True)
    files = sum(1 for p in out.rglob("*") if p.is_file())
    print(f"make-starting-state: {out} ({files} files: "
          f"{', '.join(sorted(p.name for p in out.iterdir()))}); models {models}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
