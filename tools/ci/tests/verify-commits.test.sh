#!/usr/bin/env bash
# Tests for tools/ci/verify-commits.sh, against a throwaway repository and keys, and a
# stub gh that never reaches GitHub. [VLD-ANB-MIDGARD-0069] [AC-P1.1]
#
#   bash tools/ci/tests/verify-commits.test.sh
#
# The claim under test: a change set passes only when every commit in it is signed by a
# key the base's allowed_signers lists, or is a two-parent merge by the forge that the
# forge verifies or that adds nothing of its own; an unsigned commit, one signed by an
# unlisted key (whatever the forge says of it), and one signed by a key the change itself
# adds are refused and named; with no usable allowed_signers, nothing passes.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../verify-commits.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v ssh-keygen >/dev/null 2>&1 || { echo "skip  verify-commits tests (ssh-keygen is not on PATH)"; exit 0; }

ssh-keygen -q -t ed25519 -N '' -C maintainer -f "$work/maintainer" >/dev/null
ssh-keygen -q -t ed25519 -N '' -C stranger -f "$work/stranger" >/dev/null
listed() { printf '%s@example.invalid namespaces="git" %s\n' "$1" "$(cat "$work/$1.pub")"; }

repo="$work/repo"
mkdir -p "$repo/deploy/trust"
git init -q -b main "$repo"
git -C "$repo" config core.autocrlf false
g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
# commit_as KEY|- MESSAGE: a commit touching a file of its own, signed by KEY or unsigned.
commit_as() {
    echo "$2" > "$repo/$2.txt"
    g add -A
    if [ "$1" = - ]; then g commit -q -m "$2"; else g -c gpg.format=ssh -c user.signingkey="$work/$1" commit -q -S -m "$2"; fi
}
# forge_merge BRANCH: merge BRANCH as GitHub does, unsigned, committed by noreply@github.com.
forge_merge() {
    GIT_COMMITTER_NAME=GitHub GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff -m "Merge $1" "$1"
}
# verify [VAR=VALUE...] BASE: run the script on the repository, with no CI system's
# settings but those named, so the result is the same inside a pipeline as outside.
verify() {
    local vars=()
    while [ "$#" -gt 0 ] && [[ "$1" == *=* ]]; do vars+=("$1"); shift; done
    (cd "$repo" && env -u GITHUB_ACTIONS -u GITLAB_CI -u GITHUB_TOKEN -u GH_TOKEN -u GITHUB_REPOSITORY \
        -u GITHUB_BASE_REF -u GITHUB_EVENT_NAME -u CI_MERGE_REQUEST_DIFF_BASE_SHA -u CI_COMMIT_BEFORE_SHA \
        -u CI_MERGE_REQUEST_SOURCE_BRANCH_SHA -u MIDGARD_DIFF_BASE -u MIDGARD_MERGE_COMMITTERS -u MIDGARD_FORGE \
        -u GITHUB_REF_TYPE -u CI_COMMIT_TAG VERIFY_COMMITS_ROOT="$repo" "${vars[@]}" bash "$script" "$@")
}
short() { git -C "$repo" rev-parse --short=7 "$1"; }

listed maintainer > "$repo/deploy/trust/allowed_signers"
echo readme > "$repo/README.md"
g add -A && g commit -q -m base
g tag base

# --- signed by a listed key ------------------------------------------------------------
commit_as maintainer signed
if out="$(verify base 2>&1)"; then
    grep -q "^ok      $(short HEAD) signed — signed by maintainer@example.invalid" <<<"$out" &&
        pass "a commit signed by a listed key passes, and the signer is named" || fail "the signer is named: $out"
else
    fail "a commit signed by a listed key passes: $out"
fi

# --- unsigned, and signed by an unlisted key: refused and named --------------------------
commit_as - unsigned
unsigned="$(short HEAD)"
if out="$(verify base 2>&1)"; then fail "an unsigned commit is refused"; else
    grep -q "^FAILED  $unsigned unsigned — not signed" <<<"$out" && grep -q "cannot be verified: $unsigned" <<<"$out" &&
        pass "an unsigned commit is refused, and named" || fail "an unsigned commit is named: $out"
fi
commit_as stranger unlisted
unlisted="$(short HEAD)"
if out="$(verify base 2>&1)"; then fail "a commit signed by an unlisted key is refused"; else
    grep -q "^FAILED  $unlisted unlisted — signed, but not by a key deploy/trust/allowed_signers lists" <<<"$out" &&
        pass "a commit signed by an unlisted key is refused, and named" || fail "an unlisted key is named: $out"
    grep -q "2 of 3 commit(s) in .* cannot be verified: $unsigned[0-9a-f]* $unlisted" <<<"$out" &&
        pass "every failure is named, not just the first" || fail "every failure is named: $out"
fi
g reset -q --hard base

# --- a key the change adds cannot vouch for the change ------------------------------------
listed stranger >> "$repo/deploy/trust/allowed_signers"
g add -A && g -c gpg.format=ssh -c user.signingkey="$work/stranger" commit -q -S -m "add a key"
verify base >/dev/null 2>&1 && fail "a commit signed by a key it adds itself is refused" ||
    pass "a commit signed by a key it adds itself is refused: the base's allowed_signers decides"
g tag with-stranger
commit_as stranger later
verify with-stranger >/dev/null 2>&1 && pass "once the key is in the base, its commits pass" ||
    fail "once the key is in the base, its commits pass"
g reset -q --hard base

# --- merges by the forge ----------------------------------------------------------------
g checkout -q -b topic
commit_as maintainer topic
g checkout -q main
commit_as maintainer mainline
forge_merge topic
if out="$(verify base 2>&1)"; then
    grep -q "^ok      $(short HEAD) Merge topic — a merge by noreply@github.com that adds nothing of its own" <<<"$out" &&
        pass "a forge merge (noreply@github.com, two parents) is accepted" || fail "the forge merge is reported: $out"
else
    fail "a forge merge (noreply@github.com, two parents) is accepted: $out"
fi
g reset -q --hard HEAD^
g checkout -q -b evil base && commit_as maintainer evil && g checkout -q main
GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff --no-commit evil >/dev/null
echo smuggled > "$repo/README.md" && g add -A
GIT_COMMITTER_EMAIL=noreply@github.com g commit -q -m "Merge evil"
if out="$(verify base 2>&1)"; then fail "a forge merge that changes something of its own is refused"; else
    grep -q 'a merge by noreply@github.com that changes what its parents did not' <<<"$out" &&
        pass "a forge merge that changes something of its own is refused" || fail "the evil merge is named: $out"
fi
g reset -q --hard HEAD^
# Keeping one parent's side of what the other changed leaves the combined diff empty;
# the tree is still not the merge of its parents.
GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff -s ours -m "Merge evil, dropping it" evil
if out="$(verify base 2>&1)"; then fail "a forge merge that silently drops a parent's change is refused: $out"; else
    grep -q "^FAILED  $(short HEAD) Merge evil, dropping it — a merge by noreply@github.com that changes what its parents did not" <<<"$out" &&
        pass "a forge merge that silently drops a parent's change is refused" || fail "the dropping merge is named: $out"
fi
g reset -q --hard HEAD^
g checkout -q -b clash base && echo theirs > "$repo/README.md" && g add -A &&
    g -c gpg.format=ssh -c user.signingkey="$work/maintainer" commit -q -S -m clash
g checkout -q main && echo ours > "$repo/README.md" && g add -A &&
    g -c gpg.format=ssh -c user.signingkey="$work/maintainer" commit -q -S -m "clash on main"
GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff --no-commit clash >/dev/null 2>&1
echo ours > "$repo/README.md" && g add -A
GIT_COMMITTER_EMAIL=noreply@github.com g commit -q -m "Merge clash"
if out="$(verify base 2>&1)"; then fail "a forge merge whose parents conflict is refused: $out"; else
    grep -q "^FAILED  $(short HEAD) Merge clash — a merge by noreply@github.com whose parents conflict" <<<"$out" &&
        pass "a forge merge whose parents conflict is refused, whatever its resolution" || fail "the conflicted merge is named: $out"
fi
g reset -q --hard HEAD~2
GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff -m "Octopus" evil topic >/dev/null
if out="$(verify base 2>&1)"; then fail "an octopus merge by the forge is refused: $out"; else
    grep -q "^FAILED  $(short HEAD) Octopus — an octopus merge by noreply@github.com" <<<"$out" &&
        pass "an octopus merge by the forge is refused" || fail "the octopus merge is named: $out"
fi
g reset -q --hard HEAD^
forge_merge topic
mkdir -p "$work/oldgit"
cat > "$work/oldgit/git" <<EOF
#!/usr/bin/env bash
case "\$1" in version|--version) echo "git version 2.37.1"; exit 0 ;; esac
exec "$(command -v git)" "\$@"
EOF
chmod +x "$work/oldgit/git"
if out="$(verify "PATH=$work/oldgit:$PATH" base 2>&1)"; then fail "a git older than 2.38 cannot judge a merge, and says so: $out"; else
    grep -q 'git 2.37.1 cannot check a merge: .*2.38 or later' <<<"$out" &&
        pass "a git older than 2.38 cannot judge a merge, and says so" || fail "the old git is named: $out"
fi
g reset -q --hard HEAD^
g merge -q --no-ff -m "Merge evil by hand" evil
verify base >/dev/null 2>&1 && fail "an unsigned merge by anyone but the forge is refused" ||
    pass "an unsigned merge by anyone but the forge is refused"
g reset -q --hard base
echo forged > "$repo/forged.txt" && g add -A
GIT_COMMITTER_EMAIL=noreply@github.com g commit -q -m forged
verify base >/dev/null 2>&1 && fail "the forge's identity on an unsigned non-merge commit is refused" ||
    pass "the forge's identity on an unsigned non-merge commit is refused"
g reset -q --hard base

# --- no usable allowed_signers: fail closed ----------------------------------------------
for state in empty missing; do
    g checkout -q -b "$state" base
    if [ "$state" = empty ]; then : > "$repo/deploy/trust/allowed_signers"; else g rm -q deploy/trust/allowed_signers; fi
    g add -A && g commit -q -m "$state signers"
    g tag "$state-base"
    commit_as maintainer "after-$state"
    if out="$(verify "$state-base" 2>&1)"; then fail "allowed_signers $state at the base fails closed"; else
        grep -q 'no commit can be verified' <<<"$out" && grep -q 'docs/ops/PKI.md, "Git signing"' <<<"$out" &&
            pass "allowed_signers $state at the base fails closed, pointing to PKI.md" || fail "allowed_signers $state says why: $out"
    fi
    g checkout -q main
done
sed 's/namespaces="git"/namespaces="file"/' "$repo/deploy/trust/allowed_signers" > "$work/file-ns"
g checkout -q -b other-ns base && cp "$work/file-ns" "$repo/deploy/trust/allowed_signers" && g commit -q -am "file namespace"
g tag other-ns-base && commit_as maintainer after-ns
verify other-ns-base >/dev/null 2>&1 && fail "a key listed for another namespace signs no commit" ||
    pass "a key listed for another namespace signs no commit"
g checkout -q main

# --- the forge's verdict, from a stub gh ---------------------------------------------------
mkdir -p "$work/stubs"
cat > "$work/stubs/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >> "$STUB_LOG"
case "$*" in
    "api repos/veldris-ltd/midgard-app/commits/"*" --jq .commit.verification.verified") echo "${STUB_VERIFIED:-false}" ;;
    *) echo "stub gh: unexpected call: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$work/stubs/gh"
forge=(MIDGARD_FORGE=github GITHUB_REPOSITORY=veldris-ltd/midgard-app GITHUB_TOKEN=not-a-real-token
       "PATH=$work/stubs:$PATH" "STUB_LOG=$work/gh.log")
g reset -q --hard base
commit_as stranger registered-on-github
if out="$(verify "${forge[@]}" STUB_VERIFIED=true base 2>&1)"; then
    fail "on GitHub, a commit the forge verifies but allowed_signers does not list is refused: $out"
else
    grep -q "^FAILED  $(short HEAD) registered-on-github — signed, but not by a key deploy/trust/allowed_signers lists" <<<"$out" &&
        ! grep -q not-a-real-token <<<"$out" &&
        pass "on GitHub, a commit the forge verifies but allowed_signers does not list is refused, and the token is not printed" ||
        fail "the unlisted key is named: $out"
fi
! grep -q "commits/$(git -C "$repo" rev-parse HEAD)" "$work/gh.log" 2>/dev/null &&
    pass "the forge is not asked about a commit that is not its own merge" || fail "the forge is asked only about its merges"
verify "${forge[@]}" STUB_VERIFIED=true missing-base >/dev/null 2>&1 &&
    fail "with no allowed_signers, GitHub's verification lets no commit through" ||
    pass "with no allowed_signers, GitHub's verification lets no commit through"
# A merge GitHub made and signed, with a change of its own (a conflict resolved on the
# web): only the forge's verdict can pass it.
g reset -q --hard base
g checkout -q -b gh-topic && commit_as maintainer gh-topic && g checkout -q main
GIT_COMMITTER_EMAIL=noreply@github.com g merge -q --no-ff --no-commit gh-topic >/dev/null
echo resolved > "$repo/README.md" && g add -A
GIT_COMMITTER_EMAIL=noreply@github.com g commit -q -m "Merge gh-topic"
if out="$(verify "${forge[@]}" STUB_VERIFIED=true base 2>&1)"; then
    grep -q "^ok      $(short HEAD) Merge gh-topic — a merge by noreply@github.com, verified by GitHub" <<<"$out" &&
        pass "on GitHub, a two-parent merge by the forge that it verifies passes" || fail "the forge's verdict on its merge is reported: $out"
else
    fail "on GitHub, a two-parent merge by the forge that it verifies passes: $out"
fi
verify "${forge[@]}" STUB_VERIFIED=false base >/dev/null 2>&1 && fail "on GitHub, a merge the forge does not verify is judged as any merge" ||
    pass "on GitHub, a merge the forge does not verify is judged as any merge"
verify MIDGARD_FORGE=github GITHUB_REPOSITORY=veldris-ltd/midgard-app "PATH=$work/stubs:$PATH" "STUB_LOG=$work/gh.log" \
    STUB_VERIFIED=true base >/dev/null 2>&1 && fail "without a token the forge is not asked" ||
    pass "without a token the forge is not asked"

# --- the change set itself -------------------------------------------------------------------
g reset -q --hard base
out="$(verify base 2>&1)" && grep -q 'brings no commits' <<<"$out" && pass "an empty change set passes" ||
    fail "an empty change set passes: $out"
verify no-such-ref >/dev/null 2>&1 && fail "a base that is not here is refused" || pass "a base that is not here is refused"
commit_as - pushed
if out="$(verify MIDGARD_DIFF_BASE="$(git -C "$repo" rev-parse base)" 2>&1)"; then fail "the base comes from the push when none is named"; else
    grep -q "FAILED  $(short HEAD) pushed" <<<"$out" && pass "the base comes from the push when none is named" ||
        fail "the base comes from the push when none is named: $out"
fi
verify GITLAB_CI=true CI_COMMIT_TAG=v1.0.0 >/dev/null 2>&1 && pass "a tag pipeline brings no commits" ||
    fail "a tag pipeline brings no commits"
verify >/dev/null 2>&1 && fail "with no base and no origin/main, it refuses" || pass "with no base and no origin/main, it refuses"

if [ "$failures" -gt 0 ]; then
    echo "verify-commits tests: $failures failure(s)"
    exit 1
fi
echo "verify-commits tests: all passed"
