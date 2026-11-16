#!/usr/bin/env bash
# The XCFramework script builds the four Apple slices offline and locked, writes only
# under ~/Library/Caches/midgard-build/clients-* (and the cargo target directory), and
# stays bash 3.2 safe. Static checks only: the real build needs a Mac with Xcode
# (clients/shells/apple/tools/verify-xcframework.sh checks its output). [DD-6]
#
#   bash tools/ci/tests/apple-xcframework.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
f=clients/shells/apple/tools/build-xcframework.sh
c=clients/shells/apple/tools/clean.sh
v=clients/shells/apple/tools/verify-xcframework.sh
g=clients/shells/apple/.gitignore
k=clients/shells/apple/tools/check-contract.sh
y=clients/shells/apple/tools/typecheck-ios.sh
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "script exists" "[ -f $f ]"
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim; do
    check "builds $t" "grep -q '$t' $f"
done
check "cargo is offline and locked" "! grep -E '^[^#]*cargo (build|run)' $f | grep -vq -- '--offline --locked'"
check "builds only the ffi library" "grep -E '^[^#]*cargo build' $f | grep -q -- '-p midgard-client-ffi --lib'"
check "outputs under clients-*" "grep -q 'midgard-build/clients-' $f"
check "stops on the first failing step" "grep -qx 'set -euo pipefail' $f"
check "sets the deployment targets" "grep -q 'MACOSX_DEPLOYMENT_TARGET=14.0' $f && grep -q 'IPHONEOS_DEPLOYMENT_TARGET=17.0' $f"
check "verifies its output" "grep -q 'verify-xcframework.sh' $f"
for s in $f $c $v $k $y; do
    check "$s: no bash-4 builtins" "[ -f $s ] && ! grep -Eq 'mapfile|readarray|declare -A|\\\$\\{[A-Za-z_]+,,\\}' $s"
    check "$s: no in-place sed" "[ -f $s ] && ! grep -Eq 'sed -i( |\$)' $s"
    check "$s: parses under /bin/bash" "[ -f $s ] && /bin/bash -n $s"
done
check "build outputs are gitignored" "[ -f $g ] && grep -qx '/Frameworks/' $g && grep -qx '/Sources/MidgardCore/midgard_client_ffi.swift' $g"
check "strips local paths from the libraries" "grep -q -- '--remap-path-prefix=\$HOME=' $f && grep -q -- '--remap-path-prefix=\$root=' $f"
check "builds into clients-xcframework.new and swaps it in after verifying" "grep -q 'clients-xcframework.new' $f"
for s in $f $c; do
    check "$s: refuses a relative MIDGARD_BUILD_CACHE" \
        "[ -f $s ] && (MIDGARD_BUILD_CACHE=relative/cache bash $s --no-such-flag >/dev/null 2>&1; [ \$? -eq 2 ]) && grep -q 'must be an absolute path' $s"
done

# clean.sh deletes the build outputs and nothing else. It runs from a copy in a scratch
# package, so the real package's linked framework is untouched.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/cache/clients-xcframework/x" "$tmp/cache/clients-derived" "$tmp/cache/clients-swift/x" "$tmp/cache/target-clients/release" \
    "$tmp/pkg/tools" "$tmp/pkg/Frameworks" "$tmp/pkg/Sources/MidgardCore" "$tmp/pkg/.build" "$tmp/pkg/.swiftpm"
echo keep >"$tmp/cache/sentinel"
mkdir -p "$tmp/cache/other-build"
echo keep >"$tmp/pkg/Sources/MidgardCore/Keep.swift"
echo gen >"$tmp/pkg/Sources/MidgardCore/midgard_client_ffi.swift"
[ -f $c ] && cp $c "$tmp/pkg/tools/clean.sh"
check "clean.sh --target runs" "[ -f $c ] && MIDGARD_BUILD_CACHE=$tmp/cache bash $tmp/pkg/tools/clean.sh --target >/dev/null"
check "clean.sh keeps what is not a build output" \
    "[ -f $tmp/cache/sentinel ] && [ -d $tmp/cache/other-build ] && [ -f $tmp/pkg/Sources/MidgardCore/Keep.swift ] && [ -f $tmp/pkg/tools/clean.sh ]"
check "clean.sh removes the build outputs" \
    "[ ! -e $tmp/cache/clients-xcframework ] && [ ! -e $tmp/cache/clients-derived ] && [ ! -e $tmp/cache/clients-swift ] && [ ! -e $tmp/cache/target-clients ] && [ ! -e $tmp/pkg/Frameworks ] && [ ! -e $tmp/pkg/Sources/MidgardCore/midgard_client_ffi.swift ] && [ ! -e $tmp/pkg/.build ] && [ ! -e $tmp/pkg/.swiftpm ]"
[ "$failures" -eq 0 ] || exit 1
