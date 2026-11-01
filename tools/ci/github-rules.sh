#!/usr/bin/env bash
# Apply the repository rules to the GitHub repository. [AC-RF-CA-6d] [AC-RF-CA-6e]
# [VLD-ANB-MIDGARD-0035]
#
#   bash tools/ci/github-rules.sh --repo veldris-ltd/midgard-app
#   bash tools/ci/github-rules.sh --repo ... --dry-run          print what would be sent, change nothing
#   bash tools/ci/github-rules.sh --repo ... --export DIR       also write DIR/rulesets-<date>.json
#
# The rulesets docs/dev/CI.md ("Repository rules") documents, as code; the GitLab
# equivalent is tools/ci/gitlab-rules.sh. Each is created, or updated in place when a
# ruleset of that name exists, so running it again converges:
#
#   - "signed history" on main and dev: signed commits, no force push, no deletion;
#   - "release tags are immutable" on v* tags: no deletion, no move, no update. No one
#     may bypass it;
#   - "release tags are created by release managers" on v* tags: only the
#     release-managers team (--team) may create one. It is a ruleset of its own because
#     a bypass actor bypasses every rule in its ruleset, and a release manager must not
#     be able to move or delete a published tag.
#
# A v* tag starts the release pipeline on the signing runners, so who may push one is
# part of the release's trust (VLD-ANB-MIDGARD-0035). The team must exist in the
# repository's organisation; the script looks it up and stops if it does not, rather
# than apply a creation rule nobody may bypass, which would stop every release.
#
# gh does the talking, with its own authentication (GH_TOKEN, or `gh auth login`) as a
# repository administrator; the token is never printed. --dry-run makes only reads (the
# team, the existing rulesets). --export writes the live rulesets, each in full, as one
# JSON array sorted by name with sorted keys and the volatile fields dropped, so a
# reviewer can diff what is enforced against docs/dev/CI.md; it needs jq.
set -uo pipefail

repo="" dry=0 export_dir="" team=release-managers
while [ "$#" -gt 0 ]; do
    case "$1" in
        --repo) repo="${2:?--repo needs owner/name}"; shift 2 ;;
        --team) team="${2:?--team needs a team slug}"; shift 2 ;;
        --dry-run) dry=1; shift ;;
        --export) export_dir="${2:?--export needs a directory}"; shift 2 ;;
        -h | --help) sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "github-rules: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo "github-rules: --repo owner/name is required" >&2; exit 2; }
[[ "$team" =~ ^[a-z0-9_-]+$ ]] || { echo "github-rules: --team must be a team slug" >&2; exit 2; }
command -v gh >/dev/null 2>&1 || { echo "github-rules: the gh CLI is needed" >&2; exit 2; }
org="${repo%%/*}"
failures=0

ok() { echo "github-rules: ok — $*"; }
bad() { echo "github-rules: FAILED — $*" >&2; failures=$((failures + 1)); }

# The team that may create release tags, by id.
team_id="$(gh api "orgs/$org/teams/$team" --jq .id 2>/dev/null)"
if ! [[ "$team_id" =~ ^[0-9]+$ ]]; then
    echo "github-rules: there is no team $org/$team; create it, with the release managers in it, first" >&2
    echo "github-rules: nothing was changed" >&2
    exit 1
fi

# ruleset NAME TARGET REF_PATTERNS RULES BYPASS — one ruleset's payload.
ruleset() {
    printf '{ "name": "%s", "target": "%s", "enforcement": "active",\n' "$1" "$2"
    printf '  "conditions": { "ref_name": { "include": [%s], "exclude": [] } },\n' "$3"
    printf '  "rules": [%s],\n' "$4"
    printf '  "bypass_actors": [%s] }\n' "$5"
}

# apply NAME PAYLOAD — create the ruleset, or update the one of that name.
apply() {
    local name="$1" payload="$2" id method path
    id="$(gh api "repos/$repo/rulesets" --paginate --jq ".[] | select(.name == \"$name\") | .id" 2>/dev/null | head -n 1)"
    if [ -n "$id" ]; then method=PUT path="repos/$repo/rulesets/$id"; else method=POST path="repos/$repo/rulesets"; fi
    if [ "$dry" = 1 ]; then
        echo "$method $path"
        echo "$payload"
        return
    fi
    if gh api -X "$method" "$path" --input - <<<"$payload" >/dev/null; then
        ok "\"$name\" $([ "$method" = PUT ] && echo updated || echo created)"
    else
        bad "could not apply \"$name\" ($method $path)"
    fi
}

apply "signed history" "$(ruleset "signed history" branch '"refs/heads/main", "refs/heads/dev"' \
    '{ "type": "required_signatures" }, { "type": "non_fast_forward" }, { "type": "deletion" }' '')"
apply "release tags are immutable" "$(ruleset "release tags are immutable" tag '"refs/tags/v*"' \
    '{ "type": "deletion" }, { "type": "non_fast_forward" }, { "type": "update" }' '')"
apply "release tags are created by release managers" "$(ruleset "release tags are created by release managers" tag '"refs/tags/v*"' \
    '{ "type": "creation" }' "{ \"actor_id\": $team_id, \"actor_type\": \"Team\", \"bypass_mode\": \"always\" }")"

if [ -n "$export_dir" ] && [ "$dry" = 0 ]; then
    if ! command -v jq >/dev/null 2>&1; then
        bad "--export needs jq"
    else
        mkdir -p "$export_dir"
        out="$export_dir/rulesets-$(date -u +%F).json"
        if gh api "repos/$repo/rulesets" --paginate --jq '.[].id' | while read -r id; do
            gh api "repos/$repo/rulesets/$id" || exit 1
        done | jq -S -s 'map(del(.node_id, ._links, .created_at, .updated_at, .current_user_can_bypass)) | sort_by(.name)' > "$out.tmp"; then
            mv "$out.tmp" "$out"
            ok "the live rulesets are in $out"
        else
            rm -f "$out.tmp"
            bad "could not export the rulesets"
        fi
    fi
fi

if [ "$failures" -gt 0 ]; then
    echo "github-rules: $failures step(s) failed"
    exit 1
fi
[ "$dry" = 1 ] && { echo "github-rules: dry run; nothing was changed on $repo"; exit 0; }
echo "github-rules: every ruleset applied to $repo"
