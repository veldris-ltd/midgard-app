#!/usr/bin/env bash
# The apps' sample data (MidgardDataSource = sample) is what the signed release's
# mocks serve: the committed clients/demo/sample/<release>/sample.json matches a
# fresh extraction, two extractions are byte-identical, every mock it was read from
# is the tag's copy, and a changed copy is caught. [AC-C-13] [FR-C-5]
#
#   bash tools/ci/tests/sample-data.test.sh
#
# Needs node 22.6 or later, as demo-fixtures.test.sh; without node it skips.
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
failures=0
pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

if ! command -v node >/dev/null 2>&1; then
    echo "skip  sample-data: node is not on PATH"
    exit 0
fi

release="$(bash tools/ci/release-artefacts.sh --version)"
sample="clients/demo/sample/$release/sample.json"
if out="$(bash clients/demo/tools/extract-sample.sh --check 2>&1)"; then
    pass "$sample is what the mocks serve${out:+ ($out)}"
else
    fail "$sample is stale: $out"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
bash clients/demo/tools/extract-sample.sh --out "$tmp/a.json" >/dev/null 2>&1
bash clients/demo/tools/extract-sample.sh --out "$tmp/b.json" >/dev/null 2>&1
if [ -s "$tmp/a.json" ] && cmp -s "$tmp/a.json" "$tmp/b.json"; then
    pass "two extractions are byte-identical"
else
    fail "extraction is not deterministic"
fi

# A copy with one value changed is refused (the drift the check exists for).
node -e 'const fs = require("fs"); const s = fs.readFileSync(process.argv[1], "utf8");
         fs.writeFileSync(process.argv[2], s.replace("\"conf\":54", "\"conf\":55"));' "$sample" "$tmp/drift.json"
if cmp -s "$sample" "$tmp/drift.json"; then
    fail "the seeded change did not apply"
elif bash clients/demo/tools/extract-sample.sh --check "$tmp/drift.json" >/dev/null 2>&1; then
    fail "a changed sample was accepted"
else
    pass "a changed sample is refused"
fi

# Nothing in it that the mocks do not serve: no synthetic hour.
if node -e 'const b = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
            process.exit(b.format === "midgard-sample-data" && !("heimdall.motion" in b.collections) ? 0 : 1);' "$sample"; then
    pass "the sample holds only mock collections"
else
    fail "the sample holds a collection the mocks do not serve"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
