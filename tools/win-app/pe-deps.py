#!/usr/bin/env python3
"""The DLLs a Windows program needs from its own folder (stdlib only; runs on a Mac or Windows).

    python3 -I tools/win-app/pe-deps.py DIR ROOT [EXTRA_GLOB ...]

Reads the import tables of DIR/ROOT and, recursively, of every DLL it imports that is in DIR,
plus the files matching each EXTRA_GLOB (DLLs loaded at run time rather than imported, such
as ggml's CPU backends, which ggml-base.dll picks by the CPU's features). Every file read must
be a PE32+ image for x86-64. Imports are classed as:
  - in DIR (matched without regard to case, as Windows does): kept, and walked;
  - Windows system DLLs (KERNEL32, ADVAPI32, WS2_32, ..., the api-ms-win-* API sets): left out;
  - the Visual C++ runtime (MSVCP140, VCRUNTIME140, ...): left out, and reported, since
    Windows does not itself guarantee it (the package has to account for it);
  - anything else: an error (exit 1), since the folder would not start.
Prints, one per line: `keep NAME` (the file's own name in DIR), then `system NAME`, then
`vcruntime NAME`. Nothing is run or written.
"""
import fnmatch
import os
import struct
import sys

SYSTEM = {
    "kernel32.dll", "advapi32.dll", "ws2_32.dll", "crypt32.dll", "shell32.dll", "psapi.dll",
    "ntdll.dll", "bcrypt.dll", "bcryptprimitives.dll", "user32.dll", "ole32.dll",
    "oleaut32.dll", "shlwapi.dll", "secur32.dll", "iphlpapi.dll", "dbghelp.dll", "version.dll",
    "userenv.dll", "winmm.dll", "gdi32.dll", "comctl32.dll", "setupapi.dll", "ncrypt.dll",
}
VCRUNTIME = {
    "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll", "msvcp140_atomic_wait.dll",
    "msvcp140_codecvt_ids.dll", "vcruntime140.dll", "vcruntime140_1.dll", "concrt140.dll",
    "vcomp140.dll",
}


def die(msg):
    print(f"pe-deps: {msg}", file=sys.stderr)
    sys.exit(1)


def imports(path):
    """The DLL names in a PE32+ x86-64 image's import table (and delay-load table)."""
    with open(path, "rb") as f:
        data = f.read()
    name = os.path.basename(path)
    if data[:2] != b"MZ" or len(data) < 0x40:
        die(f"{name}: not a PE image (no MZ header)")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        die(f"{name}: not a PE image (no PE signature)")
    machine, nsect = struct.unpack_from("<HH", data, pe + 4)
    optsize = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", data, opt)[0]
    if machine != 0x8664 or magic != 0x20B:
        die(f"{name}: not PE32+ x86-64 (machine {machine:#x}, magic {magic:#x})")
    ndirs = struct.unpack_from("<I", data, opt + 108)[0]
    dirs = [struct.unpack_from("<II", data, opt + 112 + 8 * i) for i in range(min(ndirs, 16))]
    sections = []
    for i in range(nsect):
        s = opt + optsize + 40 * i
        vsize, va, rawsize, raw = struct.unpack_from("<IIII", data, s + 8)
        sections.append((va, max(vsize, rawsize), raw))

    def off(rva):
        for va, size, raw in sections:
            if va <= rva < va + size:
                return rva - va + raw
        die(f"{name}: RVA {rva:#x} is in no section")

    def cstr(rva):
        o = off(rva)
        end = data.index(b"\0", o)
        return data[o:end].decode("ascii")

    out = []
    for index, size in ((1, 20), (13, 32)):  # the import table; the delay-load table
        if index >= len(dirs) or dirs[index][0] == 0:
            continue
        o = off(dirs[index][0])
        while True:
            entry = data[o:o + size]
            if len(entry) < size or entry == b"\0" * size:
                break
            name_rva = struct.unpack_from("<I", entry, 12 if index == 1 else 4)[0]
            out.append(cstr(name_rva))
            o += size
    return out


def main(argv):
    if len(argv) < 2:
        die("usage: pe-deps.py DIR ROOT [EXTRA_GLOB ...]")
    folder, root, globs = argv[0], argv[1], argv[2:]
    present = {n.lower(): n for n in os.listdir(folder) if os.path.isfile(os.path.join(folder, n))}
    if root.lower() not in present:
        die(f"{root} is not in {folder}")
    start = [present[root.lower()]]
    for g in globs:
        hits = sorted(n for n in present.values() if fnmatch.fnmatch(n.lower(), g.lower()))
        if not hits:
            die(f"nothing in {folder} matches {g}")
        start += hits
    keep, system, vcrt = [], set(), set()
    queue = list(dict.fromkeys(start))
    seen = set(n.lower() for n in queue)
    while queue:
        f = queue.pop(0)
        keep.append(f)
        for d in imports(os.path.join(folder, f)):
            low = d.lower()
            if low in present:
                if low not in seen:
                    seen.add(low)
                    queue.append(present[low])
            elif low in SYSTEM or low.startswith(("api-ms-win-", "ext-ms-")):
                system.add(d)
            elif low in VCRUNTIME:
                vcrt.add(d)
            else:
                die(f"{f} imports {d}, which is neither in {folder} nor part of Windows")
    for n in keep:
        print(f"keep {n}")
    for n in sorted(system, key=str.lower):
        print(f"system {n}")
    for n in sorted(vcrt, key=str.lower):
        print(f"vcruntime {n}")


if __name__ == "__main__":
    main(sys.argv[1:])
