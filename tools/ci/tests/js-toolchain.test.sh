#!/usr/bin/env bash
# The console toolchain pnpm-lock.yaml resolves is on a supported line, with the
# build-time advisories under it lifted. [AC-69.1] [AC-69.2] [NFR-OSS-1]
# (VLD-ANB-MIDGARD-0047, 0095 to 0100)
# The consoles route with react-router 7 and nothing else, and the launch-media
# tooling resolves sharp and image-size above their advisories. [AC-P4.1] [AC-P4.2]
# (VLD-ANB-MIDGARD-0050, 0062, 0082, 0083)
#
#   bash tools/ci/tests/js-toolchain.test.sh [LOCKFILE [LAUNCH-LOCKFILE]]
#
# The claim under test: every version of vite and @vitejs/plugin-react the lockfile
# resolves is at or above its floor, and so is every resolved version of the six
# transitive packages that carried the advisories, and of react-router. A lockfile
# that resolves no vite, no plugin-react or no react-router at all fails too: that is
# a parse that found nothing, not a toolchain that passed. react-router-dom, the v6
# package the consoles used to import, must be gone.
#
# docs/launch/pipeline is outside the pnpm workspace and has an npm lockfile of its
# own; its sharp and image-size are held to their floors the same way.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
lock="${1:-$here/../../../pnpm-lock.yaml}"
launch_lock="${2:-$here/../../../docs/launch/pipeline/package-lock.json}"
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
react-router 7.18.0 yes
"

# The same for docs/launch/pipeline/package-lock.json.
LAUNCH_FLOORS="
sharp 0.35.5 yes
image-size 2.0.3 yes
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

# Every version of NAME an npm lockfile (lockfileVersion 2 or 3) resolves: the
# "version" of each `"node_modules/NAME": {` entry, nested ones included.
npm_resolved() {
    tr -d '\r' < "$launch_lock" \
        | awk -v name="$1" '
            match($0, /^ *"([^"]*\/)?node_modules\/[^"]*": *\{/) {
                key = $0; sub(/^ *"/, "", key); sub(/".*$/, "", key)
                n = key; sub(/^.*node_modules\//, "", n)
                want = (n == name)
                next
            }
            want && /^ *"version": *"/ {
                v = $0; sub(/^ *"version": *"/, "", v); sub(/".*$/, "", v)
                print v; want = 0
            }
            /^ *\}/ { want = 0 }' \
        | sort -u
}

# at_least A B: A >= B as version numbers.
at_least() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]; }

# check FLOORS RESOLVER LABEL
check() {
    while read -r name floor required; do
        [ -n "$name" ] || continue
        versions="$("$2" "$name")"
        name="$3$name"
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
    done <<<"$1"
}

check "$FLOORS" resolved ""

# The v6 router package: the v7 line is one package, and nothing may still pull in
# the old one.
if [ -n "$(resolved react-router-dom)" ]; then
    fail "react-router-dom: the lockfile still resolves $(echo $(resolved react-router-dom))"
else
    pass "react-router-dom: not in the lockfile"
fi

if [ -f "$launch_lock" ]; then
    check "$LAUNCH_FLOORS" npm_resolved "launch pipeline: "
else
    fail "launch pipeline: no lockfile at $launch_lock"
fi

if [ "$failures" -ne 0 ]; then
    echo "js-toolchain: $failures check(s) failed" >&2
    exit 1
fi
echo "js-toolchain: OK"
