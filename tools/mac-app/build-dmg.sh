#!/usr/bin/env bash
# Build Midgard-1.0.dmg, the drag-to-Applications installer, from the app build-app.sh made.
#
# The image (HFS+, zlib-compressed, volume "Midgard") holds:
#   Midgard.app            copied with ditto from ~/Library/Caches/midgard-build/app
#   Applications           a symlink to /Applications, the drag target
#   Install Midgard.pdf    install-sheet.html rendered with headless Chrome (A4, one page)
# It is checked before it is published: hdiutil verify, then mounted read-only, the app
# inside must pass codesign --verify --deep --strict and the sheet and the link must be
# there. Then it is moved into ~/Library/Caches/midgard-build/dist/Midgard-1.0.dmg and
# copied to ~/Desktop/Midgard-1.0.dmg (outside the suite folder and its 3.5 GB budget;
# --no-desktop skips that copy). Work happens in a temporary folder under
# ~/Library/Caches/midgard-build/tmp, removed on exit.
#
#   bash tools/mac-app/build-dmg.sh [--no-desktop]
#
# Environment: MIDGARD_BUILD_DIR (default ~/Library/Caches/midgard-build), CHROME (path
# to the Chrome binary).
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO="$PWD"

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
APP="$BUILD/app/Midgard.app"
DIST="$BUILD/dist"
NAME=Midgard-1.0.dmg
VOLNAME=Midgard
SHEET_HTML="$REPO/tools/mac-app/install-sheet.html"
SHEET_PDF="Install Midgard.pdf"
DESKTOP="$HOME/Desktop/$NAME"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
COPY_DESKTOP=1

for arg in "$@"; do
  case "$arg" in
    --no-desktop) COPY_DESKTOP=0 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "build-dmg: unknown option $arg" >&2; exit 2 ;;
  esac
done

die() { echo "build-dmg: $*" >&2; exit 1; }
step() { echo "==> $*"; }

# --- Preconditions ---------------------------------------------------------------------
[ -x "$APP/Contents/MacOS/Midgard" ] \
  || die "no app at $APP; build it first: bash tools/mac-app/build-app.sh"
if pgrep -f -- "$APP/Contents/" >/dev/null 2>&1; then
  die "Midgard is running from $APP; quit it first (Cmd-Q) so the copy is consistent"
fi
[ -x "$CHROME" ] || die "headless Chrome not found at $CHROME (set CHROME=...)"
[ -f "$SHEET_HTML" ] || die "missing $SHEET_HTML"
if [ -e "$DESKTOP" ] && { [ -L "$DESKTOP" ] || [ ! -f "$DESKTOP" ]; }; then
  die "$DESKTOP exists and is not a plain file; move it aside"
fi

step "checking the app's signature"
codesign --verify --deep --strict "$APP" || die "$APP does not verify; rebuild it with build-app.sh"

# Room for the staged copy (a clone on APFS, but don't count on it), the image and the
# Desktop copy: twice the app plus 1 GB.
app_kb=$(du -sk "$APP" | awk '{print $1}')
mkdir -p "$BUILD/tmp" "$DIST"
free_kb=$(df -k "$BUILD/tmp" | awk 'NR==2 {print $4}')
need_kb=$(( app_kb * 2 + 1024 * 1024 ))
[ "$free_kb" -ge "$need_kb" ] \
  || die "not enough free disk: need about $(( need_kb / 1024 )) MB, have $(( free_kb / 1024 )) MB"

WORK=$(mktemp -d "$BUILD/tmp/dmg.XXXXXX")
STAGE="$WORK/stage"
MNT="$WORK/mnt"
TMP_DMG="$WORK/$NAME"
DESKTOP_PART="$HOME/Desktop/.$NAME.partial"
MOUNTED=0
cleanup() {
  if [ "$MOUNTED" = 1 ]; then
    hdiutil detach -quiet "$MNT" 2>/dev/null || hdiutil detach -quiet -force "$MNT" 2>/dev/null || true
  fi
  rm -rf "$WORK"
  rm -f "$DESKTOP_PART"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
mkdir -p "$STAGE" "$MNT"

# --- The install sheet -----------------------------------------------------------------
step "rendering $SHEET_PDF"
"$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
  --user-data-dir="$WORK/chrome" --no-pdf-header-footer \
  --print-to-pdf="$STAGE/$SHEET_PDF" "file://$SHEET_HTML" >"$WORK/chrome.log" 2>&1 \
  || { cat "$WORK/chrome.log" >&2; die "Chrome failed to render the sheet"; }
[ -s "$STAGE/$SHEET_PDF" ] || die "Chrome wrote no PDF"
# One page, and the design system's fonts embedded (not a fallback face).
python3 -I - "$STAGE/$SHEET_PDF" <<'PY' || die "the install sheet is not one page with its own fonts; edit install-sheet.html"
import re, sys
data = open(sys.argv[1], 'rb').read()
pages = len(re.findall(rb'/Type\s*/Page(?![a-zA-Z])', data))
missing = [f for f in (b'Oxanium', b'SpaceGrotesk', b'JetBrainsMono') if f not in data]
print(f"    {pages} page(s); fonts missing: {', '.join(m.decode() for m in missing) or 'none'}")
sys.exit(0 if pages == 1 and not missing else 1)
PY

# --- Stage -----------------------------------------------------------------------------
step "staging the app"
# ditto keeps the bundle exactly (extended attributes, the signature's sealed resources);
# --clone shares disk with the source on APFS.
ditto --clone "$APP" "$STAGE/Midgard.app"
ln -s /Applications "$STAGE/Applications"
codesign --verify --deep --strict "$STAGE/Midgard.app" || die "the staged copy does not verify"

# --- The image -------------------------------------------------------------------------
step "creating the image (compressing ~$(( app_kb / 1024 )) MB; about a minute; the model barely compresses)"
hdiutil create -quiet -volname "$VOLNAME" -srcfolder "$STAGE" -fs HFS+ \
  -format UDZO -imagekey zlib-level=9 -ov "$TMP_DMG"
step "verifying the image"
hdiutil verify -quiet "$TMP_DMG" || die "hdiutil verify failed"

step "checking the mounted image"
hdiutil attach -quiet -nobrowse -readonly -noautoopen -mountpoint "$MNT" "$TMP_DMG"
MOUNTED=1
codesign --verify --deep --strict "$MNT/Midgard.app" || die "the app inside the image does not verify"
[ -s "$MNT/$SHEET_PDF" ] || die "the image has no $SHEET_PDF"
[ -L "$MNT/Applications" ] && [ "$(readlink "$MNT/Applications")" = /Applications ] \
  || die "the image's Applications link is missing or wrong"
echo "    app verifies; $SHEET_PDF and the Applications link are there"
hdiutil detach -quiet "$MNT"
MOUNTED=0

# --- Publish ---------------------------------------------------------------------------
# Same volume as $WORK, so the move is a rename: dist never holds a half-written image.
mv -f "$TMP_DMG" "$DIST/$NAME"
OUTPUTS=("$DIST/$NAME")
if [ "$COPY_DESKTOP" = 1 ]; then
  cp -c "$DIST/$NAME" "$DESKTOP_PART" 2>/dev/null || cp "$DIST/$NAME" "$DESKTOP_PART"
  mv -f "$DESKTOP_PART" "$DESKTOP"
  OUTPUTS+=("$DESKTOP")
fi

sha=$(shasum -a 256 "$DIST/$NAME" | awk '{print $1}')
bytes=$(stat -f %z "$DIST/$NAME")
echo
echo "Midgard installer: $NAME"
echo "  size     $(( bytes / 1024 / 1024 )) MB ($bytes bytes); app $(( app_kb / 1024 )) MB unpacked"
echo "  sha256   $sha"
for o in "${OUTPUTS[@]}"; do echo "  written  $o"; done
