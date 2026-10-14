#!/usr/bin/env bash
# The two pipeline definitions must call the same jobs. [NFR-OSS-1] [RC-07]
#
#   bash tools/ci/pipelines.sh --check     exit 1 if they have drifted
#   bash tools/ci/pipelines.sh --jobs      what each pipeline calls
#
# .github/workflows/*.yml and .gitlab-ci.yml are two descriptions of one pipeline. All
# either may do is call `tools/ci/run.sh <job>`, so "the same pipeline" reduces to
# something checkable: both call exactly the same set of jobs, every job they call
# exists, and neither carries build logic of its own. A job added on one forge and
# forgotten on the other fails here, on both.
set -euo pipefail

cd "${MIDGARD_PIPELINES_ROOT:-$(dirname "$0")/../..}"
GITHUB_DIR=.github/workflows
GITLAB_FILE=.gitlab-ci.yml

# The job names a set of files passes to tools/ci/run.sh.
called() {
    cat "$@" 2>/dev/null | tr -d '\r' | sed 's/#.*//' \
        | grep -oE 'tools/ci/run\.sh +[a-z][a-z0-9-]*' | awk '{print $NF}' | sort -u
}

github_jobs() { called "$GITHUB_DIR"/*.yml; }
gitlab_jobs() { called "$GITLAB_FILE"; }
defined_jobs() { bash tools/ci/run.sh names | sort -u; }

# Lines that run something other than run.sh. Setup that only a CI system can do —
# checking out, caching, fetching an artefact, installing a shell into a bare image —
# is not build logic and is allowed; anything else belongs in a job.
ALLOWED='^(bash tools/ci/run\.sh |apk add --no-cache |docker info|mkdir -p |echo |test |\[ )'
stray_github() {
    cat "$GITHUB_DIR"/*.yml | tr -d '\r' | sed -n 's/^ *\(- \)\{0,1\}run: *//p' \
        | grep -v '^[|>]' | grep -Ev "$ALLOWED" || true
}
stray_gitlab() {
    tr -d '\r' < "$GITLAB_FILE" | awk '
        /^[^ #].*:$/ || /^  [a-z_]+:/ { inscript = ($0 ~ /^ +(before_|after_)?script:/) }
        inscript && /^ +- / { sub(/^ +- /, ""); print }' \
        | grep -Ev "$ALLOWED" || true
}

check() {
    local status=0 gh gl defined diff
    [ -d "$GITHUB_DIR" ] || { echo "pipelines: $GITHUB_DIR is missing" >&2; return 1; }
    [ -f "$GITLAB_FILE" ] || { echo "pipelines: $GITLAB_FILE is missing" >&2; return 1; }
    gh="$(github_jobs)"
    gl="$(gitlab_jobs)"
    defined="$(defined_jobs)"

    diff="$(comm -23 <(echo "$gh") <(echo "$gl"))"
    if [ -n "$diff" ]; then
        echo "pipelines: called by GitHub Actions but not by GitLab CI: $(echo $diff)" >&2
        status=1
    fi
    diff="$(comm -13 <(echo "$gh") <(echo "$gl"))"
    if [ -n "$diff" ]; then
        echo "pipelines: called by GitLab CI but not by GitHub Actions: $(echo $diff)" >&2
        status=1
    fi
    diff="$(comm -23 <(printf '%s\n%s\n' "$gh" "$gl" | sort -u) <(echo "$defined"))"
    if [ -n "$diff" ]; then
        echo "pipelines: called but not defined in tools/ci/run.sh: $(echo $diff)" >&2
        status=1
    fi
    diff="$(comm -13 <(printf '%s\n%s\n' "$gh" "$gl" | sort -u) <(echo "$defined"))"
    if [ -n "$diff" ]; then
        echo "pipelines: defined in tools/ci/run.sh but called by neither pipeline: $(echo $diff)" >&2
        status=1
    fi
    diff="$(stray_github)"
    if [ -n "$diff" ]; then
        echo "pipelines: GitHub Actions runs something other than a job — move it into tools/ci/run.sh:" >&2
        sed 's/^/  /' <<<"$diff" >&2
        status=1
    fi
    diff="$(stray_gitlab)"
    if [ -n "$diff" ]; then
        echo "pipelines: GitLab CI runs something other than a job — move it into tools/ci/run.sh:" >&2
        sed 's/^/  /' <<<"$diff" >&2
        status=1
    fi
    return "$status"
}

case "${1:-}" in
    --check)
        check
        echo "pipelines: GitHub Actions and GitLab CI call the same $(github_jobs | wc -l | tr -d ' ') jobs, all defined"
        ;;
    --jobs)
        echo "github: $(echo $(github_jobs))"
        echo "gitlab: $(echo $(gitlab_jobs))"
        ;;
    *)
        echo "usage: $0 --check | --jobs" >&2
        exit 2
        ;;
esac
