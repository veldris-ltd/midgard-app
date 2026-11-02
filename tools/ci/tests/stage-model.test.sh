#!/usr/bin/env bash
# Tests for deploy/compose/stage-model.sh, with a stand-in `docker` on PATH that records
# how it was called. [VLD-ANB-MIDGARD-0028] [AC-66.2] [AC-66.5]
#
#   bash tools/ci/tests/stage-model.test.sh
#
# The claims under test: the model-signing key's public half is never written into, or
# read from, the models volume (no /models/trust in the staging script, the compose
# files or the kind script); a key is generated only under --development, which writes
# a keystore version 2 under a passphrase, copies only the public half into the
# midgard-model-trust volume and prints a warning; and without --development the script
# fails fast, before anything is signed, unless the key ceremony's anchor
# deploy/trust/model-signing.pub is committed and the staging enclave's keystore is
# provisioned. The development compose override repeats compose.granite.yaml's
# mimir-server entrypoint with only the trust path changed.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$here/../../.."
script="$repo/deploy/compose/stage-model.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# 1. Static: no /models/trust anywhere the model is staged or served from.
for f in deploy/compose/stage-model.sh deploy/compose/compose.granite.yaml \
    deploy/compose/compose.dev-model.yaml deploy/kind/model.sh; do
    n="$(grep -c '/models/trust' "$repo/$f")"
    [ "$n" = 0 ] && pass "$f never names /models/trust" || fail "$f names /models/trust $n time(s)"
done
grep -q -- '- /etc/midgard/trust/model-signing.pub' "$repo/deploy/compose/compose.granite.yaml" \
    && pass "compose.granite.yaml trusts the anchor the image carries" \
    || fail "compose.granite.yaml does not trust /etc/midgard/trust/model-signing.pub"
n="$(grep -c 'mg-model keygen' "$script")"
[ "$n" = 1 ] && pass "stage-model.sh calls mg-model keygen once" \
    || fail "stage-model.sh calls mg-model keygen $n times"

# The two mimir-server entrypoints differ only in the trust path.
entrypoint() { awk '/^    entrypoint:/ { on = 1; next } on && /^    [a-z]/ { exit } on' "$1"; }
entrypoint "$repo/deploy/compose/compose.granite.yaml" \
    | sed 's#/etc/midgard/trust/model-signing.pub#TRUST#' > "$work/granite"
entrypoint "$repo/deploy/compose/compose.dev-model.yaml" \
    | sed 's#/model-trust/model-signing.pub#TRUST#' > "$work/dev"
if [ -s "$work/granite" ] && diff -q "$work/granite" "$work/dev" >/dev/null; then
    pass "compose.dev-model.yaml repeats compose.granite.yaml's entrypoint but for the trust path"
else
    fail "compose.dev-model.yaml's entrypoint has drifted from compose.granite.yaml's"
fi

# 2. Dry runs on a copy of the script, in a tree of its own (the script works from the
#    checkout root it lives in), with a stand-in docker that records every call. The
#    keystore check answers $STUB_KEYSTORE (0: present, 1: absent).
mkdir -p "$work/tree/deploy/compose" "$work/tree/deploy/trust" "$work/bin"
cp "$script" "$work/tree/deploy/compose/stage-model.sh"
cat > "$work/bin/docker" <<'STUB'
#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
case "$*" in
    *"test -f /keys/model-signing.keystore"*) exit "${STUB_KEYSTORE:-0}" ;;
esac
exit 0
STUB
chmod +x "$work/bin/docker"
run() { # run KEYSTORE [ARGS...]
    local keystore="$1"
    shift
    : > "$work/calls"
    STUB_LOG="$work/calls" STUB_KEYSTORE="$keystore" PATH="$work/bin:$PATH" \
        bash "$work/tree/deploy/compose/stage-model.sh" "$@" > "$work/out" 2>&1
}

# --help prints the usage and exits 0, touching nothing.
run 0 --help
status=$?
[ "$status" -eq 0 ] && grep -q -- '--development' "$work/out" && [ ! -s "$work/calls" ] \
    && pass "--help prints the usage and exits 0" || fail "--help: exit $status: $(cat "$work/out")"

# Without --development and without the ceremony's anchor: refused before docker runs.
run 0
status=$?
[ "$status" -eq 1 ] && grep -q 'deploy/trust/model-signing.pub is not committed' "$work/out" \
    && [ ! -s "$work/calls" ] \
    && pass "without the committed anchor, nothing runs" \
    || fail "without the committed anchor: exit $status: $(cat "$work/out")"

# With the anchor but no keystore from the staging enclave: refused, nothing signed.
echo "ML-DSA-65 model-signing 00" > "$work/tree/deploy/trust/model-signing.pub"
run 1
status=$?
if [ "$status" -eq 1 ] && grep -q 'holds no model-signing.keystore' "$work/out" \
    && ! grep -q 'mg-model' "$work/calls"; then
    pass "without the staging keystore, it fails fast and signs nothing"
else
    fail "without the staging keystore: exit $status: $(cat "$work/out")"
fi

# With both: it signs, never generates a key, and verifies against the committed anchor.
run 0
status=$?
if [ "$status" -eq 0 ] && grep -q 'mg-model sign' "$work/calls" \
    && ! grep -q 'keygen' "$work/calls" && ! grep -q 'midgard-model-trust' "$work/calls" \
    && grep -q -- '--trust /tmp/model-signing.pub' "$work/calls" \
    && grep -q 'midgard-model-signing:/keys:ro' "$work/calls"; then
    pass "production staging signs with the provisioned keystore and generates no key"
else
    fail "production staging: exit $status: $(cat "$work/out"); calls: $(cat "$work/calls")"
fi

# --development: a keystore v2 under a passphrase, the public half only into the trust
# volume, and a warning.
rm "$work/tree/deploy/trust/model-signing.pub"
run 1 --development
status=$?
calls="$(cat "$work/calls")"
if [ "$status" -eq 0 ] \
    && grep -q 'mg-model keygen --out /keys' <<<"$calls" \
    && grep -q 'MIDGARD_KEYSTORE_PASSPHRASE_FILE=/keystore-passphrase/passphrase' <<<"$calls" \
    && grep -q 'midgard-model-trust:/model-trust' <<<"$calls" \
    && grep -q 'cp /keys/model-signing.pub /model-trust/model-signing.pub' <<<"$calls" \
    && grep -q -- '--trust /model-trust/model-signing.pub' <<<"$calls" \
    && ! grep -q 'model-signing.keystore /model-trust' <<<"$calls"; then
    pass "--development writes a keystore under a passphrase and only the public half to midgard-model-trust"
else
    fail "--development: exit $status: $(cat "$work/out"); calls: $calls"
fi
grep -q 'WARNING: this deployment trusts a development model-signing key; production anchors come from deploy/trust' "$work/out" \
    && pass "--development prints the warning" || fail "--development printed no warning"

if [ "$failures" -gt 0 ]; then
    echo "stage-model tests: $failures failure(s)"
    exit 1
fi
echo "stage-model tests: all passed"
