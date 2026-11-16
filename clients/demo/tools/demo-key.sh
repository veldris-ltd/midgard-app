#!/usr/bin/env bash
# Create the demo-dataset signing key on this machine and pin its public half in
# clients/demo/trust/demo-dataset.pub. Never prints key material or the passphrase.
# [NFR-PQ-3] [AC-C-13]
#
#   MIDGARD_DEMO_KEY_DIR=<dir> bash clients/demo/tools/demo-key.sh
#
# The key lives only in $MIDGARD_DEMO_KEY_DIR (default ~/.midgard/demo-keys, mode
# 0700): demo-dataset.keystore (an attest keystore, mode 0600) and passphrase (mode
# 0600), apart from every real key and outside the repository. Only the public key is
# committed.
#
# What this key may sign: demo datasets, and only through `just demo-dataset`
# (clients/core/examples/build_demo_dataset.rs), which wraps the unsealed key in
# demo::dataset::DemoDatasetOnly so it signs preimage(DEMO_DATASET, ...) messages and
# nothing else. Never load this keystore into a general signer: not `mg-sign sign`,
# not a KMS signing service or any other kms::SigningRoot interface that signs
# arbitrary messages for callers.
set -euo pipefail
cd "$(dirname "$0")/../../.."
dir="${MIDGARD_DEMO_KEY_DIR:-$HOME/.midgard/demo-keys}"
anchor=clients/demo/trust/demo-dataset.pub
umask 077
mkdir -p "$dir" && chmod 700 "$dir"
if [ ! -s "$dir/passphrase" ]; then
    echo "demo-key: put a passphrase of at least 16 bytes in $dir/passphrase (mode 0600) first, e.g." >&2
    echo "  ( umask 077; openssl rand -base64 32 > \"$dir/passphrase\" )" >&2
    exit 2
fi
chmod 600 "$dir/passphrase"
if [ -e "$dir/demo-dataset.keystore" ]; then
    echo "demo-key: $dir/demo-dataset.keystore exists; refusing to replace a signing key" >&2
    exit 2
fi
mkdir -p clients/demo/trust
rm -f "$anchor"
cargo run --offline --locked -q -p mg-sign -- keygen \
    --keystore "$dir/demo-dataset.keystore" \
    --key-id "midgard-demo-dataset-$(date +%Y)" --alg ML-DSA-65 \
    --keystore-passphrase-file "$dir/passphrase" \
    --export-verifying-key "$anchor"
chmod 600 "$dir/demo-dataset.keystore"
chmod 644 "$anchor"
echo "demo-key: pinned $(cut -d' ' -f1,2 "$anchor")"
