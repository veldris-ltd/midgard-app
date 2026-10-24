# The presentation app's shared resources, the same on the Mac and on Windows: sourced (not
# run) by tools/mac-app/build-app.sh (Midgard.app's Contents/Resources) and by
# tools/win-app/layout.sh (the Windows package's Midgard\resources, for build-zip.sh and the
# windows-shell-e2e CI job), from the repo root. Each function copies one part:
#   res_check_prepared FILE     the prepared Mimir answers (home/prepared-answers.json) parse
#                               as version 1; prints a summary ("none" when missing)
#   res_consoles SRC DST        the four offline consoles (build-consoles.sh's dist/mac/consoles)
#   res_home DST                the home page (apps/mac/Resources/home, its fonts link resolved)
#   res_credits LICENSES HOME   CREDITS.txt into the licences and home/credits.txt
#   res_starting_state SRC DST  the starting state (make-starting-state.sh's), cloned
#   res_models SRC DST          the Mimir model: SRC/{granite,trust} into DST/, cloned
#   res_recording SRC DST [unpack]  a recording minus raw/, *.tmp and *.old, cloned, with a
#                               line per part (media, radar, traffic, tiles); "unpack" also
#                               writes heimdall.jsonl.gz out as heimdall.jsonl (Windows
#                               replays plain heimdall.jsonl: docs/plans/2026-10-23-windows-
#                               app-w1-services.md)
#   res_clone_copy SRC DST      a copy as APFS clones on a Mac, else (or when cloning fails)
#                               a plain copy after a free space check
# A failure calls the caller's die() if it has one.
#
# Python: RES_PYTHON (default python3; Git Bash on Windows has python).

RES_HOME_SRC=apps/mac/Resources/home
RES_FONTS=ui/design-system/src/fonts      # what home/fonts links to in the repo
RES_CREDITS=tools/mac-app/CREDITS.txt

res_die() {
  if declare -F die >/dev/null; then die "$@"; fi
  echo "resources: $*" >&2
  exit 1
}
res_free_kb() { df -k "$1" | awk 'NR == 2 { print $4 }'; }

res_clone_copy() {
  local src="$1" dst="$2" err need have
  if [ "$(uname -s)" = Darwin ]; then
    if err=$(cp -cRp "$src" "$dst" 2>&1); then return; fi
    echo "  cannot clone $src: ${err:-unknown error}; copying instead"
    rm -rf "$dst"
  fi
  need=$(du -sk "$src" | awk '{ print $1 }')
  have=$(res_free_kb "$(dirname "$dst")")
  [ "$have" -gt $((need + 1048576)) ] || res_die "not enough free disk to copy $src ($((need / 1024)) MB, $((have / 1024)) MB free)"
  cp -Rp "$src" "$dst"
}

res_check_prepared() {
  local file="$1" summary
  if [ -f "$file" ]; then
    summary=$("${RES_PYTHON:-python3}" -c '
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
v = d.get("version") if isinstance(d, dict) else None
assert type(v) is int and v == 1, "not version 1"
e, g = d.get("entries") or {}, d.get("guide") or {}
print("%d answers, guide: %d questions, %d alerts (recording %s)" % (
    len(e), len(g.get("questions") or []), len(g.get("alerts") or []), d.get("recording")))
' "$file" 2>&1) || res_die "$file is not a valid version 1 prepared answers file: ${summary##*$'\n'}"
    echo "prepared answers: ${summary%$'\r'}"
  else
    echo "prepared answers: none ($file missing): every Mimir answer is asked live"
  fi
}

res_consoles() { cp -RL "$1" "$2"; }

# home/fonts is a symbolic link in the repo (to the design system's fonts): resolved with
# `cp -RL`. A Windows checkout makes no links (it writes a file holding the link's text), so
# there the fonts are copied in from where the link points.
res_home() {
  local dst="$1" link
  if [ -L "$RES_HOME_SRC/fonts" ]; then
    cp -RL "$RES_HOME_SRC" "$dst"
  else
    link=$(tr -d '\r\n' < "$RES_HOME_SRC/fonts" 2>/dev/null || true)
    [ "$link" = "../../../../$RES_FONTS" ] || res_die "$RES_HOME_SRC/fonts is neither a link nor a checkout's link text"
    cp -R "$RES_HOME_SRC" "$dst"
    rm -f "$dst/fonts"
    cp -R "$RES_FONTS" "$dst/fonts"
  fi
}

# The data and software credits: with the licences, and in home/ for the home page's "Data
# and credits" panel (served at /home/credits.txt).
res_credits() {
  cp "$RES_CREDITS" "$1/CREDITS.txt"
  cp "$RES_CREDITS" "$2/credits.txt"
}

res_starting_state() { res_clone_copy "$1" "$2"; }

res_models() {
  res_clone_copy "$1/granite" "$2/granite"
  res_clone_copy "$1/trust" "$2/trust"
}

res_recording() {
  local src="$1" dst="$2" unpack="${3:-}" entry name
  mkdir "$dst"
  for entry in "$src"/*; do
    name=$(basename "$entry")
    case "$name" in
      raw) continue ;;              # the raw capture is not replayed
      *.tmp|*.old) continue ;;      # a capture (capture-radar.py, -traffic.py, -tiles.py) still being written or swapped
    esac
    if [ "$unpack" = unpack ] && [ "$name" = heimdall.jsonl.gz ]; then
      [ ! -e "$src/heimdall.jsonl" ] || res_die "recording $src has both heimdall.jsonl and heimdall.jsonl.gz"
      gzip -dc "$entry" > "$dst/heimdall.jsonl.part" || res_die "cannot unpack $entry"
      mv "$dst/heimdall.jsonl.part" "$dst/heimdall.jsonl"
      echo "recording: unpacked heimdall.jsonl.gz ($(du -sh "$entry" | cut -f1)) to heimdall.jsonl ($(du -sh "$dst/heimdall.jsonl" | cut -f1))"
      continue
    fi
    res_clone_copy "$entry" "$dst/$name"
  done
  echo "recording: $src"
  # Camera stills and clips (record.py pack), served by the app at /media/.
  if [ -d "$dst/media" ]; then
    echo "recording media: $(find "$dst/media" -type f | wc -l | tr -d ' ') files, $(du -sh "$dst/media" | cut -f1)"
  else
    echo "recording media: none (cameras have no image or clip; re-pack with record.py pack)"
  fi
  # Rain radar frames (capture-radar.py), served by the app at /radar/.
  if [ -f "$dst/radar/manifest.json" ]; then
    echo "recording radar: $(find "$dst/radar" -name '*.png' -type f | wc -l | tr -d ' ') tiles, $(du -sh "$dst/radar" | cut -f1)"
  else
    echo "recording radar: none (the RADAR chip shows unavailable; run tools/mac-app/capture-radar.py)"
  fi
  # Road traffic flow snapshot (capture-traffic.py), served by the app at /traffic/.
  if [ -f "$dst/traffic/manifest.json" ]; then
    echo "recording traffic: $(find "$dst/traffic" -name '*.pbf' -type f | wc -l | tr -d ' ') tiles, $(du -sh "$dst/traffic" | cut -f1)"
  else
    echo "recording traffic: none (the TRAFFIC chip shows unavailable; run tools/mac-app/capture-traffic.py)"
  fi
  # Close-up globe terrain and imagery tiles (capture-tiles.py), served by the app at /tiles/.
  if [ -f "$dst/tiles/manifest.json" ]; then
    echo "recording tiles: $(find "$dst/tiles" -type f \( -name '*.png' -o -name '*.jpg' \) | wc -l | tr -d ' ') tiles, $(du -sh "$dst/tiles" | cut -f1)"
  else
    echo "recording tiles: none (the globe keeps the bundled map close up; run tools/mac-app/capture-tiles.py)"
  fi
}
