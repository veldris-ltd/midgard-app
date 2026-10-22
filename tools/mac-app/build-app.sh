#!/usr/bin/env bash
# Build Midgard.app, the macOS presentation app, into ~/Library/Caches/midgard-build/app
# (outside the suite folder, because of its 3.5 GB budget).
#
# Steps: the services (build-services.sh → dist/mac/bin) and the consoles
# (build-consoles.sh → dist/mac/consoles) unless skipped; the Swift shell as a universal
# release build; then the bundle, assembled next to the old one, signed, verified, and only
# then swapped in:
#   Contents/Info.plist, MacOS/Midgard, Helpers/<service binaries>,
#   Resources/{consoles,home,starting-state,models/{granite,trust}[,recording]}
# The starting state comes from make-starting-state.sh (~/Library/Caches/midgard-build/
# starting-state) and the model from ~/Desktop/Midgard/mimir-models; both are copied as
# APFS clones (cp -c), so they share disk with their sources; across volumes the script
# falls back to a plain copy after checking free space. The bundle must hold no symlinks
# (codesign and the home page's web server refuse them). It is signed ad hoc: each helper,
# then the bundle. llama-server is not bundled yet (phase 5): the app uses Homebrew's.
#
#   bash tools/mac-app/build-app.sh [--skip-services] [--skip-consoles]
#                                   [--recording DIR] [--out DIR]
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
STATE="$BUILD/starting-state"
MODELS="${MIDGARD_MODELS:-$HOME/Desktop/Midgard/mimir-models}"
BIN=dist/mac/bin
CONSOLES=dist/mac/consoles
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
PRODUCTS=(midgard heimdall mimir forge)

usage() {
  cat >&2 <<'EOF'
usage: bash tools/mac-app/build-app.sh [--skip-services] [--skip-consoles]
                                       [--recording DIR] [--out DIR]
  --skip-services   reuse dist/mac/bin as it is (warns if Rust sources are newer)
  --skip-consoles   reuse dist/mac/consoles as it is (warns if console sources are newer)
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
for b in "${BINARIES[@]}"; do cp "$BIN/$b" "$C/Helpers/$b"; done
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

# Finder info and quarantine attributes make codesign fail. Inside out: each helper, then
# the bundle (which seals the helpers' signatures and the resources); no --deep.
echo "== signing (ad hoc)"
xattr -cr "$NEW"
for b in "${BINARIES[@]}"; do codesign --force --sign - "$C/Helpers/$b"; done
codesign --force --sign - "$NEW"

echo "== codesign verify"
for b in "${BINARIES[@]}"; do
  codesign --verify --strict "$C/Helpers/$b" || die "helper $b fails codesign --verify"
done
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
for part in "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP"/Contents/Resources/*; do
  du -sh "$part" | sed "s|$APP/Contents/||"
done
echo "free disk: $((FREE_BEFORE / 1024)) MB before, $((FREE_AFTER / 1024)) MB after ($(((FREE_BEFORE - FREE_AFTER) / 1024)) MB used, including any rebuilt dist/ and swift outputs)"
echo "times: services+consoles $((T_DEPS - T0)) s, shell $((T_SWIFT - T_DEPS)) s, bundle $((T_END - T_SWIFT)) s"
echo "built $APP (1.0, build $BUILD_NUMBER, $SHORT_SHA)"
echo "open it with: open \"$APP\""
