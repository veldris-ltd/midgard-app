#!/usr/bin/env bash
# Tests for where an audit record may come from, in deploy/acceptance/verify-shell-audits.sh,
# against throwaway git trees, and a check that this repository commits no record.
# [RC-49] [AC-C-11]
#
#   bash tools/ci/tests/shell-audit-gate.test.sh
#
# The claims under test: in CI a record counts only when the commit it audited is the
# commit being gated or an ancestor of it; outside CI that is a warning. A record
# committed to the tree never counts: records are written by the audit jobs and reach
# the gate as artifacts. And no record is committed here.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
script="$repo/deploy/acceptance/verify-shell-audits.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

g() { git -C "$1" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "${@:2}"; }

# tree NAME: a git repository with six declared shells and two commits; prints its root.
tree() {
    local root="$work/$1" shell
    mkdir -p "$root"
    git init -q "$root"
    for shell in apple android linux-gtk desktop-tauri web extension; do
        mkdir -p "$root/clients/shells/$shell/audits"
        echo 'screen_reader = "conformant"' > "$root/clients/shells/$shell/accessibility.toml"
        touch "$root/clients/shells/$shell/audits/.gitkeep"
    done
    g "$root" add -A
    g "$root" commit -q -m one
    echo two > "$root/two"
    g "$root" add two
    g "$root" commit -q -m two
    echo "$root"
}

# records ROOT COMMIT: a passing record for every shell, auditing COMMIT.
records() {
    local shell
    for shell in apple android linux-gtk desktop-tauri web extension; do
        printf '%s\n' '[audit]' "shell = \"$shell\"" 'release = "1.0.0"' "commit = \"$2\"" 'runner = "r1"' \
            'device = "d1"' 'tool = "t1"' 'overall = "pass"' '[results]' 'screen_reader = "pass"' \
            > "$1/clients/shells/$shell/audits/1.0.0.toml"
    done
}

# gate ROOT [VAR=value...]: run the gate outside CI unless told otherwise; output in $out.
gate() {
    local root="$1"; shift
    out="$(env -u CI -u GITHUB_SHA -u CI_COMMIT_SHA "$@" SHELL_AUDIT_ROOT="$root" bash "$script" 1.0.0 2>&1)"
}

root="$(tree t)"
head="$(g "$root" rev-parse HEAD)"
first="$(g "$root" rev-parse HEAD~1)"

records "$root" "$head"
if gate "$root" CI=true GITHUB_SHA="$head"; then pass "CI: a record of the gated commit counts"; else
    fail "CI, same commit: $out"; fi

records "$root" "$first"
if gate "$root" CI=true GITHUB_SHA="$head"; then pass "CI: a record of an ancestor counts"; else
    fail "CI, ancestor: $out"; fi
if gate "$root" CI=true CI_COMMIT_SHA="$head"; then pass "CI: GitLab's commit variable is read too"; else
    fail "CI, GitLab sha: $out"; fi

g "$root" checkout -q -b side "$first"
echo side > "$root/side"
g "$root" add side
g "$root" commit -q -m side
side="$(g "$root" rev-parse HEAD)"
g "$root" checkout -q -
records "$root" "$side"
if gate "$root" CI=true GITHUB_SHA="$head"; then fail "CI: a record of another branch's commit counts"; else
    grep -q 'is not the gated commit or an ancestor' <<<"$out" && pass "CI: a record of a commit not in the history is refused" ||
        fail "CI, other branch, reason: $out"
fi

records "$root" unknown
if gate "$root" CI=true GITHUB_SHA="$head"; then fail "CI: a record of an unknown commit counts"; else
    pass "CI: a record naming no commit is refused"; fi

records "$root" "$side"
if gate "$root"; then
    grep -q 'warning: .*is not the checked-out commit or an ancestor' <<<"$out" &&
        pass "outside CI: a record of another commit passes with a warning" || fail "outside CI, warning: $out"
else
    fail "outside CI keeps its behaviour: $out"
fi

records "$root" "$head"
g "$root" add clients/shells/apple/audits/1.0.0.toml
if gate "$root" CI=true GITHUB_SHA="$head"; then fail "a committed record counts"; else
    grep -q 'apple: .*is committed to the tree' <<<"$out" && pass "a record committed to the tree is refused" ||
        fail "committed record, reason: $out"
fi

# This repository: nothing under clients/shells/*/audits is tracked but .gitkeep.
tracked="$(git -C "$repo" ls-files -- 'clients/shells/*/audits/*' | grep -v '/\.gitkeep$' || true)"
if [ -z "$tracked" ]; then pass "no audit record is committed"; else fail "committed audit records: $tracked"; fi

[ "$failures" -eq 0 ] || exit 1
