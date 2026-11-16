#!/usr/bin/env bash
# The demo fixtures are what the release's mocks serve, extracted deterministically,
# and the generated read models match the TypeScript contract. [AC-C-13] [FR-C-1]
#
#   bash tools/ci/tests/demo-fixtures.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
failures=0
pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

# The extractor and the generator are Node scripts; the selftest job has no node.
if ! command -v node >/dev/null 2>&1; then
    echo "skip  demo-fixtures: node is not on PATH"
    exit 0
fi

release="$(bash tools/ci/release-artefacts.sh --version)"
if bash clients/demo/tools/extract-fixtures.sh --check >/dev/null 2>&1; then
    pass "clients/demo/fixtures/$release is what the mocks serve"
else
    fail "clients/demo/fixtures/$release is stale: bash clients/demo/tools/extract-fixtures.sh"
fi

a="$(mktemp -d)"; b="$(mktemp -d)"
trap 'rm -rf "$a" "$b"' EXIT
bash clients/demo/tools/extract-fixtures.sh --out "$a" >/dev/null 2>&1
bash clients/demo/tools/extract-fixtures.sh --out "$b" >/dev/null 2>&1
if [ -s "$a/manifest.json" ] && diff -r "$a" "$b" >/dev/null; then
    pass "two extractions are byte-identical"
else
    fail "extraction is not deterministic"
fi

if node clients/demo/tools/gen-read-models.mjs --check >/dev/null 2>&1; then
    pass "the console read models match sdk/ts/src"
else
    fail "stale read models: node clients/demo/tools/gen-read-models.mjs --write"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
