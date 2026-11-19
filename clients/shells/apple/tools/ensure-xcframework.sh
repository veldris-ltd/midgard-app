#!/usr/bin/env bash
# Make sure this checkout's Frameworks/MidgardCoreFFI.xcframework links to a build of
# the Rust sources as they stand, building only when needed. The app projects' pre-build
# step. [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/ensure-xcframework.sh [--inputs]
#
# Builds are content-addressed and immutable: each lives in
# ~/Library/Caches/midgard-build/clients-xcframework-by-stamp/<stamp>/ (the framework,
# swift/ with the bindings, and inputs.sha256), and the checkout links there. A build
# whose stamp directory exists is current; nothing rewrites a stamp directory once it is
# in place, so a build for another checkout (or worktree) never changes what this one
# links. build-xcframework.sh run on its own still writes clients-xcframework/, which
# this script neither reads nor writes.
#
# The stamp is a SHA-256 over:
#   - the content of the inputs as they stand: a throwaway index (and object directory)
#     is loaded from HEAD, `git add -A -- <inputs>` brings it to the working tree
#     (tracked, deleted and untracked files; ignored files left out), and its entries
#     for the inputs (mode, blob id, path) are hashed. Staging or committing an edit, or
#     another HEAD with the same content, gives the same stamp;
#   - `rustc -vV` and `cargo -V` run as the build runs them (from the repository, with
#     rustup's directory first on PATH, in the build's clean environment);
#   - every Cargo configuration file the build reads apart from the inputs:
#     $CARGO_HOME/config(.toml) and .cargo/config(.toml) in each directory above the
#     repository.
# If any of it fails (no git, no HEAD, no toolchain), there is no stamp: the script says
# why and builds. Limits: a file marked skip-worktree or assume-unchanged counts as its
# HEAD content; a file the build reads that git ignores is not in the stamp; environment
# variables are not (the build runs in a clean environment that sets its own).
#
# The inputs are everything the Rust build of clients/ffi reads (`--inputs` prints them):
# the workspace files, clients/ffi and its path dependencies (the normal and build
# dependency closure; tools/ci/tests/apple-ensure-xcframework.test.sh holds the list to
# `cargo tree -p midgard-client-ffi -e normal,build` and every include_str!/include_bytes!
# in them), the demo trust anchors clients/core includes, and the build and verify
# scripts. vendor/ is left out: Cargo.lock pins every vendored crate.
#
# Builds are serialised by a lock directory (clients-xcframework.lock) holding the
# holder's pid and process start time (UTC), and its build's. A lock is stale when neither
# process (pid and start time together) is running, or when it has had no owner record
# for over a minute; it is moved aside atomically, checked again there, and deleted only
# if still stale. A waiter re-checks the stamp once it holds the lock; the wait gives up
# after 30 minutes. The build runs in its own process group, and a signal stops the group.
# After each build, builds beyond the 3 most recently used are deleted, except any a
# worktree of this repository links and any used in the last 10 minutes.
#
# Xcode run-script phases start with a minimal PATH and an environment meant for the app
# (SDKROOT, ARCHS, deployment targets...): rustup's directory is put on PATH here, and the
# build runs in a clean environment. Paths are resolved from this script's location, so
# the working directory does not matter. MIDGARD_BUILD_CACHE (an absolute path) moves the
# cache root, as for build-xcframework.sh. Notices Xcode should show are "warning:" lines.
# For the tests only, refused with the default cache, and said when set:
# MIDGARD_TEST_LOCK_TIMEOUT (seconds) and MIDGARD_TEST_TOOLCHAIN_BIN (a directory put
# before rustup's, for the stamp and the build).
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin${PATH:+:$PATH}"
# /usr/bin/git is an xcrun shim: an SDKROOT Xcode set for the app (or a stale one) can
# break it.
unset SDKROOT
me=ensure-xcframework
here="$(cd "$(dirname "$0")" && pwd)"
apple="$(cd "$here/.." && pwd)"
root="$(cd "$apple/../../.." && pwd)"
rustup_bin=/opt/homebrew/opt/rustup/bin
die() {
    echo "error: $me: $*" >&2
    exit 1
}

inputs=(
    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    .cargo/config.toml
    clients/ffi
    clients/core
    clients/enclosure
    clients/inference
    clients/keystore
    clients/store
    clients/sync
    crypto/attest
    crypto/audit-writer
    crypto/policy
    crypto/pq-mtls
    crypto/pqc-agility
    mimir/prompts
    services/kms
    clients/demo/trust
    clients/shells/apple/tools/build-xcframework.sh
    clients/shells/apple/tools/verify-xcframework.sh
)
case "${1:-}" in
    '') ;;
    --inputs)
        printf '%s\n' "${inputs[@]}"
        exit 0
        ;;
    *)
        echo "usage: ensure-xcframework.sh [--inputs]" >&2
        exit 2
        ;;
esac

default_cache="$HOME/Library/Caches/midgard-build"
cache="${MIDGARD_BUILD_CACHE:-$default_cache}"
case "$cache" in
    /*) ;;
    *)
        echo "$me: MIDGARD_BUILD_CACHE must be an absolute path" >&2
        exit 2
        ;;
esac
# Test-only variables never act on the real cache.
if [ "$cache" = "$default_cache" ] && [ -n "$(compgen -v MIDGARD_TEST_ || true)" ]; then
    echo "error: $me: $(compgen -v MIDGARD_TEST_ | tr '\n' ' ')is set: MIDGARD_TEST_* variables are for the tests only, and need a scratch MIDGARD_BUILD_CACHE" >&2
    exit 2
fi
[ "$cache" = "$default_cache" ] || echo "warning: $me: cache root $cache (MIDGARD_BUILD_CACHE)"
timeout=1800
if [ -n "${MIDGARD_TEST_LOCK_TIMEOUT:-}" ]; then
    case "$MIDGARD_TEST_LOCK_TIMEOUT" in
        *[!0-9]*) die "MIDGARD_TEST_LOCK_TIMEOUT must be a number of seconds" ;;
    esac
    timeout="$MIDGARD_TEST_LOCK_TIMEOUT"
    echo "warning: $me: TEST MODE: lock timeout ${timeout}s (MIDGARD_TEST_LOCK_TIMEOUT)"
fi
toolchain_path="$rustup_bin:$PATH"
if [ -n "${MIDGARD_TEST_TOOLCHAIN_BIN:-}" ]; then
    toolchain_path="$MIDGARD_TEST_TOOLCHAIN_BIN:$toolchain_path"
    echo "warning: $me: TEST MODE: toolchain from $MIDGARD_TEST_TOOLCHAIN_BIN first (MIDGARD_TEST_TOOLCHAIN_BIN)"
fi
keep=3

bystamp="$cache/clients-xcframework-by-stamp"
lock="$cache/clients-xcframework.lock"
link="$apple/Frameworks/MidgardCoreFFI.xcframework"
bindings="$apple/Sources/MidgardCore/midgard_client_ffi.swift"

# The build's environment: clean, so what Xcode sets for the app (SDKROOT, ARCHS,
# *_DEPLOYMENT_TARGET, CC...) never reaches cargo; build-xcframework.sh sets what it needs.
env_keep=(HOME="$HOME" PATH="$toolchain_path" TMPDIR="${TMPDIR:-/tmp}")
for v in USER LOGNAME LANG LC_ALL MIDGARD_BUILD_CACHE DEVELOPER_DIR RUSTUP_HOME CARGO_HOME; do
    if [ -n "${!v:-}" ]; then env_keep+=("$v=${!v}"); fi
done

work="$(mktemp -d "${TMPDIR:-/tmp}/ensure-xcframework.XXXXXX")"
lock_owned=0
child=""
cleanup() {
    if [ "$lock_owned" = 1 ] && [ "$(head -n 1 "$lock/owner" 2>/dev/null || true)" = "$$" ]; then
        rm -rf "$lock"
    fi
    rm -rf "$work"
}
# The build runs in its own process group (see below): a signal stops all of it, cargo
# and rustc included.
on_signal() {
    if [ -n "$child" ]; then
        kill -TERM -- "-$child" 2>/dev/null || kill -TERM "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
    fi
    exit "$1"
}
trap cleanup EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

sha256() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 | cut -d' ' -f1
    else
        sha256sum | cut -d' ' -f1
    fi
}

# git on a throwaway index and object directory: the repository's own are never written.
tgit() {
    GIT_INDEX_FILE="$work/index" GIT_OBJECT_DIRECTORY="$work/objects" \
        GIT_ALTERNATE_OBJECT_DIRECTORIES="$git_objects" git -C "$root" "$@"
}

# Sets $stamp, or returns 1 with the reason in $why. Every step's status is checked:
# set -e does not reach into a function called from `if`.
compute_stamp() {
    local f="$work/stamp-input" err="$work/err" common i present=() d
    stamp=""
    why=""
    if ! common="$(cd "$root" 2>"$err" && cd "$(git rev-parse --git-common-dir 2>"$err")" 2>>"$err" && pwd)"; then
        why="git rev-parse failed: $(head -n 1 "$err")"
        return 1
    fi
    git_objects="$common/objects"
    rm -rf "$work/index" "$work/objects"
    mkdir -p "$work/objects"
    echo "$me stamp v2" >"$f"
    if ! tgit read-tree HEAD 2>"$err"; then
        why="git read-tree HEAD failed: $(head -n 1 "$err")"
        return 1
    fi
    # A pathspec git does not know (neither on disk nor in HEAD) would fail `git add`.
    for i in "${inputs[@]}"; do
        if [ -e "$root/$i" ] || [ -n "$(tgit ls-files -- "$i" 2>/dev/null | head -n 1)" ]; then
            present+=("$i")
        fi
    done
    [ "${#present[@]}" -gt 0 ] || {
        why="none of the inputs is in the checkout"
        return 1
    }
    if ! tgit add -A -- "${present[@]}" 2>"$err"; then
        why="git add failed: $(head -n 1 "$err")"
        return 1
    fi
    if ! tgit ls-files -s -z -- "${present[@]}" >>"$f" 2>"$err"; then
        why="git ls-files failed: $(head -n 1 "$err")"
        return 1
    fi
    echo "toolchain:" >>"$f"
    if ! (cd "$root" && env -i "${env_keep[@]}" rustc -vV && env -i "${env_keep[@]}" cargo -V) >>"$f" 2>"$err"; then
        why="rustc -vV or cargo -V failed: $(head -n 1 "$err")"
        return 1
    fi
    echo "cargo configuration:" >>"$f"
    for i in "${CARGO_HOME:-$HOME/.cargo}/config" "${CARGO_HOME:-$HOME/.cargo}/config.toml"; do
        if [ -f "$i" ]; then
            printf '%s\n' "$i" >>"$f"
            cat "$i" >>"$f" || {
                why="cannot read $i"
                return 1
            }
        fi
    done
    d="$(dirname "$root")"
    while :; do
        for i in "$d/.cargo/config" "$d/.cargo/config.toml"; do
            if [ -f "$i" ]; then
                printf '%s\n' "$i" >>"$f"
                cat "$i" >>"$f" || {
                    why="cannot read $i"
                    return 1
                }
            fi
        done
        [ "$d" != / ] || break
        d="$(dirname "$d")"
    done
    if ! stamp="$(sha256 <"$f")" || [ -z "$stamp" ]; then
        why="sha256 failed"
        stamp=""
        return 1
    fi
}

entry_ok() {
    [ -d "$1/MidgardCoreFFI.xcframework" ] && [ -f "$1/swift/midgard_client_ffi.swift" ] &&
        [ "$(cat "$1/inputs.sha256" 2>/dev/null || true)" = "$(basename "$1")" ]
}

# Link the checkout to a build and mark the build used (its mtime orders pruning).
use_entry() {
    touch "$1"
    mkdir -p "$apple/Frameworks" "$apple/Sources/MidgardCore"
    if [ "$(readlink "$link" 2>/dev/null || true)" != "$1/MidgardCoreFFI.xcframework" ]; then
        rm -rf "$link"
        ln -s "$1/MidgardCoreFFI.xcframework" "$link"
        echo "$me: linked $link"
    fi
    if ! cmp -s "$1/swift/midgard_client_ffi.swift" "$bindings"; then
        cp "$1/swift/midgard_client_ffi.swift" "$bindings"
        echo "$me: copied the bindings into Sources/MidgardCore"
    fi
}

# --- the lock ---------------------------------------------------------------------------
# In UTC and the C locale, so that runs with different TZ or LANG agree.
proc_start() { TZ=UTC0 LC_ALL=C ps -o lstart= -p "$1" 2>/dev/null | sed -e 's/^ *//' -e 's/ *$//'; }
# record <file> <pid>: pid and process start time, written atomically. Returns 1 when
# either cannot be written (the caller decides; set -e does not reach here from `if`).
record() {
    local start
    start="$(proc_start "$2")"
    [ -n "$start" ] || return 1
    printf '%s\n%s\n' "$2" "$start" >"$1.tmp.$$" 2>/dev/null || return 1
    mv "$1.tmp.$$" "$1" 2>/dev/null || return 1
}
# alive <record file>: the recorded process (same pid, same start time) is running.
# 0 alive, 1 not running, 2 no readable record.
alive() {
    local pid start
    [ -f "$1" ] || return 2
    pid="$(sed -n 1p "$1" 2>/dev/null || true)"
    start="$(sed -n 2p "$1" 2>/dev/null || true)"
    case "$pid" in '' | *[!0-9]*) return 2 ;; esac
    [ -n "$start" ] || return 2
    [ "$(proc_start "$pid")" = "$start" ] && return 0
    return 1
}
# lock_stale <dir>: neither the holder nor its build is running; or no owner record, and
# the directory is over a minute old.
lock_stale() {
    local o=0 b=0
    alive "$1/owner" || o=$?
    alive "$1/child" || b=$?
    [ "$o" != 0 ] && [ "$b" != 0 ] || return 1
    [ "$o" = 1 ] && return 0
    [ -n "$(find "$1" -maxdepth 0 -mmin +1 2>/dev/null)" ]
}
lock_holder() { head -n 1 "$lock/owner" 2>/dev/null || echo unknown; }

acquire_lock() {
    mkdir -p "$cache"
    local waited=0 said="-" holder aside before
    while :; do
        if mkdir "$lock" 2>/dev/null; then
            # Ours only once the owner record is written; otherwise remove it and retry.
            if record "$lock/owner" "$$"; then
                lock_owned=1
                return 0
            fi
            rm -rf "$lock"
            echo "warning: $me: could not write the lock's owner record; retrying"
            [ "$waited" -lt "$timeout" ] || die "gave up after ${timeout}s: cannot write $lock/owner"
            sleep 1
            waited=$((waited + 1))
            continue
        fi
        holder="$(lock_holder)"
        if [ -d "$lock" ] && lock_stale "$lock"; then
            # Move it aside (atomic), so two waiters cannot both delete it, and check it
            # again there: it is deleted only if it is still the stale lock just seen.
            before="$(cat "$lock/owner" "$lock/child" 2>/dev/null || true)"
            aside="$lock.stale.$$"
            if mv "$lock" "$aside" 2>/dev/null; then
                if [ "$(cat "$aside/owner" "$aside/child" 2>/dev/null || true)" = "$before" ] && lock_stale "$aside"; then
                    rm -rf "$aside"
                    echo "$me: removed a stale lock (pid $holder is not running)"
                elif [ ! -e "$lock" ] && mv "$aside" "$lock" 2>/dev/null && [ ! -e "$lock/$(basename "$aside")" ]; then
                    : # put back
                else
                    [ ! -e "$lock/$(basename "$aside")" ] || mv "$lock/$(basename "$aside")" "$aside" 2>/dev/null || true
                    echo "$me: a lock changed while it was being reclaimed; left it at $aside" >&2
                fi
            fi
            continue
        fi
        if [ "$waited" -ge "$timeout" ]; then
            die "gave up after ${timeout}s waiting for another build (pid $holder, lock $lock)"
        fi
        if [ "$said" != "$holder" ]; then
            echo "$me: waiting for another build of the framework (pid $holder) to finish"
            said="$holder"
        fi
        sleep 1
        waited=$((waited + 1))
    done
}

# --- pruning ----------------------------------------------------------------------------
# Keep the $keep most recently used builds, the one just linked, every build a worktree
# of this repository links, and every build used in the last 10 minutes (a concurrent run
# may be between finding it current and linking it).
prune() {
    local protect=" $(basename "$1") " wt t n count=0
    while IFS= read -r wt; do
        case "$wt" in "worktree "*) ;; *) continue ;; esac
        t="$(readlink "${wt#worktree }/clients/shells/apple/Frameworks/MidgardCoreFFI.xcframework" 2>/dev/null || true)"
        case "$t" in "$bystamp"/*/MidgardCoreFFI.xcframework)
            t="${t#"$bystamp"/}"
            protect="$protect${t%%/*} "
            ;;
        esac
    done <<<"$(git -C "$root" worktree list --porcelain 2>/dev/null || true)"
    for n in $(ls -1t "$bystamp"); do
        count=$((count + 1))
        [ "$count" -gt "$keep" ] || continue
        case "$protect" in *" $n "*) continue ;; esac
        [ -z "$(find "$bystamp/$n" -maxdepth 0 -mmin -10 2>/dev/null)" ] || continue
        if mv "$bystamp/$n" "$bystamp/.trash.$n.$$" 2>/dev/null; then
            rm -rf "$bystamp/.trash.$n.$$"
            echo "$me: pruned the build $n"
        fi
    done
}

# --- main -------------------------------------------------------------------------------
if ! compute_stamp; then
    echo "$me: cannot compute the source stamp ($why): building"
    stamp=""
fi
entry="$bystamp/${stamp:-none}"
# Mark it used before checking it, so a concurrent prune keeps it.
[ -z "$stamp" ] || touch -c "$entry" 2>/dev/null || true
if [ -n "$stamp" ] && entry_ok "$entry"; then
    use_entry "$entry"
    echo "$me: current (${stamp:0:12}): $entry/MidgardCoreFFI.xcframework"
    exit 0
fi

acquire_lock
if [ -n "$stamp" ] && entry_ok "$entry"; then
    use_entry "$entry"
    echo "$me: current (${stamp:0:12}), built by another run: $entry/MidgardCoreFFI.xcframework"
    exit 0
fi

echo "$me: sources changed or no build yet: building"
mkdir -p "$bystamp"
# Holding the lock with no other build running: what is left in there is a dead build's.
rm -rf "$bystamp"/.tmp.* "$bystamp"/.trash.*
# A failed build leaves nothing linked.
rm -rf "$link" "$bindings"
name="${stamp:-nostamp-$(date +%Y%m%d%H%M%S)-$$}"
tmpd="$bystamp/.tmp.$name.$$"
# Its own process group (job control on for the fork), so that a signal reaches cargo too.
set -m
env -i "${env_keep[@]}" /bin/bash "$here/build-xcframework.sh" --out "$tmpd" &
child=$!
set +m
record "$lock/child" "$child" || echo "warning: $me: could not record the build's pid in the lock"
rc=0
wait "$child" || rc=$?
child=""
rm -f "$lock/child"
[ "$rc" = 0 ] || die "build-xcframework.sh failed (exit $rc)"
[ -d "$tmpd/MidgardCoreFFI.xcframework" ] && [ -f "$tmpd/swift/midgard_client_ffi.swift" ] ||
    die "build-xcframework.sh did not write $tmpd"
# The stamp of the sources as they were when the build started: an edit made during the
# build makes the next run build again, never the reverse.
printf '%s\n' "$name" >"$tmpd/inputs.sha256"
entry="$bystamp/$name"
if [ -e "$entry" ]; then
    mv "$entry" "$bystamp/.trash.$name.$$"
    rm -rf "$bystamp/.trash.$name.$$"
fi
mv "$tmpd" "$entry"
use_entry "$entry"
prune "$entry"
echo "$me: built (${stamp:0:12}): $entry/MidgardCoreFFI.xcframework"
