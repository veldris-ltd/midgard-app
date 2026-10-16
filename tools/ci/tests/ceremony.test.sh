#!/usr/bin/env bash
# Tests for deploy/trust/ceremony.sh, against throwaway roots and a stand-in mg-sign.
# [RT-35] [AC-RF-CA-6a]
#
#   bash tools/ci/tests/ceremony.test.sh
#
# The claims under test: the ceremony publishes exactly the three files the release
# checks, in the forms it checks; it never puts a keystore inside the checkout; and it
# refuses, before generating anything, a private key offered as a signer, an existing
# published file, or a custody directory that is not fresh. mg-sign's own keygen is
# tested in tools/mg-sign; a stand-in is used here so the test needs no build.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../../../deploy/trust/ceremony.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

command -v ssh-keygen >/dev/null 2>&1 || { echo "skip  ceremony tests (ssh-keygen is not on PATH)"; exit 0; }

# A stand-in for `mg-sign keygen`: writes a keystore and the anchor line mg-sign exports.
cat > "$work/mg-sign" <<'EOF'
#!/usr/bin/env bash
[ "$1" = keygen ] || exit 2
shift
while [ "$#" -gt 0 ]; do
    case "$1" in
        --keystore) ks="$2" ;; --key-id) id="$2" ;; --alg) alg="$2" ;; --export-verifying-key) out="$2" ;;
    esac
    shift 2
done
echo "sealed keystore $id" > "$ks"
printf '%s %s %s\n' "$alg" "$id" "0123456789abcdef" > "$out"
EOF
chmod +x "$work/mg-sign"

ssh-keygen -q -t ed25519 -N '' -C alice -f "$work/alice" >/dev/null
ssh-keygen -q -t ed25519 -N '' -C bob -f "$work/bob" >/dev/null

root() { mkdir -p "$work/$1/deploy/trust" "$work/$1/docs/ops"; echo "$work/$1"; }
ceremony() { # ceremony ROOT ARGS...
    local r="$1"
    shift
    (cd "$r" && CEREMONY_ROOT="$r" MG_SIGN="$work/mg-sign" bash "$script" "$@")
}

# --- a ceremony ----------------------------------------------------------------------------
r="$(root good)"
if out="$(ceremony "$r" --custody "$work/custody" --year 2026 --witness "W. Itness" \
    --signer alice@example.invalid="$work/alice.pub" --signer bob@example.invalid="$work/bob.pub" 2>&1)"; then
    pass "a ceremony runs"
else
    fail "a ceremony runs: $out"
fi
grep -qxE 'ML-DSA-87 midgard-release-2026 [0-9a-f]+' "$r/deploy/trust/release-trust.pub" 2>/dev/null \
    && pass "the release anchor is ML-DSA-87, with a dated key id" || fail "the release anchor is ML-DSA-87, with a dated key id"
grep -qxE 'ML-DSA-65 midgard-bifrost-2026 [0-9a-f]+' "$r/deploy/trust/bifrost-trust.pub" 2>/dev/null \
    && pass "the Bifrost anchor is ML-DSA-65" || fail "the Bifrost anchor is ML-DSA-65"
[ -s "$work/custody/release.keystore" ] && [ -s "$work/custody/bifrost.keystore" ] && [ ! -e "$r/release.keystore" ] \
    && pass "the keystores are written to the custody directory only" || fail "the keystores are written to the custody directory only"
if [ "$(grep -c '^alice@example.invalid namespaces="git" ssh-ed25519 ' "$r/deploy/trust/allowed_signers")" = 1 ] &&
   [ "$(grep -c '^bob@example.invalid namespaces="git" ssh-ed25519 ' "$r/deploy/trust/allowed_signers")" = 1 ] &&
   ! grep -q ' alice$\| bob$' "$r/deploy/trust/allowed_signers"; then
    pass "allowed_signers lists each signer for the namespace git, without key comments"
else
    fail "allowed_signers lists each signer for the namespace git: $(cat "$r/deploy/trust/allowed_signers")"
fi
record="$(ls "$r"/docs/ops/ceremonies/*-release-keys.md 2>/dev/null | head -n 1)"
fp="$(ssh-keygen -lf "$work/alice.pub" | awk '{print $2}')"
if [ -n "$record" ] && grep -q "$fp" "$record" &&
   grep -q "$( (sha256sum "$r/deploy/trust/release-trust.pub" 2>/dev/null || shasum -a 256 "$r/deploy/trust/release-trust.pub") | awk '{print $1}')" "$record" &&
   grep -q '| Witness | W. Itness |' "$record" && ! grep -q "$work/custody" "$record"; then
    pass "the record carries the digests and fingerprints, and not where the keys are held"
else
    fail "the record carries the digests and fingerprints: $(cat "$record" 2>&1)"
fi
grep -q "$fp" <<<"$out" && pass "the fingerprints are printed for the second channel" || fail "the fingerprints are printed for the second channel"

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

# --- refusals, before anything is generated ----------------------------------------------------
refused() { # refused NAME PATTERN ROOT ARGS...
    local name="$1" pattern="$2" r="$3"
    shift 3
    if out="$(ceremony "$r" "$@" 2>&1)"; then
        fail "$name: it ran"
    elif ! grep -q "$pattern" <<<"$out"; then
        fail "$name: refused, but not for the reason: $out"
    elif [ -e "$r/deploy/trust/release-trust.pub" ] && [ "${keep:-}" != 1 ]; then
        fail "$name: refused, but a key was published"
    else
        pass "$name"
    fi
}
refused "a private key offered as a signer is refused" "private key" "$(root priv)" \
    --custody "$work/c1" --signer alice@example.invalid="$work/alice"
refused "a custody directory inside the checkout is refused" "inside this checkout" "$(root inside)" \
    --custody "$work/inside/keys" --signer alice@example.invalid="$work/alice.pub"
mkdir -p "$work/c2" && echo x > "$work/c2/old"
refused "a custody directory that is not empty is refused" "not empty" "$(root used)" \
    --custody "$work/c2" --signer alice@example.invalid="$work/alice.pub"
r="$(root again)"
echo "ML-DSA-87 old 00" > "$r/deploy/trust/release-trust.pub"
keep=1 refused "an existing anchor is never overwritten" "already exists" "$r" \
    --custody "$work/c3" --signer alice@example.invalid="$work/alice.pub"
[ "$(cat "$r/deploy/trust/release-trust.pub")" = "ML-DSA-87 old 00" ] && pass "and it is left as it was" || fail "and it is left as it was"
echo "not a key" > "$work/junk.pub"
refused "a file that is not an SSH public key is refused" "not an SSH public key" "$(root junk)" \
    --custody "$work/c4" --signer alice@example.invalid="$work/junk.pub"
refused "a signer with no principal is refused" "is not PRINCIPAL=KEY.pub" "$(root noprin)" \
    --custody "$work/c5" --signer "$work/alice.pub"
refused "no signer at all is refused" "at least one tag signer" "$(root nosign)" --custody "$work/c6"

if [ "$failures" -gt 0 ]; then
    echo "ceremony tests: $failures failure(s)"
    exit 1
fi
echo "ceremony tests: all passed"
