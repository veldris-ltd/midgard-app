#!/usr/bin/env bash
# Tests for tools/ci/github-rules.sh, against a stub gh that records every call and
# never reaches GitHub. [AC-RF-CA-6d] [VLD-ANB-MIDGARD-0035] [AC-P2.5]
# [VLD-ANB-MIDGARD-0069] [AC-P1.2]
#
#   bash tools/ci/tests/github-rules.test.sh
#
# The claim under test: the rulesets carry the rules docs/dev/CI.md documents; v* tag
# creation is a rule whose only bypass actor is the release-managers team, in a ruleset
# of its own, so the immutability ruleset has no bypass at all; a dry run writes
# nothing; a missing team stops the run before anything is written; an existing
# ruleset is updated in place rather than duplicated; main and dev take changes only
# through a reviewed pull request; the export is the live rulesets, normalised so that
# it diffs cleanly.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../github-rules.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# The stub: the team lookup answers 4242 unless STUB_NO_TEAM is set; the ruleset list
# names the ruleset in STUB_EXISTING (id 77), if any; a write records its payload. For
# the export, the rulesets are the files in STUB_RULESETS, named by id.
mkdir -p "$work/stubs"
cat > "$work/stubs/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >> "$STUB_LOG"
args="$*"
case "$args" in
    *"-X "*)
        cat >> "$STUB_LOG"
        echo '{}'
        ;;
    "api orgs/"*"/teams/release-managers --jq .id")
        [ -z "${STUB_NO_TEAM:-}" ] || { echo '{"message":"Not Found"}' >&2; exit 1; }
        echo 4242
        ;;
    "api repos/"*"/rulesets --paginate --jq .[].id")
        ls "${STUB_RULESETS:-/nonexistent}" 2>/dev/null
        ;;
    "api repos/"*"/rulesets/"[0-9]*)
        cat "$STUB_RULESETS/${args##*/}"
        ;;
    "api repos/"*"/rulesets --paginate --jq"*)
        case "$args" in *"\"${STUB_EXISTING:-none}\""*) echo 77 ;; esac
        ;;
    *) echo "stub gh: unexpected call: $args" >&2; exit 1 ;;
esac
EOF
chmod +x "$work/stubs/gh"

rules() { # rules VAR=VALUE... -- ARG...
    local vars=()
    while [ "$1" != "--" ]; do vars+=("$1"); shift; done
    shift
    : > "$work/log"
    env -u GH_TOKEN STUB_LOG="$work/log" PATH="$work/stubs:$PATH" "${vars[@]}" bash "$script" "$@" 2>&1
}

# --- a dry run: the payloads, and no write ---------------------------------------------
if out="$(rules -- --repo veldris-ltd/midgard-app --dry-run)"; then
    pass "a dry run succeeds"
else
    fail "a dry run succeeds: $out"
fi
grep -q -- '-X' "$work/log" && fail "a dry run writes nothing: $(cat "$work/log")" || pass "a dry run writes nothing"

# The payload printed after a ruleset's POST line, up to the next call.
payload() { awk -v name="\"name\": \"$1\"" '/^(POST|PUT) |^github-rules: / { on = 0 } index($0, name) { on = 1 } on' <<<"$out"; }
creation="$(payload "release tags are created by release managers")"
grep -q '"include": \["refs/tags/v\*"\]' <<<"$creation" && grep -q '"target": "tag"' <<<"$creation" &&
    grep -q '"rules": \[{ "type": "creation" }\]' <<<"$creation" &&
    pass "tag creation on refs/tags/v* is a rule" || fail "tag creation on refs/tags/v* is a rule: $creation"
grep -q '"bypass_actors": \[{ "actor_id": 4242, "actor_type": "Team", "bypass_mode": "always" }\]' <<<"$creation" &&
    pass "the only bypass actor for tag creation is the release-managers team" ||
    fail "the only bypass actor for tag creation is the release-managers team: $creation"

immutable="$(payload "release tags are immutable")"
for rule in deletion non_fast_forward update; do
    grep -q "{ \"type\": \"$rule\" }" <<<"$immutable" || fail "the immutable tag ruleset has $rule: $immutable"
done
grep -q '"bypass_actors": \[\]' <<<"$immutable" && ! grep -q creation <<<"$immutable" &&
    pass "release tags are immutable, with no bypass actor" || fail "release tags are immutable, with no bypass actor: $immutable"

history="$(payload "signed history")"
grep -q '"refs/heads/main", "refs/heads/dev"' <<<"$history" && grep -q required_signatures <<<"$history" &&
    grep -q '"bypass_actors": \[\]' <<<"$history" &&
    pass "main and dev require signed history, with no bypass actor" || fail "main and dev require signed history: $history"
# signed_history_takes_changes_only_through_a_reviewed_pull_request [AC-P1.2]
for rule in required_signatures non_fast_forward deletion pull_request; do
    grep -q "{ \"type\": \"$rule\"" <<<"$history" || fail "the signed history ruleset has $rule: $history"
done
grep -q '"required_approving_review_count": 1' <<<"$history" && grep -q '"require_code_owner_review": true' <<<"$history" &&
    grep -q '"dismiss_stale_reviews_on_push": true' <<<"$history" && grep -q '"require_last_push_approval": true' <<<"$history" &&
    grep -q '"allowed_merge_methods": \["merge"\]' <<<"$history" &&
    pass "main and dev take a change only through a pull request a code owner approved after its last push" ||
    fail "main and dev take a change only through a reviewed pull request: $history"
if command -v jq >/dev/null 2>&1; then
    for name in "signed history" "release tags are immutable" "release tags are created by release managers"; do
        jq -e . >/dev/null 2>&1 <<<"$(payload "$name")" || fail "the \"$name\" payload is JSON: $(payload "$name")"
    done
    jq -e '[.rules[].type] == ["required_signatures", "non_fast_forward", "deletion", "pull_request"]' >/dev/null <<<"$history" &&
        pass "every payload is JSON, and the signed history rules are exactly the four" ||
        fail "the signed history rules are exactly the four: $history"
fi

# --- applying ----------------------------------------------------------------------------
out="$(rules -- --repo veldris-ltd/midgard-app)"
[ "$(grep -c -- '-X POST repos/veldris-ltd/midgard-app/rulesets --input -' "$work/log")" = 3 ] &&
    pass "applying creates the three rulesets" || fail "applying creates the three rulesets: $(cat "$work/log")"
grep -q '"actor_type": "Team"' "$work/log" && pass "the creation ruleset sent names the team" ||
    fail "the creation ruleset sent names the team: $(cat "$work/log")"

out="$(rules STUB_EXISTING="release tags are created by release managers" -- --repo veldris-ltd/midgard-app)"
grep -q -- '-X PUT repos/veldris-ltd/midgard-app/rulesets/77 ' "$work/log" &&
    [ "$(grep -c -- '-X POST' "$work/log")" = 2 ] &&
    pass "an existing ruleset is updated in place" || fail "an existing ruleset is updated in place: $(cat "$work/log")"

if out="$(rules STUB_NO_TEAM=1 -- --repo veldris-ltd/midgard-app)"; then
    fail "with no release-managers team, nothing is applied"
else
    grep -q 'there is no team veldris-ltd/release-managers' <<<"$out" && ! grep -q -- '-X' "$work/log" &&
        pass "with no release-managers team, the run stops before anything is written" ||
        fail "with no release-managers team, the run stops before anything is written: $out"
fi

# --- the export [AC-P1.2] -----------------------------------------------------------------
# What GitHub answers for each ruleset, keys unsorted and with the fields that change on
# every read; the export keeps neither the order nor those fields.
mkdir -p "$work/rulesets"
cat > "$work/rulesets/11" <<'EOF'
{"updated_at": "2026-11-01T10:00:00Z", "name": "signed history", "id": 11, "node_id": "RRS_a", "target": "branch",
 "source_type": "Repository", "source": "veldris-ltd/midgard-app", "enforcement": "active", "bypass_actors": [],
 "conditions": {"ref_name": {"include": ["refs/heads/main", "refs/heads/dev"], "exclude": []}},
 "rules": [{"type": "required_signatures"}, {"type": "non_fast_forward"}, {"type": "deletion"},
           {"type": "pull_request", "parameters": {"require_code_owner_review": true, "required_approving_review_count": 1}}],
 "_links": {"self": {"href": "https://api.github.com/repos/veldris-ltd/midgard-app/rulesets/11"}},
 "created_at": "2026-11-01T09:00:00Z", "current_user_can_bypass": "never"}
EOF
cat > "$work/rulesets/7" <<'EOF'
{"name": "release tags are immutable", "id": 7, "node_id": "RRS_b", "target": "tag", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/tags/v*"], "exclude": []}}, "bypass_actors": [],
 "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}, {"type": "update"}],
 "created_at": "2026-11-01T09:00:00Z", "updated_at": "2026-11-01T09:00:00Z", "current_user_can_bypass": "never"}
EOF
if command -v jq >/dev/null 2>&1; then
    evidence="$work/evidence/rulesets-$(date -u +%F).json"
    if out="$(rules STUB_RULESETS="$work/rulesets" -- --repo veldris-ltd/midgard-app --export "$work/evidence")" && [ -s "$evidence" ]; then
        pass "the export writes rulesets-<date>.json"
    else
        fail "the export writes rulesets-<date>.json: $out"
    fi
    [ "$(jq -r '.[].name' "$evidence" 2>/dev/null | tr '\n' '|')" = "release tags are immutable|signed history|" ] &&
        pass "the export holds every ruleset, sorted by name" || fail "the export holds every ruleset, sorted by name: $(cat "$evidence")"
    jq -e 'all(.[]; has("node_id") or has("_links") or has("created_at") or has("updated_at") or has("current_user_can_bypass") | not)' \
        "$evidence" >/dev/null && pass "the export drops the fields that change on every read" ||
        fail "the export drops the fields that change on every read: $(cat "$evidence")"
    jq -S . "$evidence" | cmp -s - "$evidence" && pass "the export's keys are sorted, so it diffs cleanly" ||
        fail "the export's keys are sorted: $(cat "$evidence")"
    # The check AC-P1.5 names, on the committed evidence.
    jq -e '.[] | select(.name == "signed history") | .enforcement == "active"
        and (.conditions.ref_name.include | index("refs/heads/main") and index("refs/heads/dev"))
        and ([.rules[].type] | index("required_signatures") and index("pull_request"))' "$evidence" >/dev/null &&
        pass "the export shows signed history active on main and dev, with required_signatures and pull_request" ||
        fail "the export shows signed history active on main and dev: $(cat "$evidence")"
    cp "$evidence" "$work/first"
    rules STUB_RULESETS="$work/rulesets" -- --repo veldris-ltd/midgard-app --export "$work/evidence" >/dev/null
    cmp -s "$work/first" "$evidence" && pass "exporting again gives the same bytes" || fail "exporting again gives the same bytes"
else
    echo "skip  the export (jq is not on PATH)"
fi
out="$(rules STUB_RULESETS="$work/rulesets" -- --repo veldris-ltd/midgard-app --dry-run --export "$work/dry")"
[ ! -e "$work/dry" ] && pass "a dry run exports nothing" || fail "a dry run exports nothing"

rules -- --repo 'not a repo' >/dev/null 2>&1 && fail "a malformed --repo is refused" || pass "a malformed --repo is refused"

if [ "$failures" -gt 0 ]; then
    echo "github-rules tests: $failures failure(s)"
    exit 1
fi
echo "github-rules tests: all passed"
