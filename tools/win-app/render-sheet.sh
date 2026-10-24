#!/usr/bin/env bash
# Render the Windows install sheet, tools/win-app/install-sheet.html, to
# "Install Midgard on Windows.pdf" (A4) with headless Chrome, as build-dmg.sh renders the Mac
# sheet, and check it: exactly one page, and the design system's fonts (Oxanium, Space
# Grotesk, JetBrains Mono) embedded rather than a fallback face. The PDF is written to
# $MIDGARD_BUILD_DIR/win/sheet/ (outside the suite folder) or to the path given, and only
# after it passes; build-zip.sh puts it in the ZIP and on the Desktop. Work happens in a
# temporary folder under $MIDGARD_BUILD_DIR/tmp, removed on exit.
#
#   bash tools/win-app/render-sheet.sh [OUTPUT.pdf]
#
# Environment: MIDGARD_BUILD_DIR (default ~/Library/Caches/midgard-build), CHROME (path
# to the Chrome binary).
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO="$PWD"

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
SHEET_HTML="$REPO/tools/win-app/install-sheet.html"
SHEET_PDF="Install Midgard on Windows.pdf"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
OUT="$BUILD/win/sheet/$SHEET_PDF"

case "${1:-}" in
  -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
  -*) echo "render-sheet: unknown option $1" >&2; exit 2 ;;
  '') ;;
  *) OUT="$1" ;;
esac
[ $# -le 1 ] || { echo "render-sheet: at most one output path" >&2; exit 2; }

die() { echo "render-sheet: $*" >&2; exit 1; }
step() { echo "==> $*"; }

[ -x "$CHROME" ] || die "headless Chrome not found at $CHROME (set CHROME=...)"
command -v python3 >/dev/null 2>&1 || die "python3 is needed to check the install sheet (xcode-select --install)"
[ -f "$SHEET_HTML" ] || die "missing $SHEET_HTML"

mkdir -p "$BUILD/tmp" "$(dirname "$OUT")"
WORK=$(mktemp -d "$BUILD/tmp/sheet.XXXXXX")
CHROME_PID=
cleanup() {
  [ -n "$CHROME_PID" ] && kill "$CHROME_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

step "rendering $SHEET_PDF"
"$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
  --user-data-dir="$WORK/chrome" --no-pdf-header-footer \
  --print-to-pdf="$WORK/$SHEET_PDF" "file://$SHEET_HTML" >"$WORK/chrome.log" 2>&1 &
CHROME_PID=$!
for _ in $(seq 120); do kill -0 "$CHROME_PID" 2>/dev/null || break; sleep 1; done
if kill -0 "$CHROME_PID" 2>/dev/null; then
  kill "$CHROME_PID" 2>/dev/null || true
  die "Chrome took more than 120 s to render the sheet"
fi
chrome_status=0
wait "$CHROME_PID" || chrome_status=$?
CHROME_PID=
[ "$chrome_status" = 0 ] || { cat "$WORK/chrome.log" >&2; die "Chrome failed to render the sheet"; }
[ -s "$WORK/$SHEET_PDF" ] || die "Chrome wrote no PDF"
# One page, and the design system's fonts embedded (not a fallback face).
python3 -I - "$WORK/$SHEET_PDF" <<'PY' || die "the install sheet is not one page with its own fonts; edit install-sheet.html"
import re, sys
data = open(sys.argv[1], 'rb').read()
pages = len(re.findall(rb'/Type\s*/Page(?![a-zA-Z])', data))
missing = [f for f in (b'Oxanium', b'SpaceGrotesk', b'JetBrainsMono') if f not in data]
print(f"    {pages} page(s); fonts missing: {', '.join(m.decode() for m in missing) or 'none'}")
sys.exit(0 if pages == 1 and not missing else 1)
PY

mv -f "$WORK/$SHEET_PDF" "$OUT"
echo "    $OUT"
