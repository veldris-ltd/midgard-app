#!/usr/bin/env python3
"""Check a Windows package folder before it is zipped (stdlib only).

    python3 -I tools/win-app/check-package.py PACKAGE [--report FILE]

PACKAGE is the Midgard folder (tools/win-app/layout.sh's layout). It fails (exit 1, every
problem listed) unless:
  - the parts the shell needs are there: Midgard.exe, helpers\\ (the six services,
    midgard-frames.exe, llama\\llama-server.exe, analytics\\python.exe), resources\\
    (consoles, home, starting-state\\VERSION, models\\{granite,trust,yolox}, analytics,
    recording\\meta.json with a plain heimdall.jsonl and no .gz, licenses\\CREDITS.txt);
  - every .exe, .dll and .pyd is PE32+ for x86-64 (by its MZ and PE headers);
  - nothing is a symbolic link (nor a junction-to-be: no links at all);
  - no path, written Midgard\\..., is longer than MAX_PATH_CHARS (200), so the folder fits
    under a typical extract folder within Windows' 260-character limit;
  - no name has a character Windows forbids (<>:"/\\|?* or a control character), ends in a
    dot or a space, or is a reserved device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9, with
    or without an extension);
  - no two names in one folder differ only by case (NTFS would keep only one);
  - no Mac metadata (.DS_Store, ._* files);
  - home\\prepared-answers.json, when there, parses as version 1.
Then prints the size and file count of each top-level part (and of helpers\\ and resources\\
one level down), the total, and the longest path. Nothing is run.
"""
import argparse
import json
import os
import struct
import sys

MAX_PATH_CHARS = 200
FORBIDDEN = set('<>:"/\\|?*') | {chr(c) for c in range(32)}
RESERVED = {"CON", "PRN", "AUX", "NUL"} | {f"COM{i}" for i in range(1, 10)} | {f"LPT{i}" for i in range(1, 10)}
PE_EXTENSIONS = (".exe", ".dll", ".pyd")
SERVICES = ("http-gateway", "heimdall-feed", "heimdall-source", "mimir-server", "forge-worker", "midgard-replay")
REQUIRED_FILES = (
    ["Midgard.exe", "helpers/midgard-frames.exe", "helpers/llama/llama-server.exe",
     "helpers/analytics/python.exe", "resources/starting-state/VERSION",
     "resources/models/trust/model-signing.pub", "resources/models/yolox/yolox_s.onnx",
     "resources/analytics/analytics.py", "resources/analytics/config.json",
     "resources/recording/meta.json", "resources/recording/heimdall.jsonl",
     "resources/home/index.html", "resources/home/credits.txt", "resources/licenses/CREDITS.txt"]
    + [f"helpers/{s}.exe" for s in SERVICES]
    + [f"resources/consoles/{p}/index.html" for p in ("midgard", "heimdall", "mimir", "forge")])


def pe_machine(path):
    """(is PE, PE32+ x86-64?, what) from the headers."""
    with open(path, "rb") as f:
        head = f.read(4096)
    if head[:2] != b"MZ" or len(head) < 64:
        return False, False, "no MZ header"
    off = struct.unpack_from("<I", head, 0x3C)[0]
    if off + 26 > len(head):
        with open(path, "rb") as f:
            f.seek(off)
            head = b"\0" * off + f.read(26)
    if head[off:off + 4] != b"PE\0\0":
        return False, False, "no PE signature"
    machine = struct.unpack_from("<H", head, off + 4)[0]
    magic = struct.unpack_from("<H", head, off + 24)[0]
    return True, machine == 0x8664 and magic == 0x20B, f"machine 0x{machine:04x}, optional header 0x{magic:03x}"


def human(n):
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:.1f} {unit}" if unit != "B" else f"{n} B"
        n /= 1024


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("package")
    ap.add_argument("--report", default=None)
    a = ap.parse_args()
    root = os.path.abspath(a.package)
    top = os.path.basename(root)
    out = []

    def say(line=""):
        out.append(line)
        print(line, flush=True)

    problems = []
    if not os.path.isdir(root) or os.path.islink(root):
        raise SystemExit(f"check-package: {root} is not a folder")
    for rel in REQUIRED_FILES:
        if not os.path.isfile(os.path.join(root, *rel.split("/"))):
            problems.append(f"MISSING {rel}")
    for rel in ("resources/models/granite", "resources/consoles", "resources/starting-state"):
        if not os.path.isdir(os.path.join(root, *rel.split("/"))):
            problems.append(f"MISSING {rel}\\")
    if os.path.exists(os.path.join(root, "resources", "recording", "heimdall.jsonl.gz")):
        problems.append("recording\\heimdall.jsonl.gz: Windows replays only a plain heimdall.jsonl")

    files = dirs = pe_files = 0
    longest = ""
    sizes = {}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        rel_dir = os.path.relpath(dirpath, root)
        parts = [] if rel_dir == "." else rel_dir.split(os.sep)
        seen = {}
        for name in dirnames + filenames:
            full = os.path.join(dirpath, name)
            win = "\\".join([top] + parts + [name])
            if len(win) > len(longest):
                longest = win
            if os.path.islink(full):
                problems.append(f"LINK {win}")
            if len(win) > MAX_PATH_CHARS:
                problems.append(f"LONG ({len(win)} characters) {win}")
            bad = sorted({c for c in name if c in FORBIDDEN})
            if bad:
                problems.append(f"CHARACTERS {[c if c.isprintable() else hex(ord(c)) for c in bad]} {win}")
            if name.endswith((".", " ")):
                problems.append(f"TRAILING DOT OR SPACE {win}")
            if name.split(".")[0].upper().rstrip(" ") in RESERVED:
                problems.append(f"RESERVED NAME {win}")
            if name == ".DS_Store" or name.startswith("._"):
                problems.append(f"MAC METADATA {win}")
            key = name.casefold()
            if key in seen:
                problems.append(f"CASE DUPLICATE {win} and {seen[key]}")
            seen[key] = name
        dirs += len(dirnames)
        for name in filenames:
            full = os.path.join(dirpath, name)
            if os.path.islink(full):
                continue
            files += 1
            size = os.path.getsize(full)
            rel = parts + [name]
            keys = [rel[0]]
            if rel[0] in ("helpers", "resources") and len(rel) >= 2:
                keys.append(rel[0] + "\\" + (rel[1] if len(rel) > 2 else "(files)"))
            for key in keys:
                s = sizes.setdefault(key, [0, 0])
                s[0] += size
                s[1] += 1
            if name.lower().endswith(PE_EXTENSIONS):
                is_pe, ok, what = pe_machine(full)
                pe_files += 1
                if not ok:
                    problems.append(f"NOT PE32+ x86-64 ({what}) " + "\\".join([top] + rel))

    prepared = os.path.join(root, "resources", "home", "prepared-answers.json")
    if os.path.isfile(prepared):
        try:
            d = json.load(open(prepared, encoding="utf-8"))
            assert isinstance(d, dict) and type(d.get("version")) is int and d["version"] == 1, "not version 1"
            say(f"prepared answers: {len(d.get('entries') or {})} answers, guide: "
                f"{len((d.get('guide') or {}).get('questions') or [])} questions")
        except Exception as e:  # noqa: BLE001 - any failure is the finding
            problems.append(f"PREPARED ANSWERS not a valid version 1 file: {e}")

    say(f"== sizes ({top}\\)")
    for key in sorted(sizes):
        size, count = sizes[key]
        say(f"{human(size):>10}  {count:>6} files  {key}")
    total = sum(v[0] for k, v in sizes.items() if "\\" not in k)
    say(f"total: {human(total)} in {files} files and {dirs} folders; {pe_files} .exe/.dll/.pyd checked")
    say(f"longest path: {len(longest)} characters: {longest}")
    for p in problems:
        say(p)
    say(f"{'FAIL' if problems else 'PASS'}: {len(problems)} problems")
    if a.report:
        with open(a.report, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
