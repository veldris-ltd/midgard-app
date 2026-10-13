#!/usr/bin/env bash
# The release manifest: every binary the workspace builds, derived from the workspace
# and annotated by supply-chain/release-artefacts.txt. [RC-07] [AC-B-1] [AC-RF-SC-5a]
#
#   bash tools/ci/release-artefacts.sh --check     exit 1 if the annotation list is wrong
#   bash tools/ci/release-artefacts.sh --rows      "package binary component image" lines
#   bash tools/ci/release-artefacts.sh --matrix    the gate matrix, as JSON
#   bash tools/ci/release-artefacts.sh --images    the images to build, as a JSON array
#   bash tools/ci/release-artefacts.sh --bins IMAGE  the binaries one image carries
#   bash tools/ci/release-artefacts.sh --version   the workspace version
#
# Coverage is DERIVED, not listed. The set of binaries comes from the workspace's own
# manifests, so a binary added to the workspace is in the matrix — and therefore gated
# — with no edit anywhere. supply-chain/release-artefacts.txt only ANNOTATES that set:
# which component kind a binary is, and which image it is packed into. A binary the
# list does not mention is gated as a bare `tool` with no image, with a warning to say
# so. What the list cannot do is name something the workspace does not build: a stale
# or malformed entry fails --check, as does one binary name built by two packages.
#
# Every mode but --version checks first. Needs only bash, sed and awk.
set -euo pipefail

# RELEASE_ARTEFACTS_ROOT points the script at another workspace (its tests use this).
cd "${RELEASE_ARTEFACTS_ROOT:-$(dirname "$0")/../..}"
LIST=supply-chain/release-artefacts.txt

# The binaries the workspace builds, as "package binary" lines: each member's declared
# [[bin]] targets, its implicit src/main.rs binary, and every src/bin/<name>.rs and
# src/bin/<name>/main.rs not already declared.
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
                elif [ -f "$dir/src/bin/$name/main.rs" ]; then
                    path="src/bin/$name/main.rs"
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
        # Cargo also builds src/bin/<name>/main.rs as the binary <name>.
        for file in "$dir"/src/bin/*/main.rs; do
            [ -f "$file" ] || continue
            name="$(basename "$(dirname "$file")")"
            rel="src/bin/$name/main.rs"
            [[ "$paths" == *" $rel "* ]] || bins="$bins $name"
        done
        for name in $bins; do echo "$package $name"; done
    done | sort -u
}

# The list's entries, comments and blank lines dropped: "package binary component image".
entries() {
    [ -f "$LIST" ] || return 0
    tr -d '\r' < "$LIST" | sed 's/#.*//' | awk 'NF'
}

# The manifest: every built binary with its annotation, or the default for one the
# list does not mention. Sorted by package then binary.
rows() {
    awk 'FILENAME == ARGV[1] { note[$1 " " $2] = $3 " " $4; next }
         { key = $1 " " $2; print key, (key in note ? note[key] : "tool -") }' \
        <(entries) <(built)
}

check() {
    local status=0 extra bad clash unlisted
    extra="$(comm -13 <(built) <(entries | awk '{print $1, $2}' | sort -u))"
    bad="$(entries | awk 'NF != 4 || ($3 != "service" && $3 != "tool") {print}')"
    clash="$(built | awk '{print $2}' | sort | uniq -d)"
    unlisted="$(comm -23 <(built) <(entries | awk '{print $1, $2}' | sort -u))"
    if [ -n "$extra" ]; then
        echo "release-artefacts: in $LIST but not built by the workspace:" >&2
        sed 's/^/  /' <<<"$extra" >&2
        status=1
    fi
    if [ -n "$bad" ]; then
        echo "release-artefacts: malformed entries (want: package binary service|tool image|-):" >&2
        sed 's/^/  /' <<<"$bad" >&2
        status=1
    fi
    if [ -n "$clash" ]; then
        echo "release-artefacts: one binary name built by more than one package (the registry id would collide):" >&2
        sed 's/^/  /' <<<"$clash" >&2
        status=1
    fi
    if [ -z "$(built)" ]; then
        echo "release-artefacts: the workspace builds no binaries — nothing would be gated" >&2
        status=1
    fi
    if [ -n "$unlisted" ]; then
        echo "release-artefacts: WARNING — gated as a bare tool with no image, because $LIST does not annotate it:" >&2
        sed 's/^/  /' <<<"$unlisted" >&2
    fi
    return "$status"
}

case "${1:-}" in
    --check)
        check
        echo "release-artefacts: $(rows | wc -l | tr -d ' ') binaries, every one in the matrix"
        ;;
    --rows)
        check
        rows
        ;;
    --matrix)
        check
        rows | awk 'BEGIN { printf "{\"include\":[" }
            { printf "%s{\"package\":\"%s\",\"bin\":\"%s\",\"component\":\"%s\",\"image\":\"%s\"}",
                     (NR > 1 ? "," : ""), $1, $2, $3, $4 }
            END { print "]}" }'
        ;;
    --images)
        check
        rows | awk '$4 != "-" { print $4 }' | sort -u \
            | awk 'BEGIN { printf "[" } { printf "%s\"%s\"", (NR > 1 ? "," : ""), $1 } END { print "]" }'
        ;;
    --bins)
        [ -n "${2:-}" ] || { echo "usage: $0 --bins IMAGE" >&2; exit 2; }
        rows | awk -v image="$2" '$4 == image { print $2 }'
        ;;
    --version)
        tr -d '\r' < Cargo.toml | sed -n '/^\[workspace.package\]/,/^\[/p' \
            | sed -n 's/^version *= *"\(.*\)"/\1/p' | head -1
        ;;
    *)
        echo "usage: $0 --check | --rows | --matrix | --images | --bins IMAGE | --version" >&2
        exit 2
        ;;
esac
