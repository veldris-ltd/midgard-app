#!/usr/bin/env bash
# Install Midgard on a disconnected site from its signed offline release bundle alone:
# verify the bundle, check every part of it against its pin, put the release's own
# images into the enclosure's registry (or a kind cluster) with their digests unchanged,
# and deploy the release's own chart. [FR-B-5] [AC-B-3] [RC-19] [RC-18]
# [VLD-ANB-MIDGARD-0035]
#
#   bash install.sh BUNDLE --bundle-trust FILE --release-trust FILE|sha256:<hex>
#       (--registry REGISTRY --push | --kind CLUSTER)
#       [--rebuild] [--context KUBE_CONTEXT] [-f VALUES]...
#
#   --bundle-trust   the Bifrost admission anchor (bifrost-trust.pub), from the operator
#   --release-trust  the release anchor (release-trust.pub), or the SHA-256 of that file,
#                    from the operator: the copy the bundle carries must be the same
#   --push           push the images to REGISTRY (default registry.enclosure.local/midgard)
#   --kind           load them into a local kind cluster instead (a rehearsal)
#   --rebuild        build the service images again from the bundle's binaries, as a check
#                    and instead of its archives; needs docker and the build bases
#
# Nothing here reaches beyond the enclosure: no image is pulled, no chart repository is
# read, and nothing is taken from a checkout. It runs from a directory holding only the
# bundle, the operator's two anchors and the tools: mg-bundle, helm, kubectl, and skopeo
# or crane (--push) or kind (--kind). This script is itself in the bundle's deploy kit:
# take it from there (README.md beside it), since it refuses to run as any other copy.
#
#   1. `mg-bundle import --out-dir` verifies the bundle signature, its manifest and every
#      artefact against --bundle-trust (ML-DSA-65), and only then writes them out.
#   2. Every part an install needs must be there: the deploy kit, the chart (helm
#      package, its appVersion the bundle's version), images.lock and the chart values
#      with their release signatures, an OCI archive for each image images.lock names,
#      and one for each external image the kit's externals.lock names. The release
#      anchor the kit carries must be the operator's.
#   3. Each archive's index digest must be its pin in images.lock or externals.lock.
#   4. The tools must be there: helm and kubectl; skopeo or crane to push, which keep a
#      digest (docker push is not used: it can change one); kind for a rehearsal.
#      Until here nothing is changed, and any refusal says so.
#   5. Each image is pushed by digest and the digest the registry reports is checked,
#      or loaded into kind under the name the chart gives it.
#   6. The bundled chart is installed with the bundled, signed chart values, its images
#      at the registry: the services, and seedImage, modelServing.image and
#      egressBroker.image, by their pinned digests. The release anchor becomes the
#      chart's trust ConfigMap.
#
# --rebuild runs deploy/images/build.sh from the kit on the bundle's binaries, which
# needs the build bases supplied separately (docker, with service.Dockerfile's BASE and
# RUNTIME loaded). The images are reproducible, so their digests set into the chart's
# values must be those of the signed chart values, or it stops before anything is pushed.
#
# The enclosure's secrets (identities, keystore passphrases, seeds, forge-jobs) come from
# key custody through the chart's secrets.provider, never a bundle; services wait for them.
set -euo pipefail
export MSYS_NO_PATHCONV=1

# helm, kubectl and kind may be Windows binaries under Git Bash: hand them native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }
usage() {
    sed -n '8,18p' "$0" | sed 's/^#//' >&2
    exit 2
}
refuse() {
    echo "install: $*" >&2
    echo "install: nothing was changed" >&2
    exit 1
}
[ "$#" -ge 1 ] || usage
BUNDLE="$1"; shift
bundle_trust="" release_trust="" registry=registry.enclosure.local/midgard push=0 kind=""
context="" rebuild=0
values=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bundle-trust) bundle_trust="$2"; shift 2 ;;
        --release-trust) release_trust="$2"; shift 2 ;;
        --registry) registry="$2"; shift 2 ;;
        --push) push=1; shift ;;
        --kind) kind="$2"; shift 2 ;;
        --rebuild) rebuild=1; shift ;;
        --context) context="$2"; shift 2 ;;
        -f) values+=(-f "$(native "$2")"); shift 2 ;;
        *) echo "install: unknown option $1" >&2; usage ;;
    esac
done
[ -n "$bundle_trust" ] || { echo "install: --bundle-trust is required" >&2; usage; }
[ -n "$release_trust" ] || {
    echo "install: --release-trust is required: the release anchor, or its SHA-256, from the key ceremony; the copy inside the bundle is never trusted on its own" >&2
    usage
}
if [ "$push" = 1 ] && [ -n "$kind" ]; then echo "install: choose --push or --kind" >&2; exit 2; fi
if [ "$push" = 0 ] && [ -z "$kind" ]; then echo "install: choose --push (a registry) or --kind (a rehearsal)" >&2; exit 2; fi
[ -f "$BUNDLE" ] || refuse "no bundle $BUNDLE"
[ -f "$bundle_trust" ] || refuse "no bundle anchor $bundle_trust"

MG_BUNDLE="${MG_BUNDLE:-mg-bundle}"
HELM="${HELM:-helm}"
KIND="${KIND:-kind}"
KUBECTL="${KUBECTL:-kubectl}"
SKOPEO="${SKOPEO:-skopeo}"
CRANE="${CRANE:-crane}"
DOCKER="${DOCKER:-docker}"
command -v "$MG_BUNDLE" >/dev/null 2>&1 ||
    refuse "mg-bundle is not on PATH (or MG_BUNDLE set); bring the release's mg-bundle with the anchors"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
v="$work/verified"
kit="$work/kit"

# 1. Verify offline, and only then extract.
echo ">>> verifying $BUNDLE offline against $bundle_trust ..."
refs="$("$MG_BUNDLE" import --trust-anchor "$bundle_trust" --out-dir "$v" "$BUNDLE")" ||
    refuse "the bundle did not verify against $bundle_trust"
versions="$(sed -n 's/^[^@]*@//p' <<<"$refs" | sort -u)"
[ "$(wc -l <<<"$versions")" -eq 1 ] || refuse "the bundle carries several versions: $(echo $versions)"
version="$versions"
echo ">>> verified $(wc -l <<<"$refs") artefact(s) of $version"

# 2. Every part an install needs.
missing=()
need_member() { [ -s "$v/$1" ] || missing+=("$1"); }
for m in deploy-kit.tar helm-chart.tgz images.lock chart-values.yaml; do
    need_member "$m"
    case "$m" in images.lock|chart-values.yaml) need_member "$m.sig"; need_member "$m.att" ;; esac
done
[ "${#missing[@]}" -eq 0 ] || refuse "the bundle lacks ${missing[*]}; it was not made by run.sh release-bundle"
mkdir -p "$kit"
tar -xf "$v/deploy-kit.tar" -C "$kit" || refuse "the deploy kit is not a tar archive"
for f in deploy/air-gap-bundle/install.sh deploy/images/externals.sh deploy/images/externals.lock \
    deploy/helm/set-digests.sh deploy/trust/release-trust.pub; do
    [ -s "$kit/$f" ] || missing+=("deploy-kit.tar:$f")
done
[ "${#missing[@]}" -eq 0 ] || refuse "the deploy kit lacks ${missing[*]}"
cmp -s <(tr -d '\r' < "$0") "$kit/deploy/air-gap-bundle/install.sh" ||
    refuse "this is not the installer the bundle carries; take it from the bundle (deploy/air-gap-bundle/README.md, \"Install\")"
chart_version="$(tar -xzOf "$v/helm-chart.tgz" midgard/Chart.yaml 2>/dev/null | tr -d '\r' \
    | sed -n 's/^appVersion: *"\{0,1\}\([^"]*\)"\{0,1\}$/\1/p')"
[ "$chart_version" = "$version" ] ||
    refuse "the bundle is $version but its chart deploys ${chart_version:-nothing}"

# The images: "archive digest target use" lines, the services' from the signed
# images.lock and the external images' from the kit's externals.lock.
images="$work/images.txt"
: > "$images"
while read -r ref digest; do
    [ -n "$ref" ] || continue
    [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || refuse "images.lock has no sha256 digest for $ref"
    name="${ref##*/}"
    name="${name%:*}"
    echo "$name.oci.tar $digest $registry/$name:$version service" >> "$images"
done < <(tr -d '\r' < "$v/images.lock")
[ -s "$images" ] || refuse "images.lock names no image"
# An external image goes to the registry under its own path, without the host it was
# fetched from, and is tagged with its digest so that no later push replaces it.
while read -r ref digest member use; do
    repo="$ref"
    [[ "${repo##*/}" == *:* ]] && repo="${repo%:*}"
    first="${repo%%/*}"
    if [ "$first" != "$repo" ] && { [[ "$first" == *.* ]] || [[ "$first" == *:* ]] || [ "$first" = localhost ]; }; then
        repo="${repo#*/}"
    fi
    echo "$member $digest $registry/$repo:sha256-${digest#sha256:} $use" >> "$images"
done < <(tr -d '\r' < "$kit/deploy/images/externals.lock" | sed 's/#.*//' | awk 'NF')
while read -r archive _ _ _; do need_member "$archive"; done < "$images"
[ "${#missing[@]}" -eq 0 ] || refuse "the bundle lacks the image archive(s) ${missing[*]}"

# The release anchor: the operator's, never only the bundle's own copy.
anchor="$kit/deploy/trust/release-trust.pub"
if [[ "$release_trust" =~ ^sha256:[0-9a-f]{64}$ ]]; then
    [ "sha256:$(sha256sum < "$anchor" | cut -d' ' -f1)" = "$release_trust" ] ||
        refuse "the release anchor in the bundle does not have the SHA-256 you supplied ($release_trust)"
elif [ -f "$release_trust" ]; then
    cmp -s <(tr -d '\r' < "$release_trust") "$anchor" ||
        refuse "the release anchor in the bundle is not the one you supplied ($release_trust)"
else
    refuse "--release-trust names neither a file nor sha256:<64 hex>: $release_trust"
fi
echo ">>> the bundle carries the deploy kit, the chart, the values and $(wc -l < "$images" | tr -d ' ') image archive(s); its release anchor is yours"

# 3. Each archive's digest is its pin.
bad=()
while read -r archive digest _ _; do
    got="$(bash "$kit/deploy/images/externals.sh" digest "$v/$archive" 2>&1)" || got="unreadable ($got)"
    [ "$got" = "$digest" ] || bad+=("$archive is $got, not $digest")
done < "$images"
[ "${#bad[@]}" -eq 0 ] || refuse "image archive(s) not as pinned in images.lock or externals.lock: $(printf '%s; ' "${bad[@]}")"
echo ">>> every image archive has its pinned digest"

# 4. The tools.
tools_missing=()
command -v "$HELM" >/dev/null 2>&1 || tools_missing+=(helm)
command -v "$KUBECTL" >/dev/null 2>&1 || tools_missing+=(kubectl)
pusher=""
if [ "$push" = 1 ]; then
    if command -v "$SKOPEO" >/dev/null 2>&1; then pusher=skopeo
    elif command -v "$CRANE" >/dev/null 2>&1; then pusher=crane
    else tools_missing+=("skopeo or crane (pushing keeps an image's digest only with one of them; docker push is not used)")
    fi
else
    command -v "$KIND" >/dev/null 2>&1 || tools_missing+=(kind)
fi
[ "$rebuild" = 0 ] || command -v "$DOCKER" >/dev/null 2>&1 || tools_missing+=("docker (for --rebuild)")
[ "${#tools_missing[@]}" -eq 0 ] || refuse "not on PATH: $(printf '%s, ' "${tools_missing[@]}" | sed 's/, $//')"

if [ -n "$context" ]; then
    KUBECTL="$KUBECTL --context $context"
    HELM_CONTEXT=(--kube-context "$context")
elif [ -n "$kind" ]; then
    KUBECTL="$KUBECTL --context kind-$kind"
    HELM_CONTEXT=(--kube-context "kind-$kind")
else
    HELM_CONTEXT=()
fi
chart_values="$v/chart-values.yaml"
digests() { grep -E '^    [a-z0-9.-]+: sha256:[0-9a-f]{64}' "$1" | tr -d '\r' | sort; }
[ -n "$(digests "$chart_values")" ] || refuse "the chart values pin no image by digest"
NS="$(tr -d '\r' < "$chart_values" | sed -n 's/^namespace: *//p' | head -1)"

# --rebuild: the service images again, from the verified binaries, as a check.
archives="$v"
if [ "$rebuild" = 1 ]; then
    echo ">>> --rebuild: building the verify-before-run images from the bundle's binaries"
    lock_registry="$(head -1 "$v/images.lock" | awk '{print $1}' | sed 's|/[^/]*$||')"
    RELEASE_ARTEFACTS_ROWS="$kit/supply-chain/release-rows.txt" bash "$kit/deploy/images/build.sh" \
        --version "$version" --verified "$v" --trust "$anchor" --registry "$lock_registry" \
        --out "$work/rebuilt" || refuse "--rebuild: the images did not build"
    tar -xzOf "$v/helm-chart.tgz" midgard/values.yaml > "$work/chart-defaults.yaml"
    bash "$kit/deploy/helm/set-digests.sh" "$work/rebuilt/images.lock" "$work/chart-defaults.yaml" \
        --out "$work/rebuilt-values.yaml" >/dev/null || refuse "--rebuild: the rebuilt images.lock does not fit the chart"
    # Always compared: images built here are deployed only with the release's digests.
    if [ "$(digests "$chart_values")" != "$(digests "$work/rebuilt-values.yaml")" ]; then
        diff <(digests "$chart_values") <(digests "$work/rebuilt-values.yaml") >&2 || true
        refuse "--rebuild: the images built here do not have the digests the release signed"
    fi
    echo ">>> --rebuild: the images built here are the release's, digest for digest"
    archives="$work/rebuilt"
    # The external images are not rebuilt; they are used as the bundle carries them.
    while read -r archive _ _ use; do
        [ "$use" = service ] || cp "$v/$archive" "$archives/$archive"
    done < "$images"
fi

# 5. Make the images available, each with its digest unchanged. The kind node image is
# for creating a rehearsal cluster, not for the chart, so it goes nowhere here.
if [ "$push" = 1 ]; then
    echo ">>> pushing to $registry with $pusher, by digest"
else
    echo ">>> loading into kind cluster $kind"
fi
n=0
while read -r archive digest target use; do
    [ "$use" != kind ] || continue
    n=$((n + 1))
    if [ "$push" = 1 ]; then
        rm -f "$work/pushed"
        case "$pusher" in
            skopeo)
                "$SKOPEO" copy --all --preserve-digests --digestfile "$(native "$work/pushed")" \
                    "oci-archive:$(native "$archives/$archive")" "docker://$target"
                got="$(tr -d '\r\n' < "$work/pushed" 2>/dev/null || true)"
                ;;
            crane)
                # crane pushes an OCI layout directory's one manifest as it is.
                rm -rf "$work/layout"
                mkdir -p "$work/layout"
                tar -xf "$archives/$archive" -C "$work/layout"
                "$CRANE" push "$(native "$work/layout")" "$target" --image-refs "$(native "$work/pushed")"
                got="$(tr -d '\r\n' < "$work/pushed" 2>/dev/null | sed 's/^.*@//' || true)"
                ;;
        esac
        [ "$got" = "$digest" ] || {
            echo "install: $target was pushed as ${got:-an unknown digest}, not $digest; nothing has been deployed" >&2
            exit 1
        }
        echo "  $target@$digest"
    else
        # containerd names an imported image from its index annotation: give it the name
        # the chart's reference resolves to. Only index.json changes, never the image.
        rm -rf "$work/layout"
        mkdir -p "$work/layout"
        tar -xf "$archives/$archive" -C "$work/layout"
        index="$(tr -d '\r\n' < "$work/layout/index.json")"
        media="$(grep -o '"mediaType" *: *"[^"]*"' <<<"$index" | head -1 | sed 's/.*: *"\([^"]*\)"/\1/')"
        size="$(grep -o '"size" *: *[0-9]*' <<<"$index" | head -1 | sed 's/.*: *//')"
        printf '{"schemaVersion":2,"manifests":[{"mediaType":"%s","digest":"%s","size":%s,"annotations":{"io.containerd.image.name":"%s","org.opencontainers.image.ref.name":"%s"}}]}\n' \
            "$media" "$digest" "$size" "$target" "${target##*:}" > "$work/layout/index.json"
        tar -C "$work/layout" -cf "$work/kind-$n.tar" oci-layout index.json blobs
        [ "$(bash "$kit/deploy/images/externals.sh" digest "$work/kind-$n.tar")" = "$digest" ] || {
            echo "install: renaming $archive for kind changed its digest; nothing has been deployed" >&2
            exit 1
        }
        "$KIND" load image-archive "$(native "$work/kind-$n.tar")" --name "$kind"
        rm -f "$work/kind-$n.tar"
        echo "  $target ($digest)"
    fi
done < "$images"

# 6. Deploy the bundled chart, every image at the registry by its pinned digest.
external() { awk -v use="$1" '$4 == use { t = $3; sub(/:[^:\/]*$/, "", t); print t "@" $2 }' "$images"; }
seed="$(external seedImage)" model="$(external modelServing.image)" egress="$(external egressBroker.image)"
[ -n "$seed" ] && [ -n "$model" ] && [ -n "$egress" ] ||
    { echo "install: externals.lock does not name the seed, model and egress images" >&2; exit 1; }
echo ">>> deploying $version from the bundled chart"
$HELM upgrade --install midgard "$(native "$v/helm-chart.tgz")" "${HELM_CONTEXT[@]}" --namespace default \
    -f "$(native "$chart_values")" \
    --set image.registry="$registry" --set image.tag="$version" \
    --set seedImage.repository="${seed%@*}" --set seedImage.digest="${seed#*@}" \
    --set modelServing.image="$model" --set egressBroker.image="$egress" \
    "${values[@]}"
$KUBECTL -n "$NS" create configmap midgard-release-trust \
    --from-file=release-trust.pub="$(native "$anchor")" --dry-run=client -o yaml \
    | $KUBECTL -n "$NS" apply -f - >/dev/null
if ! $KUBECTL -n "$NS" get secret http-gateway-identity >/dev/null 2>&1; then
    echo ">>> NOTE: the enclosure's secrets are not provisioned in $NS; the services wait" \
         "until they are (identities, keystore passphrases, seeds and forge-jobs, from key" \
         "custody through the chart's secrets.provider: the chart's README.md)."
fi
echo ">>> install complete: Midgard $version, from a verified bundle alone"
