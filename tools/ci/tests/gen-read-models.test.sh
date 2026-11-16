#!/usr/bin/env bash
# The read-model generator refuses every TypeScript shape it does not consume, and
# every type name declared twice with different text, instead of generating Rust
# that silently drops it. [FR-C-1] [AC-C-13]
#
#   bash tools/ci/tests/gen-read-models.test.sh
#
# Each case copies the contract (sdk/ts/src/types.ts and the five product files)
# into a scratch tree, appends one declaration to one file, and runs the generator
# over that tree with --src-root. Without node it skips, so the selftest job, which
# has no node, still passes. The positive control also needs the pinned rustfmt on
# PATH; without it that one case skips.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
src="$(cd "$here/../../.." && pwd)"
gen="$src/clients/demo/tools/gen-read-models.mjs"
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

if ! command -v node >/dev/null 2>&1; then
    echo "skip  gen-read-models: node is not on PATH"
    exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# A fresh copy of the contract in "$tmp/case".
fresh() {
    rm -rf "$tmp/case"
    mkdir -p "$tmp/case/sdk/ts/src/products"
    cp "$src/sdk/ts/src/types.ts" "$tmp/case/sdk/ts/src/types.ts"
    for p in admin bifrost forge heimdall mimir; do
        cp "$src/sdk/ts/src/products/$p.ts" "$tmp/case/sdk/ts/src/products/$p.ts"
    done
}

# refused <name> <file under sdk/ts/src> <expected message fragment> <declaration>
refused() {
    fresh
    printf '\n%s\n' "$4" >> "$tmp/case/sdk/ts/src/$2"
    if out="$(node "$gen" --check --src-root "$tmp/case" 2>&1)"; then
        fail "$1: the generator accepted it"
    elif ! printf '%s' "$out" | grep -qF -- "$3"; then
        fail "$1: refused, but without \"$3\": $out"
    else
        pass "$1"
    fi
}

# Positive control: the unchanged contract passes --check, the UniFFI mirrors
# included (the cases above fail for their own reason, not because the scratch tree
# is unreadable).
if command -v rustfmt >/dev/null 2>&1; then
    fresh
    if out="$(node "$gen" --check --ffi-out clients/ffi/src/generated --src-root "$tmp/case" 2>&1)"; then
        pass "the unchanged contract is up to date"
    else
        fail "the unchanged contract: $out"
    fi
else
    echo "skip  positive control: rustfmt is not on PATH"
fi

refused "an interface that extends another" products/admin.ts \
    'not consumed: interface AdminExtra' \
    'export interface AdminExtra extends AdminApproval { more: string; }'
refused "a generic interface" products/forge.ts \
    'not consumed: interface ForgePage' \
    'export interface ForgePage<T> { items: T[]; }'
refused "an interface that is not exported" products/mimir.ts \
    'not consumed: interface MimirHidden' \
    'interface MimirHidden { a: string; }'
refused "an enum" products/heimdall.ts \
    'not consumed: enum HeimdallMode' \
    'export enum HeimdallMode { Live, Replay }'
refused "a const enum" types.ts \
    'not consumed: enum Shade' \
    'export const enum Shade { Dark }'
refused "an object type alias" products/bifrost.ts \
    'not consumed: type BifrostShape' \
    'export type BifrostShape = { a: string; b: number };'
refused "a generic type alias" products/bifrost.ts \
    'not consumed: type BifrostBox' \
    'export type BifrostBox<T> = T[];'
refused "an unsupported member type" products/admin.ts \
    'unsupported type "Map<string, number>"' \
    'export interface AdminCounts { byRole: Map<string, number>; }'
refused "an alias declared twice with different text" products/forge.ts \
    'declared twice: MarkingLevel' \
    "export type MarkingLevel = 'open' | 'secret';"
refused "an interface declared twice" products/mimir.ts \
    'declared twice: AuditRecord' \
    'export interface AuditRecord { id: string; }'

# A class and a function are allowed: they are behaviour, not contract shapes.
fresh
printf '\n%s\n%s\n' 'export class AdminOops extends Error {}' \
    'export function adminHelper(x: string): string { return x; }' \
    >> "$tmp/case/sdk/ts/src/products/admin.ts"
if command -v rustfmt >/dev/null 2>&1; then
    if out="$(node "$gen" --check --src-root "$tmp/case" 2>&1)"; then
        pass "a class and a function are allowed"
    else
        fail "a class and a function were refused: $out"
    fi
fi

if [ "$failures" -ne 0 ]; then
    echo "gen-read-models.test: $failures failure(s)"
    exit 1
fi
echo "gen-read-models.test: all cases pass"
