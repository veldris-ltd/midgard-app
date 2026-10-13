#!/usr/bin/env bash
# /vendor is exactly what Cargo.lock names. [NFR-PRV-1] [NFR-OSS-1] [AC-M-5]
# [AC-RF-CA-3b] [AC-RF-SC-7a]
#
#   bash tools/ci/vendor-check.sh
#
# The offline build rests on two files agreeing: Cargo.lock says which crates the
# workspace needs, and /vendor holds them. This checks, without building anything:
#
#   1. .cargo/config.toml replaces crates.io with the vendored directory, so Cargo
#      cannot reach a registry even when one is reachable;
#   2. every registry package in Cargo.lock is in /vendor at the locked version, and
#      the package checksum recorded there is the locked one;
#   3. /vendor holds no crate the lock does not name.
#
# It does not re-hash each vendored file: Cargo does that itself, against the same
# .cargo-checksum.json, whenever it compiles a vendored crate, and fails the build on
# a mismatch. What Cargo never notices is a crate that is present but unused, or a
# missing crate until the one build that needs it. Those are caught here, by name.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/lib.sh"
cd "${MIDGARD_VENDOR_ROOT:-$here/../..}"

# Directories under /vendor that are not crates: the advisory databases mg-scan reads.
NOT_CRATES='advisory-db advisory-db-npm'

status=0
problem() {
    ci_error "vendor-check: $1"
    status=1
}

for f in Cargo.lock .cargo/config.toml; do
    [ -f "$f" ] || { problem "$f is missing"; exit 1; }
done
[ -d vendor ] || { problem "vendor/ is missing"; exit 1; }

# 1. The source replacement, read with comments stripped.
config="$(tr -d '\r' < .cargo/config.toml | sed 's/#.*//')"
replacement="$(awk '
    /^\[/ { section = $0 }
    section == "[source.crates-io]" && /^ *replace-with *=/ { gsub(/.*= *"|".*/, ""); print }' <<<"$config")"
if [ -z "$replacement" ]; then
    problem ".cargo/config.toml does not replace the crates-io source"
else
    directory="$(awk -v want="[source.$replacement]" '
        /^\[/ { section = $0 }
        section == want && /^ *directory *=/ { gsub(/.*= *"|".*/, ""); print }' <<<"$config")"
    [ "$directory" = vendor ] \
        || problem ".cargo/config.toml replaces crates-io with '$replacement', which is not the vendor directory"
fi

# 2. name version checksum, for each package the lock takes from a registry.
locked="$(tr -d '\r' < Cargo.lock | awk '
    function emit() { if (source ~ /^registry\+/) print name, version, checksum }
    /^\[\[package\]\]/ { emit(); name = version = source = checksum = "" }
    /^name = /     { gsub(/^name = "|"$/, "");     name = $0 }
    /^version = /  { gsub(/^version = "|"$/, "");  version = $0 }
    /^source = /   { gsub(/^source = "|"$/, "");   source = $0 }
    /^checksum = / { gsub(/^checksum = "|"$/, ""); checksum = $0 }
    END { emit() }')"

count=0
expected=""
while read -r name version checksum; do
    [ -n "$name" ] || continue
    count=$((count + 1))
    dir="vendor/$name-$version"
    expected="$expected$name-$version"$'\n'
    if [ ! -f "$dir/.cargo-checksum.json" ]; then
        problem "$name $version is locked but not vendored (expected $dir)"
        continue
    fi
    got="$(tr -d '\r\n' < "$dir/.cargo-checksum.json" | sed -n 's/.*"package" *: *"\([0-9a-f]*\)".*/\1/p')"
    if [ -z "$checksum" ]; then
        problem "$name $version has no checksum in Cargo.lock"
    elif [ "$got" != "$checksum" ]; then
        problem "$name $version: vendored package checksum ${got:-<none>} is not the locked $checksum"
    fi
done <<<"$locked"

# 3. Nothing in /vendor the lock does not name.
for dir in vendor/*/; do
    base="$(basename "$dir")"
    case " $NOT_CRATES " in *" $base "*) continue ;; esac
    grep -qxF "$base" <<<"$expected" || problem "vendor/$base is not named by Cargo.lock"
done

if [ "$status" -eq 0 ]; then
    echo "vendor-check: $count locked registry package(s), all vendored at the locked version and checksum; nothing extra."
fi
exit "$status"
