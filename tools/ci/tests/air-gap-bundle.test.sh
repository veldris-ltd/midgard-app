#!/usr/bin/env bash
# Tests for installing Midgard from the air-gap bundle alone
# (deploy/air-gap-bundle/install.sh). [FR-B-5] [AC-B-3] [RC-19] [VLD-ANB-MIDGARD-0035]
#
#   bash tools/ci/tests/air-gap-bundle.test.sh
#
# The claims under test: install.sh, taken from the bundle's deploy kit and run in a
# directory holding only the bundle and the operator's anchors, with no checkout,
#   - installs the bundled chart, with the bundled chart values, and never a path in a
#     checkout;
#   - points the seed, model and egress images at the enclosure's registry, with their
#     pinned digests;
#   - pushes every image but the kind node with skopeo (copy --all --preserve-digests),
#     or crane, and checks the digest the registry reports;
#   - loads them into kind, for a rehearsal, under the name the chart gives each, its
#     digest unchanged;
#   - never calls docker, helm repo, or anything that pulls or fetches;
# and refuses, having changed nothing, a bundle that does not verify, a missing member,
# an archive whose digest is not its pin in images.lock or externals.lock, a release
# anchor that is not the operator's, a copy of install.sh that is not the bundle's, and a
# registry install with neither skopeo nor crane.
#
# mg-bundle, helm, kubectl, kind, skopeo, crane, docker, curl and wget are stubs that
# record their calls. The bundle is a tar the mg-bundle stub verifies against a test
# anchor; mg-bundle's own verification is tested in tools/mg-bundle/tests/cli.rs. The
# chart is the repository's, packaged as the release packages it.
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
unix() { if command -v cygpath >/dev/null 2>&1; then cygpath -u "$1"; else echo "$1"; fi; }
hex() { printf "$1%.0s" $(seq 64); }

# oci OUT SEED: a small OCI archive whose one manifest is unique to SEED; prints its digest.
oci() {
    local d h size
    d="$(mktemp -d "$work/oci.XXXXXX")"
    mkdir -p "$d/blobs/sha256"
    printf '{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[],"annotations":{"seed":"%s"}}' "$2" > "$d/m"
    h="$(sha256sum "$d/m" | cut -c1-64)"
    size="$(wc -c < "$d/m" | tr -d ' ')"
    mv "$d/m" "$d/blobs/sha256/$h"
    printf '{"imageLayoutVersion":"1.0.0"}' > "$d/oci-layout"
    printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.index.v1+json","digest":"sha256:%s","size":%s,"annotations":{"org.opencontainers.image.ref.name":"9.9.9"}}]}' "$h" "$size" > "$d/index.json"
    tar -C "$d" -cf "$1" oci-layout index.json blobs
    echo "sha256:$h"
}

# --- a release, as release-images and release-bundle leave one -------------------------
V=9.9.9
members="$work/members"
mkdir -p "$members"
for s in http-gateway mimir-server; do
    echo "registry.enclosure.local/midgard/$s:$V $(oci "$members/$s.oci.tar" "$s")"
done | sort > "$members/images.lock"
cat > "$members/chart-values.yaml" <<EOF
namespace: midgard-enclosure
image:
  registry: registry.enclosure.local/midgard
  digests:
    http-gateway: $(awk '/http-gateway/ {print $2}' "$members/images.lock")
    mimir-server: $(awk '/mimir-server/ {print $2}' "$members/images.lock")
EOF
for f in images.lock chart-values.yaml; do
    echo "test release signature" > "$members/$f.sig"
    echo "test provenance" > "$members/$f.att"
done
# The external images, in a lock of the repository's form, with test digests.
seed="$(oci "$members/seed.oci.tar" seed)" model="$(oci "$members/model.oci.tar" model)"
egress="$(oci "$members/egress.oci.tar" egress)" node="$(oci "$members/node.oci.tar" node)"
cat > "$work/externals.lock" <<EOF
# reference                            digest     member          use
gcr.io/distroless/base-debian12:debug  $seed      seed.oci.tar    seedImage
ghcr.io/ggml-org/llama.cpp:server      $model     model.oci.tar   modelServing.image
haproxy                                $egress    egress.oci.tar  egressBroker.image
kindest/node:v1.37.0                   $node      node.oci.tar    kind
EOF
# The chart, packaged as the release packages it, for version V.
mkdir -p "$work/chart"
cp -R "$root/deploy/helm/midgard" "$work/chart/midgard"
sed -i "s/^appVersion: .*/appVersion: \"$V\"/" "$work/chart/midgard/Chart.yaml"
tar -C "$work/chart" -czf "$members/helm-chart.tgz" midgard
# The release anchor the images were built with, and the operator's copy of it.
echo "ML-DSA-87 test-release 00ff" > "$work/release-trust.pub"
echo "release" > "$work/rows.txt"
EXTERNALS_LOCK="$work/externals.lock" RELEASE_ARTEFACTS_ROWS="$work/rows.txt" \
    MIDGARD_TRUST_ANCHORS="$work/release-trust.pub" \
    bash "$root/deploy/air-gap-bundle/kit.sh" "$members/deploy-kit.tar" >/dev/null ||
    { echo "FAIL  kit.sh builds the deploy kit"; exit 1; }

# bundle OUT [MEMBER...]: a "bundle" of every member but those named.
bundle() {
    local out="$1" skip=" ${*:2} " f list=()
    for f in $(ls "$members"); do [[ "$skip" == *" $f "* ]] || list+=("$f"); done
    tar -C "$members" -cf "$out" "${list[@]}"
}

# --- the site: the bundle, the operator's anchors, the tools; no checkout ---------------
site="$work/site"
mkdir -p "$site" "$work/stubs"
bundle "$site/midgard-$V.mgbundle"
echo "ML-DSA-65 test-admission 00aa" > "$site/bifrost-trust.pub"
cp "$work/release-trust.pub" "$site/release-trust.pub"
tar -xOf "$members/deploy-kit.tar" deploy/air-gap-bundle/install.sh > "$site/install.sh"
calls="$work/calls"

stub() { # stub NAME BODY: a tool that records its call, then runs BODY
    printf '#!/usr/bin/env bash\necho "%s $*" >> "%s"\n%s\n' "$1" "$calls" "$2" > "$work/stubs/$1"
    chmod +x "$work/stubs/$1"
}
# mg-bundle: verifies the "bundle" against the test anchor, then writes it out.
stub mg-bundle "
anchor=\"\$3\" out=\"\$5\" bundle=\"\$6\"
[ \"\$(cat \"\$anchor\")\" = 'ML-DSA-65 test-admission 00aa' ] || { echo 'mg-bundle: bundle REJECTED' >&2; exit 1; }
mkdir -p \"\$out\" && tar -xf \"\$bundle\" -C \"\$out\" || exit 1
for f in \$(ls \"\$out\"); do echo \"\$f@$V\"; done"
# helm: the chart it is given must exist.
stub helm "
[ \"\$1\" = upgrade ] || exit 0
chart=\"\$(if command -v cygpath >/dev/null 2>&1; then cygpath -u \"\$4\"; else echo \"\$4\"; fi)\"
[ -f \"\$chart\" ] && tar -tzf \"\$chart\" midgard/Chart.yaml >/dev/null"
stub kubectl '
case "$*" in
    *"get secret"*) exit 1 ;;
    *"create configmap"*) echo "kind: ConfigMap" ;;
    *"apply -f -"*) cat > /dev/null ;;
esac
exit 0'
# skopeo: the registry stores the archive as it is, and reports its digest (LIAR: another).
stub skopeo "
while [ \"\$#\" -gt 0 ]; do
    case \"\$1\" in --digestfile) df=\"\$2\"; shift 2 ;; oci-archive:*) a=\"\${1#oci-archive:}\"; shift ;; *) shift ;; esac
done
u() { if command -v cygpath >/dev/null 2>&1; then cygpath -u \"\$1\"; else echo \"\$1\"; fi; }
d=\"\$(bash '$root/deploy/images/externals.sh' digest \"\$(u \"\$a\")\")\" || exit 1
[ -z \"\${LIAR:-}\" ] || d=sha256:$(hex 0)
printf '%s' \"\$d\" > \"\$(u \"\$df\")\""
stub crane "
[ \"\$1\" = push ] || exit 1
u() { if command -v cygpath >/dev/null 2>&1; then cygpath -u \"\$1\"; else echo \"\$1\"; fi; }
d=\"\$(grep -o 'sha256:[0-9a-f]\{64\}' \"\$(u \"\$2\")/index.json\")\"
echo \"\$3@\$d\" > \"\$(u \"\$5\")\""
# kind: records the name containerd would give the image, and its digest.
stub kind "
[ \"\$1 \$2\" = 'load image-archive' ] || exit 1
u() { if command -v cygpath >/dev/null 2>&1; then cygpath -u \"\$1\"; else echo \"\$1\"; fi; }
a=\"\$(u \"\$3\")\"
echo \"\$(tar -xOf \"\$a\" index.json | grep -o '\"io.containerd.image.name\":\"[^\"]*\"' | cut -d'\"' -f4) \$(bash '$root/deploy/images/externals.sh' digest \"\$a\")\" >> '$work/loaded'"
for t in docker curl wget; do stub "$t" 'exit 1'; done

# install [VAR=VALUE...] ARG...: run the bundle's install.sh in the site, as an operator.
install() {
    local vars=()
    while [ "$#" -gt 0 ] && [[ "$1" == *=* ]]; do vars+=("$1"); shift; done
    rm -f "$calls" "$work/loaded"
    (cd "$site" && env PATH="$work/stubs:$PATH" "${vars[@]}" bash ./install.sh "$@" 2>&1)
}
std=(--bundle-trust bifrost-trust.pub --release-trust release-trust.pub)
calls_of() { if [ -f "$calls" ]; then grep -c "^$1 " "$calls" || true; else echo 0; fi; }
unchanged() { # nothing was pushed, loaded or deployed
    [ "$(calls_of skopeo)" = 0 ] && [ "$(calls_of crane)" = 0 ] && [ "$(calls_of kind)" = 0 ] &&
        [ "$(calls_of helm)" = 0 ] && [ "$(calls_of kubectl)" = 0 ]
}
# refused NAME TEXT ARG...: install refuses, saying TEXT, having changed nothing.
refused() {
    local name="$1" text="$2" out
    shift 2
    if out="$(install "$@")"; then
        fail "refuses $name"
    elif ! grep -qF -- "$text" <<<"$out"; then
        fail "refuses $name, saying \"$text\": $out"
    elif ! unchanged || ! grep -q 'nothing was changed' <<<"$out"; then
        fail "refuses $name, having changed nothing: $out"
    else
        pass "refuses $name, having changed nothing"
    fi
}
reg=registry.test/midgard

# --- a registry install with skopeo ---------------------------------------------------
if out="$(install "midgard-$V.mgbundle" "${std[@]}" --registry "$reg" --push)"; then
    pass "installs from the bundle alone, in a directory with no checkout"
else
    fail "installs from the bundle alone, in a directory with no checkout: $out"
fi
helm_call="$(grep '^helm upgrade' "$calls" 2>/dev/null)"
chart_arg="$(awk '{print $5}' <<<"$helm_call")"
check "installs the bundled chart" grep -qE '(verified[/\\]helm-chart\.tgz)$' <<<"$chart_arg"
check "and the bundled, signed chart values" grep -qE -- '-f [^ ]*verified[/\\]chart-values\.yaml ' <<<"$helm_call"
check "nothing from a checkout reaches helm" bash -c "! grep -qF -- '$root' <<<'$helm_call' && ! grep -qF -- '$(cygpath -w "$root" 2>/dev/null || echo "$root")' <<<'$helm_call'"
check "the seed image is the registry's, with its pinned digest" \
    grep -qF -- "--set seedImage.repository=$reg/distroless/base-debian12 --set seedImage.digest=$seed" <<<"$helm_call"
check "the model image is the registry's, with its pinned digest" \
    grep -qF -- "--set modelServing.image=$reg/ggml-org/llama.cpp@$model" <<<"$helm_call"
check "the egress broker image is the registry's, with its pinned digest" \
    grep -qF -- "--set egressBroker.image=$reg/haproxy@$egress" <<<"$helm_call"
check "the services' images are the registry's" grep -qF -- "--set image.registry=$reg " <<<"$helm_call"
check "every image but the kind node is pushed, with skopeo copy --all --preserve-digests" \
    test "$(grep -c '^skopeo copy --all --preserve-digests --digestfile ' "$calls")" = 5
check "each service image is pushed to the registry under the release's version" \
    grep -q "docker://$reg/http-gateway:$V\$" "$calls"
check "each external image is pushed under a tag of its own digest" \
    grep -q "docker://$reg/haproxy:sha256-${egress#sha256:}\$" "$calls"
check "the kind node image is not pushed" bash -c "! grep -q 'node.oci.tar' '$calls'"
check "the release anchor becomes the trust ConfigMap" grep -q '^kubectl .*create configmap midgard-release-trust' "$calls"
check "docker is never called" test "$(calls_of docker)" = 0
check "nothing pulls or fetches: no helm repo, no pull, no curl or wget" \
    bash -c "! grep -qE '^helm (repo|pull|dependency)|(^| )pull( |\$)|^curl |^wget ' '$calls'"

# A registry that reports another digest: stopped before anything is deployed.
if out="$(install LIAR=1 "midgard-$V.mgbundle" "${std[@]}" --registry "$reg" --push)"; then
    fail "a push that changes a digest stops the install"
else
    grep -q 'was pushed as sha256:0000' <<<"$out" && [ "$(calls_of helm)" = 0 ] &&
        pass "a push that changes a digest stops the install before anything is deployed" ||
        fail "a push that changes a digest stops the install before anything is deployed: $out"
fi

# crane, when skopeo is not there.
if out="$(install SKOPEO="$work/absent" "midgard-$V.mgbundle" "${std[@]}" --registry "$reg" --push)"; then
    check "with crane and no skopeo, every image but the kind node is pushed with crane" \
        test "$(grep -c '^crane push ' "$calls")" = 5
else
    fail "with crane and no skopeo, the install completes: $out"
fi
refused "a registry install with neither skopeo nor crane" "skopeo or crane" \
    SKOPEO="$work/absent" CRANE="$work/absent" "midgard-$V.mgbundle" "${std[@]}" --registry "$reg" --push

# --- a kind rehearsal -------------------------------------------------------------------
if out="$(install "midgard-$V.mgbundle" "${std[@]}" --kind rehearsal)"; then
    check "kind: every image but the kind node is loaded" test "$(grep -c . "$work/loaded")" = 5
    check "kind: each under the name the chart resolves, its digest unchanged" \
        grep -qx "registry.enclosure.local/midgard/distroless/base-debian12:sha256-${seed#sha256:} $seed" "$work/loaded"
    check "kind: a service image under its release name, its digest unchanged" \
        grep -qx "registry.enclosure.local/midgard/http-gateway:$V $(awk '/http-gateway/ {print $2}' "$members/images.lock")" "$work/loaded"
    check "kind: the chart is installed in the kind cluster" grep -q '^helm upgrade .*--kube-context kind-rehearsal' "$calls"
else
    fail "a kind rehearsal installs: $out"
fi
refused "a kind rehearsal without kind" "not on PATH: kind" KIND="$work/absent" "midgard-$V.mgbundle" "${std[@]}" --kind rehearsal

# --- refusals ---------------------------------------------------------------------------
refused "a bundle that does not verify" "did not verify" \
    "midgard-$V.mgbundle" --bundle-trust release-trust.pub --release-trust release-trust.pub --push
for m in seed.oci.tar node.oci.tar http-gateway.oci.tar helm-chart.tgz chart-values.yaml images.lock.sig deploy-kit.tar; do
    bundle "$site/missing.mgbundle" "$m"
    refused "a bundle missing $m" "$m" missing.mgbundle "${std[@]}" --push
done
# An archive that is not the one its lock pins.
cp "$members/model.oci.tar" "$work/model.keep"
cp "$members/egress.oci.tar" "$members/model.oci.tar"
bundle "$site/swapped.mgbundle"
cp "$work/model.keep" "$members/model.oci.tar"
refused "an external archive whose digest is not its pin in externals.lock" \
    "model.oci.tar is $egress, not $model" swapped.mgbundle "${std[@]}" --push
cp "$members/mimir-server.oci.tar" "$work/svc.keep"
cp "$members/http-gateway.oci.tar" "$members/mimir-server.oci.tar"
bundle "$site/swapped.mgbundle"
cp "$work/svc.keep" "$members/mimir-server.oci.tar"
refused "a service archive whose digest is not its pin in images.lock" \
    "mimir-server.oci.tar is" swapped.mgbundle "${std[@]}" --push
# The release anchor: the operator's, by file or by SHA-256.
echo "ML-DSA-87 someone-else 11ee" > "$site/other.pub"
refused "a bundled release anchor that is not the operator's" "is not the one you supplied" \
    "midgard-$V.mgbundle" --bundle-trust bifrost-trust.pub --release-trust other.pub --push
refused "a release anchor fingerprint that does not match" "does not have the SHA-256" \
    "midgard-$V.mgbundle" --bundle-trust bifrost-trust.pub --release-trust "sha256:$(hex 1)" --push
fp="sha256:$(tr -d '\r' < "$work/release-trust.pub" | sha256sum | cut -c1-64)"
if out="$(install "midgard-$V.mgbundle" --bundle-trust bifrost-trust.pub --release-trust "$fp" --push)"; then
    pass "the release anchor may be given by its SHA-256"
else
    fail "the release anchor may be given by its SHA-256: $out"
fi
if out="$(install "midgard-$V.mgbundle" --bundle-trust bifrost-trust.pub --push)"; then
    fail "the release anchor is required"
else
    grep -q 'never trusted on its own' <<<"$out" && unchanged && pass "the release anchor is required" ||
        fail "the release anchor is required: $out"
fi
# Only the installer the bundle carries runs.
cp "$site/install.sh" "$work/install.keep"
echo "# changed" >> "$site/install.sh"
refused "a copy of install.sh that is not the bundle's" "not the installer the bundle carries" \
    "midgard-$V.mgbundle" "${std[@]}" --push
cp "$work/install.keep" "$site/install.sh"

echo
if [ "$failures" -eq 0 ]; then
    echo "air-gap-bundle: all checks passed"
else
    echo "air-gap-bundle: $failures check(s) failed"
    exit 1
fi
