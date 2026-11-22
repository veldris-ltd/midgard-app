#!/usr/bin/env bash
# Each app carries what its edition allows, and nothing more (Task B7, owner decision
# 22 Nov 2026). There are three editions, each its own app on the Mac and the iPhone:
#
#   release  MidgardMac, MidgardiOS          no edition code or wording, no network entitlement
#   3d       MidgardMac3D, MidgardiOS3D      the edition library, no network entitlement
#   maps     MidgardMacMaps, MidgardiOSMaps  the edition library and, on the Mac, the client
#                                            network entitlement (the only edition with one)
#
# Builds each app present (Debug, the one configuration that builds without a team) and
# fails if, for its edition:
#   all       its Info.plist's MidgardEdition is not that edition; (Mac) it is not
#             sandboxed (com.apple.security.app-sandbox true); (build mode) its target's
#             entitlements file is not its own: <scheme>.entitlements or
#             <scheme>-AdHoc / -NoTeam.entitlements;
#   release   a file's name or bytes contain "MidgardGlobeKit", or a Mach-O file holds
#             "Globe" or any edition wording ("not in the signed release", "not the signed
#             release", "Midgard Maps", "Edition: ", "ea5899ab") or the edition provider
#             type (EditionUIProvider); the link-time hook midgard_edition_hook is defined
#             other than weakly; a Mach-O file links MetalKit.framework;
#   3d, maps  no Mach-O file defines midgard_edition_hook strongly (the edition library
#             is not linked);
#   release, 3d
#             a network entitlement (com.apple.security.network.*,
#             com.apple.developer.networking.*) or a temporary-exception entitlement
#             (com.apple.security.temporary-exception.*); and, ahead of the tile loader
#             (Task I2, its own module MidgardTileKit, linked only by the maps apps):
#             any "MidgardTileKit" symbol, or an Info.plist key NSAppTransportSecurity,
#             NSLocalNetworkUsageDescription or NSBonjourServices;
#   maps      the network server entitlement, or, on the Mac, no client one. iOS has no
#             network entitlement (an iPhone app may use the network), so an iPhone maps
#             app is not required to carry one.
# Debug binaries keep their symbol tables, so a symbol the linker kept is seen. B4/B5
# must also run this on the signed Release archives (--release/--3d/--maps <app>).
# [FR-C-1] [DD-6]
#
#   bash clients/shells/apple/tools/check-edition.sh          build and check every app present
#   bash clients/shells/apple/tools/check-edition.sh --release <app>... [--3d <app>...] [--maps <app>...]
#                                                             check built bundles only, each as
#                                                             the edition named before it
#
# Derived data goes to ~/Library/Caches/midgard-build/clients-derived-{mac,ios}
# (MIDGARD_BUILD_CACHE names another cache); the iPhone apps are built for the iPhone 17
# simulator (MIDGARD_AUDIT_SIMULATOR names another), which is not booted.
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
hook=midgard_edition_hook
# Words a release app's Mach-O files must not hold (bytes, so symbols and strings alike).
release_words=("Globe" "not in the signed release" "not the signed release" "Midgard Maps" "Edition: "
    "ea5899ab" "EditionUIProvider")
# Entitlements only the maps edition may have (and of those, only the network client one).
net_re='com\.apple\.(security\.network\.(client|server)|developer\.networking\.[A-Za-z.-]+|security\.temporary-exception\.[A-Za-z.-]+)'

say() { echo "check-edition: $*" >&2; }

# check_bundle <edition> <app>: prints one line per problem; returns non-zero when there is one.
check_bundle() {
    local edition="$1" app="$2" bad=0 info said f ents mac=0 strong=0 net="" w syms key
    fail() { echo "FAIL  $app: $*"; bad=1; }
    if [ ! -d "$app" ]; then
        echo "FAIL  $app: not a bundle"
        return 1
    fi
    if [ -f "$app/Contents/Info.plist" ]; then info="$app/Contents/Info.plist"; mac=1; else info="$app/Info.plist"; fi
    said="$(plutil -extract MidgardEdition raw -o - "$info" 2>/dev/null || true)"
    [ "$said" = "$edition" ] || fail "MidgardEdition is '${said:-missing}', not $edition"
    if [ "$edition" != maps ]; then
        for key in NSAppTransportSecurity NSLocalNetworkUsageDescription NSBonjourServices; do
            if plutil -extract "$key" xml1 -o - "$info" >/dev/null 2>&1; then
                fail "Info.plist has $key; only the maps edition may reach the network"
            fi
        done
    fi
    if [ "$edition" = release ]; then
        # Names first (a resource bundle such as MidgardKit_MidgardGlobeKit.bundle).
        while IFS= read -r f; do
            fail "holds ${f#"$app"/}, of MidgardGlobeKit"
        done < <(find "$app" -name '*MidgardGlobeKit*' -print)
    fi
    # Then bytes and symbols.
    while IFS= read -r -d '' f; do
        if [ "$edition" = release ] && LC_ALL=C grep -aq 'MidgardGlobeKit' "$f"; then
            fail "${f#"$app"/} contains a MidgardGlobeKit symbol or name"
        fi
        file -b "$f" | grep -q 'Mach-O' || continue
        if [ "$edition" = release ]; then
            for w in "${release_words[@]}"; do
                if LC_ALL=C grep -aqF -- "$w" "$f"; then
                    if [ "$w" = Globe ]; then fail "${f#"$app"/} contains a Globe type or name"
                    else fail "${f#"$app"/} holds edition-only \"$w\""; fi
                fi
            done
            if otool -L "$f" 2>/dev/null | grep -q 'MetalKit.framework'; then
                fail "${f#"$app"/} links MetalKit.framework"
            fi
        fi
        if [ "$edition" != maps ] && LC_ALL=C grep -aq 'MidgardTileKit' "$f"; then
            fail "${f#"$app"/} contains MidgardTileKit, the maps edition's tile loader"
        fi
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
    if [ "$edition" = release ] && [ "$strong" = 1 ]; then
        fail "defines $hook strongly: an edition library is linked"
    fi
    if [ "$edition" != release ] && [ "$strong" = 0 ]; then
        fail "no Mach-O file defines $hook strongly; the $edition edition links the edition library"
    fi
    case "$edition" in
        release | 3d)
            if grep -qE '(network|temporary-exception)' <<<"$net"; then
                fail "has a network or temporary-exception entitlement ($(tr ' ' '\n' <<<"$net" | sort -u | tr '\n' ' ' | sed 's/^ *//;s/ *$//')); the $edition edition has none"
            fi ;;
        maps)
            if grep -qE '(network\.server|developer\.networking|temporary-exception)' <<<"$net"; then
                fail "has an entitlement beyond the network client one; the maps edition has that one only"
            fi
            if [ "$mac" = 1 ] && ! grep -q 'network\.client' <<<"$net"; then
                fail "lacks the network client entitlement the Mac maps edition has"
            fi ;;
    esac
    if [ "$bad" -eq 0 ]; then
        case "$edition" in
            release) echo "ok    $app: release edition: no edition code or wording, hook weak or absent, no MetalKit, no network entitlement" ;;
            3d) echo "ok    $app: 3d edition: edition library linked (strong hook), no network entitlement" ;;
            maps) echo "ok    $app: maps edition: edition library linked (strong hook), network: $( [ "$mac" = 1 ] && echo client only || echo none on iOS)" ;;
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
    for pair in ":release" "3D:3d" "Maps:maps"; do
        scheme="Midgard$platform${pair%%:*}"
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
        args+=("--${pair##*:}" "$app")
    done
done
if [ "${#args[@]}" -eq 0 ]; then
    say "no app under clients/shells/apple/Apps; nothing was checked"
    exit 1
fi
check_args "${args[@]}"
rc=$?
[ "$static_bad" -eq 0 ] || rc=1
exit "$rc"
