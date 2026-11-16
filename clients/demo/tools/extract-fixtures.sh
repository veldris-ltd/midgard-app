#!/usr/bin/env bash
# Extract the demo fixtures (extract-fixtures.mjs); --check compares with the committed
# copy instead of writing it. [AC-C-13]
set -euo pipefail
cd "$(dirname "$0")/../../.."
release="$(bash tools/ci/release-artefacts.sh --version)"
run() {
    node --experimental-strip-types --no-warnings --import ./sdk/ts/scripts/ts-resolve.mjs \
        clients/demo/tools/extract-fixtures.mjs --release "$release" "$@"
}
if [ "${1:-}" = --check ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    run --out "$tmp" >/dev/null
    diff -r "clients/demo/fixtures/$release" "$tmp"
else
    run "$@"
fi
