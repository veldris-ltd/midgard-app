#!/usr/bin/env bash
# Tests for `run.sh release-bundle`: the air-gap bundle carries everything an install
# from it alone needs. [FR-B-5] [AC-B-3] [VLD-ANB-MIDGARD-0035]
#
#   bash tools/ci/tests/release-bundle.test.sh
#
# The claim under test: from a finished release (release and release-images) and the
# directory of external images, release-bundle packs every gated binary with its
# signature and provenance, every service image archive images.lock names, images.lock,
# the chart values and the rendering with theirs, the chart packaged by helm, the deploy
# kit, and every external image in externals.lock, mounted read-only from
# MIDGARD_EXTERNAL_IMAGES. It refuses, before anything is packed, when that variable is
# unset, an external archive is missing or not as pinned, or a part of the release is
# missing. A stub docker records the toolchain call instead of packing; a stub helm
# packages the chart.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}
check() {
    local name="$1"
    shift
    if "$@"; then pass "$name"; else fail "$name"; fi
}

# oci OUT SEED: a small OCI archive whose one manifest is unique to SEED; prints its digest.
oci() {
    local d h size
    d="$(mktemp -d "$work/oci.XXXXXX")"
    mkdir -p "$d/blobs/sha256"
    printf '{"schemaVersion":2,"manifests":[],"annotations":{"seed":"%s"}}' "$2" > "$d/m"
    h="$(sha256sum "$d/m" | cut -c1-64)"
    size="$(wc -c < "$d/m" | tr -d ' ')"
    mv "$d/m" "$d/blobs/sha256/$h"
    printf '{"imageLayoutVersion":"1.0.0"}' > "$d/oci-layout"
    printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.index.v1+json","digest":"sha256:%s","size":%s}]}' "$h" "$size" > "$d/index.json"
    tar -C "$d" -cf "$1" oci-layout index.json blobs
    echo "sha256:$h"
}

V="$(bash "$root/tools/ci/release-artefacts.sh" --version)"

# A finished release: test stand-ins, not real artefacts.
out="$work/release-out"
mkdir -p "$out/bin" "$out/images" "$out/chart"
echo "mg-verify $V tool bin/mg-verify" > "$out/artefacts.txt"
echo "mg-verify mg-verify tool -" > "$out/release-manifest.txt"
for f in bin/mg-verify bin/mg-verify.sig bin/mg-verify.att; do echo "test stand-in" > "$out/$f"; done
for s in http-gateway mimir-server; do
    echo "registry.enclosure.local/midgard/$s:$V $(oci "$out/images/$s.oci.tar" "$s")"
done | sort > "$out/images/images.lock"
for f in images/images.lock chart/values.yaml chart/enclosure.yaml; do
    [ -f "$out/$f" ] || echo "test stand-in" > "$out/$f"
    echo "test signature" > "$out/$f.sig"
    echo "test provenance" > "$out/$f.att"
done

# The external images, provisioned while connected, with a lock of test digests.
ext="$work/externals"
mkdir -p "$ext"
a="$(oci "$ext/seed.oci.tar" seed)" b="$(oci "$ext/node.oci.tar" node)"
cat > "$work/externals.lock" <<EOF
example.test/seed:debug  $a  seed.oci.tar  seedImage
example.test/node:v1     $b  node.oci.tar  kind
EOF
echo "ML-DSA-87 test-release 00ff" > "$work/release-trust.pub"
echo "release" > "$work/rows.txt"
for f in release.keystore release.passphrase bifrost.keystore bifrost.passphrase; do echo test > "$work/$f"; done
mkdir -p "$work/registry"

mkdir -p "$work/stubs"
cat > "$work/stubs/docker" <<EOF
#!/usr/bin/env bash
[ "\$1" = image ] && exit 0
printf '%s\n' "\$@" > "$work/docker.args"
EOF
# helm package: the chart as the release packages it.
cat > "$work/stubs/helm" <<EOF
#!/usr/bin/env bash
[ "\$1" = package ] || exit 1
v="\$(tr -d '\r' < "\$2/Chart.yaml" | sed -n 's/^version: *//p')"
tar -C "\$(dirname "\$2")" -czf "\$4/midgard-\$v.tgz" midgard
EOF
chmod +x "$work/stubs/docker" "$work/stubs/helm"

# bundle [VAR=VALUE...]: run the job as a release machine would, with no forge.
bundle() {
    rm -f "$work/docker.args"
    (cd "$root" && env -u GITHUB_ACTIONS -u GITLAB_CI -u GITHUB_REF_TYPE -u CI_COMMIT_TAG \
        -u MIDGARD_IN_TOOLCHAIN PATH="$work/stubs:$PATH" MIDGARD_RELEASE_OUT="$out" \
        MIDGARD_EXTERNAL_IMAGES="$ext" EXTERNALS_LOCK="$work/externals.lock" \
        RELEASE_ARTEFACTS_ROWS="$work/rows.txt" MIDGARD_TRUST_ANCHORS="$work/release-trust.pub" \
        MIDGARD_SIGNING_KEYSTORE="$work/release.keystore" \
        MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE="$work/release.passphrase" \
        MIDGARD_BIFROST_KEYSTORE="$work/bifrost.keystore" \
        MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE="$work/bifrost.passphrase" \
        MIDGARD_BIFROST_REGISTRY="$work/registry" "$@" bash tools/ci/run.sh release-bundle 2>&1)
}
packs() { grep -qx -- "$1" "$work/docker.args"; }

if text="$(bundle)"; then
    pass "release-bundle packs a finished release"
else
    fail "release-bundle packs a finished release: $text"
fi
if [ -f "$work/docker.args" ]; then
    o=/release-out
    check "the binaries, with their signatures and provenance" \
        bash -c "grep -qx 'tool=mg-verify:$V:$o/bin/mg-verify' '$work/docker.args' && grep -qx 'attestation=mg-verify.sig:$V:$o/bin/mg-verify.sig' '$work/docker.args'"
    check "every service image archive images.lock names" \
        bash -c "grep -qx 'image=http-gateway.oci.tar:$V:$o/images/http-gateway.oci.tar' '$work/docker.args' && grep -qx 'image=mimir-server.oci.tar:$V:$o/images/mimir-server.oci.tar' '$work/docker.args'"
    for m in images.lock:images/images.lock chart-values.yaml:chart/values.yaml enclosure.yaml:chart/enclosure.yaml; do
        check "${m%%:*}, with its signature and provenance" \
            bash -c "grep -qx 'deploy=${m%%:*}:$V:$o/${m#*:}' '$work/docker.args' && grep -qx 'attestation=${m%%:*}.sig:$V:$o/${m#*:}.sig' '$work/docker.args' && grep -qx 'attestation=${m%%:*}.att:$V:$o/${m#*:}.att' '$work/docker.args'"
    done
    check "the chart, packaged by helm" packs "deploy=helm-chart.tgz:$V:$o/bundle/midgard-$V.tgz"
    check "the deploy kit" packs "deploy=deploy-kit.tar:$V:$o/bundle/deploy-kit.tar"
    check "every external image in externals.lock" \
        bash -c "grep -qx 'image=seed.oci.tar:$V:/externals/seed.oci.tar' '$work/docker.args' && grep -qx 'image=node.oci.tar:$V:/externals/node.oci.tar' '$work/docker.args'"
    check "the external images are mounted read-only" packs "$(cd "$ext" && pwd):/externals:ro"
    check "the release directory is mounted" packs "$(cd "$out" && pwd):/release-out"
    check "the bundle is signed with the admission keystore" packs /keys/admission.keystore
    check "the kit carries install.sh" \
        bash -c "tar -tf '$out/bundle/deploy-kit.tar' | grep -qx deploy/air-gap-bundle/install.sh"
    check "the kit carries the lock the external images were packed against" \
        bash -c "cmp -s <(tar -xOf '$out/bundle/deploy-kit.tar' deploy/images/externals.lock) '$work/externals.lock'"
    check "the kit carries the release anchor the images were built with" \
        bash -c "cmp -s <(tar -xOf '$out/bundle/deploy-kit.tar' deploy/trust/release-trust.pub) '$work/release-trust.pub'"
fi

# refused NAME TEXT [VAR=VALUE...]: release-bundle refuses, saying TEXT, packing nothing.
refused() {
    local name="$1" text="$2" t
    shift 2
    if t="$(bundle "$@")"; then
        fail "refuses $name"
    elif ! grep -qF -- "$text" <<<"$t"; then
        fail "refuses $name, saying \"$text\": $t"
    elif [ -e "$work/docker.args" ]; then
        fail "refuses $name, packing nothing"
    else
        pass "refuses $name, packing nothing"
    fi
}
refused "with MIDGARD_EXTERNAL_IMAGES unset" "MIDGARD_EXTERNAL_IMAGES is not set" MIDGARD_EXTERNAL_IMAGES=
mv "$ext/node.oci.tar" "$work/node.keep"
refused "a missing external archive" "node.oci.tar is missing"
cp "$ext/seed.oci.tar" "$ext/node.oci.tar"
refused "an external archive that is not as pinned" "node.oci.tar is $a, not the pinned $b"
mv "$work/node.keep" "$ext/node.oci.tar"
mv "$out/images/mimir-server.oci.tar" "$work/svc.keep"
refused "a missing service image archive" "images/mimir-server.oci.tar"
mv "$work/svc.keep" "$out/images/mimir-server.oci.tar"
mv "$out/chart/values.yaml.sig" "$work/sig.keep"
refused "chart values with no signature" "chart/values.yaml.sig"
mv "$work/sig.keep" "$out/chart/values.yaml.sig"

echo
if [ "$failures" -eq 0 ]; then
    echo "release-bundle: all checks passed"
else
    echo "release-bundle: $failures check(s) failed"
    exit 1
fi
