#!/usr/bin/env bash
# Check the output of build-xcframework.sh before anything links it. [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/verify-xcframework.sh [<out-dir>]
#
# <out-dir> defaults to ~/Library/Caches/midgard-build/clients-xcframework. Checks:
#   1. the XCFramework lists exactly ios-arm64, ios-arm64-simulator and
#      macos-arm64_x86_64, and each library holds exactly the expected architectures;
#   2. every object in each slice is built for that slice's platform (macOS 1, iOS 2,
#      iOS simulator 7), none needs a newer OS than macOS 14 / iOS 17, and the
#      midgard_client_ffi objects are built for exactly macOS 14 / iOS 17;
#   3. every slice carries the generated header and module map unchanged, and the
#      module map is a plain (not a framework) module;
#   4. the generated Swift typechecks against each slice's headers, and on an arm64
#      Mac a small program linked against the macOS slice runs and calls the core
#      (UniFFI's contract-version and checksum checks run on that first call).
# Scratch files go to <out-dir>/verify and are deleted on exit.
set -euo pipefail
out="${1:-${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}/clients-xcframework}"
xcf="$out/MidgardCoreFFI.xcframework"
swift_src="$out/swift/midgard_client_ffi.swift"
lib=libmidgard_client_ffi.a
scratch="$out/verify"
failures=0
ok() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1" >&2
    failures=$((failures + 1))
}
[ -d "$xcf" ] || {
    echo "verify-xcframework: no $xcf (run build-xcframework.sh)" >&2
    exit 2
}
rm -rf "$scratch"
mkdir -p "$scratch"
trap 'rm -rf "$scratch"' EXIT

# 1. Slices and architectures.
plist="$xcf/Info.plist"
ids=""
i=0
while id="$(/usr/libexec/PlistBuddy -c "Print :AvailableLibraries:$i:LibraryIdentifier" "$plist" 2>/dev/null)"; do
    ids="$ids $id"
    i=$((i + 1))
done
ids="$(printf '%s\n' $ids | sort | tr '\n' ' ')"
if [ "$ids" = "ios-arm64 ios-arm64-simulator macos-arm64_x86_64 " ]; then
    ok "slices: $ids"
else
    fail "slices: expected ios-arm64 ios-arm64-simulator macos-arm64_x86_64, got $ids"
fi

# slice | architectures | LC_BUILD_VERSION platform | deployment target | swiftc target | SDK
slices="ios-arm64|arm64|2|17.0|arm64-apple-ios17.0|iphoneos
ios-arm64-simulator|arm64|7|17.0|arm64-apple-ios17.0-simulator|iphonesimulator
macos-arm64_x86_64|arm64 x86_64|1|14.0|arm64-apple-macos14.0|macosx"

# Prints "<platform> <minos> <member>" for every LC_BUILD_VERSION in one architecture.
build_versions() {
    otool -l -arch "$2" "$1" | awk '
        /\.a\(.*\):$/ { m = $0; sub(/.*\(/, "", m); sub(/\):$/, "", m) }
        $1 == "cmd" { bv = ($2 == "LC_BUILD_VERSION") }
        bv && $1 == "platform" { p = $2 }
        bv && $1 == "minos" { print p, $2, m; bv = 0 }'
}

while IFS='|' read -r slice archs platform minos target sdk; do
    a="$xcf/$slice/$lib"
    [ -f "$a" ] || {
        fail "$slice: no $lib"
        continue
    }
    got="$(lipo -archs "$a" | tr ' ' '\n' | sort | tr '\n' ' ')"
    if [ "$got" = "$archs " ]; then ok "$slice: architectures $archs"; else fail "$slice: architectures '$got', expected '$archs'"; fi

    # 2. Platform and deployment target, per architecture.
    for arch in $archs; do
        build_versions "$a" "$arch" >"$scratch/bv"
        bad="$(awk -v p="$platform" -v want="$minos" '
            function v(s,  x) { split(s, x, "."); return x[1] * 10000 + x[2] * 100 + x[3] }
            ($1 != p || v($2) > v(want)) && n++ < 3 { print }' "$scratch/bv")"
        if [ -s "$scratch/bv" ] && [ -z "$bad" ]; then
            ok "$slice/$arch: platform $platform, nothing newer than $minos"
        else
            fail "$slice/$arch: wrong platform or newer deployment target: ${bad:-no build versions}"
        fi
        own="$(awk '$3 ~ /^midgard_client_ffi-/ { print $2 }' "$scratch/bv" | sort -u)"
        if [ "$own" = "$minos" ]; then
            ok "$slice/$arch: midgard_client_ffi built for $minos"
        else
            fail "$slice/$arch: midgard_client_ffi built for '${own:-nothing}', expected $minos (stale target directory? run clean.sh --target)"
        fi
    done

    # 3. Generated header and module map, unchanged.
    for g in midgard_client_ffiFFI.h module.modulemap; do
        if cmp -s "$out/swift/$g" "$xcf/$slice/Headers/$g"; then ok "$slice: $g as generated"; else fail "$slice: $g differs from the generated file"; fi
    done
    if grep -q '^framework module' "$xcf/$slice/Headers/module.modulemap"; then
        fail "$slice: module map is a framework module (Clang cannot resolve its header in a library slice)"
    fi

    # 4a. The bindings typecheck against this slice's headers.
    if xcrun --sdk "$sdk" swiftc -typecheck -swift-version 5 -target "$target" \
        -I "$xcf/$slice/Headers" "$swift_src" >"$scratch/swiftc.log" 2>&1; then
        ok "$slice: bindings typecheck ($target)"
    else
        fail "$slice: bindings do not typecheck ($target)"
        head -20 "$scratch/swiftc.log" >&2
    fi
done <<EOF
$slices
EOF

# 4b. Link against the macOS slice and call the core once.
if [ "$(uname -m)" = arm64 ]; then
    printf '%s\n' 'let v = coreVersion()' 'precondition(!v.isEmpty)' 'print("core \(v)")' >"$scratch/main.swift"
    mac="$xcf/macos-arm64_x86_64"
    if xcrun --sdk macosx swiftc -swift-version 5 -target arm64-apple-macos14.0 -I "$mac/Headers" \
        "$swift_src" "$scratch/main.swift" -L "$mac" -lmidgard_client_ffi -o "$scratch/smoke" >"$scratch/link.log" 2>&1 &&
        "$scratch/smoke" >"$scratch/run.log" 2>&1; then
        ok "macos-arm64_x86_64: links and runs ($(cat "$scratch/run.log"))"
    else
        fail "macos-arm64_x86_64: the smoke program does not link or run"
        head -20 "$scratch/link.log" >&2
        [ ! -f "$scratch/run.log" ] || head -20 "$scratch/run.log" >&2
    fi
else
    echo "skip  smoke run: not an arm64 Mac"
fi

[ "$failures" -eq 0 ] || {
    echo "verify-xcframework: $failures check(s) failed" >&2
    exit 1
}
echo "verify-xcframework: $xcf is complete"
