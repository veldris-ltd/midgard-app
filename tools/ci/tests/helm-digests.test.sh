#!/usr/bin/env bash
# Tests for image pinning by digest: the Helm chart's image rule, the committed
# development rendering, deploy/helm/set-digests.sh, and the acceptance probes and
# their runner. [VLD-ANB-MIDGARD-0012] [AC-37.1] [AC-37.2] [AC-37.3] [AC-37.4] [AC-37.5]
#
#   bash tools/ci/tests/helm-digests.test.sh
#
# The chart cases need helm and are skipped without it; the rest run anywhere.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
chart="$root/deploy/helm/midgard"
values="$chart/values.yaml"
set_digests="$root/deploy/helm/set-digests.sh"
runner="$root/deploy/acceptance/deployment/run-deployment-tier.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}
# check NAME COMMAND...: pass when COMMAND succeeds.
check() {
    local name="$1"
    shift
    if "$@"; then pass "$name"; else fail "$name"; fi
}
# count PATTERN FILE: the number of lines of FILE matching the extended PATTERN.
count() { grep -cE "$1" "$2" || true; }
hex() { printf "$1%.0s" $(seq 64); }

services="$(tr -d '\r' < "$values" | sed -n '/^  digests:/,/^  [^ ]/s/^    \([a-z0-9-]*\):.*/\1/p')"
# One container per service, and a seed init container for each service that seeds.
containers=$(( $(tr -d '\r' < "$values" | grep -c '^  - name:') + $(tr -d '\r' < "$values" | grep -c '^    seed: true') ))
check "the chart names a digest for each of the five services" \
    test "$(echo $services)" = "http-gateway mimir-server bifrost-fleet heimdall-feed forge-worker"

# --- the chart's image rule -----------------------------------------------------------
HELM="${HELM:-helm}"
if ! command -v "$HELM" >/dev/null 2>&1; then
    echo "skip  the chart cases (helm is not on PATH)"
else
    # Every case renders with secrets.acknowledgeEtcdEncryption=true, as deploy/k8s/render.sh
    # does: since WP-29 the chart refuses kubernetes Secrets without it, and these cases are
    # about image references, not secrets custody.
    render() {
        "$HELM" template midgard "$chart" --namespace default \
            --set secrets.acknowledgeEtcdEncryption=true "$@" 2>&1 | tr -d '\r'
    }

    # AC-37.1: the defaults are a release chart, which does not render without digests.
    if render > "$work/default.yaml"; then
        fail "the default values do not render"
    else
        pass "the default values do not render"
    fi
    check "the failure names the first empty digest" grep -q 'image.digests.http-gateway is empty' "$work/default.yaml"

    # AC-37.3: allowTag renders tags, pulled every time, and says the pods are unpinned.
    if render --set image.allowTag=true > "$work/tag.yaml"; then
        pass "image.allowTag=true renders"
    else
        fail "image.allowTag=true renders: $(head -3 "$work/tag.yaml")"
    fi
    check "allowTag: no image is pinned" test "$(count '@sha256:' "$work/tag.yaml")" = 0
    check "allowTag: every image ($containers) is pulled Always" \
        test "$(count '^ *imagePullPolicy: Always$' "$work/tag.yaml")" = "$containers"
    check "allowTag: no image is pulled IfNotPresent" test "$(count 'IfNotPresent' "$work/tag.yaml")" = 0
    check "allowTag: every pod is annotated unpinned" \
        test "$(count 'midgard.veldris.io/image-pinned: "false"' "$work/tag.yaml")" = 5
    check "the committed deploy/k8s/enclosure.yaml is the allowTag rendering" \
        bash "$root/deploy/k8s/render.sh" --check

    # AC-37.2: five digests pin every container and init container.
    a="sha256:$(hex a)"
    pins=()
    for s in $services; do pins+=(--set "image.digests.$s=$a"); done
    if render "${pins[@]}" > "$work/pinned.yaml"; then
        pass "five digests render"
    else
        fail "five digests render: $(head -3 "$work/pinned.yaml")"
    fi
    check "pinned: every image ($containers) is registry/name@digest" \
        test "$(count "^ *image: registry.enclosure.local/midgard/[a-z-]+@$a\$" "$work/pinned.yaml")" = "$containers"
    check "pinned: no image line carries a tag" \
        test "$(grep -E '^ *image:' "$work/pinned.yaml" | grep -vc '@sha256:')" = 0
    check "pinned: no image line names the chart's tag" test "$(count '^ *image: .*:0\.1\.0-rc\.1' "$work/pinned.yaml")" = 0
    check "pinned: every image is pulled IfNotPresent" \
        test "$(count '^ *imagePullPolicy: IfNotPresent$' "$work/pinned.yaml")" = "$containers"
    check "pinned: every pod is annotated pinned" \
        test "$(count 'midgard.veldris.io/image-pinned: "true"' "$work/pinned.yaml")" = 5

    # A digest that is not sha256:<64 hex> is refused, not rendered.
    if render --set image.allowTag=true --set image.digests.http-gateway=sha256:abc > "$work/bad.yaml"; then
        fail "a malformed digest is refused"
    else
        pass "a malformed digest is refused"
    fi
    check "the refusal names the digest" grep -q 'image.digests.http-gateway is "sha256:abc"' "$work/bad.yaml"

    # image.single (kind) is used as given.
    render --set image.single=midgard/dev:local --set image.pullPolicy=Never > "$work/single.yaml" \
        && check "image.single is used as given" \
            test "$(count '^ *image: midgard/dev:local$' "$work/single.yaml")" = "$containers" \
        || fail "image.single renders"
fi

# --- set-digests.sh: the release values from a signed images.lock ---------------------
lock="$work/images.lock"
digit=0
for s in $services; do
    echo "registry.enclosure.local/midgard/$s:0.1.0 sha256:$(hex $digit)"
    digit=$((digit + 1))
done | sort > "$lock"
cp "$values" "$work/values.yaml"
before="$(cksum < "$work/values.yaml")"
if bash "$set_digests" "$lock" "$work/values.yaml" --out "$work/release/values.yaml" >/dev/null; then
    pass "set-digests writes the release values"
else
    fail "set-digests writes the release values"
fi
check "set-digests: the source values are unchanged" test "$(cksum < "$work/values.yaml")" = "$before"
check "set-digests: five digests are set" \
    test "$(count '^    [a-z-]+: sha256:[0-9a-f]{64}$' "$work/release/values.yaml")" = 5
check "set-digests: each service has its own digest" \
    grep -q "^    forge-worker: sha256:$(hex 4)\$" "$work/release/values.yaml"
check "set-digests: nothing else changes" \
    test "$(diff <(tr -d '\r' < "$values") "$work/release/values.yaml" | grep -c '^[<>]')" = 10

refused() { # refused NAME LOCK: set-digests must refuse LOCK and write nothing
    rm -f "$work/refused.yaml"
    if bash "$set_digests" "$2" "$work/values.yaml" --out "$work/refused.yaml" 2>/dev/null; then
        fail "set-digests refuses $1"
    elif [ -e "$work/refused.yaml" ]; then
        fail "set-digests refuses $1 (but wrote its output)"
    else
        pass "set-digests refuses $1"
    fi
}
grep -v forge-worker "$lock" > "$work/missing.lock"
refused "a lock missing forge-worker" "$work/missing.lock"
sed '1s/sha256:.*/sha256:abc/' "$lock" > "$work/short.lock"
refused "a digest that is not 64 hex digits" "$work/short.lock"
sed '1s/sha256:/md5:/' "$lock" > "$work/algorithm.lock"
refused "a digest that is not sha256" "$work/algorithm.lock"
{ cat "$lock"; head -1 "$lock"; } > "$work/twice.lock"
refused "an image named twice" "$work/twice.lock"
{ cat "$lock"; echo "registry.enclosure.local/midgard/stranger:0.1.0 sha256:$(hex e)"; } > "$work/extra.lock"
refused "an image the chart does not know" "$work/extra.lock"
if bash "$set_digests" "$lock" "$work/values.yaml" >/dev/null \
    && [ "$(count '^    [a-z-]+: sha256:' "$work/values.yaml")" = 5 ]; then
    pass "set-digests without --out rewrites the values file"
else
    fail "set-digests without --out rewrites the values file"
fi

# --- the acceptance probes and their runner (AC-37.5) ---------------------------------
probes="$root/deploy/acceptance/deployment"
check "the two probe manifests are there" test "$(ls "$probes"/*.yaml | wc -l | tr -d " ")" = 2
check "no probe manifest names an image by tag" \
    test -z "$(grep -hE '^ *image: [^@]*:[0-9]+\.[0-9]+\.[0-9]+' "$probes"/*.yaml)"
check "every probe image is the all-zero sentinel digest" \
    test "$(grep -hE '^ *image:' "$probes"/*.yaml | grep -vc '@sha256:0\{64\}$')" = 0

# The runner's functions only, with a kubectl that records any call and no docker.
mkdir -p "$work/bin"
printf '#!/bin/sh\necho "$*" >> "%s/kubectl.calls"\n' "$work" > "$work/bin/kubectl"
chmod +x "$work/bin/kubectl"
tier() { # tier IMAGE PROBE MANIFEST [DOCKER]: run probe_manifest; prints the rendered file
    PATH="$work/bin:$PATH" MIDGARD_DEPLOYMENT_TIER_FUNCTIONS_ONLY=1 MIDGARD_PROBE_IMAGE="$1" \
        DOCKER="${4:-$work/no-docker}" bash -c '. "$0" && probe_manifest "$1" "$2"' "$runner" "$3" "$2"
}
if out="$(tier registry.enclosure.local/midgard/pqmtls-probe:1.2.3 pqmtls-probe "$probes/pqmtls-probe.yaml" 2>"$work/err")"; then
    fail "a tag-only probe image without docker is refused"
else
    pass "a tag-only probe image without docker is refused"
fi
check "the refusal says the image has no digest" grep -q 'has no @sha256: digest' "$work/err"
check "nothing is applied: kubectl is never called" test ! -e "$work/kubectl.calls"

pin="registry.example/midgard/dev@sha256:$(hex b)"
if out="$(tier "$pin" pqmtls-probe "$probes/pqmtls-probe.yaml" 2>/dev/null)"; then
    check "a pinned probe image replaces both sentinels" test "$(grep -c "image: $pin\$" "$out")" = 2
    check "the rendered manifest keeps no sentinel" test "$(count '@sha256:0{64}' "$out")" = 0
else
    fail "a pinned probe image is accepted"
fi

printf '#!/bin/sh\necho "registry.example/midgard/dev@sha256:%s"\n' "$(hex c)" > "$work/bin/docker"
chmod +x "$work/bin/docker"
if out="$(tier midgard/dev:local mimir-probe "$probes/inference-probe.yaml" "$work/bin/docker" 2>/dev/null)"; then
    check "a tag is resolved to its digest with docker" grep -q "image: registry.example/midgard/dev@sha256:$(hex c)\$" "$out"
else
    fail "a tag docker resolves is accepted"
fi
printf '#!/bin/sh\necho "<no value>"\n' > "$work/bin/docker"
if tier midgard/dev:local mimir-probe "$probes/inference-probe.yaml" "$work/bin/docker" >/dev/null 2>&1; then
    fail "a tag docker cannot resolve to a digest is refused"
else
    pass "a tag docker cannot resolve to a digest is refused"
fi
# The sentinel of another probe is left in place, and so refused.
if tier "$pin" mimir-probe "$probes/pqmtls-probe.yaml" >/dev/null 2>&1; then
    fail "a manifest still holding a sentinel is refused"
else
    pass "a manifest still holding a sentinel is refused"
fi

if [ "$failures" -gt 0 ]; then
    echo "helm-digests tests: $failures failure(s)"
    exit 1
fi
echo "helm-digests tests: all passed"
