#!/usr/bin/env bash
# Tests for tools/ci/lib.sh: which CI system and which forge a checkout is on, and
# where each records the facts a job needs. [NFR-OSS-1] [RC-07]
#
#   bash tools/ci/tests/lib.test.sh
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
lib="$here/../lib.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

# expect NAME WANT COMMAND...  — run COMMAND in a clean environment and compare.
expect() {
    local name="$1" want="$2" got
    shift 2
    got="$("$@" 2>&1)"
    if [ "$got" = "$want" ]; then
        echo "ok    $name"
    else
        echo "FAIL  $name: wanted '$want', got '$got'"
        failures=$((failures + 1))
    fi
}

# in_env VAR=VALUE... -- FUNCTION [ARG...]: call a lib function with only those
# CI variables set, so this test gives the same answer inside a pipeline as outside.
in_env() {
    local vars=()
    while [ "$1" != "--" ]; do vars+=("$1"); shift; done
    shift
    env -u GITHUB_ACTIONS -u GITLAB_CI -u GITHUB_SHA -u CI_COMMIT_SHA -u GITHUB_REF_TYPE \
        -u GITHUB_REF_NAME -u CI_COMMIT_TAG -u GITHUB_BASE_REF -u CI_MERGE_REQUEST_DIFF_BASE_SHA \
        -u MIDGARD_DIFF_BASE -u CI_COMMIT_BEFORE_SHA -u MIDGARD_FORGE -u RUNNER_NAME \
        -u CI_RUNNER_DESCRIPTION -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
        "${vars[@]}" bash -c '. "$0"; "$@"' "$lib" "$@"
}

# --- the CI system --------------------------------------------------------------------
expect "GitHub Actions is recognised" github in_env GITHUB_ACTIONS=true -- ci_platform
expect "GitLab CI is recognised" gitlab in_env GITLAB_CI=true -- ci_platform
expect "neither is a local shell" local in_env -- ci_platform

# --- the forge, from the origin remote when there is no CI system ---------------------
forge_of() {
    local repo="$work/repo-$RANDOM"
    git init -q "$repo"
    git -C "$repo" remote add origin "$1"
    (cd "$repo" && in_env -- ci_forge)
}
expect "github.com over https" github forge_of https://github.com/veldris-ltd/midgard-app.git
expect "github.com over ssh" github forge_of git@github.com:veldris-ltd/midgard-app.git
expect "gitlab.com" gitlab forge_of https://gitlab.com/group/midgard-app.git
expect "a self-hosted GitLab at git.<domain>" gitlab forge_of https://git.veldris.dev/veldris/midgard-app.git
expect "a self-hosted GitLab over ssh with a port" gitlab forge_of ssh://git@gitlab.example.org:2222/group/app.git
expect "an unrecognised host is not guessed at" unknown forge_of https://code.example.org/app.git
expect "the CI system outranks the remote" gitlab in_env GITLAB_CI=true -- ci_forge
expect "MIDGARD_FORGE overrides everything" gitlab in_env GITHUB_ACTIONS=true MIDGARD_FORGE=gitlab -- ci_forge

# --- the commit, the tag, the change base ----------------------------------------------
expect "the commit on GitHub" abc123 in_env GITHUB_SHA=abc123 -- ci_sha
expect "the commit on GitLab" def456 in_env CI_COMMIT_SHA=def456 -- ci_sha
expect "a tag on GitHub" v1.2.3 in_env GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v1.2.3 -- ci_tag
expect "a branch on GitHub is not a tag" "" in_env GITHUB_REF_TYPE=branch GITHUB_REF_NAME=dev -- ci_tag
expect "a tag on GitLab" v1.2.3 in_env CI_COMMIT_TAG=v1.2.3 -- ci_tag
expect "a pull request's base" origin/main in_env GITHUB_BASE_REF=main -- ci_base_ref
expect "a merge request's base" 0a1b2c in_env CI_MERGE_REQUEST_DIFF_BASE_SHA=0a1b2c -- ci_base_ref
expect "a push's previous commit, GitHub" 111aaa in_env MIDGARD_DIFF_BASE=111aaa -- ci_base_ref
expect "a push's previous commit, GitLab" 222bbb in_env CI_COMMIT_BEFORE_SHA=222bbb -- ci_base_ref
expect "a new branch has no base" "" in_env CI_COMMIT_BEFORE_SHA=0000000000000000000000000000000000000000 -- ci_base_ref

# --- errors, in each system's own form --------------------------------------------------
expect "an error annotation on GitHub" "::error file=src/a.rs::broken" in_env GITHUB_ACTIONS=true -- ci_error broken src/a.rs
expect "an error on GitLab names the file" "ERROR: src/a.rs: broken" \
    bash -c "$(declare -f in_env); lib='$lib'; in_env GITLAB_CI=true -- ci_error broken src/a.rs | sed 's/\x1b\[[0-9;]*m//g'"
expect "an error locally" "ERROR: broken" in_env -- ci_error broken

# --- handing a value on ------------------------------------------------------------------
in_env GITHUB_ACTIONS=true GITHUB_OUTPUT="$work/gh-output" -- ci_output release_version 1.2.3 >/dev/null
expect "a step output on GitHub" "release_version=1.2.3" cat "$work/gh-output"
in_env GITLAB_CI=true MIDGARD_CI_DOTENV="$work/ci.env" -- ci_output release_version 1.2.3 >/dev/null
expect "a dotenv line on GitLab" "release_version=1.2.3" cat "$work/ci.env"

if [ "$failures" -gt 0 ]; then
    echo "lib tests: $failures failure(s)"
    exit 1
fi
echo "lib tests: all passed"
