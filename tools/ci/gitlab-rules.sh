#!/usr/bin/env bash
# Apply the repository rules to a GitLab mirror. [AC-RF-CA-6d] [AC-RF-CA-6e] [NFR-OSS-1]
#
#   GITLAB_TOKEN=... bash tools/ci/gitlab-rules.sh --url https://git.example.org --project group/midgard-app
#   bash tools/ci/gitlab-rules.sh --url ... --project ... --dry-run     print the calls, make none
#
# The GitHub repository enforces these with rulesets (docs/dev/CI.md, "Repository
# rules"); this sets the GitLab equivalents, so a mirror holds history to the same
# standard:
#
#   - main and dev are protected: maintainers push and merge, nobody force-pushes;
#   - v* tags are protected: only maintainers create them, and once pushed they are not
#     moved;
#   - push rules reject unsigned commits and the deletion of tags. Push rules are a
#     GitLab Premium feature; on a tier without them the call fails and says so, and the
#     rest still applies.
#
# GITLAB_TOKEN is a personal or project access token with the `api` scope, held by a
# maintainer. It is read from the environment and sent only to --url. Every call is
# idempotent: run it again after a change and it converges.
set -uo pipefail

url="" project="" dry=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --url) url="${2:?--url needs the GitLab base URL}"; shift 2 ;;
        --project) project="${2:?--project needs group/name}"; shift 2 ;;
        --dry-run) dry=1; shift ;;
        -h | --help) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "gitlab-rules: unknown argument '$1'" >&2; exit 2 ;;
    esac
done
[ -n "$url" ] && [ -n "$project" ] || { echo "gitlab-rules: --url and --project are required" >&2; exit 2; }
[[ "$url" =~ ^https:// ]] || { echo "gitlab-rules: --url must be https" >&2; exit 2; }
[ "$dry" = 1 ] || [ -n "${GITLAB_TOKEN:-}" ] || { echo "gitlab-rules: set GITLAB_TOKEN (scope: api)" >&2; exit 2; }

api="${url%/}/api/v4/projects/$(printf '%s' "$project" | sed 's|/|%2F|g')"
failures=0

# call METHOD PATH [FORM...] — prints the HTTP status; the response body goes to $body.
body="$(mktemp)"
trap 'rm -f "$body"' EXIT
call() {
    local method="$1" path="$2" args=() status
    shift 2
    for f in "$@"; do args+=(--data-urlencode "$f"); done
    if [ "$dry" = 1 ]; then
        echo "$method $api$path ${*}" >&2
        : > "$body"
        echo 200
        return
    fi
    status="$(curl -sS -o "$body" -w '%{http_code}' -X "$method" -H "PRIVATE-TOKEN: $GITLAB_TOKEN" "${args[@]}" "$api$path")"
    echo "$status"
}

ok() { echo "gitlab-rules: ok — $*"; }
bad() { echo "gitlab-rules: FAILED — $*" >&2; [ -s "$body" ] && sed 's/^/  /' "$body" >&2; failures=$((failures + 1)); }

for branch in main dev; do
    # Protect it if it is not; then, either way, set the levels and refuse force pushes.
    s="$(call POST /protected_branches "name=$branch" push_access_level=40 merge_access_level=40 allow_force_push=false)"
    case "$s" in
        201 | 200) ok "$branch is protected, no force push" ;;
        409 | 422)
            s="$(call PATCH "/protected_branches/$branch" allow_force_push=false)"
            [ "$s" = 200 ] && ok "$branch was protected; force push refused" || bad "could not update $branch's protection (HTTP $s)"
            ;;
        *) bad "could not protect $branch (HTTP $s)" ;;
    esac
done

s="$(call POST /protected_tags 'name=v*' create_access_level=40)"
case "$s" in
    201 | 200) ok "v* tags are protected" ;;
    409 | 422) ok "v* tags were already protected" ;;
    *) bad "could not protect v* tags (HTTP $s)" ;;
esac

# The push rule exists or not; PUT updates one, POST creates one.
s="$(call PUT /push_rule reject_unsigned_commits=true deny_delete_tag=true)"
if [ "$s" = 404 ]; then
    s="$(call POST /push_rule reject_unsigned_commits=true deny_delete_tag=true)"
fi
case "$s" in
    200 | 201) ok "unsigned commits and tag deletion are rejected" ;;
    403) bad "push rules were refused (HTTP 403): they need GitLab Premium, or a maintainer's token" ;;
    *) bad "could not set the push rule (HTTP $s)" ;;
esac

if [ "$failures" -gt 0 ]; then
    echo "gitlab-rules: $failures rule(s) not applied"
    exit 1
fi
[ "$dry" = 1 ] && { echo "gitlab-rules: dry run; nothing was changed on $project"; exit 0; }
echo "gitlab-rules: every rule applied to $project"
