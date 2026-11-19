#!/usr/bin/env bash
# ensure-xcframework.sh (the app projects' pre-build step) builds the XCFramework only
# when the Rust sources or the toolchain changed, decides that by content (not by
# modification times, the index or HEAD), keeps each build immutable under
# clients-xcframework-by-stamp/<stamp>/, serialises concurrent builds, recovers a stale
# lock and no other, and its input list covers what the Rust build of clients/ffi reads.
# No real build: the script runs from a copy in a scratch git repository, beside a
# stand-in build-xcframework.sh and stand-in rustc/cargo, against a scratch cache, so the
# real package's linked framework and the real cache are untouched. [DD-6] [FR-C-1]
#
#   bash tools/ci/tests/apple-ensure-xcframework.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
# As the script finds the toolchain.
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
e=clients/shells/apple/tools/ensure-xcframework.sh
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "script exists" "[ -f $e ]"
check "no bash-4 builtins" "[ -f $e ] && ! grep -Eq 'mapfile|readarray|declare -A|\\\$\\{[A-Za-z_]+,,\\}' $e"
check "no in-place sed" "[ -f $e ] && ! grep -Eq 'sed -i( |\$)' $e"
check "parses under /bin/bash" "[ -f $e ] && /bin/bash -n $e"
check "the default cache is the real one" "grep -q '^default_cache=\"\$HOME/Library/Caches/midgard-build\"' $e"
check "finds rustup and Homebrew itself" "grep -q '^export PATH=\"/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:' $e"
check "resolves the toolchain from rustup's directory, as the build does" "grep -q '^rustup_bin=/opt/homebrew/opt/rustup/bin\$' $e"
check "refuses a relative MIDGARD_BUILD_CACHE" "(MIDGARD_BUILD_CACHE=relative bash $e >/dev/null 2>&1; [ \$? -eq 2 ])"

# --- the input list ---------------------------------------------------------------------
inputs="$(bash $e --inputs 2>/dev/null)"
missing=""
for i in $inputs; do
    [ -n "$(git ls-files -- "$i" | head -n 1)" ] || missing="$missing $i"
done
check "every input is tracked${missing:+ (missing:$missing)}" "[ -n \"\$inputs\" ] && [ -z \"\$missing\" ]"
check "inputs leave out the shells and the datasets" "! grep -Eq '^clients/(shells/apple/(Apps|Sources)|demo/datasets)' <<<\"\$inputs\""
# in_inputs <repo-relative path>: the path is an input or inside one.
in_inputs() {
    local i
    for i in $inputs; do
        case "$1" in "$i" | "$i"/*) return 0 ;; esac
    done
    return 1
}
# Every include_str!/include_bytes! with a literal path, in every input crate, reads a
# file inside the inputs.
top="$(pwd -P)"
outside=""
nincl=0
crates=""
for i in $inputs; do [ -f "$i/Cargo.toml" ] && crates="$crates $i"; done
while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    f="${hit%%:*}"
    rel="$(sed -E 's/.*include_(str|bytes)!\([[:space:]]*"([^"]*)".*/\2/' <<<"$hit")"
    t="$(dirname "$f")/$rel"
    d="$(cd "$(dirname "$t")" 2>/dev/null && pwd -P)" || d=""
    nincl=$((nincl + 1))
    if [ -z "$d" ] || [ ! -e "$d/$(basename "$t")" ]; then
        outside="$outside $f->$rel(missing)"
        continue
    fi
    p="${d#"$top"/}/$(basename "$t")"
    in_inputs "$p" || outside="$outside $f->$p"
done <<<"$(grep -rHoE 'include_(str|bytes)!\([[:space:]]*"[^"]*"' $crates --include='*.rs' 2>/dev/null)"
check "every literal include_str!/include_bytes! ($nincl) reads inside the inputs${outside:+ (outside:$outside)}" \
    "[ $nincl -gt 0 ] && [ -z \"\$outside\" ]"
# ... and the inputs cover clients/ffi's path dependencies (the normal and build closure,
# as cargo resolves it). On a Mac with the vendored crates, cargo must be there.
if deps="$(cargo tree --offline --locked -p midgard-client-ffi -e normal,build --target all --prefix none 2>/dev/null)"; then
    for d in $(sed -n "s|.*($top/\\([^)]*\\)).*|\\1|p" <<<"$deps" | sort -u); do
        check "inputs include path dependency $d" "grep -qx '$d' <<<\"\$inputs\""
    done
elif [ "$(uname -s)" = Darwin ] && [ -d vendor ]; then
    check "cargo tree runs (needed for the dependency-closure check on macOS)" "false"
else
    echo "SKIP  inputs cover cargo's dependency closure (no cargo or no vendored crates here)"
fi

# --- behaviour, in a scratch repository -------------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
tmp="$(cd "$tmp" && pwd -P)"
r="$tmp/repo"
c="$tmp/cache"
a="$r/clients/shells/apple"
bs="$c/clients-xcframework-by-stamp"
lock="$c/clients-xcframework.lock"
mkdir -p "$a/tools" "$r/.cargo" "$r/clients/core/src" "$r/clients/ffi/src" "$r/clients/demo/datasets" "$c" "$tmp/bin" "$tmp/cargohome"
for f in Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml clients/core/src/lib.rs clients/ffi/src/lib.rs \
    clients/demo/datasets/d.mgdemo; do
    echo "// $f" >"$r/$f"
done
[ -f $e ] && cp $e "$a/tools/ensure-xcframework.sh"
echo '# stand-in' >"$a/tools/verify-xcframework.sh"
# Behaves like `build-xcframework.sh --out <dir>` (builds in <dir>.new, renames it to
# <dir>, links nothing) and logs each build. Controlled by files, as ensure-xcframework.sh
# runs it in a clean environment: $cache/fake-sleep (seconds), $cache/fake-fail,
# $cache/fake-hang (waits on a grandchild, as cargo would run under the build).
cat >"$a/tools/build-xcframework.sh" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
cache="$MIDGARD_BUILD_CACHE"
[ "${1:-}" = --out ] && [ -n "${2:-}" ] || { echo "fake build: expected --out <dir>" >&2; exit 2; }
dest="$2"
[ ! -e "$dest" ] || { echo "fake build: $dest exists" >&2; exit 2; }
if [ -n "${SDKROOT:-}${ARCHS:-}" ]; then echo "fake build: Xcode's environment reached the build" >&2; exit 3; fi
echo "$$" >>"$cache/builds.log"
if [ -f "$cache/fake-hang" ]; then
    sleep 60 &
    echo "$!" >"$cache/grandchild"
    wait
fi
[ ! -f "$cache/fake-sleep" ] || sleep "$(cat "$cache/fake-sleep")"
if [ -f "$cache/fake-fail" ]; then echo "fake build: failing" >&2; exit 1; fi
n="$(wc -l <"$cache/builds.log" | tr -d ' ')"
mkdir -p "$dest.new/MidgardCoreFFI.xcframework" "$dest.new/swift"
echo "build $n" >"$dest.new/MidgardCoreFFI.xcframework/Info.plist"
echo "// bindings $n" >"$dest.new/swift/midgard_client_ffi.swift"
mv "$dest.new" "$dest"
FAKE
# Stand-in toolchain: rustc -vV reports $cache/fake-rustc (a version line).
printf '#!/bin/sh\necho "rustc stand-in"\ncat "%s/fake-rustc" 2>/dev/null || true\n' "$c" >"$tmp/bin/rustc"
printf '#!/bin/sh\necho "cargo stand-in"\n' >"$tmp/bin/cargo"
chmod +x "$tmp/bin/rustc" "$tmp/bin/cargo"
echo "release: 1.82.0" >"$c/fake-rustc"
printf '/clients/shells/apple/Frameworks/\n/clients/shells/apple/Sources/MidgardCore/midgard_client_ffi.swift\n*.log\n' >"$r/.gitignore"
g() { git -C "$r" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
g init -q && g add -A && g commit -q -m fixture
# As Xcode runs it: a minimal PATH, an app's SDKROOT and ARCHS, from another directory,
# /bin/bash. $1 is the checkout (default the scratch repository); the rest are extra
# environment entries.
en() {
    local repo="$r"
    case "${1:-}" in /*) repo="$1" && shift ;; esac
    (cd / && env -i PATH=/usr/bin:/bin HOME="$HOME" MIDGARD_BUILD_CACHE="$c" SDKROOT=/bogus/iPhoneOS.sdk ARCHS=arm64 \
        CARGO_HOME="$tmp/cargohome" MIDGARD_TEST_TOOLCHAIN_BIN="$tmp/bin" "$@" \
        /bin/bash "$repo/clients/shells/apple/tools/ensure-xcframework.sh" 2>&1)
}
builds() { if [ -f "$c/builds.log" ]; then wc -l <"$c/builds.log" | tr -d ' '; else echo 0; fi; }
link="$a/Frameworks/MidgardCoreFFI.xcframework"
target() { readlink "${1:-$link}" 2>/dev/null; }
entries() { ls -1 "$bs" 2>/dev/null | wc -l | tr -d ' '; }
# What the script prints apart from the cache-root and test-mode notes.
said() { grep -v -e 'cache root' -e 'TEST MODE' <<<"$out"; }
start_of() { TZ=UTC0 LC_ALL=C ps -o lstart= -p "$1" 2>/dev/null | sed -e 's/^ *//' -e 's/ *$//'; }
# mklock <pid> <start>: a lock as the script writes it (owner: pid, then start time).
mklock() { mkdir "$lock" && printf '%s\n%s\n' "$1" "$2" >"$lock/owner"; }

out="$(en)"; rc=$?
e1="$(target)"
check "missing link: builds (exit 0)" "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && grep -q ': built' <<<\"\$out\""
check "the build's environment is clean (no SDKROOT or ARCHS from Xcode)" "! grep -q 'environment reached the build' <<<\"\$out\""
check "says it is in test mode (stand-in toolchain)" "grep -q 'TEST MODE: toolchain from' <<<\"\$out\""
check "test-mode and cache notices are warning: lines (Xcode shows them)" \
    "grep -q '^warning: ensure-xcframework: TEST MODE' <<<\"\$out\" && grep -q '^warning: ensure-xcframework: cache root' <<<\"\$out\""
mkdir -p "$tmp/home"
out="$(cd / && env -i PATH=/usr/bin:/bin HOME="$tmp/home" MIDGARD_TEST_LOCK_TIMEOUT=2 /bin/bash "$a/tools/ensure-xcframework.sh" 2>&1)"; rc=$?
check "a MIDGARD_TEST_* variable with the default cache: refused, nothing touched" \
    "[ $rc -ne 0 ] && grep -q '^error: .*MIDGARD_TEST_LOCK_TIMEOUT is set' <<<\"\$out\" && [ ! -e $tmp/home/Library ] && [ \"\$(target)\" = \"\$e1\" ]"
out="$(en)"
check "links a by-stamp build, named by its stamp" \
    "[[ \"\$e1\" =~ ^$bs/[0-9a-f]{64}/MidgardCoreFFI.xcframework\$ ]] && [ \"\$(cat \$(dirname \$e1)/inputs.sha256)\" = \"\$(basename \$(dirname \$e1))\" ]"
check "copies the bindings of that build" "cmp -s $a/Sources/MidgardCore/midgard_client_ffi.swift \$(dirname \$e1)/swift/midgard_client_ffi.swift"
check "no lock and no temporary directory left" "[ ! -e $lock ] && [ -z \"\$(ls -A $bs | grep '^\\.')\" ]"
check "does not use clients-xcframework (build-xcframework.sh's own output)" "[ ! -e $c/clients-xcframework ]"

out="$(en)"; rc=$?
check "second call: no build, one line saying it is current" "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && [ \"\$(said | wc -l | tr -d ' ')\" = 1 ] && said | grep -q ': current'"

touch -t 202001010000 "$(dirname "$e1")"
touch "$r/clients/core/src/lib.rs" "$r/Cargo.lock"
out="$(en)"
check "newer modification times alone: no build" "[ \$(builds) -eq 1 ]"
check "using a build marks it used (mtime), for pruning" "[ -n \"\$(find \$(dirname \$e1) -maxdepth 0 -newer $r/Cargo.toml)\" ]"

rm -rf "$a/Frameworks" "$a/Sources/MidgardCore/midgard_client_ffi.swift"
out="$(en)"; rc=$?
check "fresh checkout (no link, no bindings), same sources: relinks without building" \
    "[ $rc -eq 0 ] && [ \$(builds) -eq 1 ] && [ \"\$(target)\" = \"\$e1\" ] && [ -f $a/Sources/MidgardCore/midgard_client_ffi.swift ]"

echo "// edited" >>"$r/clients/core/src/lib.rs"
out="$(en)"
e2="$(target)"
check "uncommitted source edit: rebuilds into a new entry" "[ \$(builds) -eq 2 ] && [ \"\$e2\" != \"\$e1\" ]"
out="$(en)"
check "... and once only" "[ \$(builds) -eq 2 ] && said | grep -q ': current'"
g add clients/core/src/lib.rs
out="$(en)"
check "staging the built edit: no build" "[ \$(builds) -eq 2 ]"
g commit -q -m edit
out="$(en)"
check "committing it (another HEAD, same content): no build" "[ \$(builds) -eq 2 ] && [ \"\$(target)\" = \"\$e2\" ]"
g revert --no-edit HEAD >/dev/null
out="$(en)"
check "back to the first content: no build, relinks the first entry" "[ \$(builds) -eq 2 ] && [ \"\$(target)\" = \"\$e1\" ]"

echo "// new module" >"$r/clients/core/src/new.rs"
out="$(en)"
check "new untracked source file: rebuilds" "[ \$(builds) -eq 3 ]"
echo "// changed" >>"$r/clients/core/src/new.rs"
out="$(en)"
check "untracked file changed: rebuilds" "[ \$(builds) -eq 4 ]"
rm -f "$r/clients/core/src/new.rs"
out="$(en)"
check "untracked file removed: back to a built content, no build" "[ \$(builds) -eq 4 ] && [ \"\$(target)\" = \"\$e1\" ]"
tabbed="$(printf 'clients/core/src/a\tb "q".rs')"
echo "// odd name" >"$r/$tabbed"
out="$(en)"
check "untracked file with a tab and quotes in its name: rebuilds" "[ \$(builds) -eq 5 ]"
echo "// odd name, changed" >"$r/$tabbed"
out="$(en)"
check "... and its content counts" "[ \$(builds) -eq 6 ]"
rm -f "$r/$tabbed"
echo "noise" >"$r/clients/core/src/trace.log"
out="$(en)"
check "ignored file among the inputs: not in the stamp, no build" "[ \$(builds) -eq 6 ] && [ \"\$(target)\" = \"\$e1\" ]"
echo "changed" >>"$r/clients/demo/datasets/d.mgdemo"
echo "changed" >"$a/tools/verify-xcframework.sh.notes"
out="$(en)"
check "change outside the inputs (datasets, shells): no build" "[ \$(builds) -eq 6 ]"
g checkout -q -- clients/demo/datasets/d.mgdemo
rm -f "$a/tools/verify-xcframework.sh.notes"

echo "release: 1.83.0" >"$c/fake-rustc"
out="$(en)"
check "toolchain changed (rustc -vV): rebuilds" "[ \$(builds) -eq 7 ]"
echo "release: 1.82.0" >"$c/fake-rustc"
out="$(en)"
check "toolchain back: no build" "[ \$(builds) -eq 7 ] && [ \"\$(target)\" = \"\$e1\" ]"
printf '[build]\njobs = 2\n' >"$tmp/cargohome/config.toml"
out="$(en)"
check "\$CARGO_HOME/config.toml added: rebuilds" "[ \$(builds) -eq 8 ]"
rm -f "$tmp/cargohome/config.toml"

# A git failure means no stamp: build, and say why.
branch="$(g symbolic-ref --short HEAD)"
g symbolic-ref HEAD refs/heads/unborn
out="$(en)"; rc=$?
check "git fails (no HEAD): builds, saying why" "[ $rc -eq 0 ] && [ \$(builds) -eq 9 ] && grep -q 'cannot compute the source stamp (git read-tree HEAD failed' <<<\"\$out\""
g symbolic-ref HEAD "refs/heads/$branch"
out="$(en)"
check "git back: no build" "[ \$(builds) -eq 9 ] && [ \"\$(target)\" = \"\$e1\" ]"

# Two worktrees with the same content share one build; a build in one never changes
# what the other links.
g worktree add -q "$tmp/wt2" HEAD 2>/dev/null
l2="$tmp/wt2/clients/shells/apple/Frameworks/MidgardCoreFFI.xcframework"
out="$(en "$tmp/wt2")"
check "second worktree, same content: no build, links the same entry" "[ \$(builds) -eq 9 ] && [ \"\$(target $l2)\" = \"\$e1\" ]"
before="$(cat "$e1/Info.plist")"
echo "// only in wt2" >>"$tmp/wt2/clients/ffi/src/lib.rs"
out="$(en "$tmp/wt2")"
check "a build in the other worktree: its own entry" "[ \$(builds) -eq 10 ] && [ \"\$(target $l2)\" != \"\$e1\" ]"
check "... and this checkout's link and framework are unchanged" "[ \"\$(target)\" = \"\$e1\" ] && [ \"\$(cat \$e1/Info.plist)\" = \"\$before\" ]"
w2="$(target "$l2")"

# Pruning: beyond the 3 most recently used entries, those unused for over 10 minutes go,
# except any a worktree links.
n0=$(entries)
for k in 1 2 3; do
    echo "// prune $k" >>"$r/clients/ffi/src/lib.rs"
    out="$(en)"
done
check "entries used in the last 10 minutes are kept, beyond the 3 most recent" "[ \$(entries) -eq $((n0 + 3)) ]"
i=10
for d in "$bs"/*; do
    touch -t "2020010100$i" "$d"
    i=$((i + 1))
done
for k in 4 5; do
    echo "// prune $k" >>"$r/clients/ffi/src/lib.rs"
    out="$(en)"
done
check "entries unused for over 10 minutes are pruned to the 3 most recent, plus the linked ones" \
    "[ \$(builds) -eq 15 ] && [ \$(entries) -le 4 ] && grep -q 'pruned the build' <<<\"\$out\""
check "... never the entry another worktree links" "[ -d \"\$w2\" ]"
check "... never the entry this checkout links" "[ -d \"\$(target)\" ]"
g checkout -q -- clients/ffi/src/lib.rs
out="$(en)"

# Two app builds at once: one builds, the other waits and finds it current.
echo "// concurrent" >>"$r/clients/ffi/src/lib.rs"
echo 3 >"$c/fake-sleep"
n=$(builds)
en MIDGARD_TEST_LOCK_TIMEOUT=30 >"$tmp/o1" & p1=$!
# The second starts once the first holds the lock (and is inside its 3 s build).
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [ -s "$lock/owner" ] && break
    sleep 0.5
done
en MIDGARD_TEST_LOCK_TIMEOUT=30 >"$tmp/o2" & p2=$!
wait $p1; r1=$?
wait $p2; r2=$?
rm -f "$c/fake-sleep"
check "two concurrent calls: both succeed, one build" "[ $r1 -eq 0 ] && [ $r2 -eq 0 ] && [ \$(builds) -eq $((n + 1)) ]"
check "... the other waited and found it current" "cat $tmp/o1 $tmp/o2 | grep -q 'built by another run' && cat $tmp/o1 $tmp/o2 | grep -q 'waiting for another build'"
check "... and the lock is gone" "[ ! -e $lock ]"

# Locks. Every call has a timeout, so a regression fails instead of waiting 30 minutes.
sh -c 'exit 0' & dead=$!
wait $dead
me_start="$(start_of $$)"
# relock <recovered|waits> <description>: one call against the lock in place, after a
# source edit so that it must build.
k=0
relock() {
    k=$((k + 1))
    echo "// lock case $k" >>"$r/clients/ffi/src/lib.rs"
    n=$(builds)
    out="$(en MIDGARD_TEST_LOCK_TIMEOUT=2 "${@:3}")"; rc=$?
    if [ "$1" = recovered ]; then
        check "$2: recovered and built" "[ $rc -eq 0 ] && [ \$(builds) -eq $((n + 1)) ] && grep -q 'removed a stale lock' <<<\"\$out\" && [ ! -e $lock ]"
    else
        check "$2: waited, gave up without building, left the lock" \
            "[ $rc -ne 0 ] && [ \$(builds) -eq $n ] && grep -q 'gave up after 2s' <<<\"\$out\" && [ -d $lock ]"
        rm -rf "$lock"
    fi
}
mklock "$dead" "$me_start"
relock recovered "dead holder"
mklock $$ "Thu Jan  1 00:00:00 1970"
relock recovered "reused pid (alive, another start time)"
mklock $$ "$me_start"
relock waits "live holder"
mklock $$ "$me_start"
relock waits "live holder recorded in UTC, waiter in another time zone" TZ=Pacific/Kiritimati
mklock "$dead" "$me_start" && printf '%s\n%s\n' $$ "$me_start" >"$lock/child"
relock waits "dead holder whose build is still running"
mkdir "$lock"
relock waits "pidless lock, under a minute old"
mkdir "$lock" && touch -t 202001010000 "$lock"
relock recovered "pidless lock, over a minute old"
mkdir "$lock" && printf 'abc\nwhenever\n' >"$lock/owner"
relock waits "non-numeric pid, under a minute old"
mkdir "$lock" && printf 'abc\nwhenever\n' >"$lock/owner" && touch -t 202001010000 "$lock"
relock recovered "non-numeric pid, over a minute old"
check "a test-mode lock timeout says so" "grep -q 'TEST MODE: lock timeout 2s' <<<\"\$out\""

# A failed build leaves nothing linked and no entry; the next call builds.
echo "// fails" >>"$r/clients/ffi/src/lib.rs"
touch "$c/fake-fail"
n0=$(entries)
out="$(en)"; rc=$?
check "failed build: non-zero, says so" "[ $rc -ne 0 ] && grep -q 'build-xcframework.sh failed' <<<\"\$out\""
check "... nothing linked, no new entry, no lock" "[ ! -e $link ] && [ ! -L $link ] && [ \$(entries) -eq $n0 ] && [ ! -e $lock ]"
rm -f "$c/fake-fail"
n=$(builds)
out="$(en)"
check "... the next call builds and cleans the failed build's leftovers" \
    "[ \$(builds) -eq $((n + 1)) ] && grep -q ': built' <<<\"\$out\" && [ -z \"\$(ls -A $bs | grep '^\\.')\" ]"

# A signal stops the whole build, which runs in its own process group (cargo included).
echo "// hang" >>"$r/clients/ffi/src/lib.rs"
rm -f "$c/grandchild"
touch "$c/fake-hang"
en >"$tmp/o3" & p3=$!
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [ -s "$c/grandchild" ] && break
    sleep 0.5
done
gc="$(cat "$c/grandchild" 2>/dev/null || echo 0)"
kill -TERM "$(head -n 1 "$lock/owner" 2>/dev/null || echo 0)" 2>/dev/null
wait $p3; r3=$?
sleep 1
check "TERM during a build: exits non-zero, the build's grandchild is stopped, no lock" \
    "[ $r3 -ne 0 ] && [ $gc -gt 0 ] && ! ps -p $gc >/dev/null 2>&1 && [ ! -e $lock ]"
[ "$gc" -gt 0 ] && kill "$gc" 2>/dev/null
rm -f "$c/fake-hang"
[ "$failures" -eq 0 ] || exit 1
