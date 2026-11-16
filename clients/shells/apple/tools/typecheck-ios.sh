#!/usr/bin/env bash
# Typecheck MidgardKit and MidgardAuditKit (against the platform's XCTest) for iOS
# (device and simulator) with swiftc against the iOS SDK,
# without the iOS platform component or a simulator runtime: no build, no link, no
# run. Use it where `xcodebuild -destination 'generic/platform=iOS…'` reports the
# platform as not installed. [DD-6]
#
#   bash clients/shells/apple/tools/typecheck-ios.sh
#
# Needs the XCFramework and bindings from build-xcframework.sh. Scratch output goes to
# ~/Library/Caches/midgard-build/clients-typecheck and is deleted on exit.
set -euo pipefail
cd "$(dirname "$0")/.."
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
case "$cache" in
    /*) ;;
    *)
        echo "typecheck-ios: MIDGARD_BUILD_CACHE must be an absolute path" >&2
        exit 2
        ;;
esac
xcf=Frameworks/MidgardCoreFFI.xcframework
gen=Sources/MidgardCore/midgard_client_ffi.swift
[ -d "$xcf" ] && [ -f "$gen" ] || {
    echo "typecheck-ios: run tools/build-xcframework.sh first" >&2
    exit 2
}
scratch="$cache/clients-typecheck"
rm -rf "$scratch"
mkdir -p "$scratch"
trap 'rm -rf "$scratch"' EXIT
# SwiftPM generates Bundle.module for a target with resources; supply it here.
cat >"$scratch/BundleModule.swift" <<'EOF'
import Foundation
extension Bundle { static var module: Bundle { .main } }
EOF
kit_sources="$(find Sources/MidgardKit -name '*.swift' | sort)"
audit_sources="$(find Sources/MidgardAuditKit -name '*.swift' | sort)"
failures=0
for pair in "iphoneos:arm64-apple-ios17.0:ios-arm64" "iphonesimulator:arm64-apple-ios17.0-simulator:ios-arm64-simulator"; do
    sdk="${pair%%:*}"
    rest="${pair#*:}"
    target="${rest%%:*}"
    slice="${rest#*:}"
    out="$scratch/$sdk"
    mkdir -p "$out"
    sdk_path="$(xcrun --sdk "$sdk" --show-sdk-path)"
    platform="$(xcrun --sdk "$sdk" --show-sdk-platform-path)"
    common="-sdk $sdk_path -target $target -swift-version 5 -module-cache-path $out/mc"
    # shellcheck disable=SC2086
    if xcrun --sdk "$sdk" swiftc $common -I "$xcf/$slice/Headers" -module-name MidgardCore -parse-as-library \
        -emit-module -emit-module-path "$out/MidgardCore.swiftmodule" "$gen" \
        && xcrun --sdk "$sdk" swiftc $common -I "$xcf/$slice/Headers" -I "$out" -module-name MidgardKit \
            -parse-as-library -typecheck $kit_sources "$scratch/BundleModule.swift" \
        && xcrun --sdk "$sdk" swiftc $common -F "$platform/Developer/Library/Frameworks" \
            -I "$platform/Developer/usr/lib" -module-name MidgardAuditKit -parse-as-library -typecheck $audit_sources; then
        echo "typecheck-ios: ok    $target"
    else
        echo "typecheck-ios: FAIL  $target"
        failures=$((failures + 1))
    fi
done
[ "$failures" -eq 0 ]
