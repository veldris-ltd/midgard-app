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
rm -rf "$cache/clients-xcframework" "$cache/clients-xcframework.new" "$cache/clients-derived" "$cache/clients-archive" \
    "$cache/clients-swift" "$cache/clients-xcframework-by-stamp" "$cache/clients-xcframework.lock" \
    "$cache"/clients-xcframework.lock.stale.*
cd "$(dirname "$0")/.."
rm -rf Frameworks Sources/MidgardCore/midgard_client_ffi.swift .build .swiftpm
echo "clean: done"
