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
for s in $f $c $v; do
    check "$s: no bash-4 builtins" "[ -f $s ] && ! grep -Eq 'mapfile|readarray|declare -A|\\\$\\{[A-Za-z_]+,,\\}' $s"
    check "$s: no in-place sed" "[ -f $s ] && ! grep -Eq 'sed -i( |\$)' $s"
    check "$s: parses under /bin/bash" "[ -f $s ] && /bin/bash -n $s"
done
check "build outputs are gitignored" "[ -f $g ] && grep -qx '/Frameworks/' $g && grep -qx '/Sources/MidgardCore/midgard_client_ffi.swift' $g"
check "clean.sh only deletes clients-* and package outputs" \
    "[ -f $c ] && ! grep -E '^[^#]*rm -rf' $c | grep -Ev 'clients-|Frameworks|midgard_client_ffi.swift|\\.build|target-clients' | grep -q ."
[ "$failures" -eq 0 ] || exit 1
