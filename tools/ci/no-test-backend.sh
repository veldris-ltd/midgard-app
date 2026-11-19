#!/usr/bin/env bash
# The insecure test backend never ships. [RT-08] [MG-SEC-15]
#
#   bash tools/ci/no-test-backend.sh
#
# `pqc-agility`'s `test-backend` feature is a deterministic, forgeable crypto backend
# for tests. Two compile-time guards already stop it reaching a release build: it
# cannot be built with `backend-fips`, and it cannot be built without
# `debug_assertions`. This is the CI half: it reads the workspace's build graph —
# normal and build dependencies only, every target, the features exactly as a
# `cargo build --workspace` would unify them — and fails if anything in it turns
# `test-backend` on, in any crypto crate. A crate's own dev-dependencies (which
# enable it for `cargo test`) are not part of that graph, so they are allowed.
#
# The same graph must never turn on `midgard-client-core`'s `demo-unpinned` feature,
# which opens a demo dataset under caller-chosen anchors or loads an unsigned body:
# only the crate's own tests use it (through a dev-dependency on itself), and a
# shipped app opens the bundled dataset with `DemoEnclosure::open_pinned` alone.
# [AC-C-13] [NFR-PQ-3]
#
# The graph comes from `cargo tree`, offline, so this runs in the toolchain image
# after `just build-rust`. MIDGARD_CARGO_TREE names a file to read instead, for the
# self-test (tools/ci/tests/no-test-backend.test.sh).
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/lib.sh"
cd "$here/../.."

if [ -n "${MIDGARD_CARGO_TREE:-}" ]; then
    tree="$(cat "$MIDGARD_CARGO_TREE")" || exit 1
else
    tree="$(cargo tree --offline --locked --workspace --target all \
        -e normal,build,features --prefix none 2>&1)" || {
        ci_error "no-test-backend: cargo tree failed"
        printf '%s\n' "$tree" >&2
        exit 1
    }
fi

offending="$(printf '%s\n' "$tree" | grep -E '^[a-z0-9_-]+ feature "test-backend"' | sort -u)"
if [ -n "$offending" ]; then
    ci_error "no-test-backend: the release build graph enables the INSECURE test backend"
    printf '%s\n' "$offending" | while read -r line; do
        crate="${line%% feature*}"
        echo "  $line — see what enables it with:" >&2
        echo "    cargo tree --workspace -e normal,build,features -i $crate" >&2
    done
    exit 1
fi
unpinned="$(printf '%s\n' "$tree" | grep -E '^[a-z0-9_-]+ feature "demo-unpinned"' | sort -u)"
if [ -n "$unpinned" ]; then
    ci_error "no-test-backend: the release build graph enables the unpinned demo-dataset entry points"
    printf '%s\n' "$unpinned" | while read -r line; do
        crate="${line%% feature*}"
        echo "  $line — see what enables it with:" >&2
        echo "    cargo tree --workspace -e normal,build,features -i $crate" >&2
    done
    exit 1
fi
if ! printf '%s\n' "$tree" | grep -qE '^pqc-agility feature "backend-fips"'; then
    ci_error "no-test-backend: the release build graph does not select backend-fips at all"
    exit 1
fi
echo "no-test-backend: the release build graph selects backend-fips and never test-backend or demo-unpinned"
