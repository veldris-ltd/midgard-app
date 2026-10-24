#!/usr/bin/env python3
"""Every DLL a Windows package's programs import can be found on a clean Windows 11 PC (stdlib only).

    python -I tools/win-app/check-imports.py PACKAGE [--report FILE]

Reads the import and delay-load tables of every PE file under PACKAGE (found by its MZ/PE
headers, whatever its extension; each must be PE32+ x86-64), with tools/win-app/pe-deps.py's
reader. Each imported DLL must be, matched without regard to case as Windows does:
  (a) in the importing file's own folder, or in a folder the loader also searches for it:
      - a file under a folder holding python.exe (helpers\\analytics\\): that folder, the
        application directory, which CPython searches for an extension module's DLLs
        (LoadLibraryExW with LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR);
      - for numpy's modules, <site-packages>\\numpy.libs, which numpy\\__init__.py adds with
        os.add_dll_directory (checked: the line must be there);
  (b) part of every Windows 11 x64 install: an API set (api-ms-win-*, ext-ms-*) or a name in
      WINDOWS below. MSVCP140*, VCRUNTIME140*, CONCRT140 and VCOMP140 are NOT: they come with
      the Visual C++ Redistributable, which a clean Windows 11 does not have, so the package
      must carry them (app-local, beside the programs that import them);
  (c) a delay-load named in OPTIONAL_DELAY_LOADS, which the program loads only when present.
Anything else is listed (`MISSING <file> imports <dll>`) and the exit status is 1. Prints a
summary line, and writes everything printed to --report too. Nothing is run.
"""
import argparse
import importlib.util
import os
import sys

_spec = importlib.util.spec_from_file_location("pe_deps", os.path.join(os.path.dirname(os.path.abspath(__file__)), "pe-deps.py"))
pe = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pe)

# System32 DLLs present on every Windows 11 x64 install (Home, Pro, Enterprise, Education; the
# N editions too, so nothing of the Media Feature Pack: no mf*.dll, which midgard-frames.exe
# loads at run time rather than imports). Source: the KnownDLLs of Windows 11 24H2
# (HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs) plus the documented
# Win32 API DLLs of the Windows SDK's umbrella libraries (kernel32.lib, user32.lib, ...),
# as listed on learn.microsoft.com for each function's "DLL" requirement. Kept explicit and
# short: a name is added when a shipped binary imports it and it is such a DLL.
WINDOWS = {
    # KnownDLLs (Windows 11 24H2)
    "advapi32.dll", "clbcatq.dll", "combase.dll", "comdlg32.dll", "coml2.dll", "difxapi.dll",
    "gdi32.dll", "gdiplus.dll", "imagehlp.dll", "imm32.dll", "kernel32.dll", "msctf.dll",
    "msvcrt.dll", "normaliz.dll", "nsi.dll", "ole32.dll", "oleaut32.dll", "psapi.dll",
    "rpcrt4.dll", "sechost.dll", "setupapi.dll", "shcore.dll", "shell32.dll", "shlwapi.dll",
    "user32.dll", "wldap32.dll", "wow64.dll", "wow64base.dll", "wow64con.dll", "wow64cpu.dll",
    "wow64win.dll", "ws2_32.dll", "xinput1_3.dll",
    # Always loaded / core user mode
    "ntdll.dll", "kernelbase.dll", "ucrtbase.dll", "win32u.dll", "gdi32full.dll",
    # Security, crypto, networking
    "bcrypt.dll", "bcryptprimitives.dll", "crypt32.dll", "ncrypt.dll", "secur32.dll",
    "sspicli.dll", "wintrust.dll", "iphlpapi.dll", "dnsapi.dll", "winhttp.dll", "wininet.dll",
    "mswsock.dll", "netapi32.dll", "userenv.dll", "wtsapi32.dll", "authz.dll",
    # Shell, UI and graphics
    "comctl32.dll", "uxtheme.dll", "dwmapi.dll", "propsys.dll", "dxgi.dll", "d3d11.dll",
    "d3d12.dll", "d3dcompiler_47.dll", "dcomp.dll", "dwrite.dll", "d2d1.dll",
    "windowscodecs.dll", "oleacc.dll", "uiautomationcore.dll", "msimg32.dll", "winspool.drv",
    # Other system services
    "version.dll", "winmm.dll", "dbghelp.dll", "dbgcore.dll", "powrprof.dll", "cfgmgr32.dll",
    "pdh.dll", "wevtapi.dll", "mpr.dll", "ntmarta.dll", "profapi.dll", "rstrtmgr.dll",
}

# Delay-loaded DLLs a program loads only when they are present, with why missing is fine.
# None is needed today.
OPTIONAL_DELAY_LOADS = {}


def is_pe(path):
    try:
        with open(path, "rb") as f:
            head = f.read(0x40)
            if len(head) < 0x40 or head[:2] != b"MZ":
                return False
            f.seek(int.from_bytes(head[0x3C:0x40], "little"))
            return f.read(4) == b"PE\0\0"
    except OSError:
        return False


def listing(folder, cache={}):
    if folder not in cache:
        cache[folder] = {n.lower() for n in os.listdir(folder)} if os.path.isdir(folder) else set()
    return cache[folder]


def search_dirs(path, package):
    """The folders the loader searches for this file's imports, besides System32."""
    here = os.path.dirname(path)
    dirs = [here]
    d = here
    while True:
        if "python.exe" in listing(d) and d not in dirs:
            dirs.append(d)
        if os.path.normcase(os.path.abspath(d)) == os.path.normcase(os.path.abspath(package)):
            break
        parent = os.path.dirname(d)
        if parent == d:
            break
        d = parent
    # numpy's modules: <site-packages>/numpy/... search <site-packages>/numpy.libs.
    parts = os.path.normpath(path).split(os.sep)
    if "numpy" in parts[:-1]:
        site = os.sep.join(parts[: parts.index("numpy")])
        init = os.path.join(site, "numpy", "__init__.py")
        libs = os.path.join(site, "numpy.libs")
        if os.path.isdir(libs) and os.path.isfile(init):
            with open(init, encoding="utf-8", errors="replace") as f:
                if "add_dll_directory" in f.read():
                    dirs.append(libs)
    return dirs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("package")
    ap.add_argument("--report", default=None)
    a = ap.parse_args()

    out, missing, files, vcrt = [], [], 0, {}
    for d, subdirs, names in os.walk(a.package):
        subdirs.sort()
        for n in sorted(names):
            p = os.path.join(d, n)
            if os.path.islink(p) or not is_pe(p):
                continue
            files += 1
            rel = os.path.relpath(p, a.package)
            dirs = search_dirs(p, a.package)
            for dll, delayed in pe.imports_delayed(p):
                low = dll.lower()
                if any(low in listing(s) for s in dirs):
                    if low.startswith(("msvcp140", "vcruntime140", "concrt140", "vcomp140")):
                        vcrt.setdefault(low, set()).add(os.path.relpath(next(s for s in dirs if low in listing(s)), a.package))
                    continue
                if low in WINDOWS or low.startswith(("api-ms-win-", "ext-ms-")):
                    continue
                if delayed and low in OPTIONAL_DELAY_LOADS:
                    continue
                missing.append(f"MISSING {rel} imports {dll}{' (delay-loaded)' if delayed else ''}")
    out += missing
    for low, where in sorted(vcrt.items()):
        out.append(f"vc-runtime {low}: found in {', '.join(sorted(where))}")
    names = sorted({m.split(' imports ')[1].split(' ')[0].lower() for m in missing})
    out.append(f"{'FAIL' if missing else 'PASS'}: {files} PE files under {a.package}; "
               f"{len(missing)} imports not found" + (f" ({', '.join(names)})" if names else ""))
    print("\n".join(out))
    if a.report:
        with open(a.report, "w", encoding="utf-8") as f:
            f.write("\n".join(out) + "\n")
    sys.exit(1 if missing else 0)


if __name__ == "__main__":
    main()
