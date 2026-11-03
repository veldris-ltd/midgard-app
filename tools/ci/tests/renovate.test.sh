#!/usr/bin/env bash
# renovate.json is valid JSON and configures the update automation both ecosystems
# need: npm (the pnpm workspace and the launch pipeline), cargo with the vendoring
# task, GitHub Actions pinned by SHA and GitLab images pinned by digest, weekly and
# reviewable. [AC-P4.4] [NFR-OSS-1] (VLD-ANB-MIDGARD-0076)
#
#   bash tools/ci/tests/renovate.test.sh [RENOVATE-JSON]
#
# The file is read with jq, node or python3, whichever the runner has, and flattened
# to `path=value` lines; a file that does not parse fails before anything is checked.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../.."
config="${1:-$root/renovate.json}"
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

[ -f "$config" ] || { echo "renovate: no configuration at $config" >&2; exit 1; }

# The configuration as `a.b.0.c=value` lines, one per scalar.
flatten() {
    if command -v jq >/dev/null 2>&1; then
        # paths(scalars) would skip false and null leaves (jq keeps only the paths whose
        # filter is truthy), and "automerge": false is one of the settings checked.
        jq -r 'paths(type != "object" and type != "array") as $p | "\($p | map(tostring) | join("."))=\(getpath($p))"' "$1"
    elif command -v node >/dev/null 2>&1; then
        node -e '
            const walk = (v, p) => v !== null && typeof v === "object"
                ? Object.entries(v).forEach(([k, x]) => walk(x, p.concat(k)))
                : console.log(p.join(".") + "=" + v);
            walk(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")), []);' "$1"
    elif command -v python3 >/dev/null 2>&1; then
        python3 -c '
import json, sys
def walk(v, p):
    if isinstance(v, (dict, list)):
        for k, x in (v.items() if isinstance(v, dict) else enumerate(v)):
            walk(x, p + [str(k)])
    else:
        print(".".join(p) + "=" + (str(v).lower() if isinstance(v, bool) else str(v)))
walk(json.load(open(sys.argv[1], encoding="utf-8")), [])' "$1"
    else
        echo "renovate: needs jq, node or python3 to read JSON" >&2
        return 2
    fi
}

if ! flat="$(flatten "$config" | tr -d '\r')"; then
    echo "renovate: $config does not parse as JSON" >&2
    exit 1
fi
pass "renovate.json parses as JSON"

# has LABEL REGEX: some line of the flattened configuration matches.
has() {
    if grep -Eq "$2" <<<"$flat"; then pass "$1"; else fail "$1"; fi
}

# The five managers, and no others.
managers="$(grep -E '^enabledManagers\.[0-9]+=' <<<"$flat" | sed 's/^[^=]*=//' | sort | tr '\n' ' ')"
if [ "$managers" = "cargo custom.regex github-actions gitlabci npm " ]; then
    pass "enabledManagers: $managers"
else
    fail "enabledManagers is '$managers', not cargo custom.regex github-actions gitlabci npm"
fi

has "extends config:recommended" '^extends\.[0-9]+=config:recommended$'
has "github-actions pins by digest" '^github-actions\.pinDigests=true$'
has "gitlabci pins image digests" '^gitlabci\.pinDigests=true$'
has "a weekly schedule" '^schedule\.[0-9]+=.*(monday|mon|\* \* 1)'
has "at most three open at once" '^prConcurrentLimit=3$'
has "labelled dependencies" '^labels\.[0-9]+=dependencies$'
has "commits are signed by the forge" '^platformCommit=(true|enabled)$'
has "nothing merges itself" '^automerge=false$'
has "the launch pipeline is covered" '^packageRules\.[0-9]+\.matchFileNames\.[0-9]+=docs/launch/pipeline/'

# The cargo rule: it re-vendors and runs vendor-check.
rule="$(grep -E '^packageRules\.[0-9]+\.matchManagers\.[0-9]+=cargo$' <<<"$flat" \
    | sed 's/^packageRules\.\([0-9]*\)\..*/\1/' \
    | while read -r i; do
        grep -q "^packageRules\.$i\.postUpgradeTasks\." <<<"$flat" && echo "$i"
    done | head -1)"
if [ -z "$rule" ]; then
    fail "no cargo rule carries postUpgradeTasks"
else
    tasks="$(grep -E "^packageRules\.$rule\.postUpgradeTasks\.commands\." <<<"$flat")"
    grep -q 'cargo vendor' <<<"$tasks" && grep -q 'tools/ci/vendor-check.sh' <<<"$tasks" \
        && pass "the cargo rule re-vendors and runs vendor-check" \
        || fail "the cargo rule's postUpgradeTasks do not run cargo vendor and vendor-check"
    grep -Eq "^packageRules\.$rule\.postUpgradeTasks\.fileFilters\.[0-9]+=vendor/" <<<"$flat" \
        && pass "the cargo rule commits /vendor" \
        || fail "the cargo rule's fileFilters do not include vendor/"
fi

# And the workflow is written down for the people who finish what Renovate cannot.
if grep -q '^## Dependency updates' "$root/docs/dev/CI.md" 2>/dev/null; then
    pass "docs/dev/CI.md documents dependency updates"
else
    fail "docs/dev/CI.md has no \"Dependency updates\" section"
fi

if [ "$failures" -ne 0 ]; then
    echo "renovate: $failures check(s) failed" >&2
    exit 1
fi
echo "renovate: OK"
