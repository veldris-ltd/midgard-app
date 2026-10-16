#!/usr/bin/env bash
# The release key ceremony, as one run. [RT-35] [AC-RF-CA-6a] [NFR-PQ-3]
#
#   bash deploy/trust/ceremony.sh --custody DIR --signer PRINCIPAL=KEY.pub [--signer ...]
#                                 [--year YYYY] [--witness NAME]
#
# docs/ops/PKI.md describes the ceremony; this does its mechanical part, so that what
# the officers check is the output, not their typing. It
#
#   1. generates the release signing key (ML-DSA-87) and the Bifrost admission key
#      (ML-DSA-65), each in a sealed keystore written to DIR, outside this checkout;
#   2. writes their public halves as deploy/trust/release-trust.pub and
#      deploy/trust/bifrost-trust.pub;
#   3. writes deploy/trust/allowed_signers from the SSH public keys of the people who
#      may sign a release tag, each for the namespace "git";
#   4. checks all three the way the release does (release-preflight.sh, verify-tag.sh);
#   5. writes a ceremony record, docs/ops/ceremonies/<date>-release-keys.md, with the
#      key ids, the SHA-256 of each published file and each SSH key's fingerprint, and
#      blank lines for the officers and the witness to sign;
#   6. prints the fingerprints to publish through a second channel, and what to commit.
#
# It commits nothing, pushes nothing and uploads nothing. The keystores in DIR are the
# keys: whoever holds a keystore file can sign as Midgard. Move them to their custodian
# and set the runner's secrets to their paths (docs/ops/PKI.md, "Signing runner").
#
# A ceremony is held once per key generation. If any published file already exists this
# refuses: rotation appends a key beside the old one, which is a different procedure
# (deploy/trust/README.md).
#
# mg-sign is built from this checkout with cargo, offline, unless MG_SIGN names a binary.
set -uo pipefail

cd "${CEREMONY_ROOT:-$(dirname "$0")/../..}" || exit 2
TRUST=deploy/trust
RECORDS=docs/ops/ceremonies

die() { echo "ceremony: $*" >&2; exit 1; }
sha256_of() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }

custody="" year="$(date -u +%Y)" witness="" signers=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --custody) custody="${2:?--custody needs a directory}"; shift 2 ;;
        --signer) signers+=("${2:?--signer needs PRINCIPAL=KEY.pub}"); shift 2 ;;
        --year) year="${2:?--year needs a year}"; shift 2 ;;
        --witness) witness="${2:?--witness needs a name}"; shift 2 ;;
        -h | --help) sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument '$1' (see --help)" ;;
    esac
done
[ -n "$custody" ] || die "name the custody directory: --custody DIR"
[ "${#signers[@]}" -gt 0 ] || die "name at least one tag signer: --signer PRINCIPAL=KEY.pub"
[[ "$year" =~ ^[0-9]{4}$ ]] || die "--year must be four digits"

# --- before anything is generated --------------------------------------------------------
for f in "$TRUST/release-trust.pub" "$TRUST/bifrost-trust.pub" "$TRUST/allowed_signers"; do
    [ ! -e "$f" ] || die "$f already exists. A ceremony publishes new keys once; to rotate, append (deploy/trust/README.md)."
done
mkdir -p "$custody" || die "cannot create $custody"
custody="$(cd "$custody" && pwd)"
case "$custody/" in
    "$PWD"/*) die "$custody is inside this checkout; keep the keystores outside it, where git cannot pick them up" ;;
esac
[ -z "$(ls -A "$custody")" ] || die "$custody is not empty; give each ceremony a fresh directory"

# Each signer: a principal (an email address, as git records the tagger) and an SSH
# public key. A private key here would be published, so anything that looks like one,
# and any key git cannot sign tags with, stops the ceremony before keys are generated.
lines=()
for s in "${signers[@]}"; do
    principal="${s%%=*}" keyfile="${s#*=}"
    [ "$principal" != "$s" ] && [ -n "$principal" ] && [ -n "$keyfile" ] || die "--signer $s is not PRINCIPAL=KEY.pub"
    [[ "$principal" =~ ^[^[:space:]\"]+$ ]] || die "principal '$principal' must be one word with no quotes, e.g. an email address"
    [ -s "$keyfile" ] || die "$keyfile is missing or empty"
    ! grep -q 'PRIVATE KEY' "$keyfile" || die "$keyfile is a private key; give the .pub file"
    read -r type key _ < <(tr -d '\r' < "$keyfile" | grep -v '^[[:space:]]*$' | head -n 1)
    case "$type" in
        ssh-ed25519 | sk-ssh-ed25519@openssh.com | ecdsa-sha2-nistp256 | sk-ecdsa-sha2-nistp256@openssh.com) ;;
        *) die "$keyfile is not an SSH public key git can sign tags with (got '${type:-nothing}')" ;;
    esac
    [ -n "$key" ] || die "$keyfile holds no key after its type"
    lines+=("$principal namespaces=\"git\" $type $key")
done

if [ -n "${MG_SIGN:-}" ]; then
    mg_sign="$MG_SIGN"
else
    command -v cargo >/dev/null 2>&1 || die "no cargo to build mg-sign; build it in the toolchain image and set MG_SIGN to it"
    echo "ceremony: building mg-sign from this checkout, offline"
    cargo build --quiet --offline --release -p mg-sign || die "mg-sign did not build"
    mg_sign=target/release/mg-sign
    [ -x "$mg_sign" ] || mg_sign="$mg_sign.exe"
fi

# --- the keys ----------------------------------------------------------------------------
release_id="midgard-release-$year"
bifrost_id="midgard-bifrost-$year"
"$mg_sign" keygen --keystore "$custody/release.keystore" --key-id "$release_id" --alg ML-DSA-87 \
    --export-verifying-key "$TRUST/release-trust.pub" || die "release key generation failed"
"$mg_sign" keygen --keystore "$custody/bifrost.keystore" --key-id "$bifrost_id" --alg ML-DSA-65 \
    --export-verifying-key "$TRUST/bifrost-trust.pub" || die "Bifrost key generation failed"
# Readable by the custodian alone. (Where the filesystem has no POSIX modes, protect the
# directory instead.)
chmod 600 "$custody/release.keystore" "$custody/bifrost.keystore" 2>/dev/null || true
{
    echo "# The SSH keys that may sign a Midgard release tag. Written by the key ceremony"
    echo "# (deploy/trust/ceremony.sh); checked by tools/ci/verify-tag.sh. [AC-RF-CA-6a]"
    printf '%s\n' "${lines[@]}"
} > "$TRUST/allowed_signers"

# --- checked as the release will check them ------------------------------------------------
check_anchor() { # FILE ALG
    ! grep -qiE 'sk=|private' "$1" || die "$1 holds private key material; nothing may be committed"
    tr -d '\r' < "$1" | grep -qE "^$2 [^ ]+ [0-9a-fA-F]+$" || die "$1 has no \"$2 <key-id> <hex>\" line"
}
check_anchor "$TRUST/release-trust.pub" ML-DSA-87
check_anchor "$TRUST/bifrost-trust.pub" ML-DSA-65
! grep -q 'PRIVATE KEY' "$TRUST/allowed_signers" || die "$TRUST/allowed_signers holds a private key"

fingerprints=()
for s in "${signers[@]}"; do
    fp="$(ssh-keygen -lf "${s#*=}" 2>/dev/null | awk '{print $2}')"
    [ -n "$fp" ] || fp="(ssh-keygen could not read it)"
    fingerprints+=("${s%%=*}  $fp")
done

# --- the record --------------------------------------------------------------------------
day="$(date -u +%Y-%m-%d)"
mkdir -p "$RECORDS"
record="$RECORDS/$day-release-keys.md"
[ ! -e "$record" ] || record="$RECORDS/$day-release-keys-$(date -u +%H%M%S).md"
{
    echo "# Key ceremony — $day"
    echo
    echo "Release signing and Bifrost admission keys, and the release tag signers, per"
    echo "\`docs/ops/PKI.md\`. Generated by \`deploy/trust/ceremony.sh\` at $(date -u +%Y-%m-%dT%H:%M:%SZ) (UTC)"
    echo "from commit \`$(git rev-parse HEAD 2>/dev/null || echo unknown)\`."
    echo
    echo "## Keys"
    echo
    echo "| Key | Algorithm | Key id | Published as | SHA-256 of the published file |"
    echo "|---|---|---|---|---|"
    echo "| Release signing | ML-DSA-87 | \`$release_id\` | \`$TRUST/release-trust.pub\` | \`$(sha256_of "$TRUST/release-trust.pub")\` |"
    echo "| Bifrost admission | ML-DSA-65 | \`$bifrost_id\` | \`$TRUST/bifrost-trust.pub\` | \`$(sha256_of "$TRUST/bifrost-trust.pub")\` |"
    echo
    echo "The private halves are sealed keystores held outside the repository. Where they are"
    echo "held is not recorded here."
    echo
    echo "## Release tag signers"
    echo
    echo "\`$TRUST/allowed_signers\`, SHA-256 \`$(sha256_of "$TRUST/allowed_signers")\`:"
    echo
    echo "| Principal | SSH key fingerprint |"
    echo "|---|---|"
    for f in "${fingerprints[@]}"; do echo "| ${f%%  *} | \`${f#*  }\` |"; done
    echo
    echo "## Second channel"
    echo
    echo "The fingerprints and SHA-256 values above were also published at:"
    echo
    echo "- _where, and when_"
    echo
    echo "## Sign-off"
    echo
    echo "| Role | Name | Confirmed | Date |"
    echo "|---|---|---|---|"
    echo "| Security Officer | | | |"
    echo "| Witness | ${witness} | | |"
} > "$record"

# --- what next ---------------------------------------------------------------------------
cat <<EOF

ceremony: done. Nothing has been committed or pushed.

  Keystores (the keys themselves; move them to their custodian):
    $custody/release.keystore   ML-DSA-87  $release_id
    $custody/bifrost.keystore   ML-DSA-65  $bifrost_id

  Published files:
    $TRUST/release-trust.pub   sha256 $(sha256_of "$TRUST/release-trust.pub")
    $TRUST/bifrost-trust.pub   sha256 $(sha256_of "$TRUST/bifrost-trust.pub")
    $TRUST/allowed_signers     sha256 $(sha256_of "$TRUST/allowed_signers")

  Tag signers:
$(printf '    %s\n' "${fingerprints[@]}")

Next:
  1. Publish the SHA-256 values and fingerprints above through a second channel, and
     note where in $record.
  2. The officers and the witness check them and sign $record.
  3. Commit the three files and the record, in one signed commit.
  4. Register each signer's SSH key on the forge as a signing key, if it is not already.
  5. Set the runner's secrets to the keystores' paths; run tools/ci/release-preflight.sh.
EOF
