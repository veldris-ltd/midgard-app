#!/usr/bin/env bash
# Build the four consoles' static sites for the Mac app into dist/mac/consoles/<product>.
# Offline builds (VITE_OFFLINE=1): no internet features, and VITE_TOMTOM_KEY is set empty
# in the environment so it overrides products/heimdall/ui/.env.local and no key is
# inlined. The gateway URL comes from the app at run time (window.__MIDGARD_GATEWAY__).
# Vite writes straight to dist/, so no dist/ is left inside products/*/ui. Then the built
# files are checked: the script fails if any internet endpoint the offline build must not
# reach, any secret VITE_* value from the .env files, or an inlined env object is in
# them (endpoints matched case-insensitively), and lists every other URL found for
# review (attribution links are expected).
#
#   bash tools/mac-app/build-consoles.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=dist/mac/consoles
PRODUCTS=(midgard heimdall mimir forge)
# Endpoints (hosts the overlays fetch from); attribution links to the providers are allowed.
FORBIDDEN=(api.tomtom.com api.rainviewer.com tilecache.rainviewer.com elevation-tiles-prod tiles.maps.eox.at)

if ! command -v pnpm >/dev/null 2>&1; then
  echo "pnpm not found: install it (corepack enable, or brew install pnpm) and re-run" >&2
  exit 1
fi

export VITE_OFFLINE=1
export VITE_TOMTOM_KEY=

# Start clean, so no stale console (or one dropped from PRODUCTS) is left behind.
rm -rf "$OUT"
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

# grep -l over the builds: 0 found, 1 not found, 2+ an error (which stops the script).
found_in() { # found_in <grep -rl flags...>; patterns on stdin
  local rc=0
  grep -rl "$@" "$OUT" || rc=$?
  if [ "$rc" -ge 2 ]; then echo "FAIL: grep error ($rc)" >&2; exit 2; fi
}

for f in "${FORBIDDEN[@]}"; do
  hits=$(printf '%s\n' "$f" | found_in -iF -f -)
  if [ -n "$hits" ]; then
    echo "FAIL: '$f' found in:"
    printf '%s\n' "$hits" | sed 's/^/  /'
    fail=1
  fi
done

# Vite inlines the whole env object when code reads import.meta.env indirectly; the
# consoles read single values (import.meta.env.VITE_...), so no VITE_ name should remain.
hits=$(printf 'VITE_\n' | found_in -F -f -)
if [ -n "$hits" ]; then
  echo "FAIL: a VITE_ name (an inlined env object?) is in:"
  printf '%s\n' "$hits" | sed 's/^/  /'
  fail=1
fi

# Secrets: every non-empty VITE_* value in the consoles' and the repo's .env files must
# be absent from the builds, bar the allowlisted (non-secret) settings. Values are
# passed to grep on stdin and never printed; only file and variable names are.
ALLOW=(VITE_GATEWAY_URL VITE_GATEWAY VITE_MIDGARD_CONSOLE_URL VITE_OFFLINE)
shopt -s nullglob
ENVFILES=(.env* products/*/ui/.env*)
shopt -u nullglob
# One "name<TAB>value" line per assignment: optional `export`, spaces around `=`, single
# or double quotes, ` # comment` after an unquoted value, CRLF.
parse_env() {
  awk '{
    sub(/\r$/, ""); line = $0
    sub(/^[ \t]+/, "", line); sub(/^export[ \t]+/, "", line)
    if (!match(line, /^VITE_[A-Za-z0-9_]+[ \t]*=/)) next
    eq = index(line, "="); name = substr(line, 1, eq - 1); sub(/[ \t]+$/, "", name)
    v = substr(line, eq + 1); sub(/^[ \t]+/, "", v)
    q = substr(v, 1, 1)
    if (q == "\"" || q == "'"'"'") {
      rest = substr(v, 2); e = index(rest, q)
      v = (e > 0) ? substr(rest, 1, e - 1) : rest
    } else {
      sub(/[ \t]+#.*$/, "", v); sub(/[ \t]+$/, "", v)
    }
    printf "%s\t%s\n", name, v
  }' "$1"
}
checked=0
for ef in ${ENVFILES[@]+"${ENVFILES[@]}"}; do
  [ -f "$ef" ] || continue
  tomtom_value=0
  while IFS=$'\t' read -r name val; do
    [ -n "$val" ] || continue
    case " ${ALLOW[*]} " in *" $name "*) continue ;; esac
    case "$name" in *TOMTOM*) tomtom_value=1 ;; esac
    checked=$((checked + 1))
    hits=$(printf '%s\n' "$val" | found_in -F -f -)
    if [ -n "$hits" ]; then
      echo "FAIL: the value of $name from $ef is in:"
      printf '%s\n' "$hits" | sed 's/^/  /'
      fail=1
    fi
  done < <(parse_env "$ef")
  if grep -qi tomtom "$ef" && [ "$tomtom_value" -eq 0 ]; then
    echo "FAIL: $ef mentions TOMTOM but no value could be read from it to check for"
    fail=1
  fi
done
unset name val
echo "secret values checked: $checked (from ${#ENVFILES[@]} .env file(s))"

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
