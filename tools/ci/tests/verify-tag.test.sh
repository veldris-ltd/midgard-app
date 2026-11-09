#!/usr/bin/env bash
# Tests for tools/ci/verify-tag.sh, against throwaway repositories and keys.
# [AC-RF-CA-6a] [VLD-ANB-MIDGARD-0035] [AC-P2.1] [AC-P2.2] [AC-P2.4]
#
#   bash tools/ci/tests/verify-tag.test.sh
#
# The claim under test: a release tag passes only when it is annotated, signed, and
# signed by a key the allowed-signers file lists; when the tagged commit is on main as
# the remote has it; and, where a runner pins its own allowed-signers file, only when
# the committed copy is that file, so a key the tagged commit adds verifies nothing. A
# self-hosted runner with no pinned copy verifies nothing either.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../verify-tag.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v ssh-keygen >/dev/null 2>&1 || { echo "skip  verify-tag tests (ssh-keygen is not on PATH)"; exit 0; }

ssh-keygen -q -t ed25519 -N '' -C release -f "$work/release" >/dev/null
ssh-keygen -q -t ed25519 -N '' -C stranger -f "$work/stranger" >/dev/null

repo="$work/repo"
mkdir -p "$repo/deploy/trust"
git init -q "$repo"
git init -q --bare "$work/origin.git"
git -C "$repo" remote add origin "$work/origin.git"
git -C "$repo" config core.autocrlf false
g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
signed_tag() { # signed_tag NAME KEY
    g -c gpg.format=ssh -c user.signingkey="$work/$2" tag -s -a "$1" -m "$1"
}
# verify [VAR=VALUE...] [ARG...]: run the script on the repository, with only those
# runner settings, so the result is the same inside a pipeline as outside.
verify() {
    local vars=()
    while [ "$#" -gt 0 ] && [[ "$1" == *=* ]]; do vars+=("$1"); shift; done
    (cd "$repo" && env -u GITHUB_REF_TYPE -u GITHUB_REF_NAME -u CI_COMMIT_TAG -u GITHUB_ACTIONS -u GITLAB_CI         -u RUNNER_ENVIRONMENT -u MIDGARD_ALLOWED_SIGNERS_PATH VERIFY_TAG_ROOT="$repo" "${vars[@]}" bash "$script" "$@")
}

printf 'release@example.invalid namespaces="git" %s\n' "$(cat "$work/release.pub")" > "$repo/deploy/trust/allowed_signers"
echo readme > "$repo/README.md"
g add -A && g commit -q -m base
g push -q origin HEAD:refs/heads/main

signed_tag v1.0.0 release
if out="$(verify v1.0.0 2>&1)"; then
    grep -q 'Good "git" signature for release@example.invalid' <<<"$out" \
        && pass "a tag signed by a listed key verifies, and the signer is named" || fail "the signer is named: $out"
else
    fail "a tag signed by a listed key verifies: $out"
fi

# The checkout made for a tag's own run holds a lightweight stand-in for an annotated tag
# (verify-tag.sh, header). In a pipeline the tag is fetched afresh from origin and the tag
# origin holds is judged; on a developer's shell the checkout's own tag is.
g push -q origin v1.0.0
g tag -f v1.0.0 "$(g rev-parse 'v1.0.0^{commit}')" >/dev/null
pipeline="GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted"
verify v1.0.0 >/dev/null 2>&1 && fail "a lightweight stand-in is refused on a developer's shell" ||
    pass "a lightweight stand-in is refused on a developer's shell"
# shellcheck disable=SC2086 # two settings, split on purpose
if out="$(verify $pipeline v1.0.0 2>&1)"; then
    grep -q 'Good "git" signature for release@example.invalid' <<<"$out" &&
        pass "in a pipeline the tag origin holds replaces a lightweight stand-in, and verifies" ||
        fail "in a pipeline the tag origin holds is the one judged: $out"
else
    fail "in a pipeline the tag origin holds replaces a lightweight stand-in, and verifies: $out"
fi
[ "$(g cat-file -t refs/tags/v1.0.0)" = tag ] && pass "the stand-in is replaced by the annotated tag origin holds" ||
    fail "the stand-in is replaced by the annotated tag origin holds"

signed_tag v1.0.1 stranger
verify v1.0.1 >/dev/null 2>&1 && fail "a tag signed by an unlisted key is refused" || pass "a tag signed by an unlisted key is refused"

g tag -a v1.0.2 -m unsigned
verify v1.0.2 >/dev/null 2>&1 && fail "an unsigned annotated tag is refused" || pass "an unsigned annotated tag is refused"

g tag v1.0.3
if out="$(verify v1.0.3 2>&1)"; then fail "a lightweight tag is refused"; else
    grep -q 'lightweight' <<<"$out" && pass "a lightweight tag is refused, and why" || fail "the reason is given: $out"
fi
g push -q origin v1.0.3
# shellcheck disable=SC2086 # two settings, split on purpose
if out="$(verify $pipeline v1.0.3 2>&1)"; then fail "a lightweight tag that origin holds too is refused in a pipeline"; else
    grep -q 'lightweight' <<<"$out" && pass "a lightweight tag that origin holds too is refused in a pipeline" ||
        fail "the reason is given in a pipeline: $out"
fi

verify v9.9.9 >/dev/null 2>&1 && fail "a tag that does not exist is refused" || pass "a tag that does not exist is refused"
verify >/dev/null 2>&1 && fail "no tag at all is refused" || pass "no tag at all is refused"

echo more >> "$repo/README.md"
g commit -q -am next
if out="$(verify v1.0.0 2>&1)"; then fail "a tag on another commit than the checkout is refused"; else
    grep -q 'points at' <<<"$out" && pass "a tag on another commit than the checkout is refused" || fail "the reason is given: $out"
fi
g checkout -q v1.0.0 2>/dev/null

mv "$repo/deploy/trust/allowed_signers" "$work/kept"
verify v1.0.0 >/dev/null 2>&1 && fail "with no allowed-signers file, nothing verifies" || pass "with no allowed-signers file, nothing verifies"
: > "$repo/deploy/trust/allowed_signers"
verify v1.0.0 >/dev/null 2>&1 && fail "an empty allowed-signers file verifies nothing" || pass "an empty allowed-signers file verifies nothing"
{ cat "$work/kept"; cat "$work/release"; } > "$repo/deploy/trust/allowed_signers"
verify v1.0.0 >/dev/null 2>&1 && fail "an allowed-signers file holding a private key is refused" || pass "an allowed-signers file holding a private key is refused"
sed 's/namespaces="git"/namespaces="file"/' "$work/kept" > "$repo/deploy/trust/allowed_signers"
verify v1.0.0 >/dev/null 2>&1 && fail "a key listed for another namespace does not sign tags" || pass "a key listed for another namespace does not sign tags"

cp "$work/kept" "$repo/deploy/trust/allowed_signers"
verify v1.0.0 >/dev/null 2>&1 && pass "a tag on main verifies" || fail "a tag on main verifies"

# tag_on_a_commit_not_on_main_is_refused [AC-P2.2]: signed by a listed key, and on a
# branch that origin/main does not contain.
g checkout -q -b topic v1.0.0
echo topic >> "$repo/README.md"
g commit -q -am topic
signed_tag v1.1.0 release
if out="$(verify v1.1.0 2>&1)"; then fail "tag_on_a_commit_not_on_main_is_refused"; else
    grep -q 'the tagged commit is not on main' <<<"$out" && pass "tag_on_a_commit_not_on_main_is_refused" ||
        fail "tag_on_a_commit_not_on_main_is_refused, saying why: $out"
fi
verify --no-ancestry v1.1.0 >/dev/null 2>&1 && pass "--no-ancestry skips the main check, for these tests" ||
    fail "--no-ancestry skips the main check, for these tests"
verify GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted --no-ancestry v1.1.0 >/dev/null 2>&1 &&
    fail "--no-ancestry is refused in a pipeline" || pass "--no-ancestry is refused in a pipeline"
g push -q origin topic:refs/heads/main
verify v1.1.0 >/dev/null 2>&1 && pass "once main holds the commit, the tag verifies: main is fetched afresh" ||
    fail "once main holds the commit, the tag verifies: main is fetched afresh"

# runner_pinned_signers_are_used [AC-P2.1]: the tagged commit adds a key to the
# committed file and signs with it; the runner's copy, outside the checkout, decides.
mkdir -p "$work/runner"
cp "$work/kept" "$work/runner/allowed_signers"
printf 'stranger@example.invalid namespaces="git" %s
' "$(cat "$work/stranger.pub")" >> "$repo/deploy/trust/allowed_signers"
g commit -q -am "add a key"
signed_tag v1.2.0 stranger
g push -q origin topic:refs/heads/main
pinned="MIDGARD_ALLOWED_SIGNERS_PATH=$work/runner/allowed_signers"
if out="$(verify "$pinned" v1.2.0 2>&1)"; then fail "runner_pinned_signers_are_used: a key only the tagged commit lists is refused"; else
    grep -q 'the committed allowed_signers differs from the runner-pinned copy' <<<"$out" &&
        pass "runner_pinned_signers_are_used: a key only the tagged commit lists is refused, the copies differing" ||
        fail "runner_pinned_signers_are_used: the copies differ, and it says so: $out"
fi
verify v1.2.0 >/dev/null 2>&1 && pass "on a developer's shell the committed file is still used" ||
    fail "on a developer's shell the committed file is still used"
cp "$repo/deploy/trust/allowed_signers" "$work/runner/allowed_signers"
if out="$(verify "$pinned" GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=self-hosted v1.2.0 2>&1)"; then
    grep -q 'Good "git" signature for stranger@example.invalid' <<<"$out" &&
        pass "runner_pinned_signers_are_used: with matching copies and a listed key, it passes" ||
        fail "runner_pinned_signers_are_used: the signer is named: $out"
else
    fail "runner_pinned_signers_are_used: with matching copies and a listed key, it passes: $out"
fi
verify "MIDGARD_ALLOWED_SIGNERS_PATH=$repo/deploy/trust/allowed_signers" v1.2.0 >/dev/null 2>&1 &&
    fail "a pinned copy inside the checkout is refused" || pass "a pinned copy inside the checkout is refused"
verify "MIDGARD_ALLOWED_SIGNERS_PATH=$work/runner/absent" v1.2.0 >/dev/null 2>&1 &&
    fail "a pinned path that names no file is refused" || pass "a pinned path that names no file is refused"

# self_hosted_without_pinned_signers_fails_closed [AC-P2.4]
for runner in "GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=self-hosted" "GITLAB_CI=true"; do
    # shellcheck disable=SC2086 # two settings, split on purpose
    if out="$(verify $runner v1.2.0 2>&1)"; then fail "self_hosted_without_pinned_signers_fails_closed ($runner)"; else
        grep -q 'MIDGARD_ALLOWED_SIGNERS_PATH is not set' <<<"$out" && grep -q 'docs/ops/PKI.md, "Signing runner"' <<<"$out" &&
            pass "self_hosted_without_pinned_signers_fails_closed ($runner), pointing to PKI.md" ||
            fail "self_hosted_without_pinned_signers_fails_closed ($runner), saying why: $out"
    fi
done
verify GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted v1.2.0 >/dev/null 2>&1 &&
    pass "a hosted runner may use the committed file" || fail "a hosted runner may use the committed file"

if [ "$failures" -gt 0 ]; then
    echo "verify-tag tests: $failures failure(s)"
    exit 1
fi
echo "verify-tag tests: all passed"
