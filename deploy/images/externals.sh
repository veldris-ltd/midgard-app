#!/usr/bin/env bash
# The images Midgard runs but does not build, as OCI archives that keep their pinned
# index digests, for the air-gap bundle. [VLD-ANB-MIDGARD-0035] [VLD-ANB-MIDGARD-0068]
#
#   bash deploy/images/externals.sh fetch DIR      connected: copy each image into DIR
#   bash deploy/images/externals.sh check DIR      offline: every archive is there, pinned
#   bash deploy/images/externals.sh digest ARCHIVE offline: an archive's index digest
#
# deploy/images/externals.lock lists them, one per line: the reference without its
# digest, the index digest, the bundle member (the archive's file name in DIR and in the
# bundle) and where the image is used. EXTERNALS_LOCK names another list, and SKOPEO
# and CRANE other binaries.
#
# `fetch` is the one step of a release that needs a network. It is done once, while
# provisioning the release machine, as pulling the toolchain bases is
# (docs/ops/PKI.md). It copies every platform of each image by its digest, never by a
# tag, with skopeo (`copy --all --preserve-digests`, which refuses rather than convert a
# manifest) or, without skopeo, crane (`pull --format=oci`). `docker save` is not used:
# it writes a new manifest, and the digest would no longer be the pin. Each archive is
# checked as `check` does before it is moved into place, and an archive already there
# is checked, not fetched again.
#
# `check` and `digest` read an archive's index.json, which must name exactly one
# manifest, and require the blob it names to be in the archive with that digest: the
# digest is of content the archive carries, not only a line in its index. They need
# only tar and sha256sum, and run on the far side of the gap as well (install.sh).
set -euo pipefail
export MSYS_NO_PATHCONV=1

here="$(cd "$(dirname "$0")" && pwd)"
LOCK="${EXTERNALS_LOCK:-$here/externals.lock}"
SKOPEO="${SKOPEO:-skopeo}"
CRANE="${CRANE:-crane}"
usage() {
    sed -n '5,7p' "$0" | sed 's/^# *//' >&2
    exit 2
}

# The lock's entries, comments and blank lines dropped: "reference digest member use".
entries() {
    [ -f "$LOCK" ] || { echo "externals: no lock file $LOCK" >&2; return 1; }
    tr -d '\r' < "$LOCK" | sed 's/#.*//' | awk 'NF' | awk '
        NF != 4 || $2 !~ /^sha256:[0-9a-f]+$/ || length($2) != 71 || $3 !~ /^[A-Za-z0-9][A-Za-z0-9._-]*\.oci\.tar$/ {
            print "externals: malformed line (want: reference sha256:<64 hex> <member>.oci.tar use): " $0 > "/dev/stderr"
            bad = 1
        }
        { print }
        END { exit bad }'
}

# digest ARCHIVE: the digest of the one manifest the archive's index.json names, once
# the blob holding that manifest is found in the archive with that digest.
digest() {
    local archive="$1" members index count d blob
    [ -f "$archive" ] || { echo "externals: $archive is missing" >&2; return 1; }
    members="$(tar -tf "$archive" 2>/dev/null)" || { echo "externals: $archive is not a tar archive" >&2; return 1; }
    # An archive's members may be written with or without a leading ./.
    member() { grep -m1 -xE "(\./)?$1" <<<"$members"; }
    index="$(member index.json)" || { echo "externals: $archive has no index.json; it is not an OCI archive" >&2; return 1; }
    member oci-layout >/dev/null || { echo "externals: $archive has no oci-layout; it is not an OCI archive" >&2; return 1; }
    count="$(tar -xOf "$archive" "$index" | grep -o '"digest"' | wc -l | tr -d ' ')"
    [ "$count" = 1 ] || {
        echo "externals: $archive's index.json names $count manifests, not one" >&2
        return 1
    }
    d="$(tar -xOf "$archive" "$index" | grep -o '"digest" *: *"sha256:[0-9a-f]\{64\}"' | grep -o 'sha256:[0-9a-f]\{64\}')" || {
        echo "externals: $archive's index.json names no sha256 digest" >&2
        return 1
    }
    blob="$(member "blobs/sha256/${d#sha256:}")" || { echo "externals: $archive does not carry the manifest its index names ($d)" >&2; return 1; }
    [ "sha256:$(tar -xOf "$archive" "$blob" | sha256sum | cut -d' ' -f1)" = "$d" ] || {
        echo "externals: $archive's manifest blob does not have the digest its index names ($d)" >&2
        return 1
    }
    echo "$d"
}

# check_one ARCHIVE PIN: ARCHIVE is there and its index digest is PIN.
check_one() {
    local got
    got="$(digest "$1")" || return 1
    [ "$got" = "$2" ] || {
        echo "externals: $1 is $got, not the pinned $2" >&2
        return 1
    }
}

check() {
    local dir="$1" ref pin member use problems=0
    while read -r ref pin member use; do
        if check_one "$dir/$member" "$pin"; then
            echo "externals: ok  $member  $ref@$pin ($use)"
        else
            problems=$((problems + 1))
        fi
    done < <(entries)
    [ "$problems" -eq 0 ] || {
        echo "externals: $problems image(s) missing or not as pinned in $LOCK; fetch them while connected: $0 fetch $dir" >&2
        return 1
    }
}

fetch() {
    local dir="$1" tool ref pin member use repo tmp
    if command -v "$SKOPEO" >/dev/null 2>&1; then tool=skopeo
    elif command -v "$CRANE" >/dev/null 2>&1; then tool=crane
    else
        echo "externals: fetching needs skopeo or crane on PATH (or SKOPEO or CRANE set); neither is" >&2
        return 1
    fi
    mkdir -p "$dir"
    while read -r ref pin member use; do
        if [ -f "$dir/$member" ] && check_one "$dir/$member" "$pin" 2>/dev/null; then
            echo "externals: have $member"
            continue
        fi
        # By digest, never by tag: the tag is only a name for people.
        repo="$ref"
        [[ "${repo##*/}" == *:* ]] && repo="${repo%:*}"
        echo "externals: fetching $repo@$pin ($use) with $tool"
        tmp="$dir/.$member.partial"
        rm -rf "$tmp" "$tmp.d"
        case "$tool" in
            skopeo)
                "$SKOPEO" copy --all --preserve-digests "docker://$repo@$pin" "oci-archive:$tmp"
                ;;
            crane)
                "$CRANE" pull --format=oci "$repo@$pin" "$tmp.d"
                tar -C "$tmp.d" -cf "$tmp" oci-layout index.json blobs
                rm -rf "$tmp.d"
                ;;
        esac
        check_one "$tmp" "$pin" || { rm -f "$tmp"; return 1; }
        mv -f "$tmp" "$dir/$member"
        echo "externals: fetched $member, $pin"
    done < <(entries)
    check "$dir"
}

case "${1:-}" in
    fetch) [ "$#" -eq 2 ] || usage; fetch "$2" ;;
    check) [ "$#" -eq 2 ] || usage; check "$2" ;;
    digest) [ "$#" -eq 2 ] || usage; digest "$2" ;;
    *) usage ;;
esac
