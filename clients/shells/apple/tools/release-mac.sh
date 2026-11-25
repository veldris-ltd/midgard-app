#!/usr/bin/env bash
# Release build of the Mac app (Task B4): MidgardMacMaps, io.veldris.midgard,
# shown as "Midgard". [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/release-mac.sh [--release <name>] [--build <n>]
#        [--development | --app-store] [--notarise | --upload]
#
# Default (Developer ID): archives Release with automatic signing under the team in
# Config/Signing.local.xcconfig, exports it with method developer-id
# (Config/ExportOptions-DeveloperID.plist), packs Midgard-<release>.dmg (HFS+, UDZO),
# signs the DMG with the Developer ID Application identity and checks it with
# `codesign --verify --deep --strict` and `spctl -a -t open --context
# context:primary-signature` (which reports "Unnotarized Developer ID" until the DMG is
# notarised: printed, not a failure). It stops before that when this Mac has no
# Developer ID Application identity, and says that nothing was built or uploaded.
#
# --development: the same Release archive, exported with the Apple Development identity
# (method debugging, Config/ExportOptions-Development.plist): Midgard.app, a zip of it
# and Midgard-<release>-development.dmg, which run on this team's registered Macs. For
# trying a real Release build before a Developer ID certificate exists; never shipped,
# never notarised.
#
# Before anything is packed, tools/check-edition.sh checks the archived app (B7 review
# notes: on the signed Release archive, not only on Debug builds), and the app must hold
# no data file: the sample data is compiled into the core (MidgardDataSource = sample),
# so no .mgdemo or other dataset is bundled.
#
# --app-store: the same Release archive, exported for the Mac App Store with method
# app-store-connect and destination export (Config/ExportOptions-AppStore.plist): the
# app signed with the team's Apple Distribution (or 3rd Party Mac Developer
# Application) identity, inside Midgard-<release>.pkg signed with its Mac Installer
# Distribution (or 3rd Party Mac Developer Installer) identity. The .pkg stays local.
# It stops before building when either identity is missing, and says that nothing was
# built or uploaded. The .pkg is checked with `pkgutil --check-signature`, and its app,
# unpacked, with codesign and check-edition.sh, as the other modes' apps are.
#
# --upload (never the default; --app-store only): sends the .pkg to App Store Connect
# with `xcrun altool --upload-package -t macos`, under the same gate as --notarise and as
# release-ios.sh --upload: MIDGARD_APPLE_UPLOAD_OK=1 for this one invocation, a terminal,
# the App Store Connect API key given as MIDGARD_ASC_KEY_ID and MIDGARD_ASC_ISSUER_ID
# (key file in ~/.appstoreconnect/private_keys; never read or printed here), and the
# typed word `upload` after the file, size, SHA-256 and signers are printed.
#
# --build <n>: CFBundleVersion (CURRENT_PROJECT_VERSION) for this archive. App Store
# Connect needs a higher build number for every upload of a version, so --app-store
# defaults it to the number of commits on HEAD (`git rev-list --count HEAD`, which only
# grows along the branch); the other modes keep the project's value unless it is given.
# CFBundleShortVersionString stays MARKETING_VERSION (0.1.0).
#
# --notarise (never the default; Developer ID only): the one step that sends anything to
# Apple. It runs only when MIDGARD_APPLE_UPLOAD_OK=1 is set for this one invocation, from
# a terminal, after printing exactly what will be sent and reading the word `upload`;
# it then runs `xcrun notarytool submit --keychain-profile <profile> --wait` (profile
# MIDGARD_NOTARY_PROFILE, default midgard-notary, stored once by the owner with
# `xcrun notarytool store-credentials`; the key is never read or printed here) and
# `xcrun stapler staple`. The flag is refused before anything is built when the variable
# is not set. Without it the script ends with "nothing was uploaded".
#
# Output: ~/Library/Caches/midgard-build/clients-archive/ (MIDGARD_BUILD_CACHE names
# another cache): MidgardMac-<release>-<mode>.xcarchive and mac-<mode>/. Derived data:
# clients-derived-mac-release. tools/clean.sh removes both.
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
app_dir="$here/Apps/MidgardMac"
proj="$app_dir/MidgardMac.xcodeproj"
scheme=MidgardMacMaps
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
release=""
mode=developer-id
notarise=0
upload=0
build=""
profile="${MIDGARD_NOTARY_PROFILE:-midgard-notary}"

say() { echo "release-mac: $*" >&2; }
stop() { say "$*"; say "nothing was uploaded."; exit 1; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release) release="${2:?--release needs a name}"; shift ;;
        --development) mode=development ;;
        --app-store) mode=app-store ;;
        --build) build="${2:?--build needs a number}"; shift ;;
        --notarise | --notarize) notarise=1 ;;
        --upload) upload=1 ;;
        -h | --help) sed -n '2,62p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) say "unknown argument: $1"; exit 2 ;;
    esac
    shift
done

# The release: the one sample-data release in the tree, unless named.
if [ -z "$release" ]; then
    release="$(ls "$here/../../demo/sample" 2>/dev/null | sort | tail -n 1)"
    [ -n "$release" ] || stop "no release under clients/demo/sample; pass --release <name>."
fi

# The build number: a positive integer; for the App Store, by default the commit count.
if [ -z "$build" ] && [ "$mode" = app-store ]; then
    build="$(git -C "$here" rev-list --count HEAD 2>/dev/null || true)"
    [ -n "$build" ] || stop "no build number: pass --build <n>."
fi
if [ -n "$build" ]; then
    [[ "$build" =~ ^[1-9][0-9]{0,8}$ ]] || stop "--build must be a positive integer, not $build."
fi

# The upload gates are checked before anything is built.
if [ "$upload" = 1 ]; then
    [ "$mode" = app-store ] || stop "--upload needs --app-store; only the App Store package is uploaded."
    [ "${MIDGARD_APPLE_UPLOAD_OK:-}" = 1 ] || stop "--upload refused: set MIDGARD_APPLE_UPLOAD_OK=1 for this one invocation to allow an upload to Apple."
    [ -n "${MIDGARD_ASC_KEY_ID:-}" ] && [ -n "${MIDGARD_ASC_ISSUER_ID:-}" ] \
        || stop "--upload refused: give the App Store Connect API key as MIDGARD_ASC_KEY_ID and MIDGARD_ASC_ISSUER_ID."
    [ -t 0 ] || stop "--upload refused: the typed confirmation needs a terminal."
fi
if [ "$notarise" = 1 ]; then
    [ "$mode" = developer-id ] || stop "--notarise needs the Developer ID build; a development build is never notarised."
    [ "${MIDGARD_APPLE_UPLOAD_OK:-}" = 1 ] || stop "--notarise refused: set MIDGARD_APPLE_UPLOAD_OK=1 for this one invocation to allow an upload to Apple."
    [ -t 0 ] || stop "--notarise refused: the typed confirmation needs a terminal."
fi

# Signing prerequisites: a team, and the identity this mode signs with.
team_set=0
grep -Eq '^[[:space:]]*MIDGARD_TEAM_ID[[:space:]]*=[[:space:]]*[A-Z0-9]{10}[[:space:]]*$' \
    "$app_dir/Config/Signing.local.xcconfig" 2>/dev/null && team_set=1
[ "$team_set" = 1 ] || stop "no team: put MIDGARD_TEAM_ID in $app_dir/Config/Signing.local.xcconfig (README, Signing). Nothing was built."
identities="$(security find-identity -v -p codesigning 2>/dev/null | sed -E 's/^ *[0-9]+\) [0-9A-F]{40} //')"
if [ "$mode" = developer-id ]; then
    if ! grep -q '"Developer ID Application: ' <<<"$identities"; then
        say "no Developer ID Application identity in this Mac's keychain (security find-identity -v -p codesigning)."
        say "The Account Holder (or an Admin allowed to) creates the certificate; then run this again."
        say "For a Release build signed for development now: release-mac.sh --development."
        stop "Nothing was built."
    fi
elif [ "$mode" = app-store ]; then
    missing=""
    grep -Eq '"(Apple Distribution|3rd Party Mac Developer Application): ' <<<"$identities" \
        || missing="an Apple Distribution (or 3rd Party Mac Developer Application) identity"
    # Installer identities are not code-signing identities: look in the basic policy.
    installers="$(security find-identity -v -p basic 2>/dev/null | sed -E 's/^ *[0-9]+\) [0-9A-F]{40} //')"
    grep -Eq '"(Mac Installer Distribution|3rd Party Mac Developer Installer): ' <<<"$installers" \
        || missing="${missing:+$missing and }a Mac Installer Distribution (or 3rd Party Mac Developer Installer) identity"
    # Missing local identities are fine with Xcode's cloud-managed distribution signing:
    # the export below signs through the Apple account signed in to Xcode (an Admin or the
    # Account Holder), and fails, saying so, when there is none.
    [ -z "$missing" ] \
        || say "not in this Mac's keychain: $missing; using Xcode's cloud-managed distribution signing (an Admin or the Account Holder signed in to Xcode)."
else
    grep -q '"Apple Development: ' <<<"$identities" \
        || stop "no Apple Development identity in this Mac's keychain. Nothing was built."
fi

# The cache must be writable (it may be a link to an external volume).
mkdir -p "$cache" 2>/dev/null && [ -w "$cache" ] \
    || stop "the build cache $cache is not writable (is its volume mounted?). Nothing was built."
out="$cache/clients-archive"
archive="$out/MidgardMac-$release-$mode.xcarchive"
export_dir="$out/mac-$mode"
derived="$cache/clients-derived-mac-release"
mkdir -p "$out"
rm -rf "$archive" "$export_dir"

say "archiving $scheme (Release, $mode${build:+, build $build}) for $release"
build_setting=()
[ -z "$build" ] || build_setting=("CURRENT_PROJECT_VERSION=$build")
if ! xcodebuild archive -project "$proj" -scheme "$scheme" -configuration Release \
    -destination 'generic/platform=macOS' -derivedDataPath "$derived" \
    -archivePath "$archive" -allowProvisioningUpdates -quiet ${build_setting[@]+"${build_setting[@]}"}; then
    stop "the archive failed."
fi
archived_app="$archive/Products/Applications/Midgard.app"
[ -d "$archived_app" ] || stop "the archive holds no Midgard.app."

# The archived app: its edition, and no bundled data.
bash "$here/tools/check-edition.sh" --maps --dsyms "$archive/dSYMs" "$archived_app" || stop "check-edition.sh failed on the archived app."
data="$(find "$archived_app" -type f \( -iname '*.mgdemo' -o -iname '*.jsonl' -o -iname '*.ndjson' -o -iname '*.csv' -o -iname '*.tsv' -o -iname '*.geojson' \))"
[ -z "$data" ] || stop "the archived app bundles data files: $data"

say "exporting ($mode)"
case "$mode" in
    developer-id) options="$app_dir/Config/ExportOptions-DeveloperID.plist" ;;
    app-store) options="$app_dir/Config/ExportOptions-AppStore.plist" ;;
    *) options="$app_dir/Config/ExportOptions-Development.plist" ;;
esac
if ! xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export_dir" \
    -exportOptionsPlist "$options" -allowProvisioningUpdates >"$out/mac-$mode-export.log" 2>&1; then
    tail -n 20 "$out/mac-$mode-export.log" >&2
    stop "the export failed (log $out/mac-$mode-export.log)."
fi
if [ "$mode" = app-store ]; then
    # The export is a signed installer package; its app is checked unpacked.
    pkg="$(find "$export_dir" -maxdepth 1 -name '*.pkg' | head -n 1)"
    [ -n "$pkg" ] || stop "the export holds no .pkg."
    mv "$pkg" "$export_dir/Midgard-$release.pkg"
    pkg="$export_dir/Midgard-$release.pkg"
    pkgutil --check-signature "$pkg" >&2 || stop "pkgutil --check-signature failed on $pkg."
    unpacked="$export_dir/unpacked"
    rm -rf "$unpacked"
    pkgutil --expand-full "$pkg" "$unpacked" >/dev/null || stop "the .pkg does not expand."
    app="$(find "$unpacked" -type d -name Midgard.app -prune | head -n 1)"
else
    app="$export_dir/Midgard.app"
fi
[ -d "$app" ] || stop "the export holds no Midgard.app."
codesign --verify --deep --strict "$app" || stop "codesign --verify --deep --strict failed on $app."
bash "$here/tools/check-edition.sh" --maps --dsyms "$archive/dSYMs" "$app" || stop "check-edition.sh failed on the exported app."
authority="$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"
say "exported app signed by: $authority"
say "version $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist") ($(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$app/Contents/Info.plist"))"

if [ "$mode" = app-store ]; then
    installer="$(pkgutil --check-signature "$pkg" 2>/dev/null | sed -n 's/^ *1\. //p' | head -n 1)"
    say "sizes:"
    du -sh "$archive" "$pkg" "$app" | sed 's/^/  /' >&2
    if [ "$upload" = 0 ]; then
        say "done: $pkg (unpacked app: $app)"
        say "nothing was uploaded (App Store Connect is release-mac.sh --app-store --upload, with MIDGARD_APPLE_UPLOAD_OK=1)."
        exit 0
    fi
    size="$(stat -f %z "$pkg")"
    sum="$(shasum -a 256 "$pkg" | awk '{ print $1 }')"
    say "about to send to App Store Connect:"
    say "  file       $pkg"
    say "  size       $size bytes"
    say "  sha256     $sum"
    say "  app        io.veldris.midgard, signed by: $authority"
    say "  installer  signed by: $installer"
    say "  how        xcrun altool --upload-package -t macos --apiKey \$MIDGARD_ASC_KEY_ID --apiIssuer \$MIDGARD_ASC_ISSUER_ID"
    printf 'release-mac: type upload to send it, anything else to stop: ' >&2
    read -r word || word=""
    [ "$word" = upload ] || stop "not confirmed."
    xcrun altool --upload-package "$pkg" -t macos --apiKey "$MIDGARD_ASC_KEY_ID" --apiIssuer "$MIDGARD_ASC_ISSUER_ID" \
        || { say "altool failed; check App Store Connect before trying again."; exit 1; }
    say "uploaded: $pkg"
    exit 0
fi

# The DMG: the app and a link to /Applications.
if [ "$mode" = developer-id ]; then dmg="$export_dir/Midgard-$release.dmg"; else dmg="$export_dir/Midgard-$release-development.dmg"; fi
stage="$export_dir/dmg-stage"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
ditto "$app" "$stage/Midgard.app"
ln -s /Applications "$stage/Applications"
if ! hdiutil create -quiet -volname "Midgard $release" -fs HFS+ -srcfolder "$stage" -format UDZO "$dmg"; then
    rm -rf "$stage"
    stop "hdiutil create failed."
fi
rm -rf "$stage"
if [ "$mode" = developer-id ]; then sign_id="Developer ID Application"; else sign_id="Apple Development"; fi
codesign --sign "$sign_id" --timestamp "$dmg" 2>/dev/null || codesign --sign "$sign_id" "$dmg" \
    || stop "signing the DMG failed."
codesign --verify --deep --strict "$dmg" || stop "codesign --verify --deep --strict failed on $dmg."
gate="$(spctl -a -t open --context context:primary-signature -vv "$dmg" 2>&1 || true)"
say "spctl (Gatekeeper) on the DMG: $(tr '\n' ' ' <<<"$gate")"
if [ "$mode" = development ]; then
    ditto -c -k --keepParent "$app" "$export_dir/Midgard-$release-development.zip"
fi

say "sizes:"
du -sh "$archive" "$app" "$dmg" "$export_dir"/*.zip 2>/dev/null | sed 's/^/  /' >&2

if [ "$notarise" = 0 ]; then
    say "done: $dmg"
    say "nothing was uploaded (notarisation is release-mac.sh --notarise, with MIDGARD_APPLE_UPLOAD_OK=1)."
    exit 0
fi

# Notarisation: the gate was checked above; now the typed confirmation.
size="$(stat -f %z "$dmg")"
sum="$(shasum -a 256 "$dmg" | awk '{ print $1 }')"
say "about to send to Apple's notary service:"
say "  file    $dmg"
say "  size    $size bytes"
say "  sha256  $sum"
say "  app     io.veldris.midgard, signed by: $authority"
say "  how     xcrun notarytool submit --keychain-profile $profile --wait, then xcrun stapler staple"
printf 'release-mac: type upload to send it, anything else to stop: ' >&2
read -r word || word=""
[ "$word" = upload ] || stop "not confirmed."
xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait || { say "notarytool failed; the DMG was sent but not accepted."; exit 1; }
xcrun stapler staple "$dmg" || { say "stapling failed."; exit 1; }
spctl -a -t open --context context:primary-signature -vv "$dmg" >&2 || true
say "notarised and stapled: $dmg"
