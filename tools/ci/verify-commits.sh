#!/usr/bin/env bash
# Every commit a change brings to the protected history is signed by a key the project
# lists, or is a merge the forge made, or the change is refused. [VLD-ANB-MIDGARD-0069]
# [AC-P1.1]
#
#   bash tools/ci/verify-commits.sh [BASE [HEAD]]
#
# The change set is BASE..HEAD. BASE defaults to the one tools/ci/lib.sh resolves on
# both forges (ci_base_ref): the merge target of a pull or merge request, or the commit
# a push to main or dev moved the branch from. With none — a developer's shell, a new
# branch — it is the merge base with origin/main; a tag pipeline brings no commits, and
# its tag is verified by `release-tag`. HEAD defaults to the checkout, except where the
# CI system checked out a merge it made for the run, which never enters history: on a
# GitHub pull request that is HEAD's second parent, on a GitLab merged-results pipeline
# CI_MERGE_REQUEST_SOURCE_BRANCH_SHA.
#
# A commit passes when, in this order,
#   1. git verifies its SSH signature against deploy/trust/allowed_signers as the BASE
#      commit has it, for the namespace "git". The base's copy, not the change's: a
#      change that adds a key cannot vouch for itself, so a key is added by a commit
#      signed with a key already listed (docs/ops/PKI.md, "Git signing");
#   2. or it is a merge made by the forge — two parents, a committer in
#      MIDGARD_MERGE_COMMITTERS (default noreply@github.com; on CodeLab, set it to the
#      identity merges are made as) — and either
#      a. on GitHub with a token (GITHUB_TOKEN or GH_TOKEN, read-only), the forge says it
#         is verified: `gh api repos/$GITHUB_REPOSITORY/commits/<sha>`,
#         .commit.verification.verified, GitHub's own signature on the merges it makes;
#      b. or it adds nothing of its own: its tree is exactly what `git merge-tree
#         --write-tree` gives for its two parents, so everything it brings is in its
#         parents, which are checked like any other commit. A combined diff is not
#         enough: it hides a merge that keeps one parent's side of what the other
#         changed. A merge whose parents conflict is not exempt, whatever it resolved,
#         and neither is an octopus merge (more than two parents), which the forge
#         never makes.
#
# The forge's verdict counts for its own merges only: a commit signed by a key GitHub
# knows but allowed_signers does not list fails like any other unlisted key, so a new
# maintainer's key goes in allowed_signers first. Anything else fails too: unsigned, or
# signed by an unlisted key. One line is printed per commit, and the run exits 1 naming
# every failure. With no usable allowed_signers in the base nothing can pass, and the run
# says so before looking at a commit. VERIFY_COMMITS_ROOT names the repository (for the
# tests).
# The token is handed to gh and never printed.
set -uo pipefail

ROOT="${VERIFY_COMMITS_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
SIGNERS_PATH="deploy/trust/allowed_signers"
MERGE_COMMITTERS="${MIDGARD_MERGE_COMMITTERS:-noreply@github.com}"
. "$(dirname "$0")/lib.sh"

refuse() {
    ci_error "verify-commits: $1"
    shift
    for line in "$@"; do echo "  $line" >&2; done
    exit 1
}

cd "$ROOT" || exit 2
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

base="${1:-$(ci_base_ref)}"
head="${2:-}"
if [ -z "$head" ]; then
    head=HEAD
    if [ "$(ci_platform)" = github ] && [[ "${GITHUB_EVENT_NAME:-}" == pull_request* ]] &&
        git rev-parse --verify --quiet 'HEAD^2' >/dev/null; then
        head='HEAD^2'
    elif [ -n "${CI_MERGE_REQUEST_SOURCE_BRANCH_SHA:-}" ]; then
        head="$CI_MERGE_REQUEST_SOURCE_BRANCH_SHA"
    fi
fi
if [ -z "$base" ]; then
    if [ -n "$(ci_tag)" ]; then
        echo "verify-commits: a tag pipeline brings no commits; release-tag verifies the tag"
        exit 0
    fi
    git rev-parse --verify --quiet refs/remotes/origin/main >/dev/null ||
        refuse "there is no base to measure the change from, and no origin/main to fall back on" \
            "Name one: bash tools/ci/verify-commits.sh <base> [<head>]."
    base="$(git merge-base "$head" refs/remotes/origin/main)" ||
        refuse "$head shares no history with origin/main"
fi
base_sha="$(git rev-parse --verify --quiet "$base^{commit}")" ||
    refuse "the base $base is not a commit here" \
        "A shallow checkout may not have fetched it: check out with the full history (fetch-depth: 0)."
head_sha="$(git rev-parse --verify --quiet "$head^{commit}")" || refuse "$head is not a commit here"

# The signers, as the base has them.
signers="$work/allowed_signers"
signers_ok=0 why=""
if ! git show "$base_sha:$SIGNERS_PATH" > "$signers" 2>/dev/null; then
    why="$SIGNERS_PATH does not exist at the base, ${base_sha:0:12}"
elif [ ! -s "$signers" ]; then
    why="$SIGNERS_PATH is empty at the base, ${base_sha:0:12}"
elif grep -q 'PRIVATE KEY' "$signers"; then
    refuse "$SIGNERS_PATH holds private key material; it must hold public keys only"
elif ! grep -Ev '^[[:space:]]*(#|$)' "$signers" | grep -q 'namespaces="[^"]*git'; then
    why="$SIGNERS_PATH lists no key for the namespace \"git\" at the base, ${base_sha:0:12}"
else
    signers_ok=1
fi

# The forge, when there is one to ask.
api=0
if [ "$(ci_forge)" = github ] && [ -n "${GITHUB_TOKEN:-}${GH_TOKEN:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ] &&
    command -v gh >/dev/null 2>&1; then
    api=1
    export GH_TOKEN="${GH_TOKEN:-$GITHUB_TOKEN}"
fi

[ "$signers_ok" = 1 ] || refuse "$why: no commit can be verified" \
    "The maintainers' public signing keys go in $SIGNERS_PATH (docs/ops/PKI.md, \"Git signing\")."

# A forge merge is judged by merging its parents again, with `git merge-tree
# --write-tree`, which is git 2.38 or later (the CI images have 2.39 and newer). Asked
# for the first time a merge needs it, so an older git fails the run rather than the
# merge.
need_merge_tree() {
    [ -z "${merge_tree_ok:-}" ] || return 0
    local v major minor
    v="$(git version)" v="${v#git version }"
    major="${v%%.*}" minor="${v#*.}" minor="${minor%%.*}"
    if ! [[ "$major" =~ ^[0-9]+$ && "$minor" =~ ^[0-9]+$ ]] || [ "$major" -lt 2 ] ||
        { [ "$major" = 2 ] && [ "$minor" -lt 38 ]; }; then
        refuse "git $v cannot check a merge: \`git merge-tree --write-tree\` needs git 2.38 or later" \
            "Run the job with a newer git; the toolchain image and the CI runners have one."
    fi
    merge_tree_ok=1
}

total=0 failed=()
while read -r sha; do
    [ -n "$sha" ] || continue
    total=$((total + 1))
    line="$(git log -1 --format='%h %s' "$sha")"
    if out="$(git -c gpg.format=ssh -c gpg.ssh.allowedSignersFile="$signers" verify-commit "$sha" 2>&1)" &&
        signer="$(grep -o 'Good "git" signature for [^ ]*' <<<"$out" | head -n 1)" && [ -n "$signer" ]; then
        echo "ok      $line — signed by ${signer#Good \"git\" signature for }"
        continue
    fi
    parents=$(($(git rev-list --parents -n 1 "$sha" | wc -w) - 1))
    committer="$(git log -1 --format='%ce' "$sha")"
    if [ "$parents" -ge 2 ] && [[ " $MERGE_COMMITTERS " == *" $committer "* ]]; then
        if [ "$parents" -gt 2 ]; then
            reason="an octopus merge by $committer: the forge merges two parents at a time"
        elif [ "$api" = 1 ] &&
            [ "$(gh api "repos/$GITHUB_REPOSITORY/commits/$sha" --jq '.commit.verification.verified' 2>/dev/null)" = true ]; then
            echo "ok      $line — a merge by $committer, verified by GitHub"
            continue
        else
            need_merge_tree
            merged="$(git merge-tree --write-tree "$sha^1" "$sha^2" 2>/dev/null)"
            rc=$?
            if [ "$rc" = 0 ] && [ "$(head -n 1 <<<"$merged")" = "$(git rev-parse "$sha^{tree}")" ]; then
                echo "ok      $line — a merge by $committer that adds nothing of its own"
                continue
            elif [ "$rc" = 0 ]; then
                reason="a merge by $committer that changes what its parents did not"
            elif [ "$rc" = 1 ]; then
                reason="a merge by $committer whose parents conflict, so its resolution is its own"
            else
                reason="a merge by $committer whose parents git cannot merge again"
            fi
        fi
    elif ! git cat-file commit "$sha" | grep -q '^gpgsig'; then
        reason="not signed"
    else
        reason="signed, but not by a key $SIGNERS_PATH lists"
    fi
    echo "FAILED  $line — $reason"
    failed+=("${sha:0:12}")
done < <(git rev-list --reverse "$base_sha..$head_sha")

range="${base_sha:0:12}..${head_sha:0:12}"
if [ "${#failed[@]}" -gt 0 ]; then
    refuse "${#failed[@]} of $total commit(s) in $range cannot be verified: ${failed[*]}" \
        "Sign them and push again: CONTRIBUTING.md, \"Branches and the public tree\"."
fi
if [ "$total" = 0 ]; then
    echo "verify-commits: $range brings no commits"
else
    echo "verify-commits: all $total commit(s) in $range are signed, or merges the forge made"
fi
