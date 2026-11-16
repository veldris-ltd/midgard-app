#!/usr/bin/env bash
# Build MidgardCoreFFI.xcframework and the Swift bindings from clients/ffi.
# [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/build-xcframework.sh [--delete-target]
#
# Slices: macOS (arm64 + x86_64, lipo'd), iOS device (arm64), iOS simulator (arm64),
# for macOS 14 and iOS 17. Cargo builds offline and locked into
# ~/Library/Caches/midgard-build/target-clients (kept for the next run, deleted on
# exit with --delete-target). The XCFramework and the generated bindings go to
# ~/Library/Caches/midgard-build/clients-xcframework; the script then checks them
# (verify-xcframework.sh) and links them into the Swift package (both paths are in
# clients/shells/apple/.gitignore). Set MIDGARD_BUILD_CACHE to use another cache root.
#
# The generated midgard_client_ffi.swift, midgard_client_ffiFFI.h and module.modulemap
# are MPL-2.0 files written by UniFFI 0.28.3: they are copied byte for byte, never
# edited (supply-chain/licences.toml).
set -euo pipefail
cd "$(dirname "$0")/../../../.."
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
target_dir="$cache/target-clients"
out="$cache/clients-xcframework"
delete_target=0
case "${1:-}" in
    '') ;;
    --delete-target) delete_target=1 ;;
    *)
        echo "usage: build-xcframework.sh [--delete-target]" >&2
        exit 2
        ;;
esac
cleanup() {
    if [ "$delete_target" = 1 ]; then rm -rf "$target_dir"; fi
    rm -rf "$out/staging"
}
trap cleanup EXIT

export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export CARGO_TARGET_DIR="$target_dir"
export MACOSX_DEPLOYMENT_TARGET=14.0 IPHONEOS_DEPLOYMENT_TARGET=17.0
lib=libmidgard_client_ffi.a
targets="aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim"
installed="$(rustup target list --installed)"
for t in $targets; do
    if ! printf '%s\n' "$installed" | grep -qx "$t"; then
        echo "build-xcframework: missing target $t (rustup target add $t --toolchain 1.82.0)" >&2
        exit 2
    fi
done
for tool in lipo xcodebuild xcrun; do
    command -v "$tool" >/dev/null || {
        echo "build-xcframework: $tool not found (needs Xcode)" >&2
        exit 2
    }
done

for t in $targets; do
    echo "build-xcframework: building $t"
    cargo build --offline --locked --release -p midgard-client-ffi --lib --target "$t"
done

rm -rf "$out"
mkdir -p "$out/staging/macos" "$out/staging/headers" "$out/swift"
lipo -create "$target_dir/aarch64-apple-darwin/release/$lib" "$target_dir/x86_64-apple-darwin/release/$lib" \
    -output "$out/staging/macos/$lib"

# The bindings are the same for every slice (one crate, one interface); generate them
# once, from the macOS arm64 library.
cargo run --offline --locked -q -p midgard-client-ffi --example uniffi-bindgen -- \
    --library "$target_dir/aarch64-apple-darwin/release/$lib" --out "$out/swift"
for g in midgard_client_ffi.swift midgard_client_ffiFFI.h module.modulemap; do
    [ -s "$out/swift/$g" ] || {
        echo "build-xcframework: uniffi-bindgen did not write $g" >&2
        exit 1
    }
done
cp "$out/swift/midgard_client_ffiFFI.h" "$out/swift/module.modulemap" "$out/staging/headers/"

xcodebuild -create-xcframework \
    -library "$out/staging/macos/$lib" -headers "$out/staging/headers" \
    -library "$target_dir/aarch64-apple-ios/release/$lib" -headers "$out/staging/headers" \
    -library "$target_dir/aarch64-apple-ios-sim/release/$lib" -headers "$out/staging/headers" \
    -output "$out/MidgardCoreFFI.xcframework" >/dev/null

bash clients/shells/apple/tools/verify-xcframework.sh "$out"

# Link into the Swift package (both paths are in clients/shells/apple/.gitignore).
apple=clients/shells/apple
mkdir -p "$apple/Frameworks" "$apple/Sources/MidgardCore"
ln -sfn "$out/MidgardCoreFFI.xcframework" "$apple/Frameworks/MidgardCoreFFI.xcframework"
cp "$out/swift/midgard_client_ffi.swift" "$apple/Sources/MidgardCore/midgard_client_ffi.swift"
echo "build-xcframework: $out/MidgardCoreFFI.xcframework ($(du -sh "$out/MidgardCoreFFI.xcframework" | cut -f1))"
