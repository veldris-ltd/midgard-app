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
b=clients/shells/apple/tools/check-bindings.sh
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
for s in $f $c $v $k $y $b; do
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
# check-bindings.sh (MPL-2.0 release checklist): a fresh uniffi-bindgen run equals the
# bindings built and linked, the UniFFI acknowledgement is present, and the UniFFI
# licence exceptions have not expired. Run from a scratch tree with a stub cargo.
check "check-bindings.sh exists" "[ -f $b ]"
check "check-bindings.sh generates offline and locked" "[ -f $b ] && grep -q -- '--offline --locked' $b && grep -q 'uniffi-bindgen' $b"
r="$tmp/repo"
a="$r/clients/shells/apple"
mkdir -p "$a/tools" "$a/Sources/MidgardCore" "$a/Sources/MidgardKit/Presentation" "$r/supply-chain" \
    "$tmp/bcache/clients-xcframework/swift" "$tmp/bcache/target-clients/aarch64-apple-darwin/release" "$tmp/stub"
[ -f $b ] && cp $b "$a/tools/check-bindings.sh"
cp clients/shells/apple/Sources/MidgardKit/Presentation/Notices.swift "$a/Sources/MidgardKit/Presentation/"
printf '[exceptions."uniffi"]\nlicence = "MPL-2.0"\nexpires = "2027-05-13"\n\n[exceptions."other"]\nexpires = "2020-01-01"\n' >"$r/supply-chain/licences.toml"
: >"$tmp/bcache/target-clients/aarch64-apple-darwin/release/libmidgard_client_ffi.a"
cat >"$tmp/stub/cargo" <<'STUB'
#!/usr/bin/env bash
# writes what uniffi-bindgen would: --out DIR
while [ $# -gt 0 ]; do [ "$1" = --out ] && out="$2"; shift; done
mkdir -p "$out"
echo "// swift" >"$out/midgard_client_ffi.swift"
echo "// header" >"$out/midgard_client_ffiFFI.h"
echo "module m {}" >"$out/module.modulemap"
STUB
chmod +x "$tmp/stub/cargo"
bash "$tmp/stub/cargo" --out "$tmp/bcache/clients-xcframework/swift"
cp "$tmp/bcache/clients-xcframework/swift/midgard_client_ffi.swift" "$a/Sources/MidgardCore/"
cb() { MIDGARD_BUILD_CACHE="$tmp/bcache" CARGO="$tmp/stub/cargo" MIDGARD_TODAY="${today:-2026-11-16}" bash "$a/tools/check-bindings.sh" 2>&1; }
check "check-bindings.sh passes when the bindings are UniFFI's own" "[ -f $b ] && cb >/dev/null"
echo "// edited" >>"$a/Sources/MidgardCore/midgard_client_ffi.swift"
check "check-bindings.sh fails on an edited package copy, naming it" "[ -f $b ] && ! out=\$(cb) && grep -q 'Sources/MidgardCore/midgard_client_ffi.swift' <<<\"\$out\""
cp "$tmp/bcache/clients-xcframework/swift/midgard_client_ffi.swift" "$a/Sources/MidgardCore/"
echo "// edited" >>"$tmp/bcache/clients-xcframework/swift/midgard_client_ffiFFI.h"
check "check-bindings.sh fails on an edited built header" "[ -f $b ] && ! out=\$(cb) && grep -q 'midgard_client_ffiFFI.h' <<<\"\$out\""
echo "// header" >"$tmp/bcache/clients-xcframework/swift/midgard_client_ffiFFI.h"
check "check-bindings.sh passes again once restored" "[ -f $b ] && cb >/dev/null"
today=2027-04-01
check "check-bindings.sh warns within 60 days of the exceptions' expiry" "[ -f $b ] && out=\$(cb) && grep -q 'expire on 2027-05-13' <<<\"\$out\""
today=2027-05-14
check "check-bindings.sh fails once the exceptions have expired" "[ -f $b ] && ! out=\$(cb) && grep -q 'expired on 2027-05-13' <<<\"\$out\""
today=
cp "$r/supply-chain/licences.toml" "$tmp/lic"
printf '\n[exceptions."uniffi_meta"]\nexpires = "2027-5-13"\n' >>"$r/supply-chain/licences.toml"
check "check-bindings.sh fails on an expiry that is not YYYY-MM-DD" "[ -f $b ] && ! out=\$(cb) && grep -q 'not YYYY-MM-DD: 2027-5-13' <<<\"\$out\""
mv "$tmp/lic" "$r/supply-chain/licences.toml"
check "verify-xcframework.sh looks for test-only symbols with nm" "grep -q '^[^#]*nm -A' $v && grep -q 'from_body|_for_tests' $v"
grep -v 'uniffi' "$a/Sources/MidgardKit/Presentation/Notices.swift" >"$tmp/n" && mv "$tmp/n" "$a/Sources/MidgardKit/Presentation/Notices.swift"
check "check-bindings.sh fails without the UniFFI acknowledgement" "[ -f $b ] && ! out=\$(cb) && grep -q 'acknowledgement' <<<\"\$out\""
rm -f "$tmp/bcache/target-clients/aarch64-apple-darwin/release/libmidgard_client_ffi.a"
check "check-bindings.sh without a built library says to build first" "[ -f $b ] && ! out=\$(cb) && grep -q 'build-xcframework.sh' <<<\"\$out\""
[ "$failures" -eq 0 ] || exit 1
