#!/usr/bin/env bash
# ensure-xcframework.sh (the app projects' pre-build step) builds the XCFramework only
# when the Rust sources changed, decides that by content and not by modification times,
# serialises concurrent builds, recovers a stale lock, and its input list covers what the
# Rust build of clients/ffi reads. No real build: the script runs from a copy in a scratch
# git repository, beside a stand-in build-xcframework.sh, against a scratch cache, so the
# real package's linked framework and the real cache are untouched. [DD-6] [FR-C-1]
#
#   bash tools/ci/tests/apple-ensure-xcframework.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
e=clients/shells/apple/tools/ensure-xcframework.sh
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "script exists" "[ -f $e ]"
check "no bash-4 builtins" "[ -f $e ] && ! grep -Eq 'mapfile|readarray|declare -A|\\\$\\{[A-Za-z_]+,,\\}' $e"
check "no in-place sed" "[ -f $e ] && ! grep -Eq 'sed -i( |\$)' $e"
check "parses under /bin/bash" "[ -f $e ] && /bin/bash -n $e"
check "the default cache is the real one" "grep -q '^default_cache=\"\$HOME/Library/Caches/midgard-build\"' $e"
check "finds rustup and Homebrew itself" "grep -q '^export PATH=\"/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:' $e"
check "refuses a relative MIDGARD_BUILD_CACHE" "(MIDGARD_BUILD_CACHE=relative bash $e >/dev/null 2>&1; [ \$? -eq 2 ])"

# Every input is in the tree, and the inputs cover clients/ffi's path dependencies (the
# normal and build closure, as cargo resolves it) and the trust anchors core includes.
inputs="$(bash $e --inputs 2>/dev/null)"
missing=""
for i in $inputs; do
    [ -n "$(git ls-files -- "$i" | head -n 1)" ] || missing="$missing $i"
done
check "every input is tracked${missing:+ (missing:$missing)}" "[ -n \"\$inputs\" ] && [ -z \"\$missing\" ]"
check "inputs leave out the shells and the datasets" "! grep -Eq '^clients/(shells/apple/(Apps|Sources)|demo/datasets)' <<<\"\$inputs\""
# (Test-only includes of tests/fixtures are not part of the library build.)
for a in $(grep -rhoE 'include_(str|bytes)!\("(\.\./)+demo/[^/"]+/' clients/core/src clients/ffi/src 2>/dev/null | sed -E 's|.*/demo/([^/]+)/$|clients/demo/\1|' | sort -u); do
    check "inputs include $a (read by include_str!)" "grep -qx '$a' <<<\"\$inputs\""
done
if command -v cargo >/dev/null 2>&1 && deps="$(cargo tree --offline --locked -p midgard-client-ffi -e normal,build --target all --prefix none 2>/dev/null)"; then
    here="$(pwd)"
    for d in $(sed -n "s|.*($here/\\([^)]*\\)).*|\\1|p" <<<"$deps" | sort -u); do
        check "inputs include path dependency $d" "grep -qx '$d' <<<\"\$inputs\""
    done
else
    echo "SKIP  inputs cover cargo's dependency closure (cargo or the vendored crates not available)"
fi

# --- behaviour, in a scratch repository -------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
r="$tmp/repo"
c="$tmp/cache"
a="$r/clients/shells/apple"
mkdir -p "$a/tools" "$r/.cargo" "$r/clients/core/src" "$r/clients/ffi/src" "$r/clients/demo/datasets" "$c"
for f in Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml clients/core/src/lib.rs clients/ffi/src/lib.rs \
    clients/demo/datasets/d.mgdemo; do
    echo "// $f" >"$r/$f"
done
[ -f $e ] && cp $e "$a/tools/ensure-xcframework.sh"
echo '# stand-in' >"$a/tools/verify-xcframework.sh"
# Behaves like build-xcframework.sh (unlinks, builds into the cache, swaps, links) and
# logs each build. Controlled by files, as ensure-xcframework.sh runs it in a clean
# environment: $cache/fake-sleep (seconds), $cache/fake-fail.
cat >"$a/tools/build-xcframework.sh" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
cache="$MIDGARD_BUILD_CACHE"
if [ -n "${SDKROOT:-}${ARCHS:-}" ]; then echo "fake build: Xcode's environment reached the build" >&2; exit 3; fi
rm -rf clients/shells/apple/Frameworks/MidgardCoreFFI.xcframework
echo "$$" >>"$cache/builds.log"
[ ! -f "$cache/fake-sleep" ] || sleep "$(cat "$cache/fake-sleep")"
if [ -f "$cache/fake-fail" ]; then echo "fake build: failing" >&2; exit 1; fi
rm -rf "$cache/clients-xcframework.new"
mkdir -p "$cache/clients-xcframework.new/MidgardCoreFFI.xcframework" "$cache/clients-xcframework.new/swift"
echo "plist" >"$cache/clients-xcframework.new/MidgardCoreFFI.xcframework/Info.plist"
echo "// bindings $(wc -l <"$cache/builds.log")" >"$cache/clients-xcframework.new/swift/midgard_client_ffi.swift"
rm -rf "$cache/clients-xcframework"
mv "$cache/clients-xcframework.new" "$cache/clients-xcframework"
mkdir -p clients/shells/apple/Frameworks clients/shells/apple/Sources/MidgardCore
ln -sfn "$cache/clients-xcframework/MidgardCoreFFI.xcframework" clients/shells/apple/Frameworks/MidgardCoreFFI.xcframework
cp "$cache/clients-xcframework/swift/midgard_client_ffi.swift" clients/shells/apple/Sources/MidgardCore/
FAKE
printf '/clients/shells/apple/Frameworks/\n/clients/shells/apple/Sources/MidgardCore/midgard_client_ffi.swift\n' >"$r/.gitignore"
g() { git -C "$r" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
g init -q && g add -A && g commit -q -m fixture
# As Xcode runs it: a minimal PATH, an app's SDKROOT and ARCHS, from another directory, /bin/bash.
en() {
    (cd / && env -i PATH=/usr/bin:/bin HOME="$HOME" MIDGARD_BUILD_CACHE="$c" SDKROOT=/bogus/iPhoneOS.sdk ARCHS=arm64 "$@" \
        /bin/bash "$a/tools/ensure-xcframework.sh" 2>&1)
}
builds() { if [ -f "$c/builds.log" ]; then wc -l <"$c/builds.log" | tr -d ' '; else echo 0; fi; }
link="$a/Frameworks/MidgardCoreFFI.xcframework"

out="$(en)"; rc=$?
check "missing link: builds (exit 0)" "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && grep -q 'built' <<<\"\$out\""
check "the build's environment is clean (no SDKROOT or ARCHS from Xcode)" "! grep -q 'environment reached the build' <<<\"\$out\""
check "links the cached framework and the bindings" "[ \"\$(readlink $link)\" = $c/clients-xcframework/MidgardCoreFFI.xcframework ] && [ -f $a/Sources/MidgardCore/midgard_client_ffi.swift ]"
check "writes the stamp beside the framework" "[ -s $c/clients-xcframework/inputs.sha256 ]"
check "no lock left behind" "[ ! -e $c/clients-xcframework.lock ]"
check "says nothing of test mode when not testing" "! grep -q 'TEST MODE' <<<\"\$out\""

out="$(en)"; rc=$?
check "second call: no build, says it is current" "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && grep -q ': current' <<<\"\$out\""

touch "$r/clients/core/src/lib.rs" "$r/Cargo.lock"
out="$(en)"
check "newer modification times alone: no build" "[ \$(builds) -eq 1 ]"

rm -rf "$a/Frameworks" "$a/Sources/MidgardCore/midgard_client_ffi.swift"
out="$(en)"; rc=$?
check "fresh checkout (no link, no bindings), same sources: relinks without building" \
    "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && [ -L $link ] && [ -f $a/Sources/MidgardCore/midgard_client_ffi.swift ]"

echo "// edited" >>"$r/clients/core/src/lib.rs"
out="$(en)"
check "uncommitted source edit: rebuilds" "[ \$(builds) -eq 2 ]"
out="$(en)"
check "... and once only" "[ \$(builds) -eq 2 ] && grep -q ': current' <<<\"\$out\""
g checkout -q -- clients/core/src/lib.rs
out="$(en)"
check "edit reverted: rebuilds (stamp is of content, not of the last build)" "[ \$(builds) -eq 3 ]"

echo "// new module" >"$r/clients/core/src/new.rs"
out="$(en)"
check "new untracked source file: rebuilds" "[ \$(builds) -eq 4 ]"
echo "// changed" >>"$r/clients/core/src/new.rs"
out="$(en)"
check "untracked file changed: rebuilds" "[ \$(builds) -eq 5 ]"
rm -f "$r/clients/core/src/new.rs"
out="$(en)"

n=$(builds)
echo "changed" >>"$r/clients/demo/datasets/d.mgdemo"
echo "changed" >"$a/tools/verify-xcframework.sh.notes"
out="$(en)"
check "change outside the inputs (datasets, shells): no build" "[ \$(builds) -eq $n ]"

# Two app builds at once: one builds, the other waits and finds it current.
echo "// concurrent" >>"$r/clients/ffi/src/lib.rs"
echo 3 >"$c/fake-sleep"
n=$(builds)
en MIDGARD_TEST_LOCK_TIMEOUT=30 >"$tmp/o1" & p1=$!
# The second starts once the first holds the lock (and is inside its 3 s build).
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [ -s "$c/clients-xcframework.lock/pid" ] && break
    sleep 0.5
done
en MIDGARD_TEST_LOCK_TIMEOUT=30 >"$tmp/o2" & p2=$!
wait $p1; r1=$?
wait $p2; r2=$?
rm -f "$c/fake-sleep"
check "two concurrent calls: both succeed, one build" "[ $r1 -eq 0 ] && [ $r2 -eq 0 ] && [ \$(builds) -eq $((n + 1)) ]"
check "... the other waited and found it current" "cat $tmp/o1 $tmp/o2 | grep -q 'built by another run' && cat $tmp/o1 $tmp/o2 | grep -q 'waiting for another build'"
check "... and the lock is gone" "[ ! -e $c/clients-xcframework.lock ]"

# A lock whose holder died is reclaimed. (A timeout, so that a regression fails instead
# of waiting 30 minutes.)
sh -c 'exit 0' & dead=$!
wait $dead
mkdir "$c/clients-xcframework.lock" && echo "$dead" >"$c/clients-xcframework.lock/pid"
echo "// after a crash" >>"$r/clients/ffi/src/lib.rs"
n=$(builds)
out="$(en MIDGARD_TEST_LOCK_TIMEOUT=10)"; rc=$?
check "stale lock (dead pid): recovered and built" "[ $rc -eq 0 ] && [ \$(builds) -eq $((n + 1)) ] && grep -q 'removed a stale lock' <<<\"\$out\" && [ ! -e $c/clients-xcframework.lock ]"

# A live holder is waited for, up to the timeout; its lock is left alone.
mkdir "$c/clients-xcframework.lock" && echo "$$" >"$c/clients-xcframework.lock/pid"
echo "// while locked" >>"$r/clients/ffi/src/lib.rs"
n=$(builds)
out="$(en MIDGARD_TEST_LOCK_TIMEOUT=2)"; rc=$?
check "live lock: waits, then gives up (non-zero) without building" "[ $rc -ne 0 ] && [ \$(builds) -eq $n ] && grep -q 'gave up after 2s' <<<\"\$out\""
check "... says it is in test mode" "grep -q 'TEST MODE: lock timeout 2s' <<<\"\$out\""
check "... and leaves the holder's lock" "[ \"\$(cat $c/clients-xcframework.lock/pid)\" = $$ ]"
rm -rf "$c/clients-xcframework.lock"

# A failed build writes no stamp: the next call builds again.
touch "$c/fake-fail"
out="$(en)"; rc=$?
check "failed build: non-zero, says so" "[ $rc -ne 0 ] && grep -q 'build-xcframework.sh failed' <<<\"\$out\""
check "... and leaves no lock" "[ ! -e $c/clients-xcframework.lock ]"
rm -f "$c/fake-fail"
n=$(builds)
out="$(en)"
check "... the next call builds" "[ \$(builds) -eq $((n + 1)) ] && grep -q ': built' <<<\"\$out\""
[ "$failures" -eq 0 ] || exit 1
