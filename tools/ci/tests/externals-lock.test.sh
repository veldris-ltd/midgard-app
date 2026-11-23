#!/usr/bin/env bash
# Tests for the external images: deploy/images/externals.lock agrees with every pin of
# the same image, and deploy/images/externals.sh fetches and checks them as OCI archives
# that keep their pinned index digests. [FR-B-5] [VLD-ANB-MIDGARD-0035]
# [VLD-ANB-MIDGARD-0068]
#
#   bash tools/ci/tests/externals-lock.test.sh
#
# The claims under test: each image in the lock has the digest, and the repository, that
# the chart, compose and kind pin it by, so a pin cannot change in one place only; and a
# changed pin is caught. `check` passes only when every archive is there, names one
# manifest, carries it, and has the pinned digest. `fetch` copies by digest with skopeo
# (--all --preserve-digests) or crane, refuses with neither, and keeps nothing that is
# not as pinned. No network is used: skopeo and crane are stubs.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
externals="$root/deploy/images/externals.sh"
lock="$root/deploy/images/externals.lock"
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

# --- the pins agree -----------------------------------------------------------------
entries() { tr -d '\r' < "$1" | sed 's/#.*//' | awk 'NF'; }
# lock_field USE N: field N of the lock line used as USE.
lock_field() { entries "${LOCK_FILE:-$lock}" | awk -v use="$1" -v n="$2" '$4 == use { print $n }'; }
# repo REF: the reference without its tag.
repo() { local r="$1"; [[ "${r##*/}" == *:* ]] && r="${r%:*}"; echo "$r"; }
yaml() { tr -d '\r' < "$1"; }

# agree VALUES COMPOSE MODEL CLUSTER: every pin in those files is the lock's.
agree() {
    local values="$1" compose="$2" model="$3" cluster="$4" ref digest want got bad=0 nodes
    ref="$(lock_field seedImage 1)" digest="$(lock_field seedImage 2)"
    got="$(yaml "$values" | sed -n '/^seedImage:/,/^[^ ]/{s/^  repository: *//p}')@$(yaml "$values" | sed -n '/^seedImage:/,/^[^ ]/{s/^  digest: *//p}')"
    [ "$got" = "$(repo "$ref")@$digest" ] || { echo "  seedImage is $got, the lock $(repo "$ref")@$digest"; bad=1; }

    ref="$(lock_field modelServing.image 1)" digest="$(lock_field modelServing.image 2)"
    want="$ref@$digest"
    got="$(yaml "$values" | sed -n '/^modelServing:/,/^[^ ]/{s/^  image: *//p}')"
    [ "$got" = "$want" ] || { echo "  modelServing.image is $got, the lock $want"; bad=1; }
    got="$(yaml "$compose" | sed -n '/^  mimir-llamacpp:/,/^  [^ ]/{s/^    image: *//p}')"
    [ "$got" = "$want" ] || { echo "  compose.granite.yaml's llama.cpp is $got, the lock $want"; bad=1; }
    got="$(yaml "$model" | sed -n 's/^LLAMA_PINNED=//p')"
    [ "$got" = "$(repo "$ref")@$digest" ] || { echo "  kind/model.sh pins $got, the lock $(repo "$ref")@$digest"; bad=1; }

    ref="$(lock_field egressBroker.image 1)" digest="$(lock_field egressBroker.image 2)"
    got="$(yaml "$values" | sed -n '/^egressBroker:/,/^[^ ]/{s/^  image: *//p}')"
    [ "$got" = "$ref@$digest" ] || { echo "  egressBroker.image is $got, the lock $ref@$digest"; bad=1; }

    ref="$(lock_field kind 1)" digest="$(lock_field kind 2)"
    nodes="$(yaml "$cluster" | grep -c '^  - role:')"
    got="$(yaml "$cluster" | grep -c "^    image: $ref@$digest\$")"
    [ "$nodes" -gt 0 ] && [ "$got" = "$nodes" ] || { echo "  $got of $nodes kind nodes run $ref@$digest"; bad=1; }
    [ "$bad" = 0 ]
}

files=("$root/deploy/helm/midgard/values.yaml" "$root/deploy/compose/compose.granite.yaml"
       "$root/deploy/kind/model.sh" "$root/deploy/kind/cluster.yaml")
check "the lock has four images, one for each use" \
    test "$(entries "$lock" | awk '{print $4}' | sort -u | tr '\n' ' ')" = "egressBroker.image kind modelServing.image seedImage "
check "every member is a flat .oci.tar file name, used once" \
    test "$(entries "$lock" | awk '$3 ~ /^[A-Za-z0-9][A-Za-z0-9._-]*\.oci\.tar$/ {print $3}' | sort -u | wc -l | tr -d ' ')" = 4
if out="$(agree "${files[@]}")"; then
    pass "the lock agrees with the chart, compose, kind/model.sh and kind/cluster.yaml"
else
    fail "the lock agrees with the chart, compose, kind/model.sh and kind/cluster.yaml:"
    echo "$out"
fi

# A pin changed in one place only is caught, wherever it is.
other=sha256:$(printf 'e%.0s' $(seq 64))
changed() { # changed NAME INDEX SED: change files[INDEX] with SED; agree must refuse
    local copy=("${files[@]}") name="$1" i="$2"
    cp "${files[$i]}" "$work/changed.$i"
    sed -i "$3" "$work/changed.$i"
    copy[$i]="$work/changed.$i"
    if agree "${copy[@]}" >/dev/null; then fail "a pin changed only in $name is caught"
    else pass "a pin changed only in $name is caught"; fi
}
changed "the seed image's digest" 0 "/^seedImage:/,/^[^ ]/s/^  digest: .*/  digest: $other/"
changed "modelServing.image" 0 "/^modelServing:/,/^[^ ]/s/@sha256:.*/@$other/"
changed "egressBroker.image" 0 "/^egressBroker:/,/^[^ ]/s/@sha256:.*/@$other/"
changed "compose.granite.yaml" 1 "s/llama.cpp:server@sha256:.*/llama.cpp:server@$other/"
changed "kind/model.sh" 2 "s/^LLAMA_PINNED=.*/LLAMA_PINNED=ghcr.io\/ggml-org\/llama.cpp@$other/"
changed "one kind node" 3 "0,/@sha256:/s/@sha256:.*/@$other/"

# --- externals.sh check -------------------------------------------------------------
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
    printf '{"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.index.v1+json","digest":"sha256:%s","size":%s}]}' "$h" "$size" > "$d/index.json"
    tar -C "$d" -cf "$1" oci-layout index.json blobs
    echo "sha256:$h"
}

# A lock of two test images, and the archives as a registry would give them.
mkdir -p "$work/registry" "$work/dir"
a="$(oci "$work/registry/a.tar" a)" b="$(oci "$work/registry/b.tar" b)"
for d in "$a" "$b"; do cp "$work/registry/$([ "$d" = "$a" ] && echo a || echo b).tar" "$work/registry/${d#sha256:}.tar"; done
cat > "$work/externals.lock" <<EOF
# test images
example.test/one:v1    $a  one.oci.tar  seedImage
example.test/team/two  $b  two.oci.tar  kind
EOF
ext() { EXTERNALS_LOCK="$work/externals.lock" bash "$externals" "$@" 2>&1; }

cp "$work/registry/a.tar" "$work/dir/one.oci.tar"
if out="$(ext check "$work/dir")"; then fail "check refuses a missing archive"
else grep -q 'two.oci.tar is missing' <<<"$out" && pass "check refuses a missing archive, naming it" ||
    fail "check refuses a missing archive, naming it: $out"; fi
cp "$work/registry/a.tar" "$work/dir/two.oci.tar"
if out="$(ext check "$work/dir")"; then fail "check refuses an archive of another digest"
else grep -q "two.oci.tar is $a, not the pinned $b" <<<"$out" && pass "check refuses an archive of another digest" ||
    fail "check refuses an archive of another digest: $out"; fi
cp "$work/registry/b.tar" "$work/dir/two.oci.tar"
check "check passes when every archive is there with its pinned digest" ext check "$work/dir"
check "digest prints an archive's index digest" test "$(ext digest "$work/dir/two.oci.tar")" = "$b"

# An index.json that names the pin over a blob that is not it: the digest is of content.
t="$(mktemp -d "$work/t.XXXXXX")"
tar -C "$t" -xf "$work/dir/two.oci.tar"
echo '{"tampered":true}' > "$t/blobs/sha256/${b#sha256:}"
tar -C "$t" -cf "$work/tampered.tar" oci-layout index.json blobs
if out="$(ext digest "$work/tampered.tar")"; then fail "an index that names a blob it does not carry is refused"
else grep -q 'does not have the digest its index names' <<<"$out" && pass "an index that names a blob it does not carry is refused" ||
    fail "an index that names a blob it does not carry is refused: $out"; fi
t="$(mktemp -d "$work/t.XXXXXX")"
tar -C "$t" -xf "$work/dir/two.oci.tar"
sed -i "s/\"manifests\":\[/&{\"digest\":\"$a\"},/" "$t/index.json"
tar -C "$t" -cf "$work/two-manifests.tar" oci-layout index.json blobs
if ext digest "$work/two-manifests.tar" >/dev/null; then fail "an index naming two manifests is refused"
else pass "an index naming two manifests is refused"; fi
echo "not a tar" > "$work/plain.tar"
if ext digest "$work/plain.tar" >/dev/null; then fail "a file that is not an OCI archive is refused"
else pass "a file that is not an OCI archive is refused"; fi

# --- externals.sh fetch, with stub tools and no network -------------------------------
mkdir -p "$work/stubs"
# Neither tool: SKOPEO and CRANE name binaries that do not exist.
if out="$(SKOPEO="$work/absent/skopeo" CRANE="$work/absent/crane" ext fetch "$work/fetched")"; then
    fail "fetch refuses with neither skopeo nor crane"
else
    grep -q 'needs skopeo or crane' <<<"$out" && [ ! -e "$work/fetched" ] &&
        pass "fetch refuses with neither skopeo nor crane, naming both, and writes nothing" ||
        fail "fetch refuses with neither skopeo nor crane, naming both, and writes nothing: $out"
fi
# skopeo: copy SRC DST from the test registry, by digest, recording its arguments.
cat > "$work/stubs/skopeo" <<EOF
#!/usr/bin/env bash
echo "skopeo \$*" >> "$work/calls"
src="\${@: -2:1}" dst="\${@: -1}"
cp "$work/registry/\${src##*sha256:}.tar" "\${dst#oci-archive:}"
EOF
chmod +x "$work/stubs/skopeo"
rm -f "$work/calls"
if out="$(PATH="$work/stubs:$PATH" ext fetch "$work/fetched")"; then
    pass "fetch with skopeo writes every archive, as pinned"
else
    fail "fetch with skopeo writes every archive, as pinned: $out"
fi
check "skopeo copies every platform and keeps the digest" \
    test "$(grep -c -- 'copy --all --preserve-digests docker://' "$work/calls")" = 2
check "skopeo copies by digest, never by tag" \
    grep -q "docker://example.test/one@$a oci-archive:" "$work/calls"
check "an archive already fetched and as pinned is not fetched again" \
    bash -c "PATH='$work/stubs:$PATH' EXTERNALS_LOCK='$work/externals.lock' bash '$externals' fetch '$work/fetched' >/dev/null 2>&1 && test \$(grep -c . '$work/calls') = 2"
# crane: pull to an OCI layout directory, which fetch then archives.
cat > "$work/stubs/crane" <<EOF
#!/usr/bin/env bash
echo "crane \$*" >> "$work/calls"
src="\${@: -2:1}" dst="\${@: -1}"
mkdir -p "\$dst" && tar -C "\$dst" -xf "$work/registry/\${src##*sha256:}.tar"
EOF
chmod +x "$work/stubs/crane"
rm -f "$work/calls"
if out="$(SKOPEO="$work/absent/skopeo" CRANE="$work/stubs/crane" ext fetch "$work/craned")"; then
    pass "fetch with crane, and no skopeo, writes every archive, as pinned"
else
    fail "fetch with crane, and no skopeo, writes every archive, as pinned: $out"
fi
check "crane pulls by digest, as OCI" grep -q "pull --format=oci example.test/team/two@$b" "$work/calls"
# A registry that serves something else for the pin: nothing is kept.
cp "$work/registry/a.tar" "$work/registry/${b#sha256:}.tar"
rm -f "$work/calls"
if out="$(PATH="$work/stubs:$PATH" ext fetch "$work/wrong")"; then
    fail "fetch keeps nothing that is not as pinned"
else
    [ ! -e "$work/wrong/two.oci.tar" ] && [ -z "$(ls -A "$work/wrong" | grep -v '^one.oci.tar$')" ] &&
        pass "fetch keeps nothing that is not as pinned" || fail "fetch keeps nothing that is not as pinned: $(ls -A "$work/wrong")"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "externals-lock: all checks passed"
else
    echo "externals-lock: $failures check(s) failed"
    exit 1
fi
