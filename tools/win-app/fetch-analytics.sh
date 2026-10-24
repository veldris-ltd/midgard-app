#!/usr/bin/env bash
# Fetch the Windows app's camera analytics runtime pinned in analytics.lock and lay it out for
# the package (helpers\analytics\ and resources\models\yolox\), on a Mac or on Windows (Git
# Bash). The port of tools/mac-app/fetch-analytics.sh:
#   $ROOT/downloads/   the archive, wheels, model and YOLOX licence (checked by SHA-256)
#   $ROOT/runtime/     a relocatable CPython for x86_64-pc-windows-msvc (python-build-
#       standalone install_only: python.exe, python313.dll, DLLs\, Lib\) with numpy and
#       onnxruntime unzipped into Lib\site-packages, pruned to what the analytics need, the
#       .py files compiled (unchecked-hash .pyc, valid whatever the files' times after the
#       ZIP; the shell runs python.exe -I -B, so nothing is written there at run time).
#       It becomes the package's helpers\analytics\.
#   $ROOT/runtime.stamp  what runtime/ was made from
#   $ROOT/models/yolox_s.onnx
#   $ROOT/licenses/    CPython, numpy (with OpenBLAS's and the GCC runtime's notices, which
#       its LICENSE.txt carries), onnxruntime + ThirdPartyNotices, YOLOX, and a note on
#       python-build-standalone's statically linked libraries
#   $ROOT/manifest.txt  the lock's pins, then every file laid out: SHA-256, size, path
# where ROOT is $MIDGARD_BUILD_DIR/win/analytics (MIDGARD_BUILD_DIR defaults to
# ~/Library/Caches/midgard-build, outside the suite folder; CI sets it in the workspace).
#
# Downloads are untrusted until checked: a file already present is kept only if its SHA-256
# matches the lock; a new one is fetched over HTTPS only and checked before it takes its final
# name. Every archive's member names are checked before anything is extracted (relative, under
# the expected top folder, no "..", no backslash or drive letter, no links) and each goes into
# its own empty staging folder. A wheel is a ZIP: it is unzipped as pip would install it
# (purelib and platlib are the same folder on Windows; neither wheel has a .data folder, which
# is checked), so no pip runs and nothing runs from the downloads. The .pyc are compiled by a
# CPython 3.13 that runs here: on Windows the runtime's own python.exe, on a Mac the CPython
# 3.13.16 that tools/mac-app/analytics.lock pins for this Mac (same version, same bytecode). A
# new runtime replaces the old one only when complete. Re-runs are a no-op when the stamp
# matches the lock and this script.
#
#   bash tools/win-app/fetch-analytics.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

LOCK=tools/win-app/analytics.lock
MAC_LOCK=tools/mac-app/analytics.lock
SELF=tools/win-app/fetch-analytics.sh
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
ROOT="$BUILD_ROOT/win/analytics"
DL="$ROOT/downloads"
SITE=Lib/site-packages

CLEANUP=()
cleanup() { [ ${#CLEANUP[@]} -eq 0 ] || rm -rf "${CLEANUP[@]}"; }
trap cleanup EXIT

die() { echo "fetch-analytics: $*" >&2; exit 1; }

lock_in() {  # file key
  local v
  v=$(grep -E "^$2=" "$1" | head -n1 | cut -d= -f2- | tr -d '\r') || true
  [ -n "$v" ] || die "$1: no $2"
  printf '%s\n' "$v"
}
lock() { lock_in "$LOCK" "$1"; }

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
else
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
fi
for t in curl tar unzip; do command -v "$t" >/dev/null 2>&1 || die "$t is needed"; done

case "$(uname -s)" in
  Darwin) HOST=mac ;;
  MINGW* | MSYS* | CYGWIN*) HOST=windows ;;
  *) die "unsupported build machine $(uname -s) (a Mac, or Windows with Git Bash)" ;;
esac
# A path for a native Windows program (python.exe) on Windows; as it is elsewhere.
native() { if [ "$HOST" = windows ]; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }

# --- the lock ---
[ "$(lock python_version)" = 3.13.16 ] || die "$LOCK: python_version is not 3.13.16 (this script prunes a 3.13 layout)"
ITEMS=(python numpy onnxruntime model yolox_license)
for k in "${ITEMS[@]}"; do
  url=$(lock "${k}_url")
  sum=$(lock "${k}_sha256")
  [[ $url =~ ^https://[^[:space:]]+$ ]] || die "$LOCK: ${k}_url is not an https URL"
  [[ $sum =~ ^[0-9a-f]{64}$ ]] || die "$LOCK: ${k}_sha256 is not 64 hex digits"
done
NUMPY_VERSION=$(basename "$(lock numpy_url)" | cut -d- -f2)
ORT_VERSION=$(basename "$(lock onnxruntime_url)" | cut -d- -f2)

# The local name of a download: the URL's last part, %2B decoded (python-build-standalone's +).
local_name() { basename "$1" | sed 's/%2B/+/g'; }

# --- download and verify ---
mkdir -p "$DL"
fetch() {  # url sha256 → $DL/<name>
  local url=$1 want=$2 name f got
  name=$(local_name "$url")
  [[ $name =~ ^[A-Za-z0-9._+-]+$ ]] || die "$url: unexpected file name $name"
  f="$DL/$name"
  if [ -f "$f" ] && [ "$(sha256 "$f")" = "$want" ]; then
    echo "== $name present, SHA-256 matches"
    return
  fi
  echo "== downloading $url"
  CLEANUP+=("$f.part")
  curl --proto '=https' --proto-redir '=https' -fL --retry 3 -sS -o "$f.part" "$url"
  got=$(sha256 "$f.part")
  [ "$got" = "$want" ] || die "$url: SHA-256 $got does not match ($want)"
  mv -f "$f.part" "$f"
}
for k in "${ITEMS[@]}"; do fetch "$(lock "${k}_url")" "$(lock "${k}_sha256")"; done
PY_TGZ="$DL/$(local_name "$(lock python_url)")"
NUMPY_WHL="$DL/$(local_name "$(lock numpy_url)")"
ORT_WHL="$DL/$(local_name "$(lock onnxruntime_url)")"
MODEL_DL="$DL/$(local_name "$(lock model_url)")"
YOLOX_LICENSE="$DL/$(local_name "$(lock yolox_license_url)")"

# A member name is safe: relative, no "..", no backslash, no drive letter, no control characters.
bad_names() {  # top-folder-regex (reads names on stdin; prints the first few bad ones)
  LC_ALL=C awk -v top="$1" '
    /^\// || /(^|\/)\.\.(\/|$)/ || /\\/ || /^[A-Za-z]:/ || /[\001-\037]/ || ($0 !~ top) { print; n++ }
    n >= 3 { exit }'
}

# Extract the python-build-standalone archive into an empty folder, members checked first.
# The Mac build tool's archive (never shipped) may hold symbolic links to names in the same
# folder (bin/python3 -> python3.13): links-ok allows those, and those only.
extract_python() {  # tgz dest [links-ok]
  local tgz=$1 dest=$2 links=${3:-} bad
  bad=$(tar tzf "$tgz" | bad_names '^python(/|$)')
  [ -z "$bad" ] || die "$tgz: unexpected member names: $bad"
  # Regular files and folders only (tar tv's first column): no links, devices or FIFOs.
  bad=$(tar tvzf "$tgz" | awk -v links="$links" '
    { t = substr($1, 1, 1) }
    t == "-" || t == "d" { next }
    t == "l" && links == "links-ok" && $(NF - 1) == "->" && $NF !~ /\// && $NF != ".." { next }
    { print }' | head -3)
  [ -z "$bad" ] || die "$tgz: members that are not files or folders: $bad"
  mkdir "$dest"
  tar xzf "$tgz" -C "$dest"
}

# Unzip a wheel into site-packages, as pip installs it, members checked first.
install_wheel() {  # whl site-packages
  local whl=$1 site=$2 names bad
  names=$(unzip -Z1 "$whl")
  bad=$(printf '%s\n' "$names" | bad_names '^[A-Za-z0-9_.-]+(/|$)')
  [ -z "$bad" ] || die "$whl: unexpected member names: $bad"
  if printf '%s\n' "$names" | grep -q '^[^/]*\.data/'; then die "$whl: has a .data folder (not handled)"; fi
  # Symbolic links in a ZIP carry the Unix S_IFLNK mode (zipinfo shows "l").
  if unzip -Z -l "$whl" | awk 'substr($1, 1, 1) == "l"' | grep -q .; then die "$whl: has links"; fi
  unzip -q -o "$whl" -d "$site"
}

# What the analytics do not need, or must not ship: headers, import libraries, debug symbols,
# Tk, the test modules, pythonw, pip and ensurepip, the scripts folder.
prune() {  # runtime dir
  local p=$1
  rm -rf "$p/include" "$p/libs" "$p/tcl" "$p/Scripts" "$p/pythonw.exe"
  find "$p" -name '*.pdb' -type f -exec rm -f {} +
  rm -f "$p"/DLLs/_tkinter.* "$p"/DLLs/tcl*.dll "$p"/DLLs/tk*.dll \
    "$p"/DLLs/_test*.pyd "$p"/DLLs/_ctypes_test.pyd "$p"/DLLs/xxlimited*.pyd "$p"/DLLs/_xxtestfuzz.pyd \
    "$p"/DLLs/_xxinterpchannels.pyd "$p"/DLLs/_xxsubinterpreters.pyd
  rm -rf "$p/Lib"/{idlelib,tkinter,turtledemo,ensurepip,pydoc_data,venv,test,lib2to3,__phello__} \
    "$p/Lib/turtle.py" "$p/Lib/__hello__.py"
  rm -rf "$p/$SITE"/pip "$p/$SITE"/pip-*.dist-info "$p/$SITE/bin"
  rm -rf "$p/$SITE/numpy/f2py" "$p/$SITE"/onnxruntime/{transformers,quantization,tools,datasets}
  find "$p/$SITE" -type d -name tests -prune -exec rm -rf {} +
  find "$p" -type d -name __pycache__ -prune -exec rm -rf {} +
}

# --- a CPython 3.13 that runs here, for compileall ---
SCRIPT_SHA=$(sha256 "$SELF")
TOOL_PY=
if [ "$HOST" = mac ]; then
  case "$(uname -m)" in
    arm64) MAC_ARCH=arm64 ;;
    x86_64) MAC_ARCH=x64 ;;
    *) die "unsupported Mac $(uname -m)" ;;
  esac
  [ "$(lock_in "$MAC_LOCK" python_version)" = "$(lock python_version)" ] \
    || die "$MAC_LOCK pins CPython $(lock_in "$MAC_LOCK" python_version), not $(lock python_version)"
  fetch "$(lock_in "$MAC_LOCK" "${MAC_ARCH}_python_url")" "$(lock_in "$MAC_LOCK" "${MAC_ARCH}_python_sha256")"
  TOOL="$ROOT/tool"
  tool_stamp="python=$(lock_in "$MAC_LOCK" "${MAC_ARCH}_python_sha256") script=$SCRIPT_SHA"
  if [ ! -x "$TOOL/python/bin/python3.13" ] || [ "$(cat "$TOOL.stamp" 2>/dev/null)" != "$tool_stamp" ]; then
    echo "== the build tool runtime ($MAC_ARCH macOS, compileall only; never shipped)"
    rm -rf "$TOOL" "$TOOL.stamp" "$TOOL.new"
    CLEANUP+=("$TOOL.new")
    extract_python "$DL/$(local_name "$(lock_in "$MAC_LOCK" "${MAC_ARCH}_python_url")")" "$TOOL.new" links-ok
    mv "$TOOL.new" "$TOOL"
    printf '%s\n' "$tool_stamp" > "$TOOL.stamp"
  fi
  TOOL_PY="$TOOL/python/bin/python3.13"
fi

# --- the runtime ---
mkdir -p "$ROOT"
stamp="python=$(lock python_sha256) numpy=$(lock numpy_sha256) onnxruntime=$(lock onnxruntime_sha256) script=$SCRIPT_SHA"
RT="$ROOT/runtime"
if [ -f "$RT/python.exe" ] && [ "$(cat "$ROOT/runtime.stamp" 2>/dev/null)" = "$stamp" ]; then
  echo "== runtime: up to date (stamp matches)"
else
  echo "== laying out the runtime"
  stage="$ROOT/staging"
  out="$ROOT/runtime.new"
  CLEANUP+=("$stage" "$out")
  rm -rf "$stage" "$out"
  extract_python "$PY_TGZ" "$stage"
  mv "$stage/python" "$out"
  rm -rf "$stage"
  [ -f "$out/python.exe" ] && [ -f "$out/python313.dll" ] && [ -d "$out/$SITE" ] \
    || die "$PY_TGZ: not the expected layout (python.exe, python313.dll, $SITE)"
  install_wheel "$NUMPY_WHL" "$out/$SITE"
  install_wheel "$ORT_WHL" "$out/$SITE"
  [ -f "$out/$SITE/numpy/__init__.py" ] && [ -f "$out/$SITE/onnxruntime/__init__.py" ] \
    || die "the wheels did not give numpy/ and onnxruntime/"
  prune "$out"
  [ "$HOST" = windows ] && TOOL_PY="$out/python.exe"
  # unchecked-hash: valid whatever the files' times (a ZIP keeps them to 2 s only).
  "$TOOL_PY" -I -m compileall -q -j 0 --invalidation-mode unchecked-hash "$(native "$out/Lib")" >/dev/null \
    || die "compileall failed"
  links=$(find "$out" -type l)
  [ -z "$links" ] || die "symlinks in the runtime: $links"
  rm -f "$ROOT/runtime.stamp"
  rm -rf "$RT.old"
  [ -e "$RT" ] && mv "$RT" "$RT.old"
  mv "$out" "$RT"
  rm -rf "$RT.old"
  printf '%s\n' "$stamp" > "$ROOT/runtime.stamp"
fi

# --- the model and the licences ---
mkdir -p "$ROOT/models" "$ROOT/licenses"
MODEL="$ROOT/models/yolox_s.onnx"
if [ ! -f "$MODEL" ] || [ "$(sha256 "$MODEL")" != "$(lock model_sha256)" ]; then
  cp "$MODEL_DL" "$MODEL.part" && mv -f "$MODEL.part" "$MODEL"
fi
L="$ROOT/licenses"
cp "$RT/LICENSE.txt" "$L/CPython-LICENSE.txt"
cp "$RT/$SITE/numpy-$NUMPY_VERSION.dist-info/licenses/LICENSE.txt" "$L/numpy-LICENSE.txt"
grep -qi openblas "$L/numpy-LICENSE.txt" || die "numpy's LICENSE.txt has no OpenBLAS notice"
cp "$RT/$SITE/onnxruntime/LICENSE" "$L/onnxruntime-LICENSE.txt"
cp "$RT/$SITE/onnxruntime/ThirdPartyNotices.txt" "$L/onnxruntime-ThirdPartyNotices.txt"
cp "$YOLOX_LICENSE" "$L/YOLOX-LICENSE.txt"
check='import numpy, onnxruntime as o; print("numpy", numpy.__version__, "onnxruntime", o.__version__, o.get_available_providers())'
if [ "$HOST" = windows ]; then
  versions=$("$RT/python.exe" -I -B -c '
import ssl, sqlite3, pyexpat, zlib, _decimal
print("  OpenSSL:", ssl.OPENSSL_VERSION)
print("  SQLite:", sqlite3.sqlite_version)
print("  Expat:", pyexpat.EXPAT_VERSION)
print("  libmpdec:", _decimal.__libmpdec_version__)
print("  zlib:", zlib.ZLIB_VERSION)
' | tr -d '\r')
else
  versions="  (versions not reported: laid out on a Mac, where python.exe does not run)"
fi
cat > "$L/python-build-standalone-NOTICE.txt" <<EOF
The camera analytics' Python runtime in the Windows app (helpers\\analytics\\) is CPython
$(lock python_version) as built by python-build-standalone
(https://github.com/astral-sh/python-build-standalone, release $(lock python_release), the
install_only build for x86_64-pc-windows-msvc). CPython is under the PSF License Agreement
(CPython-LICENSE.txt, which also carries the notices of the third-party code in CPython's own
sources). The build carries these libraries, in python313.dll, DLLs\\ and the extension modules
(versions as the runtime reports them); each is under its own licence:

$versions
  OpenSSL (libcrypto-3-x64.dll, libssl-3-x64.dll): Apache License 2.0 (https://www.openssl.org/source/license.html)
  SQLite (sqlite3.dll): public domain (https://sqlite.org/copyright.html)
  Expat: MIT (https://github.com/libexpat/libexpat/blob/master/COPYING)
  libmpdec: BSD 2-clause (https://www.bytereef.org/mpdecimal/)
  libffi (libffi-8.dll): MIT (https://github.com/libffi/libffi/blob/master/LICENSE)
  zlib: zlib licence (https://zlib.net/zlib_license.html)
  bzip2: bzip2 licence, BSD-style (https://sourceware.org/bzip2/)
  XZ Utils (liblzma): 0BSD / public domain (https://tukaani.org/xz/)
  HACL* (hashlib): MIT (https://github.com/hacl-star/hacl-star)
  The Microsoft Visual C++ runtime (vcruntime140.dll, vcruntime140_1.dll): redistributable
    under the Visual Studio licence terms, as CPython's own Windows installer ships it.
The Tcl/Tk libraries of the build are not bundled. numpy, onnxruntime and the YOLOX model:
numpy-LICENSE.txt (BSD-3-Clause, with OpenBLAS and the GCC runtime libraries its wheel
bundles in numpy.libs\\), onnxruntime-LICENSE.txt (MIT) with
onnxruntime-ThirdPartyNotices.txt, and YOLOX-LICENSE.txt (Apache-2.0).
EOF

# --- the manifest ---
{
  echo "# The Windows app's camera analytics runtime, laid out by $SELF on $HOST ($(date -u +%Y-%m-%dT%H:%M:%SZ))."
  echo "# From $LOCK (dated $(lock date)):"
  for k in "${ITEMS[@]}"; do echo "#   $(local_name "$(lock "${k}_url")") $(lock "${k}_sha256")"; done
  echo "# script $SCRIPT_SHA"
  echo "# Then SHA-256, size in bytes and path of every file under $ROOT:"
  (cd "$ROOT" && find runtime models licenses -type f | LC_ALL=C sort | while IFS= read -r f; do
    printf '%s %s %s\n' "$(sha256 "$f")" "$(wc -c < "$f" | tr -d ' ')" "$f"
  done)
} > "$ROOT/manifest.txt.part"
mv -f "$ROOT/manifest.txt.part" "$ROOT/manifest.txt"

# --- report ---
files=$(grep -vc '^#' "$ROOT/manifest.txt")
echo "== runtime: $RT ($(du -sh "$RT" | awk '{print $1}'), numpy $NUMPY_VERSION, onnxruntime $ORT_VERSION)"
echo "   site-packages $(du -sh "$RT/$SITE" | awk '{print $1}'); $(find "$RT" -name '*.pyc' | wc -l | tr -d ' ') .pyc"
if [ "$HOST" = windows ]; then
  "$RT/python.exe" -I -B -c "$check" 2>&1 | sed 's/^/   /'
else
  echo "   (python.exe is checked on Windows: the windows-analytics CI job)"
fi
echo "== model: $MODEL ($(du -h "$MODEL" | awk '{print $1}'))"
echo "== licences: $(ls "$L" | tr '\n' ' ')"
echo "== manifest: $ROOT/manifest.txt ($files files)"
echo "== done: $ROOT/{runtime,models,licenses}"
