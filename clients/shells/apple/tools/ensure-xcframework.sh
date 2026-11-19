#!/usr/bin/env bash
# Make sure this checkout's Frameworks/MidgardCoreFFI.xcframework links to a build of
# the Rust sources as they stand, building only when needed. The app projects' pre-build
# step. [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/ensure-xcframework.sh [--inputs]
#
# "Current" is decided by content, never by modification times (a fresh checkout makes
# every file new): the stamp is a SHA-256 over `git ls-files -s` of the inputs, `git diff
# HEAD` of the inputs (so uncommitted edits count) and the content of untracked,
# not-ignored files among them. It is written to clients-xcframework/inputs.sha256, beside
# the framework it describes, after build-xcframework.sh has built and verified it. The
# stamp lives inside the directory build-xcframework.sh replaces, so it can never outlive
# its framework, and clean.sh removes it with the framework.
#
# The inputs are everything the Rust build of clients/ffi reads (`--inputs` prints them):
# the workspace files, clients/ffi and its path dependencies (the normal and build
# dependency closure; `cargo tree -p midgard-client-ffi -e normal,build` lists the same
# set, and tools/ci/tests/apple-ensure-xcframework.test.sh holds the two together), the
# demo trust anchors clients/core includes with include_str!, and the build and verify
# scripts themselves. vendor/ is left out: Cargo.lock pins every vendored crate.
#
# The cache is shared by every checkout (worktree) on this Mac, so two checkouts with the
# same inputs share one build. Two app builds may call this at once: a lock directory
# (clients-xcframework.lock, holding the holder's pid) serialises the builds; a waiter
# re-checks the stamp once it holds the lock, a lock whose holder is gone is reclaimed,
# and the wait gives up after 30 minutes.
#
# Xcode run-script phases start with a minimal PATH and an environment meant for the app
# (SDKROOT, ARCHS, deployment targets...): rustup's directory is put on PATH here, and the
# build runs in a clean environment. Paths are resolved from this script's location, so
# the working directory does not matter. MIDGARD_BUILD_CACHE (an absolute path) moves the
# cache root, as for build-xcframework.sh. MIDGARD_TEST_LOCK_TIMEOUT (seconds) is for the
# tests only, and the script says so when it is set.
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin${PATH:+:$PATH}"
# /usr/bin/git is an xcrun shim: an SDKROOT Xcode set for the app (or a stale one) can
# break it.
unset SDKROOT
me=ensure-xcframework
here="$(cd "$(dirname "$0")" && pwd)"
apple="$(cd "$here/.." && pwd)"
root="$(cd "$apple/../../.." && pwd)"
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
[ "$cache" = "$default_cache" ] || echo "$me: cache root $cache (MIDGARD_BUILD_CACHE)"
timeout=1800
if [ -n "${MIDGARD_TEST_LOCK_TIMEOUT:-}" ]; then
    case "$MIDGARD_TEST_LOCK_TIMEOUT" in
        *[!0-9]*) die "MIDGARD_TEST_LOCK_TIMEOUT must be a number of seconds" ;;
    esac
    timeout="$MIDGARD_TEST_LOCK_TIMEOUT"
    echo "$me: TEST MODE: lock timeout ${timeout}s (MIDGARD_TEST_LOCK_TIMEOUT)"
fi

out="$cache/clients-xcframework"
stamp_file="$out/inputs.sha256"
lock="$cache/clients-xcframework.lock"
link="$apple/Frameworks/MidgardCoreFFI.xcframework"
bindings="$apple/Sources/MidgardCore/midgard_client_ffi.swift"

sha256() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 | cut -d' ' -f1
    else
        sha256sum | cut -d' ' -f1
    fi
}

# Prints the stamp, or nothing when this is not a git checkout.
compute_stamp() {
    git -C "$root" rev-parse --verify -q HEAD >/dev/null 2>&1 || return 0
    {
        echo "$me stamp v1"
        git -C "$root" ls-files -s -- "${inputs[@]}"
        # --full-index: the patch names the full HEAD blob it applies to, so the stamp
        # determines the working-tree content.
        git -C "$root" -c core.quotepath=off diff --no-ext-diff --no-textconv --no-color \
            --binary --full-index HEAD -- "${inputs[@]}"
        local untracked
        untracked="$(git -C "$root" -c core.quotepath=off ls-files -o --exclude-standard -- "${inputs[@]}")"
        if [ -n "$untracked" ]; then
            echo "untracked:"
            printf '%s\n' "$untracked"
            printf '%s\n' "$untracked" | (cd "$root" && git hash-object --stdin-paths)
        fi
    } | sha256
}

is_current() {
    [ -n "$stamp" ] && [ -f "$stamp_file" ] && [ "$(cat "$stamp_file")" = "$stamp" ] &&
        [ -d "$out/MidgardCoreFFI.xcframework" ] && [ -f "$out/swift/midgard_client_ffi.swift" ]
}

# What build-xcframework.sh links into the package, for a build it made earlier (from
# this checkout or another).
relink() {
    mkdir -p "$apple/Frameworks" "$apple/Sources/MidgardCore"
    if [ "$(readlink "$link" 2>/dev/null || true)" != "$out/MidgardCoreFFI.xcframework" ]; then
        rm -rf "$link"
        ln -s "$out/MidgardCoreFFI.xcframework" "$link"
        echo "$me: linked $link"
    fi
    if ! cmp -s "$out/swift/midgard_client_ffi.swift" "$bindings"; then
        cp "$out/swift/midgard_client_ffi.swift" "$bindings"
        echo "$me: copied the bindings into Sources/MidgardCore"
    fi
}

lock_owned=0
release() {
    if [ "$lock_owned" = 1 ] && [ "$(cat "$lock/pid" 2>/dev/null || true)" = "$$" ]; then
        rm -rf "$lock"
    fi
}
trap release EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# A lock is stale when its holder's pid is no longer running, or when it has no pid file
# a minute after it was made (its maker died between mkdir and writing the pid).
lock_is_stale() {
    local holder="$1"
    if [ -n "$holder" ]; then
        case "$holder" in *[!0-9]*) return 0 ;; esac
        ! ps -p "$holder" >/dev/null 2>&1
    else
        [ -n "$(find "$lock" -maxdepth 0 -mmin +1 2>/dev/null)" ]
    fi
}

acquire_lock() {
    mkdir -p "$cache"
    local waited=0 said="-" holder aside
    while ! mkdir "$lock" 2>/dev/null; do
        holder="$(cat "$lock/pid" 2>/dev/null || true)"
        if [ -d "$lock" ] && lock_is_stale "$holder"; then
            # Move it aside (atomic) before deleting, so two waiters cannot both delete;
            # if what was moved is not the stale lock just seen, put it back.
            aside="$lock.stale.$$"
            if mv "$lock" "$aside" 2>/dev/null; then
                if [ "$(cat "$aside/pid" 2>/dev/null || true)" = "$holder" ]; then
                    rm -rf "$aside"
                    echo "$me: removed a stale lock (pid ${holder:-unknown} is not running)"
                else
                    mv "$aside" "$lock" 2>/dev/null || rm -rf "$aside"
                fi
            fi
            continue
        fi
        if [ "$waited" -ge "$timeout" ]; then
            die "gave up after ${timeout}s waiting for another build (pid ${holder:-unknown}, lock $lock)"
        fi
        if [ "$said" != "$holder" ]; then
            echo "$me: waiting for another build of the framework (pid ${holder:-unknown}) to finish"
            said="$holder"
        fi
        sleep 1
        waited=$((waited + 1))
    done
    lock_owned=1
    echo "$$" >"$lock/pid"
}

stamp="$(compute_stamp)"
[ -n "$stamp" ] || echo "$me: not a git checkout: cannot tell whether the build is current, building"
if is_current; then
    relink
    echo "$me: current (${stamp:0:12}): $out/MidgardCoreFFI.xcframework"
    exit 0
fi

acquire_lock
if is_current; then
    relink
    echo "$me: current (${stamp:0:12}), built by another run: $out/MidgardCoreFFI.xcframework"
    exit 0
fi

echo "$me: sources changed or no build yet: building"
# A clean environment: what Xcode sets for the app (SDKROOT, ARCHS, *_DEPLOYMENT_TARGET,
# CC...) must not reach cargo; build-xcframework.sh sets what the build needs.
env_keep=(HOME="$HOME" PATH="$PATH" TMPDIR="${TMPDIR:-/tmp}")
for v in USER LOGNAME LANG LC_ALL MIDGARD_BUILD_CACHE DEVELOPER_DIR RUSTUP_HOME CARGO_HOME; do
    if [ -n "${!v:-}" ]; then env_keep+=("$v=${!v}"); fi
done
env -i "${env_keep[@]}" /bin/bash "$here/build-xcframework.sh" || die "build-xcframework.sh failed"
[ -d "$out/MidgardCoreFFI.xcframework" ] || die "build-xcframework.sh did not write $out/MidgardCoreFFI.xcframework"
# The stamp of the sources as they were when the build started: an edit made during the
# build makes the next run build again, never the reverse.
if [ -n "$stamp" ]; then
    printf '%s\n' "$stamp" >"$stamp_file.tmp.$$"
    mv "$stamp_file.tmp.$$" "$stamp_file"
fi
relink
echo "$me: built (${stamp:0:12}): $out/MidgardCoreFFI.xcframework"
