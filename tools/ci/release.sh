#!/usr/bin/env bash
# The release, end to end: gate every artefact the workspace ships, admit each to the
# Bifrost registry, and prove the admitted set is exactly the gated set.
# [RC-07] [AC-B-1] [NFR-PQ-3] [AC-RF-SC-5a] [AC-RF-SC-5b] [AC-RF-SC-5c] [AC-RF-SC-5d]
# [AC-RF-SC-5e]
#
#   bash tools/ci/release.sh                       the release, on a provisioned signing runner
#   bash tools/ci/release.sh --dry-run             the whole gate, publishing nothing
#   bash tools/ci/release.sh --dry-run --only a,b  the same for some artefacts (dry run only)
#   ... --out DIR                                  where the bundles land (default target/release-out)
#
# What it does, in order, failing closed at the first refusal:
#   1. derives the release manifest from the workspace: every Rust binary
#      (tools/ci/release-artefacts.sh) and every JavaScript artefact — UI bundles and
#      npm packages (tools/ci/js-artefacts.sh);
#   2. gates each into one fresh directory. The stages are the same for every kind:
#      a reproducible build (two builds, bit-identical), an SBOM, the advisory and
#      licence scan, an ML-DSA signature, and verification;
#   3. admits each artefact to the Bifrost registry (`mg-register`), which re-verifies
#      the release bundle and records the artefact, its signature, its provenance and
#      its reproducibility report;
#   4. compares what the registry holds for this version with the manifest. An artefact
#      that was gated and is not there, or is there and was not gated, fails the release.
#
# A real release reads, and has no default for:
#   MIDGARD_SIGNING_KEYSTORE   the release signing keystore (ML-DSA-87)
#   MIDGARD_BIFROST_KEYSTORE   the Bifrost admission keystore (ML-DSA-65)
#   MIDGARD_BIFROST_REGISTRY   the Bifrost artefact registry directory
# and the pinned anchors deploy/trust/release-trust.pub and deploy/trust/bifrost-trust.pub
# (MIDGARD_TRUST_ANCHORS and MIDGARD_BIFROST_ANCHOR name others).
#
# JavaScript artefacts are built by tools/ci/js-artefacts.sh. Where pnpm is on PATH this
# script builds them itself; where it is not — inside the Rust toolchain image — it
# takes them from MIDGARD_JS_OUT (default target/js-out), where the `js-build` job left
# them, and refuses any that were built from a different source revision.
#
# A dry run generates throwaway keys and a throwaway registry outside the checkout,
# runs every stage above against them, and removes them. Nothing it signs is trusted
# by anything, and nothing is published. It is the release-affecting change's proof
# that the gate still works.
set -euo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib.sh

dry_run=0
only=""
out=target/release-out
while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run) dry_run=1; shift ;;
        --only) only="${2:?--only needs a comma-separated list of artefacts}"; shift 2 ;;
        --out) out="${2:?--out needs a directory}"; shift 2 ;;
        *) echo "usage: $0 [--dry-run] [--only name[,name...]] [--out DIR]" >&2; exit 2 ;;
    esac
done
if [ -n "$only" ] && [ "$dry_run" -ne 1 ]; then
    echo "release: --only is for dry runs; a release gates every artefact or none" >&2
    exit 2
fi

fail() {
    ci_error "release: $*"
    exit 1
}

# --- 1. the manifest ---------------------------------------------------------------
version="$(bash tools/ci/release-artefacts.sh --version)"
[ -n "$version" ] || fail "the workspace declares no version"
# package binary component image
manifest="$(bash tools/ci/release-artefacts.sh --rows)"
# kind id dir file package
js_manifest="$(bash tools/ci/js-artefacts.sh --rows)"
if [ -n "$only" ]; then
    wanted=" ${only//,/ } "
    # Collected first: `grep -q` in a pipeline exits at the first match, and under
    # pipefail the writer it cut off would then fail the whole test.
    shipped="$(awk 'NF {print $2}' <<<"$manifest"; awk 'NF {print $2}' <<<"$js_manifest")"
    for name in ${only//,/ }; do
        grep -qx "$name" <<<"$shipped" || fail "--only names $name, which the workspace does not ship"
    done
    manifest="$(awk -v wanted="$wanted" 'index(wanted, " " $2 " ")' <<<"$manifest")"
    js_manifest="$(awk -v wanted="$wanted" 'index(wanted, " " $2 " ")' <<<"$js_manifest")"
fi
lines() { awk 'NF' <<<"$1" | wc -l | tr -d ' '; }
count=$(( $(lines "$manifest") + $(lines "$js_manifest") ))
[ "$count" -gt 0 ] || fail "the release manifest is empty — nothing would be gated"
clash="$({ awk 'NF {print $2}' <<<"$manifest"; awk 'NF {print $2}' <<<"$js_manifest"; } | sort | uniq -d)"
[ -z "$clash" ] || fail "one registry id for two artefacts: $(echo $clash)"

# --- keys and registry -------------------------------------------------------------
scratch=""
cleanup() { [ -z "$scratch" ] || rm -rf "$scratch"; }
trap cleanup EXIT

if [ "$dry_run" -eq 1 ]; then
    # Outside the checkout, like real key material, and gone when this exits.
    scratch="$(mktemp -d)"
    echo "release: DRY RUN — throwaway keys and registry in $scratch; nothing is published"
    cargo run -q --release -p mg-sign -- keygen --key-id dry-run-release \
        --keystore "$scratch/release.keystore" --export-verifying-key "$scratch/release.pub"
    cargo run -q --release -p mg-sign -- keygen --key-id dry-run-admission --alg ML-DSA-65 \
        --keystore "$scratch/admission.keystore" --export-verifying-key "$scratch/admission.pub"
    export MIDGARD_SIGNING_KEYSTORE="$scratch/release.keystore"
    export MIDGARD_TRUST_ANCHORS="$scratch/release.pub"
    admission_keystore="$scratch/admission.keystore"
    admission_anchor="$scratch/admission.pub"
    registry="$scratch/registry"
else
    : "${MIDGARD_SIGNING_KEYSTORE:?the release signing keystore is not configured (docs/ops/PKI.md)}"
    admission_keystore="${MIDGARD_BIFROST_KEYSTORE:?the Bifrost admission keystore is not configured (docs/ops/PKI.md)}"
    registry="${MIDGARD_BIFROST_REGISTRY:?the Bifrost registry directory is not configured (docs/ops/PKI.md)}"
    export MIDGARD_TRUST_ANCHORS="${MIDGARD_TRUST_ANCHORS:-deploy/trust/release-trust.pub}"
    admission_anchor="${MIDGARD_BIFROST_ANCHOR:-deploy/trust/bifrost-trust.pub}"
    for anchor in "$MIDGARD_TRUST_ANCHORS" "$admission_anchor"; do
        [ -s "$anchor" ] || fail "$anchor is not committed; the key ceremony publishes it (docs/ops/PKI.md)"
    done
fi

# --- 2. gate everything into one fresh directory ------------------------------------
# Fresh, so nothing from an earlier run — an older binary with an older, still valid
# signature — can be picked up by a later stage as if this run had produced it.
rm -rf "$out"
mkdir -p "$out"
export MIDGARD_GATE_OUT="$out"
# id version component file — every artefact this run gates, of either kind.
: > "$out/artefacts.txt"

plural() { [ "$1" -eq 1 ] && echo "$2" || echo "$3"; }
echo "release: $version — $count $(plural "$count" artefact artefacts) to gate"

# 2a. Rust binaries: `just gate` reproduces, SBOMs, scans, signs and verifies.
while read -r package bin component image; do
    [ -n "$package" ] || continue
    ci_group "gate $bin ($package)"
    # stdin is the manifest this loop is reading; nothing inside may consume it.
    just gate "$package" "$bin" </dev/null || fail "$bin did not pass the gate"
    ci_endgroup
    if [ -f "$out/$bin.exe" ]; then file="$bin.exe"; else file="$bin"; fi
    echo "$bin $version $component $file" >> "$out/artefacts.txt"
done <<<"$manifest"

# 2b. JavaScript artefacts: the same five stages. The reproducible build is
#     tools/ci/js-artefacts.sh (two builds, compared); the other four are the same
#     tools the binaries go through.
if [ -n "$js_manifest" ]; then
    js_out="${MIDGARD_JS_OUT:-target/js-out}"
    revision="$(bash tools/ci/js-artefacts.sh --commit)"
    if command -v pnpm >/dev/null 2>&1; then
        ci_group "build $(lines "$js_manifest") JavaScript $(plural "$(lines "$js_manifest")" artefact artefacts), twice each"
        # shellcheck disable=SC2046  # the ids are single words
        bash tools/ci/js-artefacts.sh build $(awk '{print $2}' <<<"$js_manifest") --out "$js_out" </dev/null \
            || fail "a JavaScript artefact did not build reproducibly"
        ci_endgroup
    elif [ ! -f "$js_out/.commit" ]; then
        fail "pnpm is not available here and $js_out holds no prebuilt JavaScript artefacts; run the js-build job first"
    elif [ "$(cat "$js_out/.commit")" != "$revision" ]; then
        fail "the JavaScript artefacts in $js_out were built from $(cat "$js_out/.commit"), not from $revision"
    fi

    while read -r kind id dir file name; do
        [ -n "$kind" ] || continue
        ci_group "gate $id ($kind)"
        for built in "$file" "$id.repro.json"; do
            [ -s "$js_out/$built" ] || fail "$id was not built: $js_out/$built is missing"
            cp "$js_out/$built" "$out/$built"
        done
        cargo run -q --release -p mg-sbom -- --pnpm-lock pnpm-lock.yaml --importer "$dir" \
            --out "$out/$file.sbom.cdx.json" "$out/$file" </dev/null || fail "$id: no SBOM"
        cargo run -q --release -p mg-scan -- "$out/$file.sbom.cdx.json" \
            --out "$out/$id.scan-findings.json" </dev/null || fail "$id did not pass the scan"
        MIDGARD_REPRO_REPORT="$out/$id.repro.json" just sign "$out/$file" </dev/null \
            || fail "$id could not be signed"
        just verify "$out/$file" </dev/null || fail "$id did not verify"
        ci_endgroup
        echo "gate passed: $name $id ($out/$file)"
        echo "$id $version $kind $file" >> "$out/artefacts.txt"
    done <<<"$js_manifest"
fi

# --- 3. admit each to the Bifrost registry -------------------------------------------
: > "$out/registered.txt"
while read -r id _ component file; do
    [ -n "$id" ] || continue
    cargo run -q --release -p mg-register -- register \
        --registry "$registry" --artefact "$out/$file" \
        --id "$id" --version "$version" --component "$component" \
        --repro "$out/$id.repro.json" \
        --keystore "$admission_keystore" \
        --release-anchor "$MIDGARD_TRUST_ANCHORS" --admission-anchor "$admission_anchor" </dev/null \
        | tee -a "$out/registered.txt" || fail "$id was gated but the registry refused it"
done < "$out/artefacts.txt"

# --- 4. the published set must equal the gated set -----------------------------------
# Asked of the registry itself, which re-verifies everything it holds on the way.
listing="$(cargo run -q --release -p mg-register -- list \
    --registry "$registry" --admission-anchor "$admission_anchor")" \
    || fail "the registry could not be read back"

gated="$(awk '{print $1}' "$out/artefacts.txt" | sort -u)"
published="$(awk -v v="$version" '$2 == v && $3 != "attestation" {print $1}' <<<"$listing" | sort -u)"
attested="$(awk -v v="$version" '$2 == v && $3 == "attestation" {print $1}' <<<"$listing" | sort -u)"

problems=0
dropped="$(comm -23 <(echo "$gated") <(echo "$published"))"
ungated="$(comm -13 <(echo "$gated") <(echo "$published"))"
if [ -n "$dropped" ]; then
    ci_error "release: gated but NOT in the registry for $version: $(echo $dropped)"
    problems=1
fi
if [ -n "$ungated" ]; then
    ci_error "release: in the registry for $version but NOT gated by this release: $(echo $ungated)"
    problems=1
fi
# Every published artefact carries its release signature, provenance and repro report.
for id in $gated; do
    for part in sig att repro.json; do
        grep -qx "$id.$part" <<<"$attested" || {
            ci_error "release: $id is in the registry without its $part"
            problems=1
        }
    done
done
[ "$problems" -eq 0 ] || fail "the published set and the gated set differ"

{
    echo "# Midgard $version — gated and admitted. id version component sha256"
    awk -v v="$version" '$2 == v && $3 != "attestation"' <<<"$listing"
} > "$out/release-manifest.txt"

ci_output release_version "$version" >/dev/null
ci_output release_out "$out" >/dev/null
echo "release: $count $(plural "$count" artefact artefacts) gated, admitted, and accounted for"
echo "release: bundles in $out; the manifest is $out/release-manifest.txt"
if [ "$dry_run" -eq 1 ]; then
    echo "release: DRY RUN complete — the throwaway keys and registry are removed; nothing was published"
fi
