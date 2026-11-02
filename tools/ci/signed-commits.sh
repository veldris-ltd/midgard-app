#!/usr/bin/env bash
# Every commit a change brings in is signed by a key the repository trusts.
# [AC-25.4] [AC-RF-CA-6a] (VLD-ANB-MIDGARD-0088)
#
#   bash tools/ci/signed-commits.sh [RANGE]
#
# RANGE defaults to BASE..HEAD, where BASE is the pull or merge request's base, else the
# commit a pushed branch moved from (tools/ci/lib.sh, ci_base_ref), else origin/main.
# Passes only when every commit in it carries a signature that git verifies against
# deploy/trust/allowed_signers, the same file a release tag is verified against
# (verify-tag.sh), with the same rules: git's allowed-signers format, public keys only,
# at least one key for the namespace "git". A commit git reports as anything but `G`
# (unsigned, unknown key, bad or unverifiable signature) fails the job and is named.
#
# Branch protection (a required-signatures rule on main) is what stops an unsigned
# commit reaching the branch; this job is the evidence on every change, on both forges,
# and the check a developer runs before pushing. CONTRIBUTING.md says how to sign.
set -uo pipefail

ROOT="${SIGNED_COMMITS_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
SIGNERS="deploy/trust/allowed_signers"
. "$(dirname "$0")/lib.sh"

fail() {
    ci_error "signed-commits: $1"
    shift
    for line in "$@"; do echo "  $line" >&2; done
    exit 1
}

cd "$ROOT" || exit 2
range="${1:-}"
if [ -z "$range" ]; then
    base="$(ci_base_ref)"
    range="${base:-origin/main}..HEAD"
fi
git rev-parse --verify --quiet "${range%%..*}^{commit}" >/dev/null || fail "the base of $range is not a commit here" \
    "A shallow checkout may not have it: fetch with fetch-depth: 0, or name the range."

[ -s "$SIGNERS" ] || fail "$SIGNERS is missing or empty" \
    "It is published by the key ceremony and lists every key that may sign (docs/ops/PKI.md, \"Git signing\")." \
    "Until it exists no commit verifies; see CONTRIBUTING.md, \"Signed commits\"."
grep -q 'PRIVATE KEY' "$SIGNERS" && fail "$SIGNERS holds private key material; it must hold public keys only"
grep -Ev '^[[:space:]]*(#|$)' "$SIGNERS" | grep -q 'namespaces="[^"]*git' ||
    fail "$SIGNERS lists no key for the namespace \"git\""

log="$(git -c gpg.ssh.allowedSignersFile="$SIGNERS" log --format='%G? %H %s' "$range" 2>&1)" ||
    fail "cannot list the commits in $range" "$(tail -n 3 <<<"$log")"
total=0 bad=0
while IFS= read -r line; do
    [ -n "$line" ] || continue
    total=$((total + 1))
    case "$line" in
        G\ *) ;;
        *)
            bad=$((bad + 1))
            status="${line%% *}" rest="${line#* }"
            case "$status" in
                N) why="not signed" ;;
                B) why="bad signature" ;;
                U | X | Y) why="signed, but not by a key valid in $SIGNERS" ;;
                R) why="signed by a revoked key" ;;
                E) why="signature cannot be checked (no key for it in $SIGNERS)" ;;
                *) why="signature status $status" ;;
            esac
            ci_error "signed-commits: commit ${rest%% *} is $why: ${rest#* }"
            ;;
    esac
done <<<"$log"

if [ "$bad" -ne 0 ]; then
    echo "signed-commits: $bad of $total commit(s) in $range do not verify against $SIGNERS" >&2
    echo "  Sign them (CONTRIBUTING.md, \"Signed commits\") and push again; git rebase --exec 'git commit --amend --no-edit -S' re-signs a branch." >&2
    exit 1
fi
echo "signed-commits: all $total commit(s) in $range are signed by a key in $SIGNERS"
