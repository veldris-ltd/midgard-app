#!/usr/bin/env bash
# The console toolchain pnpm-lock.yaml resolves is on a supported line, with the
# build-time advisories under it lifted. [AC-69.1] [AC-69.2] [NFR-OSS-1]
# (VLD-ANB-MIDGARD-0047, 0095 to 0100)
#
#   bash tools/ci/tests/js-toolchain.test.sh [LOCKFILE]
#
# The claim under test: every version of vite and @vitejs/plugin-react the lockfile
# resolves is at or above its floor, and so is every resolved version of the six
# transitive packages that carried the advisories. A lockfile that resolves no vite
# or no plugin-react at all fails too: that is a parse that found nothing, not a
# toolchain that passed.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
lock="${1:-$here/../../../pnpm-lock.yaml}"
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

[ -f "$lock" ] || { echo "js-toolchain: no lockfile at $lock" >&2; exit 1; }

# package floor required-to-be-present
FLOORS="
vite 7.3.7 yes
@vitejs/plugin-react 5.1.0 yes
esbuild 0.25.0 no
postcss 8.5.23 no
nanoid 3.3.18 no
source-map-js 1.2.2 no
browserslist 4.28.7 no
baseline-browser-mapping 2.11.0 no
"

# Every version of NAME the lockfile resolves, one per line: the keys of its
# `packages:` and `snapshots:` entries, `  name@1.2.3:` or `  'name@1.2.3(peer)':`.
resolved() {
    tr -d '\r' < "$lock" \
        | awk -v name="$1" '
            /^[^ ]/ { in_keys = ($0 == "packages:" || $0 == "snapshots:"); next }
            in_keys && /^  [^ ]/ {
                key = substr($0, 3); gsub(/\x27/, "", key)
                if (index(key, name "@") != 1) next
                v = substr(key, length(name) + 2)
                sub(/[(:].*$/, "", v)
                print v
            }' \
        | sort -u
}

# at_least A B: A >= B as version numbers.
at_least() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]; }

while read -r name floor required; do
    [ -n "$name" ] || continue
    versions="$(resolved "$name")"
    if [ -z "$versions" ]; then
        if [ "$required" = yes ]; then
            fail "$name: the lockfile resolves no version at all"
        else
            pass "$name: not in the lockfile"
        fi
        continue
    fi
    low=""
    for v in $versions; do
        at_least "$v" "$floor" || low="$low $v"
    done
    if [ -n "$low" ]; then
        fail "$name: resolves$low, below the floor $floor"
    else
        pass "$name: $(echo $versions) >= $floor"
    fi
done <<<"$FLOORS"

if [ "$failures" -ne 0 ]; then
    echo "js-toolchain: $failures check(s) failed" >&2
    exit 1
fi
echo "js-toolchain: OK"
