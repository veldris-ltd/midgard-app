#!/usr/bin/env bash
# Platform detection and the handful of things that differ between CI systems.
# [NFR-OSS-1] [RC-07]
#
# Sourced, never run. Every pipeline — GitHub Actions, GitLab CI, a developer's shell —
# calls the same `tools/ci/run.sh <job>`, and the jobs call these helpers for the few
# things a CI system does its own way: how an error is annotated, how a log section is
# folded, where the commit and the merge base come from, how a value is handed to a
# later step. So a job is written once, and a mirror of this repository on another
# forge needs no remodelling: the pipeline file for that forge is already here and
# runs the same scripts.
#
#   . tools/ci/lib.sh
#   ci_platform        github | gitlab | local     (which CI system is running this)
#   mainline_ref       the ref a release must be on (origin/main, or MIDGARD_MAINLINE_REF)
#   refresh_mainline   bring origin/main up to date, if a forge can be reached
#   ci_forge           github | gitlab | unknown   (which forge the checkout comes from)

# Which CI system is executing this shell. Decided from the variables each system
# always sets for its jobs, never from the repository's contents: both pipeline
# definitions are committed, and each system reads only its own.
ci_platform() {
    if [ "${GITHUB_ACTIONS:-}" = "true" ]; then
        echo github
    elif [ "${GITLAB_CI:-}" = "true" ]; then
        echo gitlab
    else
        echo local
    fi
}

# Which forge this checkout belongs to: the CI system when there is one, otherwise the
# `origin` remote. A self-hosted GitLab has no fixed hostname, so anything that is not
# GitHub and carries "gitlab" or "git." in its host is taken to be one; MIDGARD_FORGE
# overrides the guess.
ci_forge() {
    local platform url host
    if [ -n "${MIDGARD_FORGE:-}" ]; then
        echo "$MIDGARD_FORGE"
        return
    fi
    platform="$(ci_platform)"
    if [ "$platform" != local ]; then
        echo "$platform"
        return
    fi
    url="$(git config --get remote.origin.url 2>/dev/null || true)"
    # scp-style (git@host:path) and URL-style (scheme://[user@]host[:port]/path).
    host="$(sed -E 's#^[a-z+]+://##; s#^[^@/]*@##; s#[:/].*$##' <<<"$url" | tr '[:upper:]' '[:lower:]')"
    case "$host" in
        github.com|*.github.com) echo github ;;
        gitlab.com|*gitlab*|git.*) echo gitlab ;;
        *) echo unknown ;;
    esac
}

# The commit being built.
ci_sha() {
    echo "${GITHUB_SHA:-${CI_COMMIT_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}}"
}

# The tag being built, or nothing when this is not a tag pipeline.
ci_tag() {
    if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
        echo "${GITHUB_REF_NAME:-}"
    else
        echo "${CI_COMMIT_TAG:-}"
    fi
}

# The runner executing this job, for records that must name a real one.
ci_runner() {
    echo "${RUNNER_NAME:-${CI_RUNNER_DESCRIPTION:-$(hostname 2>/dev/null || echo unknown)}}"
}

# Whether the runner is one the project provisions itself, where releases are signed,
# rather than one the forge hosts. GitHub says so in RUNNER_ENVIRONMENT, and a GitHub
# runner that does not say is taken to be self-hosted. A GitLab job that asks is a
# release job, which runs only on the `kms` runner, so every GitLab job is. A
# developer's shell is not. [VLD-ANB-MIDGARD-0035]
ci_self_hosted() {
    case "$(ci_platform)" in
        github) [ "${RUNNER_ENVIRONMENT:-self-hosted}" != github-hosted ] ;;
        gitlab) return 0 ;;
        *) return 1 ;;
    esac
}

# Where this run can be looked up, or nothing outside CI.
ci_run_url() {
    if [ -n "${GITHUB_RUN_ID:-}" ]; then
        echo "${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID}"
    else
        echo "${CI_PIPELINE_URL:-}"
    fi
}

# The commit a change set is measured from: the merge target on a pull or merge
# request, the commit the branch moved from on a push, else nothing. An all-zero
# "before" is a new branch, which has no prior state.
ci_base_ref() {
    local zero=0000000000000000000000000000000000000000 before
    if [ -n "${GITHUB_BASE_REF:-}" ]; then
        echo "origin/${GITHUB_BASE_REF}"
    elif [ -n "${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}" ]; then
        echo "${CI_MERGE_REQUEST_DIFF_BASE_SHA}"
    else
        before="${MIDGARD_DIFF_BASE:-${CI_COMMIT_BEFORE_SHA:-}}"
        if [ -n "$before" ] && [ "$before" != "$zero" ]; then
            echo "$before"
        fi
    fi
}

# An error the CI system should surface, optionally pinned to a file.
#   ci_error "message"            ci_error "message" path/to/file
ci_error() {
    local message="$1" file="${2:-}"
    case "$(ci_platform)" in
        github)
            if [ -n "$file" ]; then
                echo "::error file=$file::$message"
            else
                echo "::error::$message"
            fi
            ;;
        gitlab) printf '\033[31;1mERROR:\033[0m %s%s\n' "${file:+$file: }" "$message" ;;
        *) echo "ERROR: ${file:+$file: }$message" >&2 ;;
    esac
}

ci_warning() {
    local message="$1"
    case "$(ci_platform)" in
        github) echo "::warning::$message" ;;
        gitlab) printf '\033[33;1mWARNING:\033[0m %s\n' "$message" ;;
        *) echo "WARNING: $message" >&2 ;;
    esac
}

# A collapsible log section. Pair every ci_group with a ci_endgroup.
_ci_section=""
ci_group() {
    local title="$1"
    case "$(ci_platform)" in
        github) echo "::group::$title" ;;
        gitlab)
            _ci_section="section_$(date +%s)_$RANDOM"
            printf '\033[0Ksection_start:%s:%s[collapsed=true]\r\033[0K%s\n' "$(date +%s)" "$_ci_section" "$title"
            ;;
        *) echo "--- $title" ;;
    esac
}

ci_endgroup() {
    case "$(ci_platform)" in
        github) echo "::endgroup::" ;;
        gitlab) printf '\033[0Ksection_end:%s:%s\r\033[0K\n' "$(date +%s)" "$_ci_section" ;;
        *) : ;;
    esac
}

# Hand `key=value` to later steps or jobs: a step output on GitHub, a dotenv report
# on GitLab (the job declares `artifacts:reports:dotenv: ci.env`), stdout otherwise.
ci_output() {
    local key="$1" value="$2"
    case "$(ci_platform)" in
        github) echo "$key=$value" >> "${GITHUB_OUTPUT:-/dev/null}" ;;
        gitlab) echo "$key=$value" >> "${MIDGARD_CI_DOTENV:-ci.env}" ;;
        *) : ;;
    esac
    echo "$key=$value"
}

# A line for the run's summary page, where the system has one.
ci_summary() {
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
        echo "$1" >> "$GITHUB_STEP_SUMMARY"
    fi
    echo "$1"
}

# The ref a release must be on. Where a forge can be reached that is origin/main as the
# remote has it; with no forge, or none online, MIDGARD_MAINLINE_REF names the local ref
# that is main (refs/heads/main) and nothing is fetched. No step of a release needs a
# forge (docs/ops/PKI.md, "Releasing without a forge").
mainline_ref() {
    echo "${MIDGARD_MAINLINE_REF:-refs/remotes/origin/main}"
}

# Bring origin/main up to date from the remote, when there is one to reach. A failed
# fetch is a warning, never a refusal: the main this checkout already has stands in.
# With MIDGARD_MAINLINE_REF set there is nothing to fetch.
refresh_mainline() {
    [ -n "${MIDGARD_MAINLINE_REF:-}" ] && return 0
    GIT_TERMINAL_PROMPT=0 git fetch --quiet --no-tags origin '+refs/heads/main:refs/remotes/origin/main' 2>/dev/null ||
        ci_warning "${1:-ci}: could not fetch main from origin; using the origin/main already here"
}
