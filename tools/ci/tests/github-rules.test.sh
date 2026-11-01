#!/usr/bin/env bash
# Tests for tools/ci/github-rules.sh, against a stub gh that records every call and
# never reaches GitHub. [AC-RF-CA-6d] [VLD-ANB-MIDGARD-0035] [AC-P2.5]
#
#   bash tools/ci/tests/github-rules.test.sh
#
# The claim under test: the rulesets carry the rules docs/dev/CI.md documents; v* tag
# creation is a rule whose only bypass actor is the release-managers team, in a ruleset
# of its own, so the immutability ruleset has no bypass at all; a dry run writes
# nothing; a missing team stops the run before anything is written; an existing
# ruleset is updated in place rather than duplicated.
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
# names the ruleset in STUB_EXISTING (id 77), if any; a write records its payload.
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
payload() { awk -v name="\"name\": \"$1\"" '/^(POST|PUT) / { on = 0 } index($0, name) { on = 1 } on' <<<"$out"; }
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

rules -- --repo 'not a repo' >/dev/null 2>&1 && fail "a malformed --repo is refused" || pass "a malformed --repo is refused"

if [ "$failures" -gt 0 ]; then
    echo "github-rules tests: $failures failure(s)"
    exit 1
fi
echo "github-rules tests: all passed"
