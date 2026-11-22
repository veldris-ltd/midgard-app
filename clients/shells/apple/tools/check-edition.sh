#!/usr/bin/env bash
# Each app carries what its edition allows, and nothing more (Task B7, owner decision
# 22 Nov 2026). There are three editions, each its own app on the Mac and the iPhone:
#
#   release  MidgardMac, MidgardiOS          no 3D library, no Globe, no network entitlement
#   3d       MidgardMac3D, MidgardiOS3D      the 3D library, no network entitlement
#   maps     MidgardMacMaps, MidgardiOSMaps  the 3D library and, on the Mac, the client
#                                            network entitlement (the only edition with one)
#
# Builds each app present (Debug, the one configuration that builds without a team) and
# fails if, for its edition:
#   - its Info.plist's MidgardEdition is not that edition;
#   - (release) a file's name or bytes contain "MidgardGlobeKit" (the 3D library's
#     symbols, its type metadata, its resource bundle), or a Mach-O file's bytes contain
#     "Globe" (any Globe type, from any module);
#   - (3d, maps) no Mach-O file contains "MidgardGlobeKit": the 3D library is not linked;
#   - (release, 3d) it is signed with, or embeds, a network entitlement
#     (com.apple.security.network.*);
#   - (maps) it has the server network entitlement, or, on the Mac, lacks the client one.
#     iOS has no network entitlement (an iPhone app may use the network), so an iPhone
#     maps app is not required to carry one.
# Debug binaries keep their symbol tables, so a symbol the linker kept is seen.
# [FR-C-1] [DD-6]
#
#   bash clients/shells/apple/tools/check-edition.sh          build and check every app present
#   bash clients/shells/apple/tools/check-edition.sh --release <app>... [--3d <app>...] [--maps <app>...]
#                                                             check built bundles only, each as
#                                                             the edition named before it
#
# Derived data goes to ~/Library/Caches/midgard-build/clients-derived-{mac,ios}
# (MIDGARD_BUILD_CACHE names another cache); the simulator build needs no device.
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"

say() { echo "check-edition: $*" >&2; }

# check_bundle <edition> <app>: prints one line per problem; returns non-zero when there is one.
check_bundle() {
    local edition="$1" app="$2" bad=0 info said f ents mac=0 globe=0 net
    if [ ! -d "$app" ]; then
        echo "FAIL  $app: not a bundle"
        return 1
    fi
    if [ -f "$app/Contents/Info.plist" ]; then info="$app/Contents/Info.plist"; mac=1; else info="$app/Info.plist"; fi
    said="$(plutil -extract MidgardEdition raw -o - "$info" 2>/dev/null || true)"
    if [ "$said" != "$edition" ]; then
        echo "FAIL  $app: MidgardEdition is '${said:-missing}', not $edition"
        bad=1
    fi
    if [ "$edition" = release ]; then
        # Names first (a resource bundle such as MidgardKit_MidgardGlobeKit.bundle).
        while IFS= read -r f; do
            echo "FAIL  $app: holds ${f#"$app"/}, of MidgardGlobeKit"
            bad=1
        done < <(find "$app" -name '*MidgardGlobeKit*' -print)
    fi
    # Then bytes.
    net=""
    while IFS= read -r -d '' f; do
        if [ "$edition" = release ] && LC_ALL=C grep -aq 'MidgardGlobeKit' "$f"; then
            echo "FAIL  $app: ${f#"$app"/} contains a MidgardGlobeKit symbol or name"
            bad=1
        fi
        file -b "$f" | grep -q 'Mach-O' || continue
        if LC_ALL=C grep -aq 'MidgardGlobeKit' "$f"; then globe=1; fi
        if [ "$edition" = release ] && LC_ALL=C grep -aq 'Globe' "$f"; then
            echo "FAIL  $app: ${f#"$app"/} contains a Globe type or name"
            bad=1
        fi
        net="$net $(LC_ALL=C grep -aoE 'com\.apple\.security\.network\.(client|server)' "$f" | sort -u | tr '\n' ' ')"
    done < <(find "$app" -type f -print0)
    # The signature's entitlements (macOS apps and iOS device builds carry them there).
    ents="$(codesign -d --entitlements - --xml "$app" 2>/dev/null || true)"
    net="$net $(grep -oE 'com\.apple\.security\.network\.(client|server)' <<<"$ents" | sort -u | tr '\n' ' ')"
    case "$edition" in
        release | 3d)
            if grep -q 'network' <<<"$net"; then
                echo "FAIL  $app: has a network entitlement ($(tr ' ' '\n' <<<"$net" | sort -u | tr '\n' ' ' | sed 's/^ *//;s/ *$//')); the $edition edition has none"
                bad=1
            fi ;;
        maps)
            if grep -q 'network\.server' <<<"$net"; then
                echo "FAIL  $app: has the network server entitlement; the maps edition has the client one only"
                bad=1
            fi
            if [ "$mac" = 1 ] && ! grep -q 'network\.client' <<<"$net"; then
                echo "FAIL  $app: lacks the network client entitlement the Mac maps edition has"
                bad=1
            fi ;;
    esac
    if [ "$edition" != release ] && [ "$globe" = 0 ]; then
        echo "FAIL  $app: no Mach-O file contains MidgardGlobeKit; the $edition edition links it"
        bad=1
    fi
    if [ "$bad" -eq 0 ]; then
        case "$edition" in
            release) echo "ok    $app: release edition, no MidgardGlobeKit, no Globe type, no network entitlement" ;;
            3d) echo "ok    $app: 3d edition, MidgardGlobeKit linked, no network entitlement" ;;
            maps) echo "ok    $app: maps edition, MidgardGlobeKit linked, network: $( [ "$mac" = 1 ] && echo client only || echo none on iOS)" ;;
        esac
    fi
    return "$bad"
}

# check_args (--release|--3d|--maps <app>...)...: checks each app as the edition named before it.
check_args() {
    local rc=0 edition="" n=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --release) edition=release ;;
            --3d) edition=3d ;;
            --maps) edition=maps ;;
            -*) say "unknown option: $1 (use --release, --3d or --maps <app>..., or no option)"; return 2 ;;
            *)
                [ -n "$edition" ] || { say "name the edition (--release, --3d or --maps) before $1"; return 2; }
                check_bundle "$edition" "$1" || rc=1
                n=$((n + 1)) ;;
        esac
        shift
    done
    [ "$n" -gt 0 ] || { say "no app bundle given"; return 2; }
    return "$rc"
}

# built_app <project> <scheme> <destination> <derived data>: builds and prints the app's path.
built_app() {
    local proj="$1" scheme="$2" dest="$3" derived="$4" settings dir name
    xcodebuild build -project "$proj" -scheme "$scheme" -configuration Debug -destination "$dest" \
        -derivedDataPath "$derived" -quiet >&2 || return 1
    settings="$(xcodebuild -showBuildSettings -project "$proj" -scheme "$scheme" -configuration Debug \
        -destination "$dest" -derivedDataPath "$derived" 2>/dev/null)"
    dir="$(awk -F' = ' '$1 ~ /^ *TARGET_BUILD_DIR$/ { print $2; exit }' <<<"$settings")"
    name="$(awk -F' = ' '$1 ~ /^ *WRAPPER_NAME$/ { print $2; exit }' <<<"$settings")"
    [ -n "$dir" ] && [ -n "$name" ] && [ -d "$dir/$name" ] || return 1
    echo "$dir/$name"
}

if [ "$#" -gt 0 ]; then
    check_args "$@"
    exit $?
fi

args=()
for platform in Mac iOS; do
    proj="$here/Apps/Midgard$platform/Midgard$platform.xcodeproj"
    [ -d "$proj" ] || continue
    if [ "$platform" = Mac ]; then
        dest='platform=macOS'; derived="$cache/clients-derived-mac"
    else
        dest='generic/platform=iOS Simulator'; derived="$cache/clients-derived-ios"
    fi
    for pair in ":release" "3D:3d" "Maps:maps"; do
        scheme="Midgard$platform${pair%%:*}"
        app="$(built_app "$proj" "$scheme" "$dest" "$derived")" || { say "the $scheme build failed"; exit 1; }
        args+=("--${pair##*:}" "$app")
    done
done
if [ "${#args[@]}" -eq 0 ]; then
    say "no app under clients/shells/apple/Apps; nothing was checked"
    exit 1
fi
check_args "${args[@]}"
