#!/usr/bin/env bash
# Install a signed offline release bundle on a disconnected site: verify it fully
# offline, build the verify-before-run images from what it carries, and deploy them
# with the Helm chart. Nothing is built or deployed unless every signature verifies.
# [FR-B-5] [AC-B-3] [RC-19] [RC-18]
#
#   bash deploy/air-gap-bundle/install.sh BUNDLE --bundle-trust FILE
#       [--release-trust FILE]        default: deploy/trust/release-trust.pub
#       [--registry REGISTRY --push]  push the images to the enclosure's registry
#       [--kind CLUSTER]              or load them into a local kind cluster (a rehearsal)
#       [--context KUBE_CONTEXT] [-f VALUES]...
#
#   1. `mg-bundle import --out-dir` verifies the bundle signature, its manifest and every
#      artefact against the Bifrost admission anchor (--bundle-trust, ML-DSA-65), with
#      no network, and only then writes the artefacts out.
#   2. deploy/images/build.sh builds each image from them and runs mg-verify inside it
#      against the release anchor (--release-trust, ML-DSA-87): the check every
#      container repeats before it starts its binary.
#   3. The images go to the registry or the kind cluster; the release anchor becomes the
#      chart's trust ConfigMap; `helm upgrade --install` deploys the version the bundle
#      carries, which must be the chart's appVersion.
#
# The enclosure's secrets — identities, seeds, forge-jobs — come from key custody, not
# from a bundle; the services do not start until they exist.
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
# kubectl may be a Windows binary under Git Bash: hand it native paths.
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else echo "$1"; fi; }
usage() {
    sed -n '8,13p' "$0" | sed 's/^#//' >&2
    exit 2
}
[ "$#" -ge 1 ] || usage
BUNDLE="$1"; shift
bundle_trust="" release_trust=deploy/trust/release-trust.pub
registry=registry.enclosure.local/midgard push=0 kind="" context=""
values=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bundle-trust) bundle_trust="$2"; shift 2 ;;
        --release-trust) release_trust="$2"; shift 2 ;;
        --registry) registry="$2"; shift 2 ;;
        --push) push=1; shift ;;
        --kind) kind="$2"; shift 2 ;;
        --context) context="$2"; shift 2 ;;
        -f) values+=(-f "$2"); shift 2 ;;
        *) echo "install: unknown option $1" >&2; usage ;;
    esac
done
[ -n "$bundle_trust" ] || { echo "install: --bundle-trust is required" >&2; usage; }
if [ "$push" = 1 ] && [ -n "$kind" ]; then echo "install: choose --push or --kind" >&2; exit 2; fi
if [ "$push" = 0 ] && [ -z "$kind" ]; then echo "install: choose --push (a registry) or --kind (a rehearsal)" >&2; exit 2; fi

MG_BUNDLE="${MG_BUNDLE:-mg-bundle}"
command -v "$MG_BUNDLE" >/dev/null 2>&1 || MG_BUNDLE="cargo run --release -q -p mg-bundle --"
HELM="${HELM:-helm}"
KIND="${KIND:-kind}"
KUBECTL="${KUBECTL:-kubectl}"
if [ -n "$context" ]; then
    KUBECTL="$KUBECTL --context $context"
    HELM_CONTEXT=(--kube-context "$context")
elif [ -n "$kind" ]; then
    KUBECTL="$KUBECTL --context kind-$kind"
    HELM_CONTEXT=(--kube-context "kind-$kind")
else
    HELM_CONTEXT=()
fi
NS="$(sed -n 's/^namespace: *//p' deploy/helm/midgard/values.yaml | head -1)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. Verify offline, and only then extract.
echo ">>> verifying $BUNDLE offline against $bundle_trust ..."
refs="$($MG_BUNDLE import --trust-anchor "$bundle_trust" --out-dir "$work/verified" "$BUNDLE")"
versions="$(sed -n 's/^[^@]*@//p' <<<"$refs" | sort -u)"
[ "$(wc -l <<<"$versions")" -eq 1 ] || { echo "install: the bundle carries several versions: $versions" >&2; exit 1; }
version="$versions"
chart_version="$(sed -n 's/^appVersion: *"\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' deploy/helm/midgard/Chart.yaml)"
[ "$chart_version" = "$version" ] || {
    echo "install: the bundle is $version but this chart deploys $chart_version" >&2
    exit 1
}
echo ">>> verified $(wc -l <<<"$refs") artefact(s) of $version"

# 2. Build the images, each verified inside itself.
echo ">>> building verify-before-run images"
bash deploy/images/build.sh --version "$version" --verified "$work/verified" \
    --trust "$release_trust" --registry "$registry" --out "$work/images"

# 3. Make them available, and deploy.
echo ">>> making the images available"
tags="$(awk '{print $1}' "$work/images/images.lock")"
if [ "$push" = 1 ]; then
    for tag in $tags; do docker push "$tag"; done
else
    # One archive for every image, so the layers they share are loaded once.
    # shellcheck disable=SC2086
    $KIND load docker-image $tags --name "$kind"
fi

echo ">>> deploying $version"
$HELM upgrade --install midgard deploy/helm/midgard "${HELM_CONTEXT[@]}" --namespace default \
    --set image.registry="$registry" --set image.tag="$version" \
    --set image.pullPolicy=IfNotPresent "${values[@]}"
$KUBECTL -n "$NS" create configmap midgard-release-trust \
    --from-file=release-trust.pub="$(native "$release_trust")" --dry-run=client -o yaml \
    | $KUBECTL -n "$NS" apply -f - >/dev/null
if ! $KUBECTL -n "$NS" get secret http-gateway-identity >/dev/null 2>&1; then
    echo ">>> NOTE: the enclosure's secrets are not provisioned in $NS; the services wait" \
         "until they are (identities, seeds and forge-jobs, from key custody)."
fi
echo ">>> install complete: Midgard $version, from a verified bundle"
