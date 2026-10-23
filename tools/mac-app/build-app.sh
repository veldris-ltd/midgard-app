#!/usr/bin/env bash
# Build Midgard.app, the macOS presentation app, into ~/Library/Caches/midgard-build/app
# (outside the suite folder, because of its 3.5 GB budget).
#
# Steps: the services (build-services.sh → dist/mac/bin) and the consoles
# (build-consoles.sh → dist/mac/consoles) unless skipped; the Swift shell as a universal
# release build; then the bundle, assembled next to the old one, signed, verified, and only
# then swapped in:
#   Contents/Info.plist, MacOS/Midgard, Helpers/<service binaries>, Helpers/midgard-frames,
#   Helpers/llama/{arm64,x64}/ (llama-server and its dylibs, from fetch-llama.sh),
#   Resources/licenses/{llama.cpp-LICENSE,<the camera analytics' licences>},
#   Resources/analytics/{analytics.py,config.json,runtime/{arm64,x64}/} (the camera analytics
#     and a Python runtime per chip, from heimdall/analytics and fetch-analytics.sh),
#   Resources/{consoles,home,starting-state,models/{granite,trust,yolox}[,recording]}
# The starting state comes from make-starting-state.sh (~/Library/Caches/midgard-build/
# starting-state) and the model from ~/Desktop/Midgard/mimir-models; both are copied as
# APFS clones (cp -c), so they share disk with their sources; across volumes the script
# falls back to a plain copy after checking free space. The bundle must hold no symlinks
# (codesign and the home page's web server refuse them), and Helpers only Mach-O files
# (codesign treats it as nested code). It is signed ad hoc: each helper and every Mach-O of
# the model server, then the bundle. fetch-llama.sh signed the x64 server already; signing
# it again here is deliberate: every Mach-O in Helpers gets this build's signature. The shell runs the model server
# for the Mac's chip (arm64 with Metal, x64 on the CPU), and the analytics runtime likewise
# (arm64 with CoreML, x64 on the CPU). That runtime holds Python sources, so it lives in
# Resources (codesign takes only Mach-O in Helpers); each of its Mach-O files (the
# interpreter, extension modules, numpy's and onnxruntime's dylibs) is signed one by one
# before the bundle.
#
#   bash tools/mac-app/build-app.sh [--skip-services] [--skip-consoles] [--skip-llama]
#                                   [--skip-analytics] [--recording DIR] [--out DIR]
set -euo pipefail
cd "$(dirname "$0")/../.."

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
OUT="$BUILD/app"
RECORDING="${MIDGARD_RECORDING:-}"
# Given (even empty: MIDGARD_RECORDING= or --recording "" builds without one) or not.
RECORDING_GIVEN=${MIDGARD_RECORDING+1}
DEFAULT_RECORDING="$HOME/Library/Application Support/Midgard-dev/recordings/hour-1"
PREPARED=apps/mac/Resources/home/prepared-answers.json
SKIP_SERVICES=0
SKIP_CONSOLES=0
SKIP_LLAMA=0
SKIP_ANALYTICS=0
STATE="$BUILD/starting-state"
MODELS="${MIDGARD_MODELS:-$HOME/Desktop/Midgard/mimir-models}"
BIN=dist/mac/bin
CONSOLES=dist/mac/consoles
LLAMA="$BUILD/llama/current"     # fetch-llama.sh's layout: {arm64,x64}/llama-server + dylibs
LLAMA_ARCHES=(arm64 x64)
ANALYTICS="$BUILD/analytics"     # fetch-analytics.sh's layout: runtime/{arm64,x64}, models, licenses
ANALYTICS_SRC=heimdall/analytics
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
PRODUCTS=(midgard heimdall mimir forge)

usage() {
  cat >&2 <<'EOF'
usage: bash tools/mac-app/build-app.sh [--skip-services] [--skip-consoles] [--skip-llama]
                                       [--skip-analytics] [--recording DIR] [--out DIR]
  --skip-services   reuse dist/mac/bin as it is (warns if Rust sources are newer)
  --skip-consoles   reuse dist/mac/consoles as it is (warns if console sources are newer)
  --skip-llama      reuse ~/Library/Caches/midgard-build/llama/current as it is (no
                    fetch-llama.sh run)
  --skip-analytics  reuse ~/Library/Caches/midgard-build/analytics as it is (no
                    fetch-analytics.sh run)
  --recording DIR   a phase 2 recording to bundle and replay (or MIDGARD_RECORDING); it
                    must contain meta.json; its raw/ capture is left out. Not given: the
                    recorded hour (recordings/hour-1 in ~/Library/Application Support/
                    Midgard-dev) if it has meta.json, else none; "" for none
  --out DIR         where Midgard.app goes (default ~/Library/Caches/midgard-build/app)
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --skip-services) SKIP_SERVICES=1 ;;
    --skip-consoles) SKIP_CONSOLES=1 ;;
    --skip-llama) SKIP_LLAMA=1 ;;
    --skip-analytics) SKIP_ANALYTICS=1 ;;
    --recording) [ $# -ge 2 ] || usage; RECORDING="$2"; RECORDING_GIVEN=1; shift ;;
    --out) [ $# -ge 2 ] || usage; OUT="$2"; shift ;;
    -h|--help) usage ;;
    *) echo "unknown option: $1" >&2; usage ;;
  esac
  shift
done

die() { echo "build-app: $*" >&2; exit 1; }
free_kb() { df -k "$1" | awk 'NR == 2 { print $4 }'; }
now() { date +%s; }
# True if $1 is a Mach-O file (thin or universal, 32 or 64 bit, either byte order), by its
# first four bytes.
is_macho() {
  case "$(od -An -tx1 -N4 "$1" 2>/dev/null | tr -d ' \n')" in
    feedface|cefaedfe|feedfacf|cffaedfe|cafebabe|bebafeca) return 0 ;;
    *) return 1 ;;
  esac
}
T0=$(now)

# True if a process runs an executable inside $1 (compared as a fixed string, not a regex).
running_from() {
  ps -axo comm= | awk -v p="$1/Contents/" 'index($0, p) == 1 { found = 1 } END { exit !found }'
}

# --- checks before any work ---------------------------------------------------------
[ -f "$STATE/VERSION" ] || die "no starting state in $STATE: stop the suite, then run
  bash tools/mac-app/make-starting-state.sh"
for f in granite/model.manifest trust/model-signing.pub; do
  [ -f "$MODELS/$f" ] || die "missing $MODELS/$f (the Mimir model)"
done
ls "$MODELS"/granite/*.gguf >/dev/null 2>&1 || die "no .gguf model in $MODELS/granite"
# The prepared Mimir answers (prepare-answers.py) ship inside home/; check they parse.
if [ -f "$PREPARED" ]; then
  summary=$(python3 -c '
import json, sys
d = json.load(open(sys.argv[1]))
v = d.get("version") if isinstance(d, dict) else None
assert type(v) is int and v == 1, "not version 1"
e, g = d.get("entries") or {}, d.get("guide") or {}
print("%d answers, guide: %d questions, %d alerts (recording %s)" % (
    len(e), len(g.get("questions") or []), len(g.get("alerts") or []), d.get("recording")))
' "$PREPARED" 2>&1) || die "$PREPARED is not a valid version 1 prepared answers file: ${summary##*$'\n'}"
  echo "prepared answers: $summary"
else
  echo "prepared answers: none ($PREPARED missing): every Mimir answer is asked live"
fi
if [ -z "$RECORDING_GIVEN" ]; then
  if [ -f "$DEFAULT_RECORDING/meta.json" ]; then
    RECORDING="$DEFAULT_RECORDING"
    echo "recording: none given; using the recorded hour $RECORDING"
  else
    echo "recording: none given and no $DEFAULT_RECORDING/meta.json; building without one"
  fi
elif [ -z "$RECORDING" ]; then
  echo "recording: none"
fi
if [ -n "$RECORDING" ]; then
  [ -f "$RECORDING/meta.json" ] || die "recording $RECORDING has no meta.json"
  RECORDING="$(cd "$RECORDING" && pwd)"
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
APP="$OUT/Midgard.app"
NEW="$OUT/.Midgard.app.new"
OLD="$OUT/.Midgard.app.old"

# A previous run interrupted mid-swap leaves the app only in $OLD: put it back.
if [ ! -e "$APP" ] && [ -e "$OLD" ]; then
  echo "restoring $APP from an interrupted build"
  mv "$OLD" "$APP"
fi
rm -rf "$NEW" "$OLD"
running_from "$APP" && die "Midgard is running from $APP: quit it first"
FREE_BEFORE=$(free_kb "$OUT")

# --- services and consoles ------------------------------------------------------------
# A skipped step reuses dist/ as it is; a warning names sources changed since it was built.
stale_warning() {  # $1 = what, $2 = reference file; then find's name tests, --, paths
  local what="$1" ref="$2"; shift 2
  local names=()
  while [ "$1" != -- ]; do names+=("$1"); shift; done
  shift
  local newer
  newer=$(find "$@" \( -name node_modules -o -name dist -o -name target -o -name .git \) -prune \
    -o -type f \( "${names[@]}" \) -newer "$ref" -print 2>/dev/null | head -3)
  if [ -n "$newer" ]; then
    echo "WARNING: $what: sources changed since the last build (e.g. ${newer//$'\n'/, })" >&2
  fi
}
oldest_of() { ls -tr "$@" | head -1; }

if [ "$SKIP_SERVICES" = 1 ]; then
  for b in "${BINARIES[@]}"; do [ -x "$BIN/$b" ] || die "--skip-services: $BIN/$b is missing"; done
  stale_warning "$BIN" "$(oldest_of "${BINARIES[@]/#/$BIN/}")" \
    -name '*.rs' -o -name Cargo.toml -o -name Cargo.lock -- .
  echo "== services: reusing $BIN"
else
  echo "== services"
  bash tools/mac-app/build-services.sh
fi

if [ "$SKIP_CONSOLES" = 1 ]; then
  for p in "${PRODUCTS[@]}"; do [ -f "$CONSOLES/$p/index.html" ] || die "--skip-consoles: $CONSOLES/$p is missing"; done
  pages=()
  for p in "${PRODUCTS[@]}"; do pages+=("$CONSOLES/$p/index.html"); done
  stale_warning "$CONSOLES" "$(oldest_of "${pages[@]}")" -true -- ui sdk products
  echo "== consoles: reusing $CONSOLES"
else
  echo "== consoles"
  bash tools/mac-app/build-consoles.sh
fi

if [ "$SKIP_LLAMA" = 1 ]; then
  echo "== model server: reusing $LLAMA"
else
  echo "== model server"
  MIDGARD_BUILD_DIR="$BUILD" bash tools/mac-app/fetch-llama.sh
fi
for a in "${LLAMA_ARCHES[@]}"; do
  [ -x "$LLAMA/$a/llama-server" ] || die "no $LLAMA/$a/llama-server: run bash tools/mac-app/fetch-llama.sh"
done

if [ "$SKIP_ANALYTICS" = 1 ]; then
  echo "== camera analytics runtime: reusing $ANALYTICS"
else
  echo "== camera analytics runtime"
  MIDGARD_BUILD_DIR="$BUILD" bash tools/mac-app/fetch-analytics.sh
fi
for a in arm64 x64; do
  [ -x "$ANALYTICS/runtime/$a/bin/python3.13" ] || die "no $ANALYTICS/runtime/$a: run bash tools/mac-app/fetch-analytics.sh"
done
[ -f "$ANALYTICS/models/yolox_s.onnx" ] || die "no $ANALYTICS/models/yolox_s.onnx: run bash tools/mac-app/fetch-analytics.sh"
for f in analytics.py config.json; do [ -f "$ANALYTICS_SRC/$f" ] || die "no $ANALYTICS_SRC/$f"; done
T_DEPS=$(now)

# --- the shell ------------------------------------------------------------------------
echo "== shell (swift, universal release)"
SWIFT_ARGS=(-c release --arch arm64 --arch x86_64 --package-path apps/mac --scratch-path "$BUILD/swift")
swift build "${SWIFT_ARGS[@]}"
SHELL_BIN="$(swift build "${SWIFT_ARGS[@]}" --show-bin-path)/Midgard"
FRAMES_BIN="$(dirname "$SHELL_BIN")/midgard-frames"   # the camera analytics' decoder
for b in "$SHELL_BIN" "$FRAMES_BIN"; do
  [ -x "$b" ] || die "built $(basename "$b") not found at $b"
  archs=$(lipo -archs "$b")
  for a in arm64 x86_64; do
    case " $archs " in *" $a "*) ;; *) die "$(basename "$b") is not universal: $archs" ;; esac
  done
done
echo "shell, midgard-frames: $archs"
T_SWIFT=$(now)

# --- the bundle -----------------------------------------------------------------------
echo "== assembling $APP"
# On any exit before the swap finishes: drop the half-built bundle, and if the old app was
# already moved aside, put it back.
restore() {
  rm -rf "$NEW"
  if [ ! -e "$APP" ] && [ -e "$OLD" ]; then mv "$OLD" "$APP"; fi
}
trap restore EXIT
C="$NEW/Contents"
R="$C/Resources"
mkdir -p "$C/MacOS" "$C/Helpers" "$R/models"

# Copies $1 to the new path $2 as APFS clones; a plain copy (after a free space check)
# when cloning is not possible, e.g. across volumes.
clone_copy() {
  local src="$1" dst="$2" err
  if err=$(cp -cRp "$src" "$dst" 2>&1); then return; fi
  echo "  cannot clone $src: ${err:-unknown error}; copying instead"
  rm -rf "$dst"
  local need have
  need=$(du -sk "$src" | awk '{ print $1 }')
  have=$(free_kb "$(dirname "$dst")")
  [ "$have" -gt $((need + 1048576)) ] || die "not enough free disk to copy $src ($((need / 1024)) MB, $((have / 1024)) MB free)"
  cp -Rp "$src" "$dst"
}

SHORT_SHA=$(git rev-parse --short HEAD)
# Uncommitted changes mark the build "-dirty", which build-dmg.sh refuses.
[ -z "$(git status --porcelain --untracked-files=no)" ] || SHORT_SHA="$SHORT_SHA-dirty"
BUILD_NUMBER=$(git rev-list --count HEAD)
cat > "$C/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>app.midgard.presenter</string>
  <key>CFBundleName</key><string>Midgard</string>
  <key>CFBundleDisplayName</key><string>Midgard</string>
  <key>CFBundleExecutable</key><string>Midgard</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
  <key>MidgardGitCommit</key><string>$SHORT_SHA</string>
  <key>LSMinimumSystemVersion</key><string>13.3</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.business</string>
</dict>
</plist>
EOF
plutil -lint "$C/Info.plist"

cp "$SHELL_BIN" "$C/MacOS/Midgard"
for b in "${BINARIES[@]}"; do cp "$BIN/$b" "$C/Helpers/$b"; done
cp "$FRAMES_BIN" "$C/Helpers/midgard-frames"
# The model server: only its Mach-O files go in Helpers (real files: -L dereferences);
# llama.cpp's LICENSE goes in Resources/licenses (one copy when both builds' are the same).
mkdir -p "$C/Helpers/llama" "$R/licenses"
for a in "${LLAMA_ARCHES[@]}"; do
  mkdir "$C/Helpers/llama/$a"
  for f in "$LLAMA/$a"/*; do
    name=$(basename "$f")
    if [ "$name" = LICENSE ]; then
      continue
    elif [ -f "$f" ] && is_macho "$f"; then
      cp -L "$f" "$C/Helpers/llama/$a/$name"
    else
      die "unexpected file in $LLAMA/$a: $name (only Mach-O files and LICENSE)"
    fi
  done
done
[ -f "$LLAMA/arm64/LICENSE" ] && [ -f "$LLAMA/x64/LICENSE" ] || die "llama.cpp's LICENSE is missing from $LLAMA/{arm64,x64}"
if cmp -s "$LLAMA/arm64/LICENSE" "$LLAMA/x64/LICENSE"; then
  cp -L "$LLAMA/arm64/LICENSE" "$R/licenses/llama.cpp-LICENSE"
else
  for a in "${LLAMA_ARCHES[@]}"; do cp -L "$LLAMA/$a/LICENSE" "$R/licenses/llama.cpp-LICENSE-$a"; done
fi
# The camera analytics: the code, a runtime per chip (real files only, as fetch-analytics.sh
# checked), the model and the licences.
mkdir -p "$R/analytics/runtime" "$R/models/yolox"
cp "$ANALYTICS_SRC/analytics.py" "$ANALYTICS_SRC/config.json" "$R/analytics/"
for a in arm64 x64; do clone_copy "$ANALYTICS/runtime/$a" "$R/analytics/runtime/$a"; done
clone_copy "$ANALYTICS/models/yolox_s.onnx" "$R/models/yolox/yolox_s.onnx"
for f in "$ANALYTICS"/licenses/*; do cp "$f" "$R/licenses/"; done
cp -RL "$CONSOLES" "$R/consoles"
cp -RL apps/mac/Resources/home "$R/home"          # fonts is a symlink in the repo
clone_copy "$STATE" "$R/starting-state"
clone_copy "$MODELS/granite" "$R/models/granite"
clone_copy "$MODELS/trust" "$R/models/trust"
if [ -n "$RECORDING" ]; then
  mkdir "$R/recording"
  for entry in "$RECORDING"/*; do
    case "$(basename "$entry")" in
      raw) continue ;;              # the raw capture is not replayed
      *.tmp|*.old) continue ;;      # a capture (capture-radar.py, -traffic.py, -tiles.py) still being written or swapped
    esac
    clone_copy "$entry" "$R/recording/$(basename "$entry")"
  done
  echo "recording: $RECORDING"
  # Camera stills and clips (record.py pack), served by the app at /media/.
  if [ -d "$R/recording/media" ]; then
    echo "recording media: $(find "$R/recording/media" -type f | wc -l | tr -d ' ') files, $(du -sh "$R/recording/media" | cut -f1)"
  else
    echo "recording media: none (cameras have no image or clip; re-pack with record.py pack)"
  fi
  # Rain radar frames (capture-radar.py), served by the app at /radar/.
  if [ -f "$R/recording/radar/manifest.json" ]; then
    echo "recording radar: $(find "$R/recording/radar" -name '*.png' -type f | wc -l | tr -d ' ') tiles, $(du -sh "$R/recording/radar" | cut -f1)"
  else
    echo "recording radar: none (the RADAR chip shows unavailable; run tools/mac-app/capture-radar.py)"
  fi
  # Road traffic flow snapshot (capture-traffic.py), served by the app at /traffic/.
  if [ -f "$R/recording/traffic/manifest.json" ]; then
    echo "recording traffic: $(find "$R/recording/traffic" -name '*.pbf' -type f | wc -l | tr -d ' ') tiles, $(du -sh "$R/recording/traffic" | cut -f1)"
  else
    echo "recording traffic: none (the TRAFFIC chip shows unavailable; run tools/mac-app/capture-traffic.py)"
  fi
  # Close-up globe terrain and imagery tiles (capture-tiles.py), served by the app at /tiles/.
  if [ -f "$R/recording/tiles/manifest.json" ]; then
    echo "recording tiles: $(find "$R/recording/tiles" -type f \( -name '*.png' -o -name '*.jpg' \) | wc -l | tr -d ' ') tiles, $(du -sh "$R/recording/tiles" | cut -f1)"
  else
    echo "recording tiles: none (the globe keeps the bundled map close up; run tools/mac-app/capture-tiles.py)"
  fi
else
  echo "recording: none (the app starts without replay)"
fi

links=$(find "$NEW" -type l)
[ -z "$links" ] || die "the bundle contains symlinks:
$links"

# Finder info and quarantine attributes make codesign fail. Inside out: each helper, then
# the bundle (which seals the helpers' signatures and the resources); no --deep.
echo "== signing (ad hoc)"
xattr -cr "$NEW"
# Helpers is nested code to codesign: anything there that is not Mach-O fails the bundle.
while IFS= read -r -d '' f; do
  is_macho "$f" || die "not a Mach-O file in Contents/Helpers: ${f#"$C/"}"
done < <(find "$C/Helpers" -type f -print0)
for b in "${BINARIES[@]}" midgard-frames; do codesign --force --sign - "$C/Helpers/$b"; done
# Every file of the model server (llama-server and its dylibs, all Mach-O as checked).
LLAMA_MACHOS=()
while IFS= read -r -d '' f; do LLAMA_MACHOS+=("$f"); done < <(find "$C/Helpers/llama" -type f -print0)
for a in "${LLAMA_ARCHES[@]}"; do
  [ -f "$C/Helpers/llama/$a/llama-server" ] || die "no llama-server in Helpers/llama/$a"
done
for f in "${LLAMA_MACHOS[@]}"; do codesign --force --sign - "$f"; done
echo "model server: ${#LLAMA_MACHOS[@]} Mach-O files signed"
# Every Mach-O of the analytics runtimes, inside site-packages too, before the bundle.
ANALYTICS_MACHOS=()
while IFS= read -r -d '' f; do
  is_macho "$f" && ANALYTICS_MACHOS+=("$f")
done < <(find "$R/analytics/runtime" -type f -print0)
[ ${#ANALYTICS_MACHOS[@]} -gt 0 ] || die "no Mach-O files in Resources/analytics/runtime"
for f in "${ANALYTICS_MACHOS[@]}"; do codesign --force --sign - "$f"; done
echo "camera analytics runtime: ${#ANALYTICS_MACHOS[@]} Mach-O files signed"
codesign --force --sign - "$NEW"

echo "== codesign verify"
for b in "${BINARIES[@]}" midgard-frames; do
  codesign --verify --strict "$C/Helpers/$b" || die "helper $b fails codesign --verify"
done
for f in "${ANALYTICS_MACHOS[@]}"; do
  codesign --verify --strict "$f" || die "${f#"$C/"} fails codesign --verify"
done
[ "$(lipo -archs "$R/analytics/runtime/arm64/bin/python3.13")" = arm64 ] || die "the arm64 analytics runtime is not arm64"
[ "$(lipo -archs "$R/analytics/runtime/x64/bin/python3.13")" = x86_64 ] || die "the x64 analytics runtime is not x86_64"
for f in "${LLAMA_MACHOS[@]}"; do
  codesign --verify --strict "$f" || die "${f#"$C/"} fails codesign --verify"
done
[ "$(lipo -archs "$C/Helpers/llama/arm64/llama-server")" = arm64 ] || die "Helpers/llama/arm64/llama-server is not arm64"
[ "$(lipo -archs "$C/Helpers/llama/x64/llama-server")" = x86_64 ] || die "Helpers/llama/x64/llama-server is not x86_64"
codesign --verify --deep --strict --verbose=1 "$NEW" 2>&1 || die "the bundle fails codesign --verify"

# Swap in: the old bundle aside, the new one in place; the trap restores the old one if
# this is interrupted in between. Only then is the old one removed.
running_from "$APP" && die "Midgard was started from $APP during the build: quit it and build again"
if [ -e "$APP" ]; then mv "$APP" "$OLD"; fi
mv "$NEW" "$APP"
trap - EXIT
rm -rf "$OLD"
T_END=$(now)

# --- report ---------------------------------------------------------------------------
FREE_AFTER=$(free_kb "$OUT")
echo "== sizes (clones count in full here but share disk with their sources)"
du -sh "$APP"
for part in "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP"/Contents/Helpers/llama/* "$APP"/Contents/Resources/* \
    "$APP"/Contents/Resources/analytics/runtime/* "$APP/Contents/Resources/models/yolox"; do
  du -sh "$part" | sed "s|$APP/Contents/||"
done
for part in media radar traffic tiles; do
  [ ! -d "$APP/Contents/Resources/recording/$part" ] || du -sh "$APP/Contents/Resources/recording/$part" | sed "s|$APP/Contents/||"
done
echo "free disk: $((FREE_BEFORE / 1024)) MB before, $((FREE_AFTER / 1024)) MB after ($(((FREE_BEFORE - FREE_AFTER) / 1024)) MB used, including any rebuilt dist/ and swift outputs)"
echo "times: services+consoles+model server+analytics $((T_DEPS - T0)) s, shell $((T_SWIFT - T_DEPS)) s, bundle $((T_END - T_SWIFT)) s"
echo "built $APP (1.0, build $BUILD_NUMBER, $SHORT_SHA)"
echo "open it with: open \"$APP\""
