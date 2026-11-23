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
# forgotten on the other fails here, on both. It also holds both to running only what
# is pinned by content: GitLab images by digest, GitHub actions by commit.
set -euo pipefail

cd "${MIDGARD_PIPELINES_ROOT:-$(dirname "$0")/../..}"
GITHUB_DIR=.github/workflows
GITLAB_FILE=.gitlab-ci.yml

# The job names a set of files passes to tools/ci/run.sh. The release gate calls its
# jobs through .ci-trusted/tools/ci/run.sh, a checkout of main beside the tagged one,
# so that the code deciding whether a tag is released is not the tag's own
# (VLD-ANB-MIDGARD-0035): the same jobs, from another checkout, and counted as such.
called() {
    cat "$@" 2>/dev/null | tr -d '\r' | sed 's/#.*//' \
        | grep -oE '(\.ci-trusted/)?tools/ci/run\.sh +[a-z][a-z0-9-]*' | awk '{print $NF}' | sort -u
}

github_jobs() { called "$GITHUB_DIR"/*.yml; }
gitlab_jobs() { called "$GITLAB_FILE"; }
# Jobs that exist to be run by hand where there is no pipeline, so neither pipeline
# calls them: release-offline runs, on one machine, the jobs the release pipeline runs
# (docs/ops/PKI.md, "Releasing without a forge"). [VLD-ANB-MIDGARD-0035]
BY_HAND_ONLY="release-offline"
defined_jobs() { bash tools/ci/run.sh names | grep -vxF -e "$BY_HAND_ONLY" | sort -u; }

# Lines that run something other than run.sh. Setup that only a CI system can do —
# checking out, caching, fetching an artefact, installing a shell into a bare image —
# is not build logic and is allowed; anything else belongs in a job. GitLab has no
# second-checkout step, so the release gate's clone of main into .ci-trusted is allowed
# in exactly the form .gitlab-ci.yml writes it.
ALLOWED='^(bash (\.ci-trusted/)?tools/ci/run\.sh |apk add --no-cache |docker info|mkdir -p |echo |test |\[ |git clone --branch main --depth [0-9]+ "\$CI_REPOSITORY_URL" \.ci-trusted$)'
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

# What a job runs in is pinned by content, on both forges: every GitLab `image:` and
# service (`- name:`, or the short `- <image>` form) ends in @sha256:<64 hex>, and
# every GitHub `uses:` names a 40-hex commit (or a workflow in this repository).
# $CI_REGISTRY_IMAGE is expanded first. The one reference by variable allowed is the
# published toolchain image, $CI_REGISTRY_IMAGE/toolchain@$MIDGARD_TOOLCHAIN_DIGEST:
# every job that uses it checks, first, that the variable is the digest committed in
# tools/ci/toolchain.digest. Each offending line is printed as file:line: text.
# [VLD-ANB-MIDGARD-0022]
unpinned_gitlab() {
    tr -d '\r' < "$GITLAB_FILE" | awk -v file="$GITLAB_FILE" '
        function indent(s) { match(s, /^[ \t]*/); return RLENGTH }
        # No {n} intervals: not every awk a runner has (mawk, busybox) supports them.
        function digest_pinned(v,    i, h) {
            i = index(v, "@sha256:")
            if (!i) return 0
            h = substr(v, i + 8)
            return length(h) == 64 && h ~ /^[0-9a-f]+$/
        }
        function judge(value, n, text,    v) {
            v = value
            gsub(/^["\047]|["\047]$/, "", v)
            gsub(/\$\{CI_REGISTRY_IMAGE\}|\$CI_REGISTRY_IMAGE/, "registry.invalid/project", v)
            if (digest_pinned(v)) return
            if (v == "registry.invalid/project/toolchain@$MIDGARD_TOOLCHAIN_DIGEST") return
            printf "%s:%d: %s\n", file, n, text
        }
        {
            text = $0
            line = $0
            if (line ~ /^[ \t]*#/ || line ~ /^[ \t]*$/) next
            sub(/[ \t]+#.*$/, "", line)
            ind = indent(line)
            if (inservices && (ind < svcind || (ind == svcind && line !~ /^[ \t]*- /))) inservices = 0
            if (inimage && ind <= imgind) inimage = 0
        }
        inimage && match(line, /^[ \t]*name:[ \t]*/) { judge(substr(line, RLENGTH + 1), NR, text); inimage = 0; next }
        match(line, /^[ \t]*image:[ \t]*/) {
            value = substr(line, RLENGTH + 1)
            if (value == "") { inimage = 1; imgind = ind } else judge(value, NR, text)
            next
        }
        match(line, /^[ \t]*services:[ \t]*/) {
            value = substr(line, RLENGTH + 1)
            if (value == "") { inservices = 1; svcind = ind } else printf "%s:%d: %s\n", file, NR, text
            next
        }
        match(line, /^[ \t]*- name:[ \t]*/) { judge(substr(line, RLENGTH + 1), NR, text); next }
        inservices && match(line, /^[ \t]*- /) {
            value = substr(line, RLENGTH + 1)
            if (value !~ /:[ \t]/ && value !~ /:$/) judge(value, NR, text)
            next
        }'
}
unpinned_github() {
    local f
    for f in "$GITHUB_DIR"/*.yml; do
        [ -f "$f" ] || continue
        tr -d '\r' < "$f" | awk -v file="$f" '
            /^[ \t]*#/ { next }
            match($0, /^[ \t]*(- )?uses:[ \t]*/) {
                v = substr($0, RLENGTH + 1)
                sub(/[ \t]+#.*$/, "", v)
                gsub(/^["\047]|["\047]$/, "", v)
                if (v ~ /^\.\//) next
                ref = v
                sub(/.*@/, "", ref)
                if (v ~ /^docker:\/\/[^ ]+@sha256:[0-9a-f]+$/ && length(ref) == 71) next
                if (v ~ /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.\/-]+@[0-9a-f]+$/ && length(ref) == 40) next
                printf "%s:%d: %s\n", file, NR, $0
            }'
    done
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
    diff="$(unpinned_gitlab)"
    if [ -n "$diff" ]; then
        echo "pipelines: GitLab CI names an image not pinned by digest — pin it as name:tag@sha256:<digest>:" >&2
        sed 's/^/  /' <<<"$diff" >&2
        status=1
    fi
    diff="$(unpinned_github)"
    if [ -n "$diff" ]; then
        echo "pipelines: GitHub Actions uses an action not pinned to a 40-hex commit:" >&2
        sed 's/^/  /' <<<"$diff" >&2
        status=1
    fi
    return "$status"
}

case "${1:-}" in
    --check)
        check
        echo "pipelines: GitHub Actions and GitLab CI call the same $(github_jobs | wc -l | tr -d ' ') jobs, all defined; every image and action is pinned"
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
