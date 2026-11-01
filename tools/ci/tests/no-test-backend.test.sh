#!/usr/bin/env bash
# tools/ci/no-test-backend.sh fails a build graph that ships the test backend.
# [RT-08] [MG-SEC-15]
#
#   bash tools/ci/tests/no-test-backend.test.sh
#
# Runs on the host, without cargo: each case hands the check a `cargo tree` listing
# through MIDGARD_CARGO_TREE.
set -uo pipefail

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
check="$repo_root/tools/ci/no-test-backend.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

run() {
    printf '%s\n' "$2" > "$work/tree"
    MIDGARD_CARGO_TREE="$work/tree" bash "$check" > "$work/out" 2>&1
    local rc=$?
    if [ "$rc" -eq "$3" ]; then pass "$1"; else fail "$1 (exit $rc)"; sed 's/^/      /' "$work/out"; fi
}

run "a FIPS-only graph passes" 'gateway v0.1.0-rc.1 (/work/services/gateway)
pqc-agility feature "backend-fips"
pqc-agility feature "default"
pqc-agility v0.1.0-rc.1 (/work/crypto/pqc-agility)' 0

run "the test backend anywhere in the graph fails" 'pqc-agility feature "backend-fips"
pqc-agility feature "test-backend"
pqc-agility v0.1.0-rc.1 (/work/crypto/pqc-agility)' 1

run "a crypto crate forwarding the test backend fails" 'pqc-agility feature "backend-fips"
audit-writer feature "test-backend"
audit-writer v0.1.0-rc.1 (/work/crypto/audit-writer)' 1

run "a graph with no FIPS backend at all fails" 'pqc-agility feature "default"
pqc-agility v0.1.0-rc.1 (/work/crypto/pqc-agility)' 1

run "a test named for the backend is not mistaken for the feature" 'pqc-agility feature "backend-fips"
some-crate v1.0.0 (test-backend-helpers)' 0

[ "$failures" -eq 0 ] || exit 1
