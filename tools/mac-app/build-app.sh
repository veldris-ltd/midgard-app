#!/usr/bin/env bash
# Build Midgard.app, the macOS presentation app, into ~/Library/Caches/midgard-build/app
# (outside the suite folder, because of its 3.5 GB budget).
#
# Steps: the services (build-services.sh → dist/mac/bin) and the consoles
# (build-consoles.sh → dist/mac/consoles) unless skipped; the Swift shell as a universal
# release build; then the bundle, assembled next to the old one and swapped in at the end:
#   Contents/Info.plist, MacOS/Midgard,
#   Resources/{bin,consoles,home,starting-state,models/{granite,trust}[,recording]}
# The starting state comes from make-starting-state.sh (~/Library/Caches/midgard-build/
# starting-state) and the model from ~/Desktop/Midgard/mimir-models; both are copied as
# APFS clones (cp -c), so they share disk with their sources; across volumes the script
# falls back to a plain copy after checking free space. The bundle must hold no symlinks
# (codesign and the home page's web server refuse them). It is signed ad hoc and verified.
# llama-server is not bundled yet (phase 5): the app uses Homebrew's.
#
#   bash tools/mac-app/build-app.sh [--skip-services] [--skip-consoles]
#                                   [--recording DIR] [--out DIR]
#
# --recording DIR (or MIDGARD_RECORDING): a phase 2 recording to bundle and replay; it must
# contain meta.json (its raw/ capture is left out). --out DIR: where Midgard.app goes.
set -euo pipefail
cd "$(dirname "$0")/../.."

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
OUT="$BUILD/app"
RECORDING="${MIDGARD_RECORDING:-}"
SKIP_SERVICES=0
SKIP_CONSOLES=0
STATE="$BUILD/starting-state"
MODELS="${MIDGARD_MODELS:-$HOME/Desktop/Midgard/mimir-models}"
BIN=dist/mac/bin
CONSOLES=dist/mac/consoles
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
PRODUCTS=(midgard heimdall mimir forge)

usage() {
  sed -n '17,18p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --skip-services) SKIP_SERVICES=1 ;;
    --skip-consoles) SKIP_CONSOLES=1 ;;
    --recording) [ $# -ge 2 ] || usage; RECORDING="$2"; shift ;;
    --out) [ $# -ge 2 ] || usage; OUT="$2"; shift ;;
    -h|--help) usage ;;
    *) echo "unknown option: $1" >&2; usage ;;
  esac
  shift
done

die() { echo "build-app: $*" >&2; exit 1; }
free_kb() { df -k "$1" | awk 'NR == 2 { print $4 }'; }
now() { date +%s; }
T0=$(now)

# --- checks before any work ---------------------------------------------------------
[ -f "$STATE/VERSION" ] || die "no starting state in $STATE: stop the suite, then run
  bash tools/mac-app/make-starting-state.sh"
for f in granite/model.manifest trust/model-signing.pub; do
  [ -f "$MODELS/$f" ] || die "missing $MODELS/$f (the Mimir model)"
done
ls "$MODELS"/granite/*.gguf >/dev/null 2>&1 || die "no .gguf model in $MODELS/granite"
if [ -n "$RECORDING" ]; then
  [ -f "$RECORDING/meta.json" ] || die "recording $RECORDING has no meta.json"
  RECORDING="$(cd "$RECORDING" && pwd)"
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
APP="$OUT/Midgard.app"
if pgrep -f "$APP/Contents/" >/dev/null 2>&1; then
  die "Midgard is running from $APP: quit it first"
fi
FREE_BEFORE=$(free_kb "$OUT")

# --- services and consoles ------------------------------------------------------------
# A skipped step reuses dist/ as it is; a warning names sources changed since it was built.
stale_warning() {  # $1 = output to compare against, rest = source paths
  local ref="$1"; shift
  local newer
  newer=$(find "$@" -type f -newer "$ref" -not -path '*/node_modules/*' -not -path '*/dist/*' 2>/dev/null | head -3)
  if [ -n "$newer" ]; then
    echo "WARNING: sources changed since $ref was built (e.g. ${newer//$'\n'/, })" >&2
  fi
}

if [ "$SKIP_SERVICES" = 1 ]; then
  for b in "${BINARIES[@]}"; do [ -x "$BIN/$b" ] || die "--skip-services: $BIN/$b is missing"; done
  oldest=$(ls -tr "${BINARIES[@]/#/$BIN/}" | head -1)
  stale_warning "$oldest" services tools/replay Cargo.lock
  echo "== services: reusing $BIN"
else
  echo "== services"
  bash tools/mac-app/build-services.sh
fi

if [ "$SKIP_CONSOLES" = 1 ]; then
  for p in "${PRODUCTS[@]}"; do [ -f "$CONSOLES/$p/index.html" ] || die "--skip-consoles: $CONSOLES/$p is missing"; done
  stale_warning "$CONSOLES/midgard/index.html" "${PRODUCTS[@]/#/products/}" ui/shared ui/design-system
  echo "== consoles: reusing $CONSOLES"
else
  echo "== consoles"
  bash tools/mac-app/build-consoles.sh
fi
T_DEPS=$(now)

# --- the shell ------------------------------------------------------------------------
echo "== shell (swift, universal release)"
SWIFT_ARGS=(-c release --arch arm64 --arch x86_64 --package-path apps/mac --scratch-path "$BUILD/swift")
swift build "${SWIFT_ARGS[@]}"
SHELL_BIN="$(swift build "${SWIFT_ARGS[@]}" --show-bin-path)/Midgard"
[ -x "$SHELL_BIN" ] || die "built shell not found at $SHELL_BIN"
archs=$(lipo -archs "$SHELL_BIN")
for a in arm64 x86_64; do
  case " $archs " in *" $a "*) ;; *) die "shell is not universal: $archs" ;; esac
done
echo "shell: $archs"
T_SWIFT=$(now)

# --- the bundle -----------------------------------------------------------------------
echo "== assembling $APP"
NEW="$OUT/.Midgard.app.new"
OLD="$OUT/.Midgard.app.old"
rm -rf "$NEW" "$OLD"
trap 'rm -rf "$NEW"' EXIT
C="$NEW/Contents"
R="$C/Resources"
mkdir -p "$C/MacOS" "$R/models"

# Copies $1 to the new path $2 as APFS clones; a plain copy (after a free space check)
# when cloning is not possible, e.g. across volumes.
clone_copy() {
  local src="$1" dst="$2"
  if cp -cRp "$src" "$dst" 2>/dev/null; then return; fi
  rm -rf "$dst"
  local need have
  need=$(du -sk "$src" | awk '{ print $1 }')
  have=$(free_kb "$(dirname "$dst")")
  [ "$have" -gt $((need + 1048576)) ] || die "not enough free disk to copy $src ($((need / 1024)) MB, $((have / 1024)) MB free)"
  echo "  (cannot clone $src; copying)"
  cp -Rp "$src" "$dst"
}

SHORT_SHA=$(git rev-parse --short HEAD)
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
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.business</string>
</dict>
</plist>
EOF
plutil -lint "$C/Info.plist"

cp "$SHELL_BIN" "$C/MacOS/Midgard"
cp -RL "$BIN" "$R/bin"
cp -RL "$CONSOLES" "$R/consoles"
cp -RL apps/mac/Resources/home "$R/home"          # fonts is a symlink in the repo
clone_copy "$STATE" "$R/starting-state"
clone_copy "$MODELS/granite" "$R/models/granite"
clone_copy "$MODELS/trust" "$R/models/trust"
if [ -n "$RECORDING" ]; then
  mkdir "$R/recording"
  for entry in "$RECORDING"/*; do
    [ "$(basename "$entry")" = raw ] && continue   # the raw capture is not replayed
    clone_copy "$entry" "$R/recording/$(basename "$entry")"
  done
  echo "recording: $RECORDING"
else
  echo "recording: none (the app starts without replay)"
fi

links=$(find "$NEW" -type l)
[ -z "$links" ] || die "the bundle contains symlinks:
$links"

# Finder info and quarantine attributes make codesign fail; the binaries in Resources/bin
# are signed one by one (--deep only reaches nested bundles).
xattr -cr "$NEW"
for b in "$R"/bin/*; do codesign --force --sign - "$b"; done
codesign --force --deep --sign - "$NEW"

# Swap in: the old bundle aside, the new one in place, then the old one removed.
[ -e "$APP" ] && mv "$APP" "$OLD"
mv "$NEW" "$APP"
trap - EXIT
rm -rf "$OLD"

echo "== codesign verify"
codesign --verify --deep --strict --verbose=1 "$APP" 2>&1
T_END=$(now)

# --- report ---------------------------------------------------------------------------
FREE_AFTER=$(free_kb "$OUT")
echo "== sizes (clones count in full here but share disk with their sources)"
du -sh "$APP"
for part in "$APP/Contents/MacOS" "$APP"/Contents/Resources/*; do
  du -sh "$part" | sed "s|$APP/Contents/||"
done
echo "free disk: $((FREE_BEFORE / 1024)) MB before, $((FREE_AFTER / 1024)) MB after ($(((FREE_BEFORE - FREE_AFTER) / 1024)) MB used, including any rebuilt dist/ and swift outputs)"
echo "times: services+consoles $((T_DEPS - T0)) s, shell $((T_SWIFT - T_DEPS)) s, bundle $((T_END - T_SWIFT)) s"
echo "built $APP (1.0, build $BUILD_NUMBER, $SHORT_SHA)"
echo "open it with: open \"$APP\""
