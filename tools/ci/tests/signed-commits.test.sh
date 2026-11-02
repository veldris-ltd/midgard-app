#!/usr/bin/env bash
# Tests for tools/ci/signed-commits.sh, against a throwaway repository and keys.
# [AC-25.4] (VLD-ANB-MIDGARD-0088)
#
#   bash tools/ci/tests/signed-commits.test.sh
#
# The claim under test: a change passes only when every commit in its range is signed by
# a key deploy/trust/allowed_signers lists, and a failure names the commit at fault.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../signed-commits.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v ssh-keygen >/dev/null 2>&1 || { echo "skip  signed-commits tests (ssh-keygen is not on PATH)"; exit 0; }

ssh-keygen -q -t ed25519 -N '' -C author -f "$work/author" >/dev/null
ssh-keygen -q -t ed25519 -N '' -C stranger -f "$work/stranger" >/dev/null

repo="$work/repo"
mkdir -p "$repo/deploy/trust"
git init -q "$repo"
git -C "$repo" config core.autocrlf false
g() { git -C "$repo" -c user.name=test -c user.email=author@example.invalid -c commit.gpgsign=false "$@"; }
# commit MESSAGE [KEY]: a commit, signed with KEY when one is named
commit() {
    echo "$1" >> "$repo/log.txt"
    g add -A
    if [ -n "${2:-}" ]; then
        g -c gpg.format=ssh -c user.signingkey="$work/$2" commit -q -S -m "$1"
    else
        g commit -q -m "$1"
    fi
    g rev-parse HEAD
}
check() {
    (cd "$repo" && unset GITHUB_BASE_REF CI_MERGE_REQUEST_DIFF_BASE_SHA MIDGARD_DIFF_BASE CI_COMMIT_BEFORE_SHA &&
        SIGNED_COMMITS_ROOT="$repo" bash "$script" "$@" 2>&1)
}

printf 'author@example.invalid namespaces="git" %s\n' "$(cat "$work/author.pub")" > "$repo/deploy/trust/allowed_signers"
base="$(commit base author)"

signed="$(commit "signed change" author)"
if out="$(check "$base..HEAD")"; then
    grep -q 'all 1 commit(s)' <<<"$out" && pass "a range whose commits are all signed by a listed key passes" \
        || fail "the passing run says what it checked: $out"
else
    fail "a range whose commits are all signed by a listed key passes: $out"
fi

unsigned="$(commit "unsigned change")"
if out="$(check "$base..HEAD")"; then
    fail "a range holding an unsigned commit fails"
else
    grep -q "commit $unsigned is not signed" <<<"$out" && ! grep -q "commit $signed" <<<"$out" \
        && pass "a range holding an unsigned commit fails, naming that commit and no other" \
        || fail "the unsigned commit is named: $out"
fi

g reset -q --hard "$signed"
foreign="$(commit "signed by a stranger" stranger)"
if out="$(check "$base..HEAD")"; then
    fail "a commit signed by an unlisted key fails"
else
    grep -q "commit $foreign is signed, but not by a key valid" <<<"$out" \
        && pass "a commit signed by a key the file does not list fails, and is named" \
        || fail "the stranger's commit is named: $out"
fi

g reset -q --hard "$signed"
mv "$repo/deploy/trust/allowed_signers" "$work/allowed_signers"
g add -A && g -c gpg.format=ssh -c user.signingkey="$work/author" commit -q -S -m "drop signers"
if out="$(check "$base..HEAD")"; then fail "a missing allowed_signers is a refusal"; else
    grep -q 'allowed_signers is missing' <<<"$out" && pass "a missing allowed_signers is a refusal, never a pass" \
        || fail "the missing file is named: $out"
fi
mv "$work/allowed_signers" "$repo/deploy/trust/allowed_signers"

# With no range named, the base comes from the CI system, as for the other range jobs.
g reset -q --hard "$signed"
if out="$(cd "$repo" && SIGNED_COMMITS_ROOT="$repo" MIDGARD_DIFF_BASE="$base" \
    GITHUB_BASE_REF='' CI_MERGE_REQUEST_DIFF_BASE_SHA='' bash "$script" 2>&1)"; then
    grep -q "in $base..HEAD" <<<"$out" && pass "with no range named, it checks MIDGARD_DIFF_BASE..HEAD" \
        || fail "the default range: $out"
else
    fail "with no range named, it checks MIDGARD_DIFF_BASE..HEAD: $out"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "signed-commits: all tests passed"
else
    echo "signed-commits: $failures test(s) failed"
    exit 1
fi
