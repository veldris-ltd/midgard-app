#!/usr/bin/env bash
# Build the four consoles' static sites for the Mac app into dist/mac/consoles/<product>.
# Offline builds (VITE_OFFLINE=1): no internet features, and VITE_TOMTOM_KEY is set empty
# in the environment so it overrides products/heimdall/ui/.env.local and no key is
# inlined. The gateway URL comes from the app at run time (window.__MIDGARD_GATEWAY__).
# Vite writes straight to dist/, so no dist/ is left inside products/*/ui. Then the built
# files are checked: the script fails if any internet endpoint the offline build must not
# reach, or the TomTom key itself, is in them (case-insensitive for the endpoints), and
# lists every other URL found for review (attribution links are expected).
#
#   bash tools/mac-app/build-consoles.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=dist/mac/consoles
PRODUCTS=(midgard heimdall mimir forge)
FORBIDDEN=(api.tomtom.com rainviewer elevation-tiles-prod tiles.maps.eox.at)

export VITE_OFFLINE=1
export VITE_TOMTOM_KEY=

mkdir -p "$OUT"
OUT_ABS="$(cd "$OUT" && pwd)"
for p in "${PRODUCTS[@]}"; do
  echo "== building $p"
  dir="products/$p/ui"
  (cd "$dir" && pnpm exec tsc -p tsconfig.json --noEmit)
  (cd "$dir" && pnpm exec vite build --logLevel warn --emptyOutDir --outDir "$OUT_ABS/$p")
done

echo "== checking the builds"
fail=0
for f in "${FORBIDDEN[@]}"; do
  if grep -rliF -- "$f" "$OUT" >/dev/null; then
    echo "FAIL: '$f' found in:"
    grep -rliF -- "$f" "$OUT" | sed 's/^/  /'
    fail=1
  fi
done

# The real key, read from .env.local without printing it.
envfile=products/heimdall/ui/.env.local
key=""
if [ -f "$envfile" ]; then
  key=$(sed -n 's/^VITE_TOMTOM_KEY=//p' "$envfile" | tail -n 1 | tr -d "\"' \r")
fi
if [ -n "$key" ]; then
  if grep -rlF -- "$key" "$OUT" >/dev/null; then
    echo "FAIL: the TomTom key from $envfile is in:"
    grep -rlF -- "$key" "$OUT" | sed 's/^/  /'
    fail=1
  fi
else
  echo "note: no TomTom key in $envfile to check for"
fi
unset key

echo "== other URLs in the builds (for review)"
grep -rhoE 'https?://[A-Za-z0-9._~:/?#@!$&+,;=%-]+' "$OUT" | sort -u | sed 's/^/  /' || true

echo "== sizes"
for p in "${PRODUCTS[@]}"; do
  echo "$p: $(du -sh "$OUT/$p" | cut -f1)"
done
echo "total: $(du -sh "$OUT" | cut -f1)"

if [ "$fail" -ne 0 ]; then
  echo "checks FAILED"
  exit 1
fi
echo "checks passed"
