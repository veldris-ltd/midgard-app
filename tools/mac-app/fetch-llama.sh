#!/usr/bin/env bash
# Fetch the llama.cpp model server pinned in llama.lock (official macOS builds) and lay it
# out for the app bundle:
#   $MIDGARD_BUILD_DIR/llama/<build>/llama-<build>-bin-macos-{arm64,x64}.tar.gz  (downloads)
#   $MIDGARD_BUILD_DIR/llama/<build>/{arm64,x64}/  llama-server + the dylibs it loads, LICENSE
#   $MIDGARD_BUILD_DIR/llama/current -> <build>    (stable path for build-app.sh)
# (MIDGARD_BUILD_DIR defaults to ~/Library/Caches/midgard-build, outside the suite folder.)
#
# A download already present is kept only if its SHA-256 matches llama.lock; a fresh one
# must match or the script fails. Each archive is extracted into a staging folder, and the
# dylibs llama-server needs are found by walking `otool -L` recursively. They are copied as
# real files (no symlinks: the bundle must hold none) under the name the loader asks for
# (e.g. libggml.0.dylib). Libraries referenced by an absolute path outside /usr/lib and
# /System are rewritten to @loader_path with install_name_tool (b11669 needs none: it uses
# @rpath with an @loader_path rpath), and any Mach-O changed is re-signed ad hoc. The Metal
# shaders are embedded in libggml-metal (no default.metallib in the release). Extended
# attributes (quarantine, provenance) are cleared, and Mach-Os that are unsigned (the x64
# release) are signed ad hoc. Prints the layout, sizes and
# `llama-server --version` of each (x64 under Rosetta, if installed).
#
#   bash tools/mac-app/fetch-llama.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

LOCK=tools/mac-app/llama.lock
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
ARCHES=(arm64 x64)

lock() {
  local v
  v=$(grep -E "^$1=" "$LOCK" | head -n1 | cut -d= -f2-)
  if [ -z "$v" ]; then
    echo "$LOCK: no $1" >&2
    exit 1
  fi
  printf '%s\n' "$v"
}

sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

BUILD=$(lock build)
DIR="$BUILD_ROOT/llama/$BUILD"
mkdir -p "$DIR"

# --- download and verify ---
for a in "${ARCHES[@]}"; do
  url=$(lock "${a}_url")
  want=$(lock "${a}_sha256")
  tgz="$DIR/$(basename "$url")"
  if [ -f "$tgz" ] && [ "$(sha256 "$tgz")" = "$want" ]; then
    echo "== $a: $(basename "$tgz") present, SHA-256 matches"
  else
    echo "== $a: downloading $url"
    curl -fL --retry 3 -sS -o "$tgz.part" "$url"
    mv "$tgz.part" "$tgz"
  fi
  got=$(sha256 "$tgz")
  if [ "$got" != "$want" ]; then
    echo "$tgz: SHA-256 $got does not match llama.lock ($want)" >&2
    exit 1
  fi
done

# --- lay out llama-server and its libraries per architecture ---
is_system() { case "$1" in /usr/lib/*|/System/*) return 0 ;; *) return 1 ;; esac; }

# The libraries a Mach-O loads (its own install name left out).
deps() {
  local id
  id=$(otool -D "$1" | sed -n 2p)
  otool -L "$1" | tail -n +2 | awk '{print $1}' | while read -r d; do
    [ "$d" = "$id" ] || printf '%s\n' "$d"
  done
}

has_rpath() { otool -l "$1" | grep -A2 LC_RPATH | grep -q "path $2 "; }

layout() {
  local a=$1 tgz=$2 stage out top queue f d name src changed
  stage="$DIR/staging-$a"
  out="$DIR/$a.new"
  rm -rf "$stage" "$out"
  mkdir -p "$stage" "$out"
  tar xzf "$tgz" -C "$stage"
  top=$(find "$stage" -name llama-server -type f -perm -u+x | head -n1)
  if [ -z "$top" ]; then
    echo "$tgz: no llama-server inside" >&2
    exit 1
  fi
  top=$(dirname "$top")
  cp "$top/llama-server" "$out/"
  [ -f "$top/LICENSE" ] && cp "$top/LICENSE" "$out/"

  queue=("$out/llama-server")
  while [ ${#queue[@]} -gt 0 ]; do
    f=${queue[0]}
    queue=("${queue[@]:1}")
    changed=0
    while read -r d; do
      [ -n "$d" ] || continue
      is_system "$d" && continue
      name=$(basename "$d")
      case "$d" in
        @rpath/*|@loader_path/*|@executable_path/*) ;;
        /*)
          # Absolute path outside the system: load it from beside the binary instead.
          install_name_tool -change "$d" "@loader_path/$name" "$f" 2>/dev/null
          changed=1
          ;;
        *) echo "$f: unexpected library reference $d" >&2; exit 1 ;;
      esac
      case "$d" in
        @rpath/*)
          if ! has_rpath "$f" @loader_path; then
            install_name_tool -add_rpath @loader_path "$f" 2>/dev/null
            changed=1
          fi
          ;;
      esac
      [ -e "$out/$name" ] && continue
      src="$top/$name"
      [ -e "$src" ] || src="$d"   # absolute path not in the release
      if [ ! -e "$src" ]; then
        echo "$f needs $d, which is not in the release" >&2
        exit 1
      fi
      cp -L "$src" "$out/$name"   # follow the release's version symlinks
      chmod u+w "$out/$name"
      case "$d" in
        /*) install_name_tool -id "@rpath/$name" "$out/$name" 2>/dev/null ;;
      esac
      queue+=("$out/$name")
    done < <(deps "$f")
    if [ $changed = 1 ]; then
      echo "   rewrote library paths in $(basename "$f"); re-signing ad hoc"
      codesign --force -s - "$f" 2>/dev/null
    fi
  done
  # Sign ad hoc whatever does not verify: copies whose install name changed, and the x64
  # release, which ships unsigned (arm64's is linker-signed).
  xattr -cr "$out"
  for f in "$out"/llama-server "$out"/*.dylib; do
    if ! codesign -v "$f" 2>/dev/null; then
      echo "   signing $(basename "$f") ad hoc (unsigned or changed)"
      codesign --force -s - "$f" 2>/dev/null
    fi
  done
  for f in "$out"/llama-server "$out"/*.dylib; do
    if ! codesign -v "$f" 2>/dev/null; then
      echo "$f: signature does not verify" >&2
      exit 1
    fi
  done
  rm -rf "$stage" "$DIR/$a"
  mv "$out" "$DIR/$a"
}

for a in "${ARCHES[@]}"; do
  echo "== laying out $a"
  layout "$a" "$DIR/$(basename "$(lock "${a}_url")")"
done

ln -sfn "$BUILD" "$BUILD_ROOT/llama/current"

# --- report ---
for a in "${ARCHES[@]}"; do
  echo "== $a: $BUILD_ROOT/llama/current/$a ($(du -sh "$DIR/$a" | awk '{print $1}'))"
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
echo "== done: $BUILD_ROOT/llama/current/{arm64,x64} -> $DIR"
