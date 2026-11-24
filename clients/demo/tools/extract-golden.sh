#!/usr/bin/env bash
# Record the mocks' golden answers (extract-golden.mjs) into clients/demo/golden/<release>/;
# --check compares a fresh recording with the committed copy instead. [FR-C-4] [AC-C-13]
#
#   bash clients/demo/tools/extract-golden.sh            write the committed copy
#   bash clients/demo/tools/extract-golden.sh --check    compare only
set -euo pipefail
cd "$(dirname "$0")/../../.."
release="$(bash tools/ci/release-artefacts.sh --version)"
run() {
    node --experimental-strip-types --no-warnings --import ./sdk/ts/scripts/ts-resolve.mjs \
        clients/demo/tools/extract-golden.mjs --release "$release" "$@"
}
if [ "${1:-}" = --check ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    run --out "$tmp" >/dev/null
    diff -r "clients/demo/golden/$release" "$tmp"
else
    run "$@"
fi
