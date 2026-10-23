#!/usr/bin/env bash
# Fetch the camera analytics runtime pinned in analytics.lock and lay it out for the app
# bundle (build-app.sh):
#   $MIDGARD_BUILD_DIR/analytics/downloads/   the archives, wheels, model and YOLOX licence
#   $MIDGARD_BUILD_DIR/analytics/runtime/{arm64,x64}/  a relocatable CPython (python-build-
#       standalone install_only) with numpy and onnxruntime in its site-packages, pruned to
#       what the analytics need, the standard library compiled (unchecked-hash .pyc, so the
#       signed bundle is never written to), every Mach-O signed (ad hoc if it did not verify)
#   $MIDGARD_BUILD_DIR/analytics/runtime/{arm64,x64}.stamp  (what each folder was made from)
#   $MIDGARD_BUILD_DIR/analytics/tool/python  this Mac's runtime unpruned: runs pip and compileall
#       for both architectures (never bundled)
#   $MIDGARD_BUILD_DIR/analytics/models/yolox_s.onnx
#   $MIDGARD_BUILD_DIR/analytics/licenses/  CPython, numpy, onnxruntime (+ third-party notices),
#       YOLOX, and a note on python-build-standalone's statically linked libraries
# (MIDGARD_BUILD_DIR defaults to ~/Library/Caches/midgard-build, outside the suite folder.)
#
# Downloads are untrusted until checked: a file already present is kept only if its SHA-256
# matches analytics.lock; a new one is fetched over HTTPS only and checked before it takes its
# final name. Each archive's member names are checked (no absolute paths, no ..) and it is
# extracted into its own empty staging folder; the wheels are installed by the runtime's own
# pip from the checked files (--no-index --no-deps, binary only, for the target platform, so
# the x64 runtime is assembled with this Mac's runtime and needs no Rosetta). Python always
# runs with -I. The bundle must hold no symlinks: the runtime's (bin/python3 → python3.13 …)
# are removed with the parts it does not need, and any left fails the script. A new folder
# replaces the old one only when complete.
#
# Re-runs are a no-op: an arch folder whose stamp matches the lock's SHA-256s and this
# script's hash is left untouched. Prints the sizes and each runtime's numpy and onnxruntime
# versions and execution providers (x64 under Rosetta, if installed).
#
#   bash tools/mac-app/fetch-analytics.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

LOCK=tools/mac-app/analytics.lock
SELF=tools/mac-app/fetch-analytics.sh
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
ROOT="$BUILD_ROOT/analytics"
DL="$ROOT/downloads"
ARCHES=(arm64 x64)
PY=python3.13
SITE="lib/$PY/site-packages"
YOLOX_LICENSE_URL=https://raw.githubusercontent.com/Megvii-BaseDetection/YOLOX/0.1.1rc0/LICENSE
YOLOX_LICENSE_SHA256=577c03d505ec80f667ebf96ebd0cc4f6825c817ca1088ee348c48aaabd51bd92

CLEANUP=()
cleanup() { [ ${#CLEANUP[@]} -eq 0 ] || rm -rf "${CLEANUP[@]}"; }
trap cleanup EXIT

die() { echo "fetch-analytics: $*" >&2; exit 1; }

lock() {
  local v
  v=$(grep -E "^$1=" "$LOCK" | head -n1 | cut -d= -f2-) || true
  [ -n "$v" ] || die "$LOCK: no $1"
  printf '%s\n' "$v"
}

sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

is_macho() {
  case "$(od -An -tx1 -N4 "$1" 2>/dev/null | tr -d ' \n')" in
    feedface|cefaedfe|feedfacf|cffaedfe|cafebabe|bebafeca) return 0 ;;
    *) return 1 ;;
  esac
}

# --- the lock ---
[ "$(lock python_version)" = 3.13.16 ] || die "$LOCK: python_version is not 3.13.16 (this script prunes a 3.13 layout)"
ITEMS=(model)
for a in "${ARCHES[@]}"; do ITEMS+=("${a}_python" "${a}_numpy" "${a}_onnxruntime"); done
for k in "${ITEMS[@]}"; do
  url=$(lock "${k}_url")
  sum=$(lock "${k}_sha256")
  [[ $url =~ ^https://[^[:space:]]+$ ]] || die "$LOCK: ${k}_url is not an https URL"
  [[ $sum =~ ^[0-9a-f]{64}$ ]] || die "$LOCK: ${k}_sha256 is not 64 hex digits"
done

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
fetch "$YOLOX_LICENSE_URL" "$YOLOX_LICENSE_SHA256"

SCRIPT_SHA=$(sha256 "$SELF")
mkdir -p "$ROOT/runtime"

# Extract a python-build-standalone archive into an empty folder, member names checked first.
extract() {  # tgz dest
  local tgz=$1 dest=$2 bad
  bad=$(tar tzf "$tgz" | awk '/^\// || /(^|\/)\.\.(\/|$)/ || !/^python(\/|$)/' | head -3)
  [ -z "$bad" ] || die "$tgz: unexpected member names: $bad"
  mkdir "$dest"
  tar xzf "$tgz" -C "$dest"
  [ -x "$dest/python/bin/$PY" ] || die "$tgz: no python/bin/$PY"
}

# What the analytics do not need (or must not ship: symlinks, headers, Tk, pip).
prune() {  # python dir
  local p=$1 f
  rm -rf "$p/include" "$p/share" "$p/lib/pkgconfig" "$p/lib/libpython3.13.dylib" \
    "$p"/lib/libtcl* "$p"/lib/tcl* "$p"/lib/tk* "$p"/lib/itcl* "$p"/lib/thread*
  for f in "$p"/bin/*; do [ "$(basename "$f")" = "$PY" ] || rm -f "$f"; done
  rm -rf "$p/lib/$PY"/{idlelib,tkinter,turtledemo,ensurepip,pydoc_data,config-3.13-darwin} \
    "$p/lib/$PY/turtle.py" "$p/lib/$PY"/lib-dynload/_tkinter.*
  rm -rf "$p/$SITE"/pip "$p/$SITE"/pip-*.dist-info "$p/$SITE/bin"
  rm -rf "$p/$SITE/numpy/f2py" "$p/$SITE"/onnxruntime/{transformers,quantization,tools,datasets}
  find "$p/$SITE" -type d -name tests -prune -exec rm -rf {} +
  find "$p" -type d -name __pycache__ -prune -exec rm -rf {} +
}

# --- an unpruned copy of this Mac's runtime runs pip and compileall for both (not bundled) ---
case "$(uname -m)" in
  arm64) HOST=arm64 ;;
  x86_64) HOST=x64 ;;
  *) die "unsupported build machine $(uname -m)" ;;
esac
TOOL="$ROOT/tool"
tool_stamp="python=$(lock "${HOST}_python_sha256") script=$SCRIPT_SHA"
if [ ! -x "$TOOL/python/bin/$PY" ] || [ "$(cat "$TOOL.stamp" 2>/dev/null)" != "$tool_stamp" ]; then
  echo "== the build tool runtime ($HOST)"
  rm -rf "$TOOL" "$TOOL.stamp"
  CLEANUP+=("$TOOL.new")
  rm -rf "$TOOL.new"
  extract "$DL/$(local_name "$(lock "${HOST}_python_url")")" "$TOOL.new"
  mv "$TOOL.new" "$TOOL"
  printf '%s\n' "$tool_stamp" > "$TOOL.stamp"
fi
TOOL_PY="$TOOL/python/bin/$PY"

layout() {  # arch
  local a=$1 stage out tgz plats
  stage="$ROOT/runtime/staging-$a"
  out="$ROOT/runtime/$a.new"
  CLEANUP+=("$stage" "$out")
  rm -rf "$stage" "$out"
  tgz="$DL/$(local_name "$(lock "${a}_python_url")")"
  extract "$tgz" "$stage"
  mv "$stage/python" "$out"
  rm -rf "$stage"
  if [ "$a" = arm64 ]; then
    plats=(--platform macosx_11_0_arm64 --platform macosx_13_0_arm64)
  else
    plats=(--platform macosx_10_13_x86_64 --platform macosx_13_0_x86_64)
  fi
  # The tool runtime's pip installs the checked wheels for the target platform.
  "$TOOL_PY" -I -m pip install --disable-pip-version-check --no-cache-dir --no-index --no-deps \
    --no-compile --only-binary=:all: "${plats[@]}" --python-version 3.13 --implementation cp \
    --abi cp313 --target "$out/$SITE" --upgrade \
    "$DL/$(local_name "$(lock "${a}_numpy_url")")" "$DL/$(local_name "$(lock "${a}_onnxruntime_url")")" \
    | sed 's/^/   /'
  prune "$out"
  # Compiled once here (unchecked-hash: valid whatever the files' times), so the runtime never
  # writes .pyc into the signed bundle (it runs with -B too).
  "$TOOL_PY" -I -m compileall -q -j 0 --invalidation-mode unchecked-hash "$out/lib/$PY" >/dev/null \
    || die "compileall failed for $a"
  links=$(find "$out" -type l)
  [ -z "$links" ] || die "symlinks left in the $a runtime: $links"
  # Every Mach-O: ad hoc signed if it does not verify, then checked (build-app.sh signs again).
  xattr -cr "$out"
  local n=0 f
  while IFS= read -r -d '' f; do
    is_macho "$f" || continue
    n=$((n + 1))
    codesign -v "$f" 2>/dev/null || codesign --force -s - "$f"
    codesign -v "$f" || die "$f: signature does not verify"
  done < <(find "$out" -type f -print0)
  echo "   $n Mach-O files"
  rm -f "$ROOT/runtime/$a.stamp"
  rm -rf "$ROOT/runtime/$a.old"
  [ -e "$ROOT/runtime/$a" ] && mv "$ROOT/runtime/$a" "$ROOT/runtime/$a.old"
  mv "$out" "$ROOT/runtime/$a"
  rm -rf "$ROOT/runtime/$a.old"
}

for a in "${ARCHES[@]}"; do
  stamp="python=$(lock "${a}_python_sha256") numpy=$(lock "${a}_numpy_sha256") onnxruntime=$(lock "${a}_onnxruntime_sha256") script=$SCRIPT_SHA"
  if [ -x "$ROOT/runtime/$a/bin/$PY" ] && [ -f "$ROOT/runtime/$a.stamp" ] && [ "$(cat "$ROOT/runtime/$a.stamp")" = "$stamp" ]; then
    echo "== $a runtime: up to date (stamp matches)"
    continue
  fi
  echo "== laying out the $a runtime"
  layout "$a"
  printf '%s\n' "$stamp" > "$ROOT/runtime/$a.stamp"
done

# --- the model and the licences ---
mkdir -p "$ROOT/models" "$ROOT/licenses"
MODEL="$ROOT/models/yolox_s.onnx"
if [ ! -f "$MODEL" ] || [ "$(sha256 "$MODEL")" != "$(lock model_sha256)" ]; then
  cp "$DL/yolox_s.onnx" "$MODEL.part" && mv -f "$MODEL.part" "$MODEL"
fi
L="$ROOT/licenses"
H="$ROOT/runtime/$HOST"
cp "$H/lib/$PY/LICENSE.txt" "$L/CPython-LICENSE.txt"
cp "$H/$SITE/numpy-2.5.4.dist-info/licenses/LICENSE.txt" "$L/numpy-LICENSE.txt"
cp "$H/$SITE/onnxruntime/LICENSE" "$L/onnxruntime-LICENSE.txt"
cp "$H/$SITE/onnxruntime/ThirdPartyNotices.txt" "$L/onnxruntime-ThirdPartyNotices.txt"
cp "$DL/LICENSE" "$L/YOLOX-LICENSE.txt"
versions=$("$H/bin/$PY" -I -B -c '
import ssl, sqlite3, pyexpat, zlib, _decimal
print("OpenSSL:", ssl.OPENSSL_VERSION)
print("SQLite:", sqlite3.sqlite_version)
print("Expat:", pyexpat.EXPAT_VERSION)
print("libmpdec:", _decimal.__libmpdec_version__)
print("zlib (system):", zlib.ZLIB_VERSION)
')
cat > "$L/python-build-standalone-NOTICE.txt" <<EOF
The camera analytics' Python runtime in Midgard.app is CPython $(lock python_version) as built by
python-build-standalone (https://github.com/astral-sh/python-build-standalone, release
$(lock python_release), install_only builds for aarch64-apple-darwin and x86_64-apple-darwin).
CPython is under the PSF License Agreement (CPython-LICENSE.txt, which also carries the notices of
the third-party code in CPython's own sources). python-build-standalone compiles these libraries
into the interpreter and its extension modules (versions as reported by the runtime where it
reports one); each is under its own licence:

$versions
  OpenSSL: Apache License 2.0 (https://www.openssl.org/source/license.html)
  SQLite: public domain (https://sqlite.org/copyright.html)
  Expat: MIT (https://github.com/libexpat/libexpat/blob/master/COPYING)
  libmpdec: BSD 2-clause (https://www.bytereef.org/mpdecimal/)
  libffi: MIT (https://github.com/libffi/libffi/blob/master/LICENSE)
  bzip2: bzip2 licence, BSD-style (https://sourceware.org/bzip2/)
  XZ Utils (liblzma): 0BSD / public domain (https://tukaani.org/xz/)
  HACL* (hashlib): MIT (https://github.com/hacl-star/hacl-star)
The Tcl/Tk libraries of the build are not bundled. numpy, onnxruntime and the YOLOX model:
numpy-LICENSE.txt (BSD-3-Clause, with OpenBLAS and the GCC runtime libraries its wheel
bundles), onnxruntime-LICENSE.txt (MIT) with onnxruntime-ThirdPartyNotices.txt, and
YOLOX-LICENSE.txt (Apache-2.0).
EOF

# --- report ---
for a in "${ARCHES[@]}"; do
  echo "== $a: $ROOT/runtime/$a ($(du -sh "$ROOT/runtime/$a" | awk '{print $1}'))"
  echo "   $(file -b "$ROOT/runtime/$a/bin/$PY")"
done
check='import numpy, onnxruntime as o; print("numpy", numpy.__version__, "onnxruntime", o.__version__, o.get_available_providers())'
echo "== arm64"
"$ROOT/runtime/arm64/bin/$PY" -I -B -c "$check" 2>&1 | sed 's/^/   /'
echo "== x64 (Rosetta)"
if arch -x86_64 /usr/bin/true 2>/dev/null; then
  arch -x86_64 "$ROOT/runtime/x64/bin/$PY" -I -B -c "$check" 2>&1 | sed 's/^/   /'
else
  echo "   skipped: Rosetta is not installed (softwareupdate --install-rosetta)"
fi
echo "== model: $MODEL ($(du -h "$MODEL" | awk '{print $1}'))"
echo "== licences: $(ls "$L" | tr '\n' ' ')"
echo "== done: $ROOT/{runtime/{arm64,x64},models,licenses}"
