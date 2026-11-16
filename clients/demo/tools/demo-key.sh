#!/usr/bin/env bash
# Create a demo signing key on this machine and pin its public half in
# clients/demo/trust/. Never prints key material or the passphrase.
# [NFR-PQ-3] [AC-C-13]
#
#   MIDGARD_DEMO_KEY_DIR=<dir> bash clients/demo/tools/demo-key.sh [dataset|audit]
#
# dataset (the default): demo-dataset.keystore, key id midgard-demo-dataset-<year>,
#   anchor clients/demo/trust/demo-dataset.pub. Signs demo datasets.
# audit: demo-audit.keystore, key id midgard-demo-audit-<year>, anchor
#   clients/demo/trust/demo-audit.pub. Signs the audit rows built into a dataset.
#
# The keys live only in $MIDGARD_DEMO_KEY_DIR (default ~/.midgard/demo-keys, mode
# 0700): the keystores (attest keystores, mode 0600) and one passphrase file (mode
# 0600), apart from every real key and outside the repository. Only the public keys
# are committed.
#
# What these keys may sign, and only through `just demo-dataset`
# (clients/core/examples/build_demo_dataset.rs): the dataset key, wrapped in
# demo::dataset::DemoDatasetOnly, signs preimage(DEMO_DATASET, ...) messages; the
# audit key, wrapped in demo::audit::DemoAuditOnly, signs preimage(DEMO_AUDIT, ...)
# audit-row messages. Nothing else. Never load either keystore into a general
# signer: not `mg-sign sign`, not a KMS signing service or any other
# kms::SigningRoot interface that signs arbitrary messages for callers.
set -euo pipefail
cd "$(dirname "$0")/../../.."
kind="${1:-dataset}"
case "$kind" in
    dataset | audit) ;;
    *)
        echo "demo-key: unknown key kind '$kind' (use dataset or audit)" >&2
        exit 2
        ;;
esac
dir="${MIDGARD_DEMO_KEY_DIR:-$HOME/.midgard/demo-keys}"
keystore="$dir/demo-$kind.keystore"
anchor="clients/demo/trust/demo-$kind.pub"
umask 077
mkdir -p "$dir" && chmod 700 "$dir"
if [ ! -s "$dir/passphrase" ]; then
    echo "demo-key: put a passphrase of at least 16 bytes in $dir/passphrase (mode 0600) first, e.g." >&2
    echo "  ( umask 077; openssl rand -base64 32 > \"$dir/passphrase\" )" >&2
    exit 2
fi
chmod 600 "$dir/passphrase"
if [ -e "$keystore" ]; then
    echo "demo-key: $keystore exists; refusing to replace a signing key" >&2
    exit 2
fi
mkdir -p clients/demo/trust
# Generate into temporary names, then put the anchor in place first and the key
# second; if the key cannot be put in place, the old anchor comes back. A failed run
# leaves the tracked anchor and the key directory as they were.
tmp_key="$dir/.demo-$kind.keystore.tmp.$$"
tmp_anchor="clients/demo/trust/.demo-$kind.pub.tmp.$$"
old_anchor="clients/demo/trust/.demo-$kind.pub.old.$$"
cleanup() { rm -f "$tmp_key" "$tmp_anchor" "$old_anchor"; }
trap cleanup EXIT
rm -f "$tmp_key" "$tmp_anchor" "$old_anchor"
cargo run --offline --locked -q -p mg-sign -- keygen \
    --keystore "$tmp_key" \
    --key-id "midgard-demo-$kind-$(date +%Y)" --alg ML-DSA-65 \
    --keystore-passphrase-file "$dir/passphrase" \
    --export-verifying-key "$tmp_anchor"
if [ ! -s "$tmp_key" ] || [ ! -s "$tmp_anchor" ]; then
    echo "demo-key: keygen produced no key; nothing replaced" >&2
    exit 1
fi
chmod 600 "$tmp_key"
chmod 644 "$tmp_anchor"
had_anchor=0
if [ -e "$anchor" ]; then
    cp -p "$anchor" "$old_anchor"
    had_anchor=1
fi
mv -f "$tmp_anchor" "$anchor"
if ! mv "$tmp_key" "$keystore"; then
    if [ "$had_anchor" = 1 ]; then mv -f "$old_anchor" "$anchor"; else rm -f "$anchor"; fi
    echo "demo-key: could not put the key in place; the anchor is as it was" >&2
    exit 1
fi
echo "demo-key: pinned $(cut -d' ' -f1,2 "$anchor")"
