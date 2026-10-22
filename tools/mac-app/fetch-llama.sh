#!/usr/bin/env bash
# Fetch the llama.cpp model server pinned in llama.lock (official macOS builds) and lay it
# out for the app bundle:
#   $MIDGARD_BUILD_DIR/llama/<build>/llama-<build>-bin-macos-{arm64,x64}.tar.gz  (downloads)
#   $MIDGARD_BUILD_DIR/llama/<build>/{arm64,x64}/  llama-server + the dylibs it loads, LICENSE
#   $MIDGARD_BUILD_DIR/llama/<build>/{arm64,x64}.stamp  (what each folder was made from)
#   $MIDGARD_BUILD_DIR/llama/current -> <build>    (stable path for build-app.sh)
# (MIDGARD_BUILD_DIR defaults to ~/Library/Caches/midgard-build, outside the suite folder.)
#
# A download already present is kept only if its SHA-256 matches llama.lock; a new one is
# fetched over HTTPS only and checked before it takes its final name. Each archive is
# extracted into a staging folder (removed on exit), and the dylibs llama-server needs are
# found by walking `otool -L` recursively. Only references to system libraries (/usr/lib,
# /System) and `@rpath/<name>` or `@loader_path/<name>` naming a file in the release are
# accepted; anything else, or a library missing from the release, fails the script (no
# library is ever taken from this Mac). Libraries are copied as real files (the bundle must
# hold no symlinks) under the name the loader asks for (e.g. libggml.0.dylib). b11669 uses
# @rpath with an @loader_path rpath, and its Metal shaders are built into libggml-metal.
# Extended attributes are cleared and any Mach-O that does not verify (the x64 release is
# unsigned) is signed ad hoc. The new folder replaces the old one only when complete.
#
# Re-runs are a no-op: an arch folder whose stamp matches the archive's SHA-256 and this
# script's hash is left untouched (no re-signing, no new mtimes). Prints the layout, sizes
# and `llama-server --version` of each (x64 under Rosetta, if installed).
#
#   bash tools/mac-app/fetch-llama.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

LOCK=tools/mac-app/llama.lock
SELF=tools/mac-app/fetch-llama.sh
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
ARCHES=(arm64 x64)

CLEANUP=()
cleanup() { [ ${#CLEANUP[@]} -eq 0 ] || rm -rf "${CLEANUP[@]}"; }
trap cleanup EXIT

die() { echo "$*" >&2; exit 1; }

lock() {
  local v
  v=$(grep -E "^$1=" "$LOCK" | head -n1 | cut -d= -f2-) || true
  [ -n "$v" ] || die "$LOCK: no $1"
  printf '%s\n' "$v"
}

sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

# --- the lock ---
BUILD=$(lock build)
[[ $BUILD =~ ^b[0-9]+$ ]] || die "$LOCK: build '$BUILD' is not b<number>"
for a in "${ARCHES[@]}"; do
  url=$(lock "${a}_url")
  sum=$(lock "${a}_sha256")
  [[ $url =~ ^https://[^[:space:]]+\.tar\.gz$ ]] || die "$LOCK: ${a}_url is not an https .tar.gz URL"
  [[ $sum =~ ^[0-9a-f]{64}$ ]] || die "$LOCK: ${a}_sha256 is not 64 hex digits"
done

LLAMA="$BUILD_ROOT/llama"
DIR="$LLAMA/$BUILD"
if [ -e "$LLAMA/current" ] && [ ! -L "$LLAMA/current" ]; then
  die "$LLAMA/current is a real folder, not a symlink: move it away and re-run"
fi
mkdir -p "$DIR"
SCRIPT_SHA=$(sha256 "$SELF")

# --- download and verify ---
for a in "${ARCHES[@]}"; do
  url=$(lock "${a}_url")
  want=$(lock "${a}_sha256")
  tgz="$DIR/$(basename "$url")"
  if [ -f "$tgz" ] && [ "$(sha256 "$tgz")" = "$want" ]; then
    echo "== $a: $(basename "$tgz") present, SHA-256 matches"
    continue
  fi
  echo "== $a: downloading $url"
  CLEANUP+=("$tgz.part")
  curl --proto '=https' --proto-redir '=https' -fL --retry 3 -sS -o "$tgz.part" "$url"
  got=$(sha256 "$tgz.part")
  [ "$got" = "$want" ] || die "$url: SHA-256 $got does not match llama.lock ($want)"
  mv -f "$tgz.part" "$tgz"
done

# --- lay out llama-server and its libraries per architecture ---

# The libraries a Mach-O loads (its own install name left out).
deps() {
  local f=$1 id lst
  id=$(otool -D "$f") || die "otool -D failed on $f"
  id=$(sed -n 2p <<<"$id")
  lst=$(otool -L "$f") || die "otool -L failed on $f"
  tail -n +2 <<<"$lst" | awk '{print $1}' | while read -r d; do
    [ "$d" = "$id" ] || printf '%s\n' "$d"
  done
}

rpaths() {
  local out
  out=$(otool -l "$1") || die "otool -l failed on $1"
  awk '/cmd LC_RPATH/ { getline; getline; print $2 }' <<<"$out"
}

layout() {
  local a=$1 tgz=$2 stage out top topreal queue f d name src real changed list rp
  stage="$DIR/staging-$a"
  out="$DIR/$a.new"
  CLEANUP+=("$stage" "$out")
  rm -rf "$stage" "$out"
  mkdir -p "$stage" "$out"
  tar xzf "$tgz" -C "$stage"
  top=$(find "$stage" -name llama-server -type f -perm -u+x | head -n1)
  [ -n "$top" ] || die "$tgz: no llama-server inside"
  top=$(dirname "$top")
  topreal=$(cd "$top" && pwd -P)
  cp "$top/llama-server" "$out/"
  [ -f "$top/LICENSE" ] && cp "$top/LICENSE" "$out/"

  queue=("$out/llama-server")
  while [ ${#queue[@]} -gt 0 ]; do
    f=${queue[0]}
    queue=("${queue[@]:1}")
    changed=0
    list=$(deps "$f")
    while read -r d; do
      [ -n "$d" ] || continue
      case "$d" in
        /usr/lib/*|/System/*) continue ;;
        @rpath/*|@loader_path/*) name=${d#*/} ;;
        *) die "$(basename "$f") ($a): library reference $d is not @rpath/@loader_path or a system library" ;;
      esac
      case "$name" in
        ''|*/*) die "$(basename "$f") ($a): library reference $d has a subpath" ;;
      esac
      if [[ $d == @rpath/* ]]; then
        rp=$(rpaths "$f")
        if ! grep -qx '@loader_path' <<<"$rp"; then
          install_name_tool -add_rpath @loader_path "$f"
          changed=1
        fi
      fi
      [ -e "$out/$name" ] && continue
      src="$top/$name"
      [ -e "$src" ] || die "$(basename "$f") ($a) needs $name, which is not in the release"
      real=$(realpath "$src")
      [[ $real == "$topreal"/* ]] || die "$src ($a) points outside the release ($real)"
      cp "$real" "$out/$name"   # the release's version symlinks, resolved
      chmod u+w "$out/$name"
      queue+=("$out/$name")
    done <<<"$list"
    if [ $changed = 1 ]; then
      echo "   added an @loader_path rpath to $(basename "$f"); re-signing ad hoc"
      codesign --force -s - "$f"
    fi
  done
  # Sign ad hoc whatever does not verify (the x64 release ships unsigned; arm64's is
  # linker-signed), then check every signature.
  xattr -cr "$out"
  for f in "$out"/llama-server "$out"/*.dylib; do
    if ! codesign -v "$f" 2>/dev/null; then
      echo "   signing $(basename "$f") ad hoc (unsigned)"
      codesign --force -s - "$f"
    fi
  done
  for f in "$out"/llama-server "$out"/*.dylib; do
    codesign -v "$f" || die "$f: signature does not verify"
  done
  # Swap in: old folder aside first, so an interruption never leaves a half folder in place
  # (and the stamp is gone, so the next run lays it out again).
  rm -f "$DIR/$a.stamp"
  rm -rf "$DIR/$a.old"
  [ -e "$DIR/$a" ] && mv "$DIR/$a" "$DIR/$a.old"
  mv "$out" "$DIR/$a"
  rm -rf "$DIR/$a.old" "$stage"
}

for a in "${ARCHES[@]}"; do
  want=$(lock "${a}_sha256")
  stamp="archive_sha256=$want script_sha256=$SCRIPT_SHA"
  if [ -x "$DIR/$a/llama-server" ] && [ -f "$DIR/$a.stamp" ] && [ "$(cat "$DIR/$a.stamp")" = "$stamp" ]; then
    echo "== $a: up to date (stamp matches)"
    continue
  fi
  echo "== laying out $a"
  layout "$a" "$DIR/$(basename "$(lock "${a}_url")")"
  printf '%s\n' "$stamp" > "$DIR/$a.stamp"
done

if [ "$(readlink "$LLAMA/current" 2>/dev/null || true)" != "$BUILD" ]; then
  ln -sfn "$BUILD" "$LLAMA/current"
fi

# --- report ---
for a in "${ARCHES[@]}"; do
  echo "== $a: $LLAMA/current/$a ($(du -sh "$DIR/$a" | awk '{print $1}'))"
  (cd "$DIR/$a" && ls -l | tail -n +2 | awk '{printf "   %10d  %s\n", $5, $9}')
  echo "   $(file -b "$DIR/$a/llama-server")"
done
echo "== arm64 --version"
"$DIR/arm64/llama-server" --version 2>&1 | sed 's/^/   /'
echo "== x64 --version (Rosetta)"
if arch -x86_64 /usr/bin/true 2>/dev/null; then
  arch -x86_64 "$DIR/x64/llama-server" --version 2>&1 | sed 's/^/   /'
else
  echo "   skipped: Rosetta is not installed (softwareupdate --install-rosetta)"
fi
echo "== done: $LLAMA/current/{arm64,x64} -> $DIR"
