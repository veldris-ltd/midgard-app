#!/usr/bin/env bash
# Cut a release tag, once the commit has earned it. [AC-RF-CA-6a] [AC-RF-CA-6b]
#
#   bash tools/ci/tag-release.sh            check everything, then make the signed tag
#   bash tools/ci/tag-release.sh --check    check everything, make nothing
#
# The tag is named from the workspace version (`[workspace.package] version` in
# Cargo.toml): v0.1.0-rc.1 for 0.1.0-rc.1. It is made only when, on the commit checked
# out,
#   - the tree is clean, and the commit is the tip of origin/main;
#   - the gate record covers the commit: gate-record.sh --check --release;
#   - the numbers sheet is the tree's and every document agrees with it: numbers.sh --check;
#   - the claims ledger checks, strictly: claims.sh --check --strict;
#   - release notes exist at docs/releases/<tag>.md;
#   - deploy/trust/allowed_signers exists, so the tag can be verified once made.
# Then it makes an annotated tag signed with your configured signing key (git's
# user.signingkey; an SSH key listed in allowed_signers), verifies it with
# verify-tag.sh, and prints the push command. It does not push: publishing the tag
# starts the release, and that is a decision to take with the output in front of you.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 2
check_only=0
case "${1:-}" in
    --check) check_only=1 ;;
    "") ;;
    -h | --help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "tag-release: unknown argument '$1'" >&2; exit 2 ;;
esac

problems=0
ok() { echo "tag-release: ok — $*"; }
bad() { echo "tag-release: NOT READY — $*" >&2; problems=$((problems + 1)); }

version="$(tr -d '\r' < Cargo.toml |
    awk '/^\[workspace\.package\]/ { on = 1; next } on && /^\[/ { exit } on && /^version *=/ { gsub(/[" ]/, ""); sub(/^version=/, ""); print; exit }')"
[ -n "$version" ] || { echo "tag-release: no [workspace.package] version in Cargo.toml" >&2; exit 1; }
tag="v$version"
commit="$(git rev-parse HEAD)"
echo "tag-release: $tag on ${commit:0:12}"

if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    bad "$tag already exists, on $(git rev-parse --short "refs/tags/$tag^{commit}"). A tag is never moved."
fi
[ -z "$(git status --porcelain)" ] && ok "the tree is clean" || bad "the tree has uncommitted changes"
git fetch --quiet origin main 2>/dev/null || bad "could not fetch origin/main to compare with"
if [ "$(git rev-parse --verify --quiet origin/main || true)" = "$commit" ]; then
    ok "the commit is the tip of origin/main"
else
    bad "the commit is not the tip of origin/main; a release is tagged on main as pushed"
fi
bash tools/ci/gate-record.sh --check --release && ok "the gate record covers this commit" || bad "the gate record does not cover this commit"
bash tools/ci/numbers.sh --check && ok "the numbers sheet is this tree's" || bad "the numbers sheet or a document disagrees with the tree"
bash tools/ci/claims.sh --check --strict >/dev/null && ok "the claims ledger checks" || bad "the claims ledger does not check: bash tools/ci/claims.sh --check --strict"
[ -s "docs/releases/$tag.md" ] && ok "release notes: docs/releases/$tag.md" || bad "no release notes at docs/releases/$tag.md"
[ -s deploy/trust/allowed_signers ] && ok "deploy/trust/allowed_signers is published" ||
    bad "deploy/trust/allowed_signers is missing: hold the key ceremony first (docs/ops/PKI.md, \"Git signing\")"

if [ "$problems" -gt 0 ]; then
    echo "tag-release: $problems item(s) not ready; no tag made." >&2
    exit 1
fi
if [ "$check_only" = 1 ]; then
    echo "tag-release: ready to tag $tag on ${commit:0:12}."
    exit 0
fi

gate="$(tr -d ' \r\n' < docs/compliance/gate-runs/CURRENT)"
git -c gpg.format=ssh tag -s -a "$tag" -m "Midgard $tag

Release candidate, not a supported release; see docs/releases/$tag.md.
GA is NO-GO: docs/compliance/GA_READINESS.md names each blocker.

Gate run: docs/compliance/gate-runs/$gate.md
Numbers:  docs/compliance/NUMBERS.md
Claims:   docs/compliance/CLAIMS.md" || { echo "tag-release: git could not make the signed tag" >&2; exit 1; }

if ! bash tools/ci/verify-tag.sh "$tag"; then
    git tag -d "$tag" >/dev/null
    echo "tag-release: the new tag did not verify, so it was deleted. Is your signing key in deploy/trust/allowed_signers?" >&2
    exit 1
fi
echo
echo "tag-release: $tag is made and verifies. Publishing it starts the release workflow:"
echo "  git push origin $tag"
