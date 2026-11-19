#!/usr/bin/env bash
# Delete every local Apple-client build output. [DD-6]
#
#   bash clients/shells/apple/tools/clean.sh [--target]
#
# Removes ~/Library/Caches/midgard-build/clients-* outputs (including clients-swift,
# the `swift build --scratch-path` directory, and ensure-xcframework.sh's
# clients-xcframework-by-stamp builds and lock) and what
# build-xcframework.sh linked into the Swift package; --target also removes the cargo
# target directory (~/Library/Caches/midgard-build/target-clients).
set -euo pipefail
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
case "$cache" in
    /*) ;;
    *)
        echo "clean: MIDGARD_BUILD_CACHE must be an absolute path" >&2
        exit 2
        ;;
esac
case "${1:-}" in
    '') ;;
    --target) rm -rf "$cache/target-clients" ;;
    *)
        echo "usage: clean.sh [--target]" >&2
        exit 2
        ;;
esac
# ensure-xcframework.sh's lock (pid and start time of its holder and of the build): while
# either runs, the lock and the by-stamp builds are left alone.
alive() {
    local pid start
    [ -f "$1" ] || return 1
    pid="$(sed -n 1p "$1")"
    start="$(sed -n 2p "$1")"
    case "$pid" in '' | *[!0-9]*) return 1 ;; esac
    [ -n "$start" ] && [ "$(TZ=UTC0 LC_ALL=C ps -o lstart= -p "$pid" 2>/dev/null | sed -e 's/^ *//' -e 's/ *$//')" = "$start" ]
}
lock="$cache/clients-xcframework.lock"
if alive "$lock/owner" || alive "$lock/child"; then
    echo "clean: a framework build is running (lock $lock, pid $(head -n 1 "$lock/owner" 2>/dev/null || echo unknown)): left it and clients-xcframework-by-stamp" >&2
    kept=1
else
    rm -rf "$lock" "$cache/clients-xcframework-by-stamp"
    kept=0
fi
rm -rf "$cache/clients-xcframework" "$cache/clients-xcframework.new" "$cache/clients-derived" "$cache/clients-archive" \
    "$cache/clients-swift" "$cache"/clients-xcframework.lock.stale.*
cd "$(dirname "$0")/.."
rm -rf Frameworks Sources/MidgardCore/midgard_client_ffi.swift .build .swiftpm
if [ "$kept" = 1 ]; then
    echo "clean: done, except the running build's lock and builds"
else
    echo "clean: done"
fi
