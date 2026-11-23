#!/usr/bin/env bash
# The sample data the apps show (MidgardDataSource = sample): extracted afresh from
# the release's mocks (extract-fixtures.sh into a scratch directory), then shaped by
# extract-sample.mjs into clients/demo/sample/<release>/sample.json, which the core
# compiles in. [AC-C-13] [FR-C-5]
#
#   bash clients/demo/tools/extract-sample.sh            write the committed copy
#   bash clients/demo/tools/extract-sample.sh --check [<f>]  compare only (the committed
#                                                        copy, or <f>); also check that
#                                                        every mock input is the tag's
#   bash clients/demo/tools/extract-sample.sh --out <f>  write elsewhere
set -euo pipefail
cd "$(dirname "$0")/../../.."
release="$(bash tools/ci/release-artefacts.sh --version)"
committed="clients/demo/sample/$release/sample.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
bash clients/demo/tools/extract-fixtures.sh --out "$tmp/fixtures" >/dev/null
case "${1:-}" in
    --check)
        committed="${2:-$committed}"
        node clients/demo/tools/extract-sample.mjs --fixtures "$tmp/fixtures" --out "$tmp/sample.json" >/dev/null
        if ! cmp -s "$committed" "$tmp/sample.json"; then
            echo "extract-sample: $committed is not what the mocks serve: bash clients/demo/tools/extract-sample.sh" >&2
            exit 1
        fi
        # Every mock the sample was read from is byte-identical to the signed tag's
        # copy, and nothing else under sdk/ts/src has moved from the tag either (the
        # mocks import from it). Without the tag (a shallow clone) this part is skipped
        # with a warning, unless MIDGARD_REQUIRE_TAG=1 (CI), where a missing tag fails.
        # MIDGARD_SAMPLE_TAG names another tag or commit (the tests' seeded cases).
        tag="${MIDGARD_SAMPLE_TAG:-v$release}"
        if git rev-parse -q --verify "$tag^{commit}" >/dev/null; then
            if ! git diff --quiet "$tag" -- sdk/ts/src; then
                echo "extract-sample: sdk/ts/src differs from $tag:" >&2
                git diff --stat "$tag" -- sdk/ts/src >&2
                exit 1
            fi
            node -e 'const b = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
                     for (const [p, id] of Object.entries(b.inputs)) if (p.startsWith("sdk/ts/src/mock/")) console.log(p, id);' \
                "$committed" | while read -r path id; do
                want="$(git rev-parse -q --verify "$tag:$path" 2>/dev/null || echo missing)"
                if [ "$want" != "$id" ]; then
                    echo "extract-sample: $path is not the $tag copy ($id, the tag has $want)" >&2
                    exit 1
                fi
            done
        elif [ "${MIDGARD_REQUIRE_TAG:-0}" = 1 ]; then
            echo "extract-sample: tag $tag not present and MIDGARD_REQUIRE_TAG=1: cannot check the mocks against it" >&2
            exit 1
        else
            echo "extract-sample: warning: tag $tag not present; compared with the tree's mocks only" >&2
        fi
        ;;
    --out)
        node clients/demo/tools/extract-sample.mjs --fixtures "$tmp/fixtures" --out "$2"
        ;;
    '')
        node clients/demo/tools/extract-sample.mjs --fixtures "$tmp/fixtures" --out "$committed"
        ;;
    *)
        echo "extract-sample: unknown option $1" >&2
        exit 2
        ;;
esac
