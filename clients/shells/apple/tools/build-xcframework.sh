#!/usr/bin/env bash
# Build MidgardCoreFFI.xcframework and the Swift bindings from clients/ffi.
# [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/build-xcframework.sh [--delete-target] [--out <dir>]
#
# Slices: macOS (arm64 + x86_64, lipo'd), iOS device (arm64), iOS simulator (arm64),
# for macOS 14 and iOS 17. Cargo builds offline and locked into
# ~/Library/Caches/midgard-build/target-clients (kept for the next run, deleted on
# exit with --delete-target). The XCFramework and the generated bindings go to
# ~/Library/Caches/midgard-build/clients-xcframework: built in clients-xcframework.new,
# checked there (verify-xcframework.sh), then swapped in and linked into the Swift
# package (both paths are in clients/shells/apple/.gitignore). The package's links are
# removed first, so a failed run never leaves an unchecked framework linked. Set
# MIDGARD_BUILD_CACHE (an absolute path) to use another cache root.
#
# --out <dir> (absolute, must not exist; used by ensure-xcframework.sh): build in
# <dir>.new, check it there, rename it to <dir>, and link nothing into the package (the
# caller links it). clients-xcframework is then neither read nor written.
#
# Local paths are remapped (the home directory to /home/build, the repository to
# /src), so the libraries carry no path of this machine.
#
# The generated midgard_client_ffi.swift, midgard_client_ffiFFI.h and module.modulemap
# are MPL-2.0 files written by UniFFI 0.28.3: they are copied byte for byte, never
# edited (supply-chain/licences.toml).
set -euo pipefail
cd "$(dirname "$0")/../../../.."
root="$(pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
case "$cache" in
    /*) ;;
    *)
        echo "build-xcframework: MIDGARD_BUILD_CACHE must be an absolute path" >&2
        exit 2
        ;;
esac
target_dir="$cache/target-clients"
out="$cache/clients-xcframework"
new="$cache/clients-xcframework.new"
apple=clients/shells/apple
delete_target=0
dest=""
usage() {
    echo "usage: build-xcframework.sh [--delete-target] [--out <dir>]" >&2
    exit 2
}
while [ $# -gt 0 ]; do
    case "$1" in
        --delete-target) delete_target=1 ;;
        --out)
            [ $# -ge 2 ] || usage
            dest="$2"
            shift
            ;;
        *) usage ;;
    esac
    shift
done
if [ -n "$dest" ]; then
    case "$dest" in
        /*) ;;
        *)
            echo "build-xcframework: --out must be an absolute path" >&2
            exit 2
            ;;
    esac
    if [ -e "$dest" ]; then
        echo "build-xcframework: --out $dest already exists" >&2
        exit 2
    fi
    out="$dest"
    new="$dest.new"
fi
cleanup() {
    if [ "$delete_target" = 1 ]; then rm -rf "$target_dir"; fi
    rm -rf "$new"
}
trap cleanup EXIT

export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export CARGO_TARGET_DIR="$target_dir"
export MACOSX_DEPLOYMENT_TARGET=14.0 IPHONEOS_DEPLOYMENT_TARGET=17.0
# rustc applies the last matching prefix: the repository (inside $HOME) comes last.
CARGO_ENCODED_RUSTFLAGS="$(printf '%s\037%s' "--remap-path-prefix=$HOME=/home/build" "--remap-path-prefix=$root=/src")"
export CARGO_ENCODED_RUSTFLAGS
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

[ -n "$dest" ] || rm -rf "$apple/Frameworks/MidgardCoreFFI.xcframework" "$apple/Sources/MidgardCore/midgard_client_ffi.swift"
for t in $targets; do
    echo "build-xcframework: building $t"
    cargo build --offline --locked --release -p midgard-client-ffi --lib --target "$t"
done

rm -rf "$new"
mkdir -p "$new/staging/macos" "$new/staging/headers" "$new/swift"
lipo -create "$target_dir/aarch64-apple-darwin/release/$lib" "$target_dir/x86_64-apple-darwin/release/$lib" \
    -output "$new/staging/macos/$lib"

# The bindings are the same for every slice (one crate, one interface); generate them
# once, from the macOS arm64 library.
cargo run --offline --locked -q -p midgard-client-ffi --example uniffi-bindgen -- \
    --library "$target_dir/aarch64-apple-darwin/release/$lib" --out "$new/swift"
for g in midgard_client_ffi.swift midgard_client_ffiFFI.h module.modulemap; do
    [ -s "$new/swift/$g" ] || {
        echo "build-xcframework: uniffi-bindgen did not write $g" >&2
        exit 1
    }
done
cp "$new/swift/midgard_client_ffiFFI.h" "$new/swift/module.modulemap" "$new/staging/headers/"

xcodebuild -create-xcframework \
    -library "$new/staging/macos/$lib" -headers "$new/staging/headers" \
    -library "$target_dir/aarch64-apple-ios/release/$lib" -headers "$new/staging/headers" \
    -library "$target_dir/aarch64-apple-ios-sim/release/$lib" -headers "$new/staging/headers" \
    -output "$new/MidgardCoreFFI.xcframework" >/dev/null

rm -rf "$new/staging"
bash clients/shells/apple/tools/verify-xcframework.sh "$new"
if [ -n "$dest" ]; then
    mv "$new" "$out"
    echo "build-xcframework: $out/MidgardCoreFFI.xcframework ($(du -sh "$out/MidgardCoreFFI.xcframework" | cut -f1))"
    exit 0
fi
rm -rf "$out"
mv "$new" "$out"

# Link into the Swift package (both paths are in clients/shells/apple/.gitignore).
mkdir -p "$apple/Frameworks" "$apple/Sources/MidgardCore"
ln -sfn "$out/MidgardCoreFFI.xcframework" "$apple/Frameworks/MidgardCoreFFI.xcframework"
cp "$out/swift/midgard_client_ffi.swift" "$apple/Sources/MidgardCore/midgard_client_ffi.swift"
echo "build-xcframework: $out/MidgardCoreFFI.xcframework ($(du -sh "$out/MidgardCoreFFI.xcframework" | cut -f1))"
