#!/usr/bin/env bash
# A release tag verifies against the committed signers, or nothing is released.
# [AC-RF-CA-6a] [AC-RF-CA-6b] [NFR-PQ-3]
#
#   bash tools/ci/verify-tag.sh [TAG]       TAG defaults to the tag CI is building
#
# Passes only when TAG
#   - is an annotated tag, not a lightweight one: a lightweight tag carries no signature;
#   - carries an SSH signature that git verifies against deploy/trust/allowed_signers,
#     from a key listed there for the namespace "git" — the file the key ceremony
#     publishes (docs/ops/PKI.md, "Git signing");
#   - points at the commit checked out, when the checkout is a commit and not a branch.
#
# deploy/trust/allowed_signers is git's allowed-signers format (ssh-keygen(1), "ALLOWED
# SIGNERS"): one `<principal> namespaces="git" <key type> <public key>` per line. A
# missing or empty file is a refusal, never a pass, and a file holding private key
# material is refused outright.
set -uo pipefail

ROOT="${VERIFY_TAG_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
SIGNERS="deploy/trust/allowed_signers"
. "$(dirname "$0")/lib.sh"

fail() {
    ci_error "verify-tag: $1"
    shift
    for line in "$@"; do echo "  $line" >&2; done
    exit 1
}

tag="${1:-$(cd "$ROOT" && ci_tag)}"
[ -n "$tag" ] || fail "no tag to verify: name one, or run on a tag pipeline"
cd "$ROOT" || exit 2

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || fail "there is no tag $tag here" \
    "A shallow checkout may not have fetched it: fetch with tags, or fetch-depth: 0."
[ "$(git cat-file -t "refs/tags/$tag")" = tag ] || fail "$tag is a lightweight tag, which cannot carry a signature" \
    "Make it annotated and signed: bash tools/ci/tag-release.sh"

[ -s "$SIGNERS" ] || fail "$SIGNERS is missing or empty" \
    "It is published by the key ceremony (docs/ops/PKI.md, \"Git signing\"). Until then no tag verifies."
grep -q 'PRIVATE KEY' "$SIGNERS" && fail "$SIGNERS holds private key material; it must hold public keys only"
grep -Ev '^[[:space:]]*(#|$)' "$SIGNERS" | grep -q 'namespaces="[^"]*git' ||
    fail "$SIGNERS lists no key for the namespace \"git\""

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

echo "verify-tag: $tag is an annotated tag on ${target:0:12}. $signer."
