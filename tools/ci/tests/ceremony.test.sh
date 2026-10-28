#!/usr/bin/env bash
# Tests for deploy/trust/ceremony.sh, against throwaway roots. [RT-35] [AC-RF-CA-6a]
#
#   bash tools/ci/tests/ceremony.test.sh
#
# The ceremony itself is tools/mg-ceremony, and its claims are tested there
# (tools/mg-ceremony/tests/cli.rs): what it publishes, where the keys go, and every
# refusal. The claims under test here are the wrapper's and the release's: the script
# runs the ceremony against the checkout it is given, a refusal stops it with a
# failing exit, and the allowed_signers file it publishes is one a signed release tag
# really verifies against (tools/ci/verify-tag.sh).
#
# MG_CEREMONY names a built mg-ceremony; without it, this builds one offline.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$here/../../.."
script="$repo_root/deploy/trust/ceremony.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v ssh-keygen >/dev/null 2>&1 || { echo "skip  ceremony tests (ssh-keygen is not on PATH)"; exit 0; }
if [ -z "${MG_CEREMONY:-}" ]; then
    command -v cargo >/dev/null 2>&1 || { echo "skip  ceremony tests (no cargo, and MG_CEREMONY is not set)"; exit 0; }
    (cd "$repo_root" && cargo build --quiet --offline --release -p mg-ceremony) || { echo "FAIL  mg-ceremony did not build"; exit 1; }
    MG_CEREMONY="$repo_root/target/release/mg-ceremony"
    [ -x "$MG_CEREMONY" ] || MG_CEREMONY="$MG_CEREMONY.exe"
fi

ssh-keygen -q -t ed25519 -N '' -C alice -f "$work/alice" >/dev/null

root() { mkdir -p "$work/$1/deploy/trust" "$work/$1/docs/ops"; echo anchors > "$work/$1/deploy/trust/README.md"; echo "$work/$1"; }
ceremony() { # ceremony ROOT ARGS...
    local r="$1"
    shift
    (cd "$r" && CEREMONY_ROOT="$r" MG_CEREMONY="$MG_CEREMONY" bash "$script" "$@")
}

# --- a ceremony, through the wrapper ---------------------------------------------------------
r="$(root good)"
if out="$(ceremony "$r" --custody "$work/custody" --year 2026 --officer "A. Officer" --witness "W. Itness" \
    --signer alice@example.invalid="$work/alice.pub" 2>&1)"; then
    pass "the wrapper runs the ceremony against the checkout it is given"
else
    fail "the wrapper runs the ceremony: $out"
fi
grep -qxE 'ML-DSA-87 midgard-release-2026 [0-9a-f]+' "$r/deploy/trust/release-trust.pub" 2>/dev/null &&
    grep -qxE 'ML-DSA-65 midgard-bifrost-2026 [0-9a-f]+' "$r/deploy/trust/bifrost-trust.pub" 2>/dev/null &&
    [ -s "$work/custody/release.keystore" ] \
    && pass "it publishes both anchors and keeps the keystores in the custody directory" \
    || fail "it publishes both anchors and keeps the keystores in the custody directory"
fp="$(ssh-keygen -lf "$work/alice.pub" | awk '{print $2}')"
record="$(ls "$r"/docs/ops/ceremonies/*-release-keys.md 2>/dev/null | head -n 1)"
[ -n "$record" ] && grep -q "$fp" "$record" && grep -q "$fp" <<<"$out" \
    && pass "the record and the output carry the fingerprint ssh-keygen prints" \
    || fail "the record and the output carry the fingerprint ssh-keygen prints ($fp)"

# The published signers file works for real: a tag signed by alice verifies.
repo="$work/repo"
git init -q "$repo" && git -C "$repo" config core.autocrlf false
mkdir -p "$repo/deploy/trust" && cp "$r/deploy/trust/allowed_signers" "$repo/deploy/trust/"
g() { git -C "$repo" -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false "$@"; }
g add -A && g commit -q -m base
g -c gpg.format=ssh -c user.signingkey="$work/alice" tag -s -a v9.9.9 -m v9.9.9
(cd "$repo" && unset GITHUB_REF_TYPE GITHUB_REF_NAME CI_COMMIT_TAG && VERIFY_TAG_ROOT="$repo" bash "$here/../verify-tag.sh" v9.9.9 >/dev/null 2>&1) \
    && pass "a tag signed by a listed signer verifies against the published file" \
    || fail "a tag signed by a listed signer verifies against the published file"

# The anchors are committable: the repository ignores *.pub, but not these two.
if git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
    if git -C "$repo_root" check-ignore -q deploy/trust/release-trust.pub ||
       git -C "$repo_root" check-ignore -q deploy/trust/bifrost-trust.pub; then
        fail "the published anchors are not ignored by git"
    elif ! git -C "$repo_root" check-ignore -q deploy/trust/some-other.pub; then
        fail "other .pub files stay ignored by git"
    else
        pass "the published anchors are not ignored by git, and other .pub files are"
    fi
fi

# --- a refusal stops the wrapper ---------------------------------------------------------------
r="$(root priv)"
if out="$(ceremony "$r" --custody "$work/c1" --signer alice@example.invalid="$work/alice" 2>&1)"; then
    fail "a private key offered as a signer is refused: it ran"
elif ! grep -q "private key" <<<"$out" || [ -e "$r/deploy/trust/release-trust.pub" ]; then
    fail "a private key offered as a signer is refused, before a key is published: $out"
else
    pass "a private key offered as a signer is refused, and the wrapper fails"
fi
r="$(root again)"
echo "ML-DSA-87 old 00" > "$r/deploy/trust/release-trust.pub"
if ceremony "$r" --custody "$work/c2" --signer alice@example.invalid="$work/alice.pub" >/dev/null 2>&1; then
    fail "an existing anchor is never overwritten: it ran"
else
    [ "$(cat "$r/deploy/trust/release-trust.pub")" = "ML-DSA-87 old 00" ] \
        && pass "an existing anchor is never overwritten" || fail "an existing anchor is never overwritten"
fi

if [ "$failures" -gt 0 ]; then
    echo "ceremony tests: $failures failure(s)"
    exit 1
fi
echo "ceremony tests: all passed"
