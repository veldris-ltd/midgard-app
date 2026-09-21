#!/usr/bin/env bash
# The release artefact list, checked against the workspace and printed for the release
# workflow. [RC-07] [AC-B-1]
#
#   bash tools/ci/release-artefacts.sh --check     exit 1 unless supply-chain/release-artefacts.txt
#                                                  names exactly the binaries the workspace builds
#   bash tools/ci/release-artefacts.sh --matrix    the gate matrix, as GitHub Actions JSON
#   bash tools/ci/release-artefacts.sh --images    the images to build, as a JSON array
#   bash tools/ci/release-artefacts.sh --bins IMAGE  the binaries one image carries
#   bash tools/ci/release-artefacts.sh --version   the workspace version
#
# Every mode but --version and --bins checks first. Needs only bash, sed and awk.
set -euo pipefail

cd "$(dirname "$0")/../.."
LIST=supply-chain/release-artefacts.txt

# The binaries the workspace builds, as "package binary" lines: each member's declared
# [[bin]] targets, its implicit src/main.rs binary, and any src/bin/*.rs not declared.
built() {
    local members dir manifest package declared paths bins name path file rel
    members="$(tr -d '\r' < Cargo.toml | sed -n '/^members = \[/,/^\]/p' \
               | sed -n 's/^ *"\([^"]*\)",\{0,1\} *$/\1/p')"
    for dir in $members; do
        manifest="$dir/Cargo.toml"
        [ -f "$manifest" ] || continue
        package="$(tr -d '\r' < "$manifest" | sed -n 's/^name *= *"\(.*\)"/\1/p' | head -1)"
        declared="$(tr -d '\r' < "$manifest" | awk '
            /^\[\[bin\]\]/ { if (inb) print name "|" path; inb = 1; name = ""; path = ""; next }
            /^\[/          { if (inb) print name "|" path; inb = 0; next }
            inb && /^name *=/ { v = $0; sub(/^name *= *"/, "", v); sub(/".*/, "", v); name = v }
            inb && /^path *=/ { v = $0; sub(/^path *= *"/, "", v); sub(/".*/, "", v); path = v }
            END { if (inb) print name "|" path }')"
        paths=" "
        bins=""
        while IFS='|' read -r name path; do
            [ -n "$name" ] || continue
            if [ -z "$path" ]; then
                if [ "$name" = "$package" ] && [ -f "$dir/src/main.rs" ]; then
                    path=src/main.rs
                else
                    path="src/bin/$name.rs"
                fi
            fi
            bins="$bins $name"
            paths="$paths$path "
        done <<<"$declared"
        if [ -f "$dir/src/main.rs" ] && [[ "$paths" != *" src/main.rs "* ]]; then
            bins="$bins $package"
        fi
        for file in "$dir"/src/bin/*.rs; do
            [ -f "$file" ] || continue
            rel="src/bin/$(basename "$file")"
            [[ "$paths" == *" $rel "* ]] || bins="$bins $(basename "$file" .rs)"
        done
        for name in $bins; do echo "$package $name"; done
    done | sort -u
}

# The list's entries, comments and blank lines dropped: "package binary component image".
entries() { tr -d '\r' < "$LIST" | sed 's/#.*//' | awk 'NF'; }

check() {
    local status=0 missing extra bad
    missing="$(comm -23 <(built) <(entries | awk '{print $1, $2}' | sort -u))"
    extra="$(comm -13 <(built) <(entries | awk '{print $1, $2}' | sort -u))"
    bad="$(entries | awk 'NF != 4 || ($3 != "service" && $3 != "tool") {print}')"
    if [ -n "$missing" ]; then
        echo "release-artefacts: built but not in $LIST (add each, with its component and image):" >&2
        sed 's/^/  /' <<<"$missing" >&2
        status=1
    fi
    if [ -n "$extra" ]; then
        echo "release-artefacts: in $LIST but no longer built:" >&2
        sed 's/^/  /' <<<"$extra" >&2
        status=1
    fi
    if [ -n "$bad" ]; then
        echo "release-artefacts: malformed entries (want: package binary service|tool image|-):" >&2
        sed 's/^/  /' <<<"$bad" >&2
        status=1
    fi
    return "$status"
}

case "${1:-}" in
    --check)
        check
        echo "release-artefacts: $(entries | wc -l | tr -d ' ') binaries, every one listed"
        ;;
    --matrix)
        check >&2
        entries | awk 'BEGIN { printf "{\"include\":[" }
            { printf "%s{\"package\":\"%s\",\"bin\":\"%s\",\"component\":\"%s\",\"image\":\"%s\"}",
                     (NR > 1 ? "," : ""), $1, $2, $3, $4 }
            END { print "]}" }'
        ;;
    --images)
        check >&2
        entries | awk '$4 != "-" { print $4 }' | sort -u \
            | awk 'BEGIN { printf "[" } { printf "%s\"%s\"", (NR > 1 ? "," : ""), $1 } END { print "]" }'
        ;;
    --bins)
        [ -n "${2:-}" ] || { echo "usage: $0 --bins IMAGE" >&2; exit 2; }
        entries | awk -v image="$2" '$4 == image { print $2 }'
        ;;
    --version)
        tr -d '\r' < Cargo.toml | sed -n '/^\[workspace.package\]/,/^\[/p' \
            | sed -n 's/^version *= *"\(.*\)"/\1/p' | head -1
        ;;
    *)
        echo "usage: $0 --check | --matrix | --images | --bins IMAGE | --version" >&2
        exit 2
        ;;
esac
