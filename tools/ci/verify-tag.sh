#!/usr/bin/env bash
# A release tag verifies against signers the tag cannot choose, on a commit that is on
# main, or nothing is released. [AC-RF-CA-6a] [AC-RF-CA-6b] [NFR-PQ-3]
# [VLD-ANB-MIDGARD-0035]
#
#   bash tools/ci/verify-tag.sh [TAG]       TAG defaults to the tag CI is building
#   bash tools/ci/verify-tag.sh --no-ancestry TAG
#                                           skip the main check: for this script's
#                                           tests only, and refused in a pipeline
#
# Passes only when TAG
#   - is an annotated tag, not a lightweight one: a lightweight tag carries no signature;
#   - carries an SSH signature that git verifies, from a key listed for the namespace
#     "git" in the allowed-signers file (below);
#   - points at the commit checked out, when the checkout is a commit and not a branch;
#   - points at a commit on main: an ancestor of origin/main, fetched afresh.
#
# In a pipeline the tag itself is fetched afresh from origin first. The checkout made
# for a tag's own run holds a lightweight stand-in for an annotated tag: actions/checkout
# fetches every tag, then compares the pushed commit with `git rev-parse refs/tags/TAG`,
# which for an annotated tag is the tag object and never the commit, and so "corrects"
# the tag to the commit. The stand-in carries no signature; the tag origin holds does,
# and only a release manager can have put it there (docs/dev/CI.md, "Repository rules").
# On a developer's shell the tag in the checkout is judged as it is: tag-release.sh
# verifies the tag it has just made, before it is pushed.
#
# The allowed-signers file is git's format (ssh-keygen(1), "ALLOWED SIGNERS"): one
# `<principal> namespaces="git" <key type> <public key>` per line, published by the key
# ceremony as deploy/trust/allowed_signers (docs/ops/PKI.md, "Git signing"). That copy
# comes from the commit being verified, so a commit that added a key would verify a tag
# on itself. Where releases are signed it is therefore not the authority: the runner
# holds its own copy, outside the checkout, named by MIDGARD_ALLOWED_SIGNERS_PATH
# (docs/ops/PKI.md, "Signing runner"), and the tag verifies against that copy only when
# the committed one is byte-for-byte the same. A key change reaches the runner first,
# from the release manager, or no tag verifies. On a self-hosted runner the variable is
# mandatory; elsewhere (a developer's shell, tag-release.sh) the committed file is used.
#
# A missing or empty file is a refusal, never a pass, and a file holding private key
# material is refused outright. In the release pipeline this script is run from a
# separate checkout of main (VERIFY_TAG_ROOT names the tagged checkout), so the code
# that decides whether the tagged commit's scripts run is not itself the tag's.
set -uo pipefail

ancestry=1
if [ "${1:-}" = --no-ancestry ]; then
    ancestry=0
    shift
fi
ROOT="${VERIFY_TAG_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
COMMITTED="deploy/trust/allowed_signers"
SIGNERS="$COMMITTED"
. "$(dirname "$0")/lib.sh"

fail() {
    ci_error "verify-tag: $1"
    shift
    for line in "$@"; do echo "  $line" >&2; done
    exit 1
}

if [ "$ancestry" = 0 ] && [ "$(ci_platform)" != local ]; then
    fail "--no-ancestry is for this script's tests; a pipeline always checks that the tag is on main"
fi
tag="${1:-$(cd "$ROOT" && ci_tag)}"
[ -n "$tag" ] || fail "no tag to verify: name one, or run on a tag pipeline"
cd "$ROOT" || exit 2
ROOT="$(pwd -P)"

# The tag as origin has it now, in a pipeline (see the header): the checkout made for
# the tag's run holds a lightweight stand-in for it. When the fetch cannot be made, the
# tag this checkout holds is judged, and the checks below decide.
if [ "$(ci_platform)" != local ]; then
    GIT_TERMINAL_PROMPT=0 git fetch --quiet --no-tags origin "+refs/tags/$tag:refs/tags/$tag" 2>/dev/null ||
        ci_warning "verify-tag: could not fetch $tag from origin; checking the tag this checkout holds"
fi

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || fail "there is no tag $tag here" \
    "A shallow checkout may not have fetched it: fetch with tags, or fetch-depth: 0."
[ "$(git cat-file -t "refs/tags/$tag")" = tag ] || fail "$tag is a lightweight tag, which cannot carry a signature" \
    "Make it annotated and signed: bash tools/ci/tag-release.sh"

pinned="${MIDGARD_ALLOWED_SIGNERS_PATH:-}"
if [ -n "$pinned" ]; then
    [ -f "$pinned" ] || fail "MIDGARD_ALLOWED_SIGNERS_PATH names $pinned, which is not a file" \
        "Provision the runner's copy of allowed_signers (docs/ops/PKI.md, \"Signing runner\")."
    pinned="$(cd "$(dirname "$pinned")" && pwd -P)/$(basename "$pinned")"
    case "$pinned" in
        "$ROOT"/*) fail "MIDGARD_ALLOWED_SIGNERS_PATH names a file inside the checkout being verified" \
            "The runner's copy must live outside the workspace, like the keystores (docs/ops/PKI.md, \"Signing runner\")." ;;
    esac
    SIGNERS="$pinned"
elif ci_self_hosted; then
    fail "MIDGARD_ALLOWED_SIGNERS_PATH is not set: on a self-hosted runner the tag is verified against the runner's own copy of allowed_signers, never the tagged commit's" \
        "Provision it as docs/ops/PKI.md, \"Signing runner\", describes."
fi

[ -s "$SIGNERS" ] || fail "$SIGNERS is missing or empty" \
    "It is published by the key ceremony (docs/ops/PKI.md, \"Git signing\"). Until then no tag verifies."
grep -q 'PRIVATE KEY' "$SIGNERS" && fail "$SIGNERS holds private key material; it must hold public keys only"
grep -Ev '^[[:space:]]*(#|$)' "$SIGNERS" | grep -q 'namespaces="[^"]*git' ||
    fail "$SIGNERS lists no key for the namespace \"git\""
if [ "$SIGNERS" != "$COMMITTED" ]; then
    [ -s "$COMMITTED" ] || fail "$COMMITTED is missing or empty in the checkout" \
        "The committed copy is kept for review and must match the runner's (docs/ops/PKI.md, \"Signing runner\")."
    cmp -s "$SIGNERS" "$COMMITTED" ||
        fail "the committed allowed_signers differs from the runner-pinned copy; a key change must be provisioned on the runner first" \
            "See docs/ops/PKI.md, \"Signing runner\"."
fi

if ! out="$(git -c gpg.format=ssh -c gpg.ssh.allowedSignersFile="$SIGNERS" tag -v "$tag" 2>&1)"; then
    fail "the signature on $tag does not verify against $SIGNERS" "$(tail -n 3 <<<"$out")"
fi
signer="$(grep -o 'Good "git" signature for [^ ]* with [^ ]* key [^ ]*' <<<"$out" | head -n 1)"
[ -n "$signer" ] || fail "git accepted $tag but reported no SSH signature from $SIGNERS" "$(tail -n 3 <<<"$out")"

target="$(git rev-parse "refs/tags/$tag^{commit}")"
head="$(git rev-parse --verify --quiet HEAD || true)"
if [ -n "$head" ] && [ "$head" != "$target" ]; then
    fail "$tag points at ${target:0:12}, but the checkout is ${head:0:12}" \
        "A release is built from exactly the commit its tag names."
fi

# On main: an ancestor of origin/main as the remote has it now. When the fetch cannot be
# made (a checkout that keeps no credentials), the main the job's own checkout fetched
# moments ago stands in for it; with neither, the tag is refused.
if [ "$ancestry" = 1 ]; then
    GIT_TERMINAL_PROMPT=0 git fetch --quiet --no-tags origin '+refs/heads/main:refs/remotes/origin/main' 2>/dev/null ||
        ci_warning "verify-tag: could not fetch main from origin; checking against the origin/main this checkout fetched"
    git rev-parse --verify --quiet refs/remotes/origin/main >/dev/null ||
        fail "there is no origin/main here to check $tag against" \
            "Check out with the full history (fetch-depth: 0), or let this job fetch main."
    git merge-base --is-ancestor "$target" refs/remotes/origin/main ||
        fail "the tagged commit is not on main: ${target:0:12} is not an ancestor of origin/main" \
            "A release is tagged on main as pushed; merge first (docs/dev/CI.md, \"Cutting the tag\")."
    where=", on main"
fi

echo "verify-tag: $tag is an annotated tag on ${target:0:12}${where:-}. $signer."
