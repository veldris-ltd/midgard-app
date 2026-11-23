#!/usr/bin/env bash
# The apps' sample data (MidgardDataSource = sample) is what the signed release's
# mocks serve: the committed clients/demo/sample/<release>/sample.json matches a
# fresh extraction, two extractions are byte-identical, every mock it was read from
# is the tag's copy, nothing under sdk/ts/src has moved from the tag, a missing tag
# fails under MIDGARD_REQUIRE_TAG=1 (CI) and only warns without it, and a changed
# copy is caught. [AC-C-13] [FR-C-5]
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

# sdk/ts/src moved from the tag is refused. Seeded with the commit before the last
# one that touched sdk/ts/src, which differs there by construction (skipped in a
# clone too shallow to have it).
before="$(git rev-list -1 HEAD -- sdk/ts/src 2>/dev/null)"
if [ -n "$before" ] && git rev-parse -q --verify "$before^^{commit}" >/dev/null; then
    out="$(MIDGARD_SAMPLE_TAG="$before^" bash clients/demo/tools/extract-sample.sh --check 2>&1)"
    if [ $? -ne 0 ] && grep -q "sdk/ts/src differs from" <<<"$out"; then
        pass "sdk/ts/src that differs from the tag is refused"
    else
        fail "sdk/ts/src that differs from the tag was not refused: $out"
    fi
else
    echo "skip  sdk/ts/src drift: no commit before the last sdk/ts/src change in this clone"
fi

# A missing tag fails where it is required (CI), and only warns where it is not.
missing="v0.0.0-no-such-tag"
if out="$(MIDGARD_SAMPLE_TAG="$missing" MIDGARD_REQUIRE_TAG=1 bash clients/demo/tools/extract-sample.sh --check 2>&1)"; then
    fail "a missing tag was accepted under MIDGARD_REQUIRE_TAG=1"
elif grep -q "MIDGARD_REQUIRE_TAG=1" <<<"$out"; then
    pass "a missing tag fails under MIDGARD_REQUIRE_TAG=1"
else
    fail "a missing tag failed for another reason: $out"
fi
if out="$(MIDGARD_SAMPLE_TAG="$missing" MIDGARD_REQUIRE_TAG= bash clients/demo/tools/extract-sample.sh --check 2>&1)" \
        && grep -q "warning: tag $missing not present" <<<"$out"; then
    pass "a missing tag only warns without MIDGARD_REQUIRE_TAG"
else
    fail "a missing tag without MIDGARD_REQUIRE_TAG did not just warn: $out"
fi

# Nothing in it that the mocks do not serve: no synthetic hour.
if node -e 'const b = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
            process.exit(b.format === "midgard-sample-data" && !("heimdall.motion" in b.collections) ? 0 : 1);' "$sample"; then
    pass "the sample holds only mock collections"
else
    fail "the sample holds a collection the mocks do not serve"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
