#!/usr/bin/env bash
# Fetch the Windows llama.cpp model server pinned in tools/mac-app/llama.lock (the official
# Windows CPU x64 build of the same release as the Mac app's) and lay it out for the package's
# helpers\llama\, on a Mac or on Windows (Git Bash):
#   $ROOT/downloads/   the release ZIP and the tag's LICENSE (checked by SHA-256)
#   $ROOT/server/      llama-server.exe and the DLLs it loads, LICENSE (llama.cpp, MIT) and
#                      LICENSE-LLVM-OpenMP (for libomp.dll); it becomes helpers\llama\
#   $ROOT/server.stamp what server/ was made from
#   $ROOT/manifest.txt the lock's pins, then SHA-256, size and name of every file in server/
# where ROOT is $MIDGARD_BUILD_DIR/win/llama (MIDGARD_BUILD_DIR defaults to
# ~/Library/Caches/midgard-build, outside the suite folder; CI sets it in the workspace).
#
# The DLLs: tools/win-app/pe-deps.py walks the import tables from llama-server.exe (a stub
# that imports llama-server-impl.dll, which imports llama-common, mtmd, llama, ggml,
# ggml-base and libomp), plus every ggml-cpu-*.dll: those are not imported but loaded at run
# time by ggml-base.dll, which picks the best one for the CPU (x64, sse42 ... zen4), so all
# are kept. An import that is neither in the release nor part of Windows fails the script.
# Everything else in the release is left out: the other tools (llama-cli, llama-bench, ...,
# ggml-rpc-server.exe) and ggml-rpc.dll (the network backend, which ggml would otherwise load
# from the folder). The Visual C++ runtime the DLLs import (MSVCP140, VCRUNTIME140,
# VCRUNTIME140_1) is not in the release and is reported, not shipped, here: the package gets
# it app-local from the shell artefact's vcruntime/ (tools/ci/run.sh windows_vcruntime).
#
# Downloads are untrusted until checked: a file already present is kept only if its SHA-256
# matches the lock; a new one is fetched over HTTPS only and checked before it takes its final
# name. The ZIP's member names are checked before anything is extracted (one flat folder of
# plain names: no folders, "..", backslashes, drive letters or links), into an empty staging
# folder. A new server/ replaces the old one only when complete. Re-runs are a no-op when the
# stamp matches the lock and this script. On Windows, `llama-server.exe --version` is run and
# must name the build; on a Mac nothing in server/ runs (the windows-llama CI job runs it).
#
#   bash tools/win-app/fetch-llama.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

LOCK=tools/mac-app/llama.lock
SELF=tools/win-app/fetch-llama.sh
DEPS=tools/win-app/pe-deps.py
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
ROOT="$BUILD_ROOT/win/llama"
DL="$ROOT/downloads"
SERVER="$ROOT/server"

CLEANUP=()
cleanup() { [ ${#CLEANUP[@]} -eq 0 ] || rm -rf "${CLEANUP[@]}"; }
trap cleanup EXIT

die() { echo "fetch-llama: $*" >&2; exit 1; }

lock() {
  local v
  v=$(grep -E "^$1=" "$LOCK" | head -n1 | cut -d= -f2- | tr -d '\r') || true
  [ -n "$v" ] || die "$LOCK: no $1"
  printf '%s\n' "$v"
}

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
else
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
fi
for t in curl unzip; do command -v "$t" >/dev/null 2>&1 || die "$t is needed"; done

case "$(uname -s)" in
  Darwin) HOST=mac ;;
  MINGW* | MSYS* | CYGWIN*) HOST=windows ;;
  *) die "unsupported build machine $(uname -s) (a Mac, or Windows with Git Bash)" ;;
esac
# A path for a native Windows program (python.exe) on Windows; as it is elsewhere.
native() { if [ "$HOST" = windows ]; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
# python on Windows (as the other Windows jobs find it), python3 on a Mac.
if [ "$HOST" = windows ]; then
  PY=$(command -v python || command -v python3) || die "python is needed (for $DEPS)"
else
  PY=$(command -v python3) || die "python3 is needed (for $DEPS)"
fi

# --- the lock: one build number, which every URL names ---
BUILD=$(lock build)
[[ $BUILD =~ ^b[0-9]+$ ]] || die "$LOCK: build '$BUILD' is not b<number>"
for k in arm64_url x64_url win_x64_url; do
  case "$(lock $k)" in
    https://github.com/ggml-org/llama.cpp/releases/download/"$BUILD"/llama-"$BUILD"-bin-*) ;;
    *) die "$LOCK: $k does not name the release $BUILD: $(lock $k)" ;;
  esac
done
ZIP_URL=$(lock win_x64_url)
ZIP_SHA=$(lock win_x64_sha256)
LIC_URL=$(lock license_url)
LIC_SHA=$(lock license_sha256)
[ "$ZIP_URL" = "https://github.com/ggml-org/llama.cpp/releases/download/$BUILD/llama-$BUILD-bin-win-cpu-x64.zip" ] \
  || die "$LOCK: win_x64_url is not $BUILD's Windows CPU x64 ZIP"
[ "$LIC_URL" = "https://raw.githubusercontent.com/ggml-org/llama.cpp/$BUILD/LICENSE" ] \
  || die "$LOCK: license_url is not the LICENSE at tag $BUILD"
for s in "$ZIP_SHA" "$LIC_SHA"; do
  [[ $s =~ ^[0-9a-f]{64}$ ]] || die "$LOCK: a SHA-256 is not 64 hex digits: $s"
done

# --- download and verify ---
mkdir -p "$DL"
fetch() {  # url sha256 local-name
  local url=$1 want=$2 f="$DL/$3" got
  if [ -f "$f" ] && [ "$(sha256 "$f")" = "$want" ]; then
    echo "== $3 present, SHA-256 matches"
    return
  fi
  echo "== downloading $url"
  CLEANUP+=("$f.part")
  curl --proto '=https' --proto-redir '=https' -fL --retry 3 -sS -o "$f.part" "$url"
  got=$(sha256 "$f.part")
  [ "$got" = "$want" ] || die "$url: SHA-256 $got does not match $LOCK ($want)"
  mv -f "$f.part" "$f"
}
ZIP_NAME=$(basename "$ZIP_URL")
LIC_NAME="llama.cpp-$BUILD-LICENSE"
fetch "$ZIP_URL" "$ZIP_SHA" "$ZIP_NAME"
fetch "$LIC_URL" "$LIC_SHA" "$LIC_NAME"

# --- lay out server/ ---
SCRIPT_SHA="$(sha256 "$SELF") $(sha256 "$DEPS")"
STAMP="build=$BUILD zip_sha256=$ZIP_SHA license_sha256=$LIC_SHA scripts=$SCRIPT_SHA"
VCRT=""
if [ -x "$SERVER/llama-server.exe" ] && [ -f "$ROOT/server.stamp" ] && [ "$(cat "$ROOT/server.stamp")" = "$STAMP" ]; then
  echo "== server: up to date (stamp matches)"
  VCRT=$(sed -n 's/^# needs the Visual C++ runtime: //p' "$ROOT/manifest.txt" 2>/dev/null || true)
else
  echo "== laying out $SERVER"
  stage="$ROOT/staging"
  out="$ROOT/server.new"
  CLEANUP+=("$stage" "$out")
  rm -rf "$stage" "$out"
  mkdir -p "$stage" "$out"
  # One flat folder of plain names (what b11669's ZIP is), and regular files only: a ZIP
  # entry with the Unix S_IFLNK mode shows as "l" in zipinfo.
  bad=$(unzip -Z1 "$DL/$ZIP_NAME" | LC_ALL=C grep -Ev '^[A-Za-z0-9][A-Za-z0-9._-]*$' | head -3 || true)
  [ -z "$bad" ] || die "$ZIP_NAME: unexpected member names (not one flat folder): $bad"
  if unzip -Z -l "$DL/$ZIP_NAME" | awk 'substr($1, 1, 1) == "l"' | grep -q .; then
    die "$ZIP_NAME: has links"
  fi
  unzip -q "$DL/$ZIP_NAME" -d "$stage"
  deps=$("$PY" -I "$(native "$DEPS")" "$(native "$stage")" llama-server.exe 'ggml-cpu-*.dll' | tr -d '\r') \
    || die "the DLL walk failed (above)"
  while read -r kind name; do
    [ "$kind" = keep ] || continue
    cp "$stage/$name" "$out/"
  done <<<"$deps"
  VCRT=$(awk '$1 == "vcruntime" { printf "%s%s", sep, $2; sep = " " }' <<<"$deps")
  cp "$DL/$LIC_NAME" "$out/LICENSE"
  [ -f "$stage/LICENSE-LLVM-OpenMP" ] || die "$ZIP_NAME: no LICENSE-LLVM-OpenMP (for libomp.dll)"
  cp "$stage/LICENSE-LLVM-OpenMP" "$out/"
  chmod 755 "$out"
  chmod 644 "$out"/*
  chmod 755 "$out"/*.exe "$out"/*.dll
  rm -rf "$stage"

  {
    echo "# The Windows llama.cpp model server, laid out by $SELF on $HOST ($(date -u +%Y-%m-%dT%H:%M:%SZ))."
    echo "# From $LOCK (build $BUILD, dated $(lock date)):"
    echo "#   $ZIP_NAME $ZIP_SHA"
    echo "#   LICENSE ($LIC_URL) $LIC_SHA"
    echo "# scripts $SCRIPT_SHA"
    echo "# needs the Visual C++ runtime: $VCRT"
    echo "# Then SHA-256, size in bytes and name of every file in server/:"
    (cd "$out" && ls | LC_ALL=C sort | while IFS= read -r f; do
      printf '%s %s %s\n' "$(sha256 "$f")" "$(wc -c < "$f" | tr -d ' ')" "$f"
    done)
  } > "$ROOT/manifest.txt.part"

  # Swap in: the old folder aside first, so an interruption never leaves a half folder in place
  # (and the stamp is gone, so the next run lays it out again).
  rm -f "$ROOT/server.stamp"
  rm -rf "$ROOT/server.old"
  [ -e "$SERVER" ] && mv "$SERVER" "$ROOT/server.old"
  mv "$out" "$SERVER"
  rm -rf "$ROOT/server.old"
  mv -f "$ROOT/manifest.txt.part" "$ROOT/manifest.txt"
  printf '%s\n' "$STAMP" > "$ROOT/server.stamp"
fi

# --- report ---
echo "== server: $SERVER ($(du -sh "$SERVER" | awk '{print $1}'), $(ls "$SERVER" | wc -l | tr -d ' ') files)"
(cd "$SERVER" && for f in $(ls | LC_ALL=C sort); do printf '   %10s  %s\n' "$(wc -c < "$f" | tr -d ' ')" "$f"; done)
if command -v file >/dev/null 2>&1; then echo "   llama-server.exe: $(file -b "$SERVER/llama-server.exe")"; fi
echo "   needs the Visual C++ runtime (not in the release): ${VCRT:-nothing}"
if [ "$HOST" = windows ]; then
  version=$("$SERVER/llama-server.exe" --version 2>&1 | tr -d '\r')
  printf '%s\n' "$version" | sed 's/^/   /'
  grep -q "(build ${BUILD#b}," <<<"$version" || die "llama-server.exe --version does not name build ${BUILD#b}"
else
  echo "   (llama-server.exe is run on Windows: the windows-llama CI job)"
fi
echo "== manifest: $ROOT/manifest.txt"
echo "== done: $SERVER"
