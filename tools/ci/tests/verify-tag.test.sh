#!/usr/bin/env bash
# Tests for tools/ci/verify-tag.sh, against throwaway repositories and keys.
# [AC-RF-CA-6a]
#
#   bash tools/ci/tests/verify-tag.test.sh
#
# The claim under test: a release tag passes only when it is annotated, signed, and
# signed by a key the committed allowed-signers file lists.
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
git -C "$repo" config core.autocrlf false
g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
signed_tag() { # signed_tag NAME KEY
    g -c gpg.format=ssh -c user.signingkey="$work/$2" tag -s -a "$1" -m "$1"
}
verify() { (cd "$repo" && unset GITHUB_REF_TYPE GITHUB_REF_NAME CI_COMMIT_TAG && VERIFY_TAG_ROOT="$repo" bash "$script" "$@"); }

printf 'release@example.invalid namespaces="git" %s\n' "$(cat "$work/release.pub")" > "$repo/deploy/trust/allowed_signers"
echo readme > "$repo/README.md"
g add -A && g commit -q -m base

signed_tag v1.0.0 release
if out="$(verify v1.0.0 2>&1)"; then
    grep -q 'Good "git" signature for release@example.invalid' <<<"$out" \
        && pass "a tag signed by a listed key verifies, and the signer is named" || fail "the signer is named: $out"
else
    fail "a tag signed by a listed key verifies: $out"
fi

signed_tag v1.0.1 stranger
verify v1.0.1 >/dev/null 2>&1 && fail "a tag signed by an unlisted key is refused" || pass "a tag signed by an unlisted key is refused"

g tag -a v1.0.2 -m unsigned
verify v1.0.2 >/dev/null 2>&1 && fail "an unsigned annotated tag is refused" || pass "an unsigned annotated tag is refused"

g tag v1.0.3
if out="$(verify v1.0.3 2>&1)"; then fail "a lightweight tag is refused"; else
    grep -q 'lightweight' <<<"$out" && pass "a lightweight tag is refused, and why" || fail "the reason is given: $out"
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

if [ "$failures" -gt 0 ]; then
    echo "verify-tag tests: $failures failure(s)"
    exit 1
fi
echo "verify-tag tests: all passed"
