#!/usr/bin/env bash
# The demo fixtures are what the release's mocks serve, extracted deterministically;
# the extractor fails closed on a marking it cannot place; switching persona changes
# what is visible; and the generated read models match the TypeScript contract.
# [AC-C-13] [FR-C-1]
#
#   bash tools/ci/tests/demo-fixtures.test.sh
#
# Runs in the js-build CI job (tools/ci/run.sh), which has node. Without node it
# skips, so the selftest job, which has none, still passes. The read-model check
# also needs the pinned rustfmt on PATH; without one that check skips.
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
failures=0
pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

if ! command -v node >/dev/null 2>&1; then
    echo "skip  demo-fixtures: node is not on PATH"
    exit 0
fi

release="$(bash tools/ci/release-artefacts.sh --version)"
fixtures="clients/demo/fixtures/$release"
if bash clients/demo/tools/extract-fixtures.sh --check >/dev/null 2>&1; then
    pass "$fixtures is what the mocks serve"
else
    fail "$fixtures is stale: bash clients/demo/tools/extract-fixtures.sh"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
bash clients/demo/tools/extract-fixtures.sh --out "$tmp/a" >/dev/null 2>&1
bash clients/demo/tools/extract-fixtures.sh --out "$tmp/b" >/dev/null 2>&1
if [ -s "$tmp/a/manifest.json" ] && diff -r "$tmp/a" "$tmp/b" >/dev/null; then
    pass "two extractions are byte-identical"
else
    fail "extraction is not deterministic"
fi

# refused <name> <expected message fragment> <js editing the overlay object `o`>
# Writes the committed overlay, edited, to a scratch file and extracts with it.
refused() {
    node -e 'const fs = require("fs"); const o = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
             eval(process.argv[2]); fs.writeFileSync(process.argv[3], JSON.stringify(o));' \
        clients/demo/overlay.json "$3" "$tmp/overlay.json"
    if out="$(bash clients/demo/tools/extract-fixtures.sh --overlay "$tmp/overlay.json" --out "$tmp/neg" 2>&1)"; then
        fail "$1: the extractor accepted it"
    elif ! printf '%s' "$out" | grep -qF -- "$2"; then
        fail "$1: refused, but not with \"$2\": $out"
    else
        pass "$1 is refused"
    fi
}
refused "an overlay level outside the four" "unknown level secret" \
    'o.default.level = "secret";'
refused "overlay compartments that are not a list of strings" "compartments must be a list of strings" \
    'o.collections["admin.roles"] = { default: { level: "internal", compartments: "OPS-N" } };'
refused "an overlay id the collection does not have" "overlay names admin.roles/no-such-row, which does not exist" \
    'o.collections["admin.roles"] = { ids: { "no-such-row": { level: "internal", compartments: [] } } };'
refused "an unrecognised item marking covered only by a default" \
    "heimdall.syncSites/SITE-ALPHA (this): unrecognised marking" \
    'delete o.collections["heimdall.syncSites"].ids;'
refused "a row marked below a marking inside it" "below confidential inside the row" \
    'o.collections["midgard.searchAround"].default = { level: "internal", compartments: [] };'
refused "an overlay id below the row's own marking" "below its own marking" \
    'o.collections["midgard.objectTypes"] = { ids: { aircraft: { level: "internal", compartments: [] } } };'
# SITE-ALPHA carries the string "C // OPS-N": an overlay id may not pin it lower.
refused "an overlay id below a marking string the row carries" \
    "heimdall.syncSites/SITE-ALPHA (this): overlay marks it internal, below its own marking confidential [OPS-N]" \
    'o.collections["heimdall.syncSites"].ids["SITE-ALPHA (this)"] = { level: "internal", compartments: [] };'

# visible <persona> <console>: rows of that console the persona's clearance admits.
visible() {
    node -e 'const fs = require("fs"); const [dir, who, c] = process.argv.slice(1);
             const L = ["open", "internal", "restricted", "confidential"];
             const p = JSON.parse(fs.readFileSync(dir + "/personas.json", "utf8")).find((x) => x.id === who);
             const cols = JSON.parse(fs.readFileSync(dir + "/" + c + ".json", "utf8")).collections;
             let n = 0;
             for (const col of Object.values(cols)) for (const r of col.rows)
               if (L.indexOf(r.marking.level) <= L.indexOf(p.clearance)
                   && r.marking.compartments.every((k) => p.compartments.includes(k))) n++;
             console.log(n);' "$fixtures" "$1" "$2"
}
# k.rao (restricted, no compartment) sees the Heimdall picture, feeds and the
# restricted entity, track and sites, but no OPS-N audit, workflow or decision.
n="$(visible k.rao heimdall)"
if [ "$n" = 13 ]; then pass "k.rao sees 13 Heimdall rows"; else fail "k.rao sees $n Heimdall rows, not 13"; fi
# a.holt (internal, release manager) sees every Bifrost row, the fleet included.
n="$(visible a.holt bifrost)"
if [ "$n" = 30 ]; then pass "a.holt sees all 30 Bifrost rows"; else fail "a.holt sees $n Bifrost rows, not 30"; fi

if ! command -v rustfmt >/dev/null 2>&1; then
    echo "skip  read models: rustfmt is not on PATH"
elif node clients/demo/tools/gen-read-models.mjs --check >/dev/null 2>&1; then
    pass "the console read models match sdk/ts/src"
else
    fail "stale read models: node clients/demo/tools/gen-read-models.mjs --write"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
