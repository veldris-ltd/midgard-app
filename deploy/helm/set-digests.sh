#!/usr/bin/env bash
# Pin the chart's images by digest from a release's signed images.lock.
# [VLD-ANB-MIDGARD-0012] [RC-18]
#
#   bash deploy/helm/set-digests.sh IMAGES.LOCK [VALUES] [--out FILE]
#
# IMAGES.LOCK is what deploy/images/build.sh writes and the release signs: one
# "<registry>/<image>:<version> sha256:<digest>" line per image. For each, the image
# name's entry under `image.digests:` in VALUES (default deploy/helm/midgard/values.yaml)
# is set to the digest. With --out the result goes to FILE and VALUES is left as it is;
# without, VALUES is rewritten.
#
# It refuses, changing nothing, when a lock line is not "<ref> sha256:<64 hex>", when an
# image is in the lock twice or has no entry under image.digests, and when a service
# under image.digests has no lock line: a release chart pins every image or none.
set -euo pipefail

usage() {
    sed -n '5p' "$0" | sed 's/^# *//' >&2
    exit 2
}
lock="" values="" out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) [ "$#" -ge 2 ] || usage; out="$2"; shift 2 ;;
        -*) echo "set-digests: unknown option $1" >&2; usage ;;
        *)
            if [ -z "$lock" ]; then lock="$1"
            elif [ -z "$values" ]; then values="$1"
            else usage
            fi
            shift ;;
    esac
done
[ -n "$lock" ] || usage
[ -n "$values" ] || values="$(dirname "$0")/midgard/values.yaml"
[ -f "$lock" ] || { echo "set-digests: no lock file $lock" >&2; exit 1; }
[ -f "$values" ] || { echo "set-digests: no values file $values" >&2; exit 1; }

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

# One pass over the lock, then one over the values: the digests block is the lines
# indented under `  digests:` inside the top-level `image:` map.
if ! awk '
    function refuse(msg) { print "set-digests: " msg > "/dev/stderr"; bad = 1; exit 1 }
    { sub(/\r$/, "") }
    FNR == NR {
        if ($0 ~ /^[ \t]*$/) next
        if (NF != 2) refuse("lock line " FNR " is not \"<ref> <digest>\": " $0)
        ref = $1; digest = $2
        if (digest !~ /^sha256:[0-9a-f]+$/ || length(digest) != 71)
            refuse("lock line " FNR " has no sha256 digest of 64 hex digits: " $0)
        name = ref
        sub(/^.*\//, "", name)
        sub(/:[^:]*$/, "", name)
        sub(/@.*$/, "", name)
        if (name !~ /^[a-z0-9][a-z0-9.-]*$/) refuse("lock line " FNR " names no image: " $0)
        if (name in want) refuse("the lock names " name " twice")
        want[name] = digest
        next
    }
    /^[^ #]/ { inimage = ($0 ~ /^image:/); indigests = 0 }
    inimage && /^  [^ #]/ { indigests = ($0 ~ /^  digests:/) }
    indigests && match($0, /^    [a-z0-9][a-z0-9.-]*:/) {
        name = substr($0, 5, RLENGTH - 5)
        if (!(name in want)) refuse("the lock has no image for service " name " (image.digests." name ")")
        print "    " name ": " want[name]
        done[name] = 1; ndone++
        next
    }
    { print }
    END {
        if (bad) exit 1
        for (name in want)
            if (!(name in done)) { print "set-digests: image " name " in the lock has no image.digests entry" > "/dev/stderr"; exit 1 }
        if (ndone == 0) { print "set-digests: the values file has no image.digests block" > "/dev/stderr"; exit 1 }
    }
' "$lock" "$values" > "$tmp"; then
    exit 1
fi

if [ -n "$out" ]; then
    mkdir -p "$(dirname "$out")"
    cp "$tmp" "$out"
    echo "set-digests: $(grep -c '^    [a-z0-9.-]*: sha256:' "$out") image digest(s) from $lock -> $out"
else
    cp "$tmp" "$values"
    echo "set-digests: $(grep -c '^    [a-z0-9.-]*: sha256:' "$values") image digest(s) from $lock -> $values"
fi
