#!/usr/bin/env bash
# Tests for tools/ci/vendor-check.sh, against throwaway trees.
# [AC-RF-CA-3b] [AC-RF-SC-7a]
#
#   bash tools/ci/tests/vendor-check.test.sh
#
# The claim under test: a crate the lock names but /vendor lacks, a crate /vendor
# holds but the lock does not name, a vendored crate whose checksum is not the locked
# one, and a Cargo configuration that could reach a registry each fail by name.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../vendor-check.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

SUM_A=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
SUM_B=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb

# tree NAME   a root with a workspace member, two locked registry crates, both vendored
tree() {
    local root="$work/$1"
    mkdir -p "$root/.cargo" "$root/vendor/alpha-1.0.0" "$root/vendor/beta-2.1.0+meta" "$root/vendor/advisory-db"
    cat > "$root/Cargo.lock" <<LOCK
version = 3

[[package]]
name = "alpha"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "$SUM_A"

[[package]]
name = "beta"
version = "2.1.0+meta"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "$SUM_B"
dependencies = [
 "alpha",
]

[[package]]
name = "member"
version = "0.1.0"
dependencies = [
 "beta",
]
LOCK
    printf '{"files":{"src/lib.rs":"00"},"package":"%s"}' "$SUM_A" > "$root/vendor/alpha-1.0.0/.cargo-checksum.json"
    printf '{"files":{},"package":"%s"}' "$SUM_B" > "$root/vendor/beta-2.1.0+meta/.cargo-checksum.json"
    printf '# pinned\n[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "vendor"\n' \
        > "$root/.cargo/config.toml"
    echo "$root"
}

run() { MIDGARD_VENDOR_ROOT="$1" bash "$script" 2>&1; }

# expect_fail LABEL ROOT TEXT   the check fails and names TEXT
expect_fail() {
    local out
    if out="$(run "$2")"; then
        fail "$1: passed, expected a failure"
    elif grep -qF -- "$3" <<<"$out"; then
        pass "$1"
    else
        fail "$1: failed without naming '$3': $out"
    fi
}

root="$(tree clean)"
if out="$(run "$root")" && grep -q '2 locked registry package' <<<"$out"; then
    pass "a tree that matches its lock passes, and workspace members are not expected in /vendor"
else
    fail "a matching tree: $out"
fi

root="$(tree missing)"
rm -rf "$root/vendor/alpha-1.0.0"
expect_fail "a locked crate missing from /vendor fails, named" "$root" "alpha 1.0.0 is locked but not vendored"

root="$(tree version)"
mv "$root/vendor/alpha-1.0.0" "$root/vendor/alpha-1.0.1"
expect_fail "a crate vendored at another version is not the locked one" "$root" "alpha 1.0.0 is locked but not vendored"
expect_fail "and the other version is reported as extra" "$root" "vendor/alpha-1.0.1 is not named by Cargo.lock"

root="$(tree extra)"
mkdir -p "$root/vendor/gamma-0.3.0"
expect_fail "a vendored crate the lock does not name fails, named" "$root" "vendor/gamma-0.3.0 is not named by Cargo.lock"

root="$(tree checksum)"
printf '{"files":{},"package":"%s"}' "$SUM_A" > "$root/vendor/beta-2.1.0+meta/.cargo-checksum.json"
expect_fail "a vendored crate with another package checksum fails" "$root" "beta 2.1.0+meta: vendored package checksum"

root="$(tree unpinned)"
printf '# [source.crates-io]\n# replace-with = "vendored-sources"\n' > "$root/.cargo/config.toml"
expect_fail "a commented-out source replacement fails" "$root" "does not replace the crates-io source"

root="$(tree elsewhere)"
printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "https://example.invalid/index"\n' \
    > "$root/.cargo/config.toml"
expect_fail "a replacement that is not the vendor directory fails" "$root" "which is not the vendor directory"

root="$(tree noconfig)"
rm "$root/.cargo/config.toml"
expect_fail "a missing Cargo configuration fails" "$root" ".cargo/config.toml is missing"

echo
if [ "$failures" -eq 0 ]; then
    echo "vendor-check: all tests passed"
else
    echo "vendor-check: $failures test(s) failed"
    exit 1
fi
