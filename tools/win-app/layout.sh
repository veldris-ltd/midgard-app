# The Windows package's layout, in one place: sourced (not run), from the repo root, by
# tools/win-app/build-zip.sh (the presenters' ZIP, on the dev Mac) and by tools/ci/run.sh's
# windows-shell-e2e job (the same layout with the fixture, on a Windows runner), so CI tests
# the folder presenters get (docs/plans/2026-10-24-windows-app-w4-package.md, Task 4).
#
#   win_layout PKG     lays out PKG (a new folder, normally .../Midgard) from:
#     WL_SHELL      Midgard.exe and midgard-frames.exe (the shell artefact, or a publish folder)
#     WL_SERVICES   the six services' .exe
#     WL_LLAMA      tools/win-app/fetch-llama.sh's server/ folder
#     WL_ANALYTICS  tools/win-app/fetch-analytics.sh's folder (runtime/, models/, licenses/)
#     WL_CONSOLES   the offline consoles (tools/mac-app/build-consoles.sh's dist/mac/consoles)
#     WL_STATE      the starting state
#     WL_MODELS     the Mimir model: granite/ and trust/
#     WL_RECORDING  the recording (its heimdall.jsonl.gz is unpacked to heimdall.jsonl)
#     WL_NOTICES    the shell artefact's notices/ (.NET runtime and WebView2 licences)
#     WL_VCRUNTIME  the shell artefact's vcruntime/ (its VERSIONS.txt is the runtime's notice)
#     WL_SHEET      optional: "Install Midgard on Windows.pdf" (tools/win-app/render-sheet.sh)
#   win_place_vcruntime RT PKG   the Visual C++ runtime DLLs in RT into each of
#                 VCRUNTIME_FOLDERS of PKG (separate, so CI can check the imports before)
#
# The result:
#   Midgard.exe, Install Midgard on Windows.pdf
#   helpers\    the six services, midgard-frames.exe, llama\ (llama-server.exe and its DLLs),
#               analytics\ (the Python runtime), and the Visual C++ runtime in each of those
#   resources\  consoles\, home\ (with credits.txt), starting-state\, models\{granite,trust,
#               yolox}\, analytics\{analytics.py,config.json}, recording\, licenses\ (CREDITS.txt
#               and every third-party licence and notice)
# The shared parts are tools/mac-app/resources.sh's, as Midgard.app gets them.

. tools/mac-app/resources.sh

WL_SERVICE_NAMES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
WL_ANALYTICS_SRC=heimdall/analytics
WL_SHEET_NAME="Install Midgard on Windows.pdf"
# The Visual C++ runtime the package ships app-local (tools/ci/run.sh windows_vcruntime), and
# the package folders that get a copy: the services', llama.cpp's and the analytics'
# python.exe's.
VCRUNTIME_DLLS=(vcruntime140.dll vcruntime140_1.dll msvcp140.dll msvcp140_1.dll)
VCRUNTIME_FOLDERS=(helpers helpers/llama helpers/analytics)

win_place_vcruntime() {
  local d f
  for d in "${VCRUNTIME_FOLDERS[@]}"; do
    [ -d "$2/$d" ] || continue
    for f in "${VCRUNTIME_DLLS[@]}"; do cp -f "$1/$f" "$2/$d/$f"; done
  done
}

win_layout() {
  local pkg="$1" v b f name
  for v in WL_SHELL WL_SERVICES WL_LLAMA WL_ANALYTICS WL_CONSOLES WL_STATE WL_MODELS WL_RECORDING WL_NOTICES WL_VCRUNTIME; do
    [ -n "${!v:-}" ] && [ -d "${!v}" ] || res_die "layout: $v is not a folder (${!v:-unset})"
  done
  [ ! -e "$pkg" ] || res_die "layout: $pkg exists"
  for f in "$WL_SHELL/Midgard.exe" "$WL_SHELL/midgard-frames.exe" "$WL_LLAMA/llama-server.exe" \
      "$WL_LLAMA/LICENSE" "$WL_LLAMA/LICENSE-LLVM-OpenMP" "$WL_ANALYTICS/runtime/python.exe" \
      "$WL_ANALYTICS/models/yolox_s.onnx" "$WL_VCRUNTIME/VERSIONS.txt" "$WL_NOTICES/VERSIONS.txt" \
      "$WL_STATE/VERSION" "$WL_MODELS/trust/model-signing.pub" "$WL_RECORDING/meta.json" \
      "$WL_ANALYTICS_SRC/analytics.py" "$WL_ANALYTICS_SRC/config.json"; do
    [ -f "$f" ] || res_die "layout: missing $f"
  done
  for b in "${WL_SERVICE_NAMES[@]}"; do
    [ -f "$WL_SERVICES/$b.exe" ] || res_die "layout: missing $WL_SERVICES/$b.exe"
  done
  local h="$pkg/helpers" r="$pkg/resources"
  mkdir -p "$h/llama" "$r/models/yolox" "$r/analytics" "$r/licenses"

  # The programs.
  cp "$WL_SHELL/Midgard.exe" "$pkg/"
  for b in "${WL_SERVICE_NAMES[@]}"; do cp "$WL_SERVICES/$b.exe" "$h/"; done
  cp "$WL_SHELL/midgard-frames.exe" "$h/"
  # The model server: its programs in helpers\llama, its licences with the others.
  for f in "$WL_LLAMA"/*; do
    name=$(basename "$f")
    case "$name" in
      LICENSE) cp "$f" "$r/licenses/llama.cpp-LICENSE.txt" ;;
      LICENSE-LLVM-OpenMP) cp "$f" "$r/licenses/LLVM-OpenMP-LICENSE.txt" ;;   # libomp.dll's
      *.exe|*.dll) cp "$f" "$h/llama/$name" ;;
      *) res_die "layout: unexpected file in $WL_LLAMA: $name" ;;
    esac
  done
  # The camera analytics: the runtime, the code and the model; their licences.
  res_clone_copy "$WL_ANALYTICS/runtime" "$h/analytics"
  cp "$WL_ANALYTICS_SRC/analytics.py" "$WL_ANALYTICS_SRC/config.json" "$r/analytics/"
  res_clone_copy "$WL_ANALYTICS/models/yolox_s.onnx" "$r/models/yolox/yolox_s.onnx"
  for f in "$WL_ANALYTICS"/licenses/*; do cp "$f" "$r/licenses/"; done
  # The .NET runtime inside Midgard.exe and midgard-frames.exe, WebView2's SDK, and the
  # Visual C++ runtime's notice.
  for f in "$WL_NOTICES"/*; do
    name=$(basename "$f")
    [ "$name" = VERSIONS.txt ] && name=dotnet-and-WebView2-VERSIONS.txt
    cp "$f" "$r/licenses/$name"
  done
  cp "$WL_VCRUNTIME/VERSIONS.txt" "$r/licenses/Microsoft-Visual-C++-runtime-NOTICE.txt"

  # The resources Midgard.app has too.
  res_consoles "$WL_CONSOLES" "$r/consoles"
  res_home "$r/home"
  res_credits "$r/licenses" "$r/home"
  res_starting_state "$WL_STATE" "$r/starting-state"
  res_models "$WL_MODELS" "$r/models"
  res_recording "$WL_RECORDING" "$r/recording" unpack

  if [ -n "${WL_SHEET:-}" ]; then
    [ -f "$WL_SHEET" ] || res_die "layout: no install sheet at $WL_SHEET"
    cp "$WL_SHEET" "$pkg/$WL_SHEET_NAME"
  fi
}
