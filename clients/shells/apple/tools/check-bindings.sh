#!/usr/bin/env bash
# The MPL-2.0 items of the Apple release checklist (clients/shells/SHELL_BUILD.md).
# [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/check-bindings.sh
#
# Run after build-xcframework.sh. Checks:
#   1. a fresh uniffi-bindgen run over the built macOS arm64 library writes exactly the
#      bindings that were built (clients-xcframework/swift/) and linked into the package
#      (Sources/MidgardCore/midgard_client_ffi.swift): the generated MPL-2.0 files are
#      UniFFI's output, never edited (MPL-2.0 section 3.2);
#   2. MidgardKit's Acknowledgements carries the UniFFI line the apps show;
#   3. the UniFFI licence exceptions in supply-chain/licences.toml have not expired
#      (an expired exception fails the release licence gate); within 60 days of the
#      expiry it warns, so the renewal is decided before a release needs it.
#
# Environment: MIDGARD_BUILD_CACHE (absolute; default ~/Library/Caches/midgard-build),
# CARGO (default cargo), MIDGARD_TODAY (YYYY-MM-DD, default today; for the tests).
set -euo pipefail
cd "$(dirname "$0")/../../../.."
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
case "$cache" in
    /*) ;;
    *)
        echo "check-bindings: MIDGARD_BUILD_CACHE must be an absolute path" >&2
        exit 2
        ;;
esac
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export CARGO_TARGET_DIR="$cache/target-clients"
cargo="${CARGO:-cargo}"
built="$cache/clients-xcframework/swift"
lib="$CARGO_TARGET_DIR/aarch64-apple-darwin/release/libmidgard_client_ffi.a"
pkg=clients/shells/apple/Sources/MidgardCore/midgard_client_ffi.swift
ack='UniFFI 0.28.3, Mozilla Public License 2.0, source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3'
failures=0
ok() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1" >&2
    failures=$((failures + 1))
}
if [ ! -f "$lib" ] || [ ! -d "$built" ]; then
    echo "check-bindings: no built library or bindings (run clients/shells/apple/tools/build-xcframework.sh first)" >&2
    exit 2
fi
fresh="$(mktemp -d)"
trap 'rm -rf "$fresh"' EXIT

# 1. A fresh generation equals what was built and linked.
"$cargo" run --offline --locked -q -p midgard-client-ffi --example uniffi-bindgen -- --library "$lib" --out "$fresh"
for g in midgard_client_ffi.swift midgard_client_ffiFFI.h module.modulemap; do
    if [ -s "$fresh/$g" ] && cmp -s "$fresh/$g" "$built/$g"; then
        ok "$g: the built copy is a fresh generation's"
    else
        fail "$g: $built/$g differs from a fresh uniffi-bindgen run"
    fi
done
if [ -f "$pkg" ] && cmp -s "$fresh/midgard_client_ffi.swift" "$pkg"; then
    ok "$pkg is a fresh generation's"
else
    fail "$pkg is missing or differs from a fresh uniffi-bindgen run"
fi

# 2. The acknowledgement.
if grep -qF "\"$ack\"" clients/shells/apple/Sources/MidgardKit/Presentation/Notices.swift 2>/dev/null; then
    ok "the UniFFI acknowledgement is in MidgardKit's Acknowledgements"
else
    fail "the UniFFI acknowledgement (\"$ack\") is missing from MidgardKit/Presentation/Notices.swift"
fi

# 3. The UniFFI licence exceptions' expiry (the earliest, if they differ).
today="${MIDGARD_TODAY:-$(date +%Y-%m-%d)}"
# BSD date (macOS), then GNU date; a date that can do neither skips only the warning.
soon="$(date -j -v+60d -f %Y-%m-%d "$today" +%Y-%m-%d 2>/dev/null ||
    date -d "$today + 60 days" +%Y-%m-%d 2>/dev/null || echo "$today")"
expires="$(awk '/^\[exceptions\."uniffi/ { u = 1; next } /^\[/ { u = 0 } u && /^expires/ { gsub(/[" ]/, ""); sub(/^expires=/, ""); print }' \
    supply-chain/licences.toml | sort | head -1)"
bad_dates="$(awk '/^\[exceptions\."uniffi/ { u = 1; next } /^\[/ { u = 0 } u && /^expires/ { v = $0; gsub(/[" ]/, "", v); sub(/^expires=/, "", v); if (v !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/) print v }' \
    supply-chain/licences.toml)"
if [ -z "$expires" ]; then
    fail "supply-chain/licences.toml has no UniFFI exception with an expiry"
elif [ -n "$bad_dates" ]; then
    # A malformed date would compare as a string and could pass as "not expired".
    fail "a UniFFI exception's expiry is not YYYY-MM-DD: $(printf '%s ' $bad_dates)(supply-chain/licences.toml)"
elif [[ "$today" > "$expires" ]]; then
    fail "the UniFFI licence exceptions expired on $expires: renew or remove them (supply-chain/licences.toml)"
elif [[ "$soon" > "$expires" ]]; then
    echo "warn  the UniFFI licence exceptions expire on $expires: decide the renewal now (supply-chain/licences.toml)"
else
    ok "the UniFFI licence exceptions run until $expires"
fi

[ "$failures" -eq 0 ] || exit 1
echo "check-bindings: the bindings are UniFFI's own, acknowledged, and licensed until $expires"
