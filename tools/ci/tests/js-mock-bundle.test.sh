#!/usr/bin/env bash
# The mock gateway and its fixtures stay out of the midgard console's http build.
# [AC-25.2] (VLD-ANB-MIDGARD-0086)
#
#   bash tools/ci/tests/js-mock-bundle.test.sh
#
# The console loads `@midgard/sdk/mock` by dynamic import in mock mode only, so a build
# with VITE_GATEWAY=http must hold no trace of it, and one with VITE_GATEWAY=mock must.
# The witness is AUD-1042, the id of the first seeded audit record in
# sdk/ts/src/mock/fixtures.ts: a string that exists in the mock fixtures and nowhere
# else in the workspace's sources.
#
# Needs pnpm and an installed workspace (`pnpm install --frozen-lockfile`); without
# them it skips, so the selftest job, which has no node, still passes. The js-build
# job, which has both, runs it.
set -uo pipefail

cd "$(dirname "$0")/../../.."
LITERAL=AUD-1042
DIST=products/midgard/ui/dist
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

if ! command -v pnpm >/dev/null 2>&1; then
    echo "skip  js-mock-bundle: pnpm is not on PATH"
    exit 0
fi
if [ ! -d products/midgard/ui/node_modules/vite ]; then
    echo "skip  js-mock-bundle: the workspace is not installed (pnpm install --frozen-lockfile)"
    exit 0
fi

# The witness must be fixture-only, or its absence proves nothing.
others="$(grep -rl --include='*.ts' --include='*.tsx' -- "$LITERAL" sdk ui products 2>/dev/null \
    | grep -v -e '/node_modules/' -e '/dist/' -e '^sdk/ts/src/mock/fixtures\.ts$' || true)"
if [ -z "$others" ] && grep -q -- "$LITERAL" sdk/ts/src/mock/fixtures.ts; then
    pass "$LITERAL appears in the mock fixtures and in no other source"
else
    fail "$LITERAL is not a fixture-only literal (also in: ${others:-nothing, and not in fixtures.ts})"
fi

# build MODE: build the console with VITE_GATEWAY=MODE; 0 when it built.
build() {
    rm -rf "$DIST"
    VITE_GATEWAY="$1" pnpm --filter @midgard/console-midgard build >/dev/null 2>&1
}

# bundled: 0 when the literal is in any emitted script.
bundled() { grep -l -- "$LITERAL" "$DIST"/assets/*.js >/dev/null 2>&1; }

if build http; then
    if bundled; then
        fail "the http build holds the mock fixtures ($LITERAL in $(grep -l -- "$LITERAL" "$DIST"/assets/*.js | tr '\n' ' '))"
    else
        pass "the http build holds no mock fixture"
    fi
else
    fail "the http build failed (VITE_GATEWAY=http pnpm --filter @midgard/console-midgard build)"
fi

if build mock; then
    if bundled; then
        pass "the mock build holds the mock fixtures"
    else
        fail "the mock build does not hold $LITERAL: the witness no longer tells the builds apart"
    fi
else
    fail "the mock build failed (VITE_GATEWAY=mock pnpm --filter @midgard/console-midgard build)"
fi
# Leave dist/ as a default (http) build would, so later steps see nothing of this test.
rm -rf "$DIST"

if [ "$failures" -ne 0 ]; then
    echo "js-mock-bundle: $failures failure(s)"
    exit 1
fi
echo "js-mock-bundle: all passed"
