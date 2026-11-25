#!/usr/bin/env bash
# Each app carries what its edition allows, and nothing more. There is one edition, Maps
# (owner decision, 23 November 2026: two apps, one branch), built as one app per platform:
#
#   MidgardMacMaps  io.veldris.midgardmaps.mac   the edition library and the client
#                                                network entitlement (map tiles)
#   MidgardiOSMaps  io.veldris.midgardmaps.ios   the edition library (iOS has no network
#                                                entitlement)
#
# Builds each app present (Debug, the one configuration that builds without a team) and
# fails if:
#   - its Info.plist's MidgardEdition is not maps;
#   - (Mac) it is not sandboxed (com.apple.security.app-sandbox true);
#   - (build mode) its target's entitlements file is not its own: <scheme>.entitlements or
#     <scheme>-AdHoc / -NoTeam.entitlements;
#   - no Mach-O file defines the link-time hook midgard_edition_hook strongly (the edition
#     library MidgardGlobeKit is not linked);
#   - it has a network entitlement other than the client one (com.apple.security.network.server,
#     com.apple.developer.networking.*) or a temporary-exception entitlement
#     (com.apple.security.temporary-exception.*);
#   - (Mac) it lacks the client network entitlement;
#   - (Phase J, your own data) it bundles data: a data file (.jsonl, .tsv, .csv, .mgdemo,
#     .geojson, .ndjson), or any resource file (not a Mach-O file) naming a leak archive
#     (Mauritius Leaks, Offshore/Panama/Pandora/Paradise Papers, ICIJ) or the local live
#     suite's switches, live area or feed hosts. The apps fetch nothing of their own: the
#     user adds a gateway or feed URLs at run time (Your Data), and only the map tile
#     loader has fixed hosts. The bundled JSON allowed is the Natural Earth outlines;
#   - its App Transport Security asks for more than NSAllowsLocalNetworking (loopback HTTP
#     for a gateway or feed on the device): no arbitrary loads, no exception domains.
# Debug binaries keep their symbol tables, so a symbol the linker kept is seen. B4/B5
# must also run this on the signed Release archives (<app>... below). A Release build's
# executable is stripped and keeps no symbol table: give the archive's dSYMs with
# --dsyms <dir>, and the hook is looked up in the dSYM whose UUID matches one of the
# app's Mach-O files (the same binary, before stripping; re-signing keeps the UUID).
# [FR-C-1] [DD-6]
#
#   bash clients/shells/apple/tools/check-edition.sh               build and check both apps present
#   bash clients/shells/apple/tools/check-edition.sh [--maps] [--dsyms <dir>] <app>...
#                                                  check built bundles only
#
# Derived data goes to ~/Library/Caches/midgard-build/clients-derived-{mac,ios}
# (MIDGARD_BUILD_CACHE names another cache); the iPhone app is built for the iPhone 17
# simulator (MIDGARD_AUDIT_SIMULATOR names another), which is not booted.
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
hook=midgard_edition_hook
net_re='com\.apple\.(security\.network\.(client|server)|developer\.networking\.[A-Za-z.-]+|security\.temporary-exception\.[A-Za-z.-]+)'

say() { echo "check-edition: $*" >&2; }

# What only local-suite or leak-archive data contains (clients/core/src/demo/guard.rs's
# DENY_MARKERS, the ones a public bundle file can never hold innocently).
data_re='mauritius leaks|offshore leaks|panama papers|pandora papers|paradise papers|icij|port louis|agal[eé]ga|ceb\.mu|govmu\.org|live-feeds|midgard_replay|adsb\.lol|adsbexchange|airplanes\.live|opensky|aishub|aisstream|marinetraffic|seismicportal\.eu|api\.tfl\.gov\.uk|data\.elexon\.co\.uk|api\.carbonintensity\.org\.uk|environment\.data\.gov\.uk|data\.sensor\.community'
data_ext_re='\.(jsonl|ndjson|tsv|csv|mgdemo|geojson)$'
dsyms=""

# check_bundle <app>: prints one line per problem; returns non-zero when there is one.
check_bundle() {
    local app="$1" bad=0 info said f ents mac=0 strong=0 net="" syms uuids="" hook_in="" d u
    fail() { echo "FAIL  $app: $*"; bad=1; }
    if [ ! -d "$app" ]; then
        echo "FAIL  $app: not a bundle"
        return 1
    fi
    if [ -f "$app/Contents/Info.plist" ]; then info="$app/Contents/Info.plist"; mac=1; else info="$app/Info.plist"; fi
    said="$(plutil -extract MidgardEdition raw -o - "$info" 2>/dev/null || true)"
    [ "$said" = maps ] || fail "MidgardEdition is '${said:-missing}', not maps"
    while IFS= read -r -d '' f; do
        if ! file -b "$f" | grep -q 'Mach-O'; then
            case "$f" in */_CodeSignature/*|*.car|*.ttf|*.png|*.icns|*.nib) continue ;; esac
            if grep -Eiq "$data_ext_re" <<<"$f"; then
                fail "bundles a data file: ${f#"$app"/}"
            elif LC_ALL=C grep -Eiaq "$data_re" "$f"; then
                fail "bundles local-suite or leak-archive data: ${f#"$app"/} ($(LC_ALL=C grep -Eioa "$data_re" "$f" | sort -u | head -3 | tr '\n' ' '))"
            fi
            continue
        fi
        uuids="$uuids $(dwarfdump --uuid "$f" 2>/dev/null | awk '{ print $2 }' | tr '\n' ' ')"
        syms="$(nm -m "$f" 2>/dev/null | grep -E " _${hook}\$" || true)"
        if grep -v 'weak external' <<<"$syms" | grep -q 'external'; then strong=1; fi
        net="$net $(LC_ALL=C grep -aoE "$net_re" "$f" | sort -u | tr '\n' ' ')"
    done < <(find "$app" -type f -print0)
    # The signature's entitlements (macOS apps and iOS device builds carry them there).
    ents="$(codesign -d --entitlements - --xml "$app" 2>/dev/null | tr -d ' \t\n' || true)"
    net="$net $(grep -oE "$net_re" <<<"$ents" | sort -u | tr '\n' ' ')"
    if [ "$mac" = 1 ] && ! grep -q '<key>com.apple.security.app-sandbox</key><true/>' <<<"$ents"; then
        fail "is not sandboxed (com.apple.security.app-sandbox is not true)"
    fi
    # A stripped Release binary: the hook in the dSYM of the same binary (matching UUID).
    if [ "$strong" = 0 ] && [ -n "$dsyms" ]; then
        while IFS= read -r -d '' d; do
            for u in $(dwarfdump --uuid "$d" 2>/dev/null | awk '{ print $2 }'); do
                case " $uuids " in
                    *" $u "*)
                        syms="$(nm -m "$d" 2>/dev/null | grep -E " _${hook}\$" || true)"
                        if grep -v 'weak external' <<<"$syms" | grep -q 'external'; then
                            strong=1; hook_in=", found in the matching dSYM"
                        fi ;;
                esac
            done
        done < <(find "$dsyms" -path '*.dSYM/Contents/Resources/DWARF/*' -type f -print0 2>/dev/null)
    fi
    if [ "$strong" = 0 ]; then
        fail "no Mach-O file defines $hook strongly; the app links the edition library"
    fi
    if grep -qE '(network\.server|developer\.networking|temporary-exception)' <<<"$net"; then
        fail "has an entitlement beyond the network client one ($(tr ' ' '\n' <<<"$net" | sort -u | tr '\n' ' ' | sed 's/^ *//;s/ *$//'))"
    fi
    if [ "$mac" = 1 ] && ! grep -q 'network\.client' <<<"$net"; then
        fail "lacks the network client entitlement the Mac app has"
    fi
    local ats
    ats="$(plutil -extract NSAppTransportSecurity json -o - "$info" 2>/dev/null || true)"
    if [ -n "$ats" ] && [ "$ats" != '{"NSAllowsLocalNetworking":true}' ]; then
        fail "App Transport Security asks for more than NSAllowsLocalNetworking ($ats)"
    fi
    if [ "$bad" -eq 0 ]; then
        echo "ok    $app: maps edition: edition library linked (strong hook$hook_in), network: $( [ "$mac" = 1 ] && echo "client only, sandboxed" || echo "none on iOS"), ATS: ${ats:-default}, no data bundled"
    fi
    return "$bad"
}

# check_args [--maps] <app>...: checks each app.
check_args() {
    local rc=0 n=0
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --maps) ;;
            --dsyms)
                dsyms="${2:-}"
                [ -d "$dsyms" ] || { say "--dsyms needs a directory"; return 2; }
                shift ;;
            -*) say "unknown option: $1 (give app bundles, or no argument)"; return 2 ;;
            *) check_bundle "$1" || rc=1; n=$((n + 1)) ;;
        esac
        shift
    done
    [ "$n" -gt 0 ] || { say "no app bundle given"; return 2; }
    return "$rc"
}

# built_app <project> <scheme> <destination> <derived data>: builds; prints the app's path,
# then the entitlements file its target signs with.
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
    awk -F' = ' '$1 ~ /^ *CODE_SIGN_ENTITLEMENTS$/ { print $2; exit }' <<<"$settings"
}

if [ "$#" -gt 0 ]; then
    check_args "$@"
    exit $?
fi

args=()
static_bad=0
for platform in Mac iOS; do
    proj="$here/Apps/Midgard$platform/Midgard$platform.xcodeproj"
    [ -d "$proj" ] || continue
    if [ "$platform" = Mac ]; then
        dest='platform=macOS'; derived="$cache/clients-derived-mac"
    else
        # A named simulator: the XCFramework's simulator slice is arm64 only, and a generic
        # destination would also build x86_64.
        dest="platform=iOS Simulator,name=${MIDGARD_AUDIT_SIMULATOR:-iPhone 17}"; derived="$cache/clients-derived-ios"
    fi
    scheme="Midgard${platform}Maps"
    out="$(built_app "$proj" "$scheme" "$dest" "$derived")" || { say "the $scheme build failed"; exit 1; }
    app="$(sed -n 1p <<<"$out")"
    ents="$(sed -n 2p <<<"$out")"
    ents="${ents##*/}"
    case "$ents" in
        "$scheme.entitlements" | "$scheme-AdHoc.entitlements" | "$scheme-NoTeam.entitlements")
            echo "ok    $scheme: signs with its own entitlements, $ents" ;;
        *)
            echo "FAIL  $scheme: signs with '${ents:-no file}', not its own $scheme{,-AdHoc,-NoTeam}.entitlements"
            static_bad=1 ;;
    esac
    args+=("$app")
done
if [ "${#args[@]}" -eq 0 ]; then
    say "no app under clients/shells/apple/Apps; nothing was checked"
    exit 1
fi
check_args "${args[@]}"
rc=$?
[ "$static_bad" -eq 0 ] || rc=1
exit "$rc"
