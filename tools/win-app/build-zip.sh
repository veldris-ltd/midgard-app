#!/usr/bin/env bash
# Build Midgard-1.0-Windows.zip, the Windows presentation app, on the dev Mac
# (docs/plans/2026-10-24-windows-app-w4-package.md, Task 2; design §4):
#   - the Windows binaries from the CI run for this commit (tools/win-app/fetch-artifacts.sh:
#     the six services; Midgard.exe, midgard-frames.exe, the Visual C++ runtime and the .NET
#     and WebView2 licences), which must be all green;
#   - the pinned Windows llama.cpp and analytics runtime (fetch-llama.sh, fetch-analytics.sh);
#   - the resources Midgard.app has, by the same steps (tools/mac-app/resources.sh): the
#     offline consoles (build-consoles.sh), home with prepared answers and credits, the
#     starting state (make-starting-state.sh's), the Granite model and its trust key, the
#     recording (minus raw/, *.tmp, *.old; heimdall.jsonl.gz unpacked), CREDITS.txt;
#   - "Install Midgard on Windows.pdf" (render-sheet.sh).
# laid out by tools/win-app/layout.sh (as the windows-shell-e2e CI job lays out its package)
# in ~/Library/Caches/midgard-build/win/package/Midgard, checked (check-imports.py,
# check-package.py), zipped with Info-ZIP (deflate; Zip64 whenever the archive needs it) to
# ~/Library/Caches/midgard-build/win/dist/Midgard-1.0-Windows.zip, tested (unzip -t, and its
# list of names against the folder), and cloned to the Desktop with the install sheet. Big
# inputs are copied as APFS clones; the package folder is removed after zipping unless
# --keep-package. Nothing is uploaded anywhere.
#
#   bash tools/win-app/build-zip.sh [--binaries SHA] [--recording DIR] [--out ZIP]
#                                   [--skip-consoles] [--keep-package] [--no-desktop]
#
#   --binaries SHA   take the CI binaries from SHA instead of HEAD: allowed only when SHA is
#                    an ancestor of HEAD and nothing but docs/ and *.md files changed since
#                    (git diff --quiet SHA HEAD -- . ':!docs' ':!*.md'), e.g. when HEAD only
#                    records results
#   --recording DIR  the recording (default: the recorded hour, recordings/hour-1 in
#                    ~/Library/Application Support/Midgard-dev); it must contain meta.json
#   --out ZIP        where the ZIP goes (default ~/Library/Caches/midgard-build/win/dist/
#                    Midgard-1.0-Windows.zip)
#   --skip-consoles  reuse dist/mac/consoles (warns if console sources are newer)
#   --keep-package   keep the package folder after zipping
#   --no-desktop     no copies on the Desktop
set -euo pipefail
cd "$(dirname "$0")/../.."
. tools/win-app/layout.sh

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
NAME=Midgard-1.0-Windows.zip
OUT="$BUILD/win/dist/$NAME"
RECORDING="$HOME/Library/Application Support/Midgard-dev/recordings/hour-1"
STATE="$BUILD/starting-state"
MODELS="${MIDGARD_MODELS:-$HOME/Desktop/Midgard/mimir-models}"
CONSOLES=dist/mac/consoles
PREPARED=apps/mac/Resources/home/prepared-answers.json
STAGING="$BUILD/win/package"
DESKTOP="$HOME/Desktop"
KEEP_FREE_KB=$((5 * 1024 * 1024))     # leave at least 5 GB free
BINARIES=
SKIP_CONSOLES=0
KEEP_PACKAGE=0
COPY_DESKTOP=1

usage() { sed -n '2,36p' "$0" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case "$1" in
    --binaries) [ $# -ge 2 ] || usage; BINARIES="$2"; shift ;;
    --recording) [ $# -ge 2 ] || usage; RECORDING="$2"; shift ;;
    --out) [ $# -ge 2 ] || usage; OUT="$2"; shift ;;
    --skip-consoles) SKIP_CONSOLES=1 ;;
    --keep-package) KEEP_PACKAGE=1 ;;
    --no-desktop) COPY_DESKTOP=0 ;;
    -h|--help) usage ;;
    *) echo "unknown option: $1" >&2; usage ;;
  esac
  shift
done

die() { echo "build-zip: $*" >&2; exit 1; }
step() { echo "== $*"; }
sha256() { shasum -a 256 "$1" | awk '{ print $1 }'; }
T0=$(date +%s)

# --- the commit ------------------------------------------------------------------------
[ -z "$(git status --porcelain --untracked-files=no)" ] \
  || die "uncommitted changes: the ZIP is built only from a clean HEAD (commit or stash them)"
HEAD_SHA=$(git rev-parse HEAD)
if [ -n "$BINARIES" ]; then
  BIN_SHA=$(git rev-parse --verify --quiet "$BINARIES^{commit}") || die "--binaries $BINARIES is not a commit here"
  if [ "$BIN_SHA" != "$HEAD_SHA" ]; then
    git merge-base --is-ancestor "$BIN_SHA" HEAD || die "--binaries $BINARIES is not an ancestor of HEAD"
    if ! git diff --quiet "$BIN_SHA" HEAD -- . ':(exclude)docs' ':(exclude)*.md'; then
      git diff --stat "$BIN_SHA" HEAD -- . ':(exclude)docs' ':(exclude)*.md' >&2
      die "more than docs changed between $BINARIES and HEAD (above): its binaries are not HEAD's"
    fi
    echo "binaries: from $BIN_SHA; since then only docs changed: $(git diff --name-only "$BIN_SHA" HEAD | tr '\n' ' ')"
  fi
else
  BIN_SHA=$HEAD_SHA
fi
echo "commit: $HEAD_SHA (binaries from $BIN_SHA)"

# --- inputs that must be there already ---------------------------------------------------
command -v zip >/dev/null && command -v unzip >/dev/null || die "zip and unzip are needed"
# (Read whole first: grep -q stopping early would fail the pipe under pipefail.)
features="$(zip -v 2>&1 || true)"
case "$features" in *ZIP64_SUPPORT*) ;; *) die "this zip has no Zip64 support" ;; esac
features="$(unzip -v 2>&1 || true)"
case "$features" in *ZIP64_SUPPORT*) ;; *) die "this unzip has no Zip64 support" ;; esac
[ -f "$STATE/VERSION" ] || die "no starting state in $STATE: stop the suite, then run
  bash tools/mac-app/make-starting-state.sh"
for f in granite/model.manifest trust/model-signing.pub; do
  [ -f "$MODELS/$f" ] || die "missing $MODELS/$f (the Mimir model)"
done
ls "$MODELS"/granite/*.gguf >/dev/null 2>&1 || die "no .gguf model in $MODELS/granite"
[ -f "$RECORDING/meta.json" ] || die "recording $RECORDING has no meta.json"
RECORDING="$(cd "$RECORDING" && pwd)"
res_check_prepared "$PREPARED"
echo "starting state: $(cat "$STATE/VERSION")"

# --- the binaries, runtimes, consoles and sheet --------------------------------------------
step "CI binaries for $BIN_SHA (tools/win-app/fetch-artifacts.sh)"
MIDGARD_BUILD_DIR="$BUILD" bash tools/win-app/fetch-artifacts.sh "$BIN_SHA"
WIN="$BUILD/win/$BIN_SHA"
step "the Windows llama.cpp (tools/win-app/fetch-llama.sh)"
MIDGARD_BUILD_DIR="$BUILD" bash tools/win-app/fetch-llama.sh
step "the Windows analytics runtime (tools/win-app/fetch-analytics.sh)"
MIDGARD_BUILD_DIR="$BUILD" bash tools/win-app/fetch-analytics.sh
if [ "$SKIP_CONSOLES" = 1 ]; then
  for p in midgard heimdall mimir forge; do [ -f "$CONSOLES/$p/index.html" ] || die "--skip-consoles: $CONSOLES/$p is missing"; done
  newer=$(find ui sdk products \( -name node_modules -o -name dist -o -name target \) -prune -o -type f \
    -newer "$CONSOLES/midgard/index.html" -print 2>/dev/null | head -3 || true)
  [ -z "$newer" ] || echo "WARNING: $CONSOLES: sources changed since the last build (e.g. ${newer//$'\n'/, })" >&2
  step "consoles: reusing $CONSOLES"
else
  step "consoles (tools/mac-app/build-consoles.sh)"
  bash tools/mac-app/build-consoles.sh
fi
step "install sheet (tools/win-app/render-sheet.sh)"
SHEET="$BUILD/win/sheet/$WL_SHEET_NAME"
MIDGARD_BUILD_DIR="$BUILD" bash tools/win-app/render-sheet.sh "$SHEET"

# --- free disk ---------------------------------------------------------------------------
# The package is mostly APFS clones, but the unpacked heimdall.jsonl and the ZIP are new
# data: the ZIP is at most about the package's size.
mkdir -p "$STAGING" "$(dirname "$OUT")"
OUT="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
jsonl_kb=0
if [ -f "$RECORDING/heimdall.jsonl.gz" ]; then
  jsonl_kb=$(( $(gzip -l "$RECORDING/heimdall.jsonl.gz" | awk 'NR == 2 { print $2 }') / 1024 ))
fi
inputs_kb=$(du -sk "$MODELS/granite" "$STATE" "$WIN" "$BUILD/win/llama/server" "$BUILD/win/analytics/runtime" \
  "$BUILD/win/analytics/models" "$CONSOLES" | awk '{ s += $1 } END { print s }')
rec_kb=$(find "$RECORDING" -mindepth 1 -maxdepth 1 ! -name raw ! -name '*.tmp' ! -name '*.old' -print0 \
  | xargs -0 du -sk | awk '{ s += $1 } END { print s }')
need_kb=$((inputs_kb + rec_kb + 2 * jsonl_kb))
FREE_BEFORE=$(res_free_kb "$STAGING")
echo "disk: $((FREE_BEFORE / 1024)) MB free; the package is about $(((inputs_kb + rec_kb + jsonl_kb) / 1024)) MB" \
  "(mostly clones), the ZIP at most about $((need_kb / 1024)) MB new; keeping $((KEEP_FREE_KB / 1024)) MB free"
[ "$FREE_BEFORE" -gt $((need_kb + KEEP_FREE_KB)) ] \
  || die "not enough free disk: need about $(((need_kb + KEEP_FREE_KB) / 1024)) MB, have $((FREE_BEFORE / 1024)) MB"

# --- the package -------------------------------------------------------------------------
step "layout $STAGING/Midgard (tools/win-app/layout.sh)"
rm -rf "$STAGING/Midgard"
cleanup() { [ "$KEEP_PACKAGE" = 1 ] || rm -rf "$STAGING/Midgard"; rm -f "$OUT.part"; }
trap cleanup EXIT
PKG="$STAGING/Midgard"
WL_SHELL="$WIN/shell" WL_SERVICES="$WIN/services" WL_LLAMA="$BUILD/win/llama/server" \
  WL_ANALYTICS="$BUILD/win/analytics" WL_CONSOLES="$CONSOLES" WL_STATE="$STATE" WL_MODELS="$MODELS" \
  WL_RECORDING="$RECORDING" WL_NOTICES="$WIN/shell/notices" WL_VCRUNTIME="$WIN/shell/vcruntime" \
  WL_SHEET="$SHEET" win_layout "$PKG"
win_place_vcruntime "$WIN/shell/vcruntime" "$PKG"
{
  echo "Midgard 1.0 for Windows"
  echo "commit: $HEAD_SHA"
  echo "binaries: $BIN_SHA ($(sed -n 's/^date: //p' "$WIN/shell/BUILD.txt" | tr -d '\r'))"
  echo "starting state: $(cat "$STATE/VERSION")"
  echo "recording: $(basename "$RECORDING")"
  echo "built: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$PKG/resources/BUILD.txt"
# No Finder info or quarantine attributes in the archive's sources (zip -X leaves them out too).
xattr -cr "$PKG"

step "checks"
REPORTS="$BUILD/win/dist/reports"
mkdir -p "$REPORTS"
python3 -I tools/win-app/check-imports.py "$PKG" --report "$REPORTS/imports.txt" | tail -n 1 \
  || die "check-imports.py failed: $REPORTS/imports.txt"
[ "$(tail -n 1 "$REPORTS/imports.txt" | cut -c1-4)" = PASS ] || die "check-imports.py: $(tail -n 1 "$REPORTS/imports.txt")"
python3 -I tools/win-app/check-package.py "$PKG" --report "$REPORTS/package.txt" \
  || die "check-package.py failed (above; $REPORTS/package.txt)"

# --- the ZIP -----------------------------------------------------------------------------
step "zip $OUT"
rm -f "$OUT.part"
T_ZIP=$(date +%s)
( cd "$STAGING" && zip -r -X -q "$OUT.part" Midgard )
echo "zipped in $(( $(date +%s) - T_ZIP )) s"
step "unzip -t"
unzip -tq "$OUT.part" || die "unzip -t failed on the new ZIP"
# The archive holds exactly the package's files and folders.
( cd "$STAGING" && find Midgard -type f | LC_ALL=C sort ) > "$REPORTS/package-files.txt"
unzip -Z1 "$OUT.part" | grep -v '/$' | LC_ALL=C sort > "$REPORTS/zip-files.txt"
cmp -s "$REPORTS/package-files.txt" "$REPORTS/zip-files.txt" \
  || { diff "$REPORTS/package-files.txt" "$REPORTS/zip-files.txt" | head -20 >&2; die "the ZIP's files are not the package's"; }
echo "the ZIP lists the package's $(wc -l < "$REPORTS/zip-files.txt" | tr -d ' ') files"
mv -f "$OUT.part" "$OUT"
SIZE=$(stat -f %z "$OUT")
SUM=$(sha256 "$OUT")
echo "$SUM  $NAME" > "$OUT.sha256"

if [ "$COPY_DESKTOP" = 1 ]; then
  step "Desktop"
  for f in "$DESKTOP/$NAME" "$DESKTOP/$WL_SHEET_NAME"; do
    [ ! -e "$f" ] || [ -f "$f" ] || die "$f exists and is not a plain file; move it aside"
  done
  rm -f "$DESKTOP/$NAME"
  cp -c "$OUT" "$DESKTOP/$NAME" 2>/dev/null || cp "$OUT" "$DESKTOP/$NAME"
  cp "$SHEET" "$DESKTOP/$WL_SHEET_NAME"
  [ "$(sha256 "$DESKTOP/$NAME")" = "$SUM" ] || die "the Desktop copy differs from $OUT"
  echo "$DESKTOP/$NAME"
  echo "$DESKTOP/$WL_SHEET_NAME"
fi

if [ "$KEEP_PACKAGE" = 1 ]; then
  echo "package kept: $PKG"
else
  rm -rf "$PKG"
fi
trap - EXIT
FREE_AFTER=$(res_free_kb "$STAGING")
echo "free disk: $((FREE_BEFORE / 1024)) MB before, $((FREE_AFTER / 1024)) MB after"
echo "time: $(( $(date +%s) - T0 )) s"
echo "built $OUT"
echo "size: $SIZE bytes ($(( SIZE / 1024 / 1024 )) MB)"
echo "sha256: $SUM"
