#!/usr/bin/env bash
# Release build of the iPhone app (Task B5): MidgardiOSMaps, io.veldris.midgardmaps.ios,
# shown as "Midgard". [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/release-ios.sh [--release <name>] [--development] [--upload]
#
# Default (App Store Connect export): archives Release for generic iOS with automatic
# signing under the team in Config/Signing.local.xcconfig and exports an .ipa with method
# app-store-connect and destination export (Config/ExportOptions-AppStore.plist): the
# .ipa is written locally and nothing is sent. It stops before that when this Mac has no
# Apple Distribution identity, and says that nothing was built or uploaded.
#
# --development: the same Release archive, exported with the Apple Development identity
# (method debugging, Config/ExportOptions-Development.plist): an .ipa that installs on
# this team's registered devices, for example with
#   xcrun devicectl device install app --device <id> <ipa or app>
# For trying a real Release build on a phone before a distribution certificate exists;
# never uploaded.
#
# Before the export, tools/check-edition.sh checks the archived app (B7 review notes),
# and the app must hold no data file: the sample data is compiled into the core
# (MidgardDataSource = sample), so no .mgdemo or other dataset is bundled. The exported
# .ipa's app is unpacked and checked again (codesign --verify --deep --strict, and
# check-edition.sh).
#
# --upload (never the default; App Store Connect export only): the one step that sends
# anything to Apple (TestFlight). It runs only when MIDGARD_APPLE_UPLOAD_OK=1 is set for
# this one invocation, from a terminal, with the App Store Connect API key the owner gives
# through the environment at that moment (MIDGARD_ASC_KEY_ID and MIDGARD_ASC_ISSUER_ID;
# the key file in altool's usual place, ~/.appstoreconnect/private_keys; never read or
# printed here), after printing exactly what will be sent and reading the word `upload`.
# It then runs `xcrun altool --upload-app`. The flag is refused before anything is built
# when the variable is not set. Without it the script ends with "nothing was uploaded".
# App Store submission is out of scope.
#
# Output: ~/Library/Caches/midgard-build/clients-archive/ (MIDGARD_BUILD_CACHE names
# another cache): MidgardiOS-<release>-<mode>.xcarchive and ios-<mode>/. Derived data:
# clients-derived-ios-release. tools/clean.sh removes both.
set -uo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
app_dir="$here/Apps/MidgardiOS"
proj="$app_dir/MidgardiOS.xcodeproj"
scheme=MidgardiOSMaps
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
release=""
mode=app-store
upload=0

say() { echo "release-ios: $*" >&2; }
stop() { say "$*"; say "nothing was uploaded."; exit 1; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release) release="${2:?--release needs a name}"; shift ;;
        --development) mode=development ;;
        --upload) upload=1 ;;
        -h | --help) sed -n '2,41p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) say "unknown argument: $1"; exit 2 ;;
    esac
    shift
done

if [ -z "$release" ]; then
    release="$(ls "$here/../../demo/sample" 2>/dev/null | sort | tail -n 1)"
    [ -n "$release" ] || stop "no release under clients/demo/sample; pass --release <name>."
fi

# The upload gate is checked before anything is built.
if [ "$upload" = 1 ]; then
    [ "$mode" = app-store ] || stop "--upload needs the App Store Connect export; a development build is never uploaded."
    [ "${MIDGARD_APPLE_UPLOAD_OK:-}" = 1 ] || stop "--upload refused: set MIDGARD_APPLE_UPLOAD_OK=1 for this one invocation to allow an upload to Apple."
    [ -n "${MIDGARD_ASC_KEY_ID:-}" ] && [ -n "${MIDGARD_ASC_ISSUER_ID:-}" ] \
        || stop "--upload refused: give the App Store Connect API key as MIDGARD_ASC_KEY_ID and MIDGARD_ASC_ISSUER_ID."
    [ -t 0 ] || stop "--upload refused: the typed confirmation needs a terminal."
fi

team_set=0
grep -Eq '^[[:space:]]*MIDGARD_TEAM_ID[[:space:]]*=[[:space:]]*[A-Z0-9]{10}[[:space:]]*$' \
    "$app_dir/Config/Signing.local.xcconfig" 2>/dev/null && team_set=1
[ "$team_set" = 1 ] || stop "no team: put MIDGARD_TEAM_ID in $app_dir/Config/Signing.local.xcconfig (README, Signing). Nothing was built."
identities="$(security find-identity -v -p codesigning 2>/dev/null | sed -E 's/^ *[0-9]+\) [0-9A-F]{40} //')"
if [ "$mode" = app-store ]; then
    if ! grep -Eq '"(Apple|iPhone) Distribution: ' <<<"$identities"; then
        say "no Apple Distribution identity in this Mac's keychain (security find-identity -v -p codesigning)."
        say "The Account Holder or an Admin creates the certificate; then run this again."
        say "For a Release build signed for development now: release-ios.sh --development."
        stop "Nothing was built."
    fi
else
    grep -q '"Apple Development: ' <<<"$identities" \
        || stop "no Apple Development identity in this Mac's keychain. Nothing was built."
fi

mkdir -p "$cache" 2>/dev/null && [ -w "$cache" ] \
    || stop "the build cache $cache is not writable (is its volume mounted?). Nothing was built."
out="$cache/clients-archive"
archive="$out/MidgardiOS-$release-$mode.xcarchive"
export_dir="$out/ios-$mode"
derived="$cache/clients-derived-ios-release"
mkdir -p "$out"
rm -rf "$archive" "$export_dir"

say "archiving $scheme (Release, $mode) for $release"
if ! xcodebuild archive -project "$proj" -scheme "$scheme" -configuration Release \
    -destination 'generic/platform=iOS' -derivedDataPath "$derived" \
    -archivePath "$archive" -allowProvisioningUpdates -quiet; then
    stop "the archive failed."
fi
archived_app="$archive/Products/Applications/$scheme.app"
[ -d "$archived_app" ] || stop "the archive holds no $scheme.app."

check_app() { # check_app <app> <what>
    bash "$here/tools/check-edition.sh" --maps --dsyms "$archive/dSYMs" "$1" || stop "check-edition.sh failed on the $2 app."
    local data
    data="$(find "$1" -type f \( -iname '*.mgdemo' -o -iname '*.jsonl' -o -iname '*.ndjson' -o -iname '*.csv' -o -iname '*.tsv' -o -iname '*.geojson' \))"
    [ -z "$data" ] || stop "the $2 app bundles data files: $data"
}
check_app "$archived_app" archived

say "exporting ($mode)"
if [ "$mode" = app-store ]; then options="$app_dir/Config/ExportOptions-AppStore.plist"; else options="$app_dir/Config/ExportOptions-Development.plist"; fi
if ! xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export_dir" \
    -exportOptionsPlist "$options" -allowProvisioningUpdates >"$out/ios-$mode-export.log" 2>&1; then
    tail -n 20 "$out/ios-$mode-export.log" >&2
    stop "the export failed (log $out/ios-$mode-export.log)."
fi
ipa="$(find "$export_dir" -maxdepth 1 -name '*.ipa' | head -n 1)"
[ -n "$ipa" ] || stop "the export holds no .ipa."
if [ "$mode" = development ]; then
    mv "$ipa" "$export_dir/Midgard-$release-development.ipa"
    ipa="$export_dir/Midgard-$release-development.ipa"
else
    mv "$ipa" "$export_dir/Midgard-$release.ipa"
    ipa="$export_dir/Midgard-$release.ipa"
fi

# The .ipa's app, unpacked: signature and edition.
unpacked="$export_dir/unpacked"
rm -rf "$unpacked"
mkdir -p "$unpacked"
ditto -x -k "$ipa" "$unpacked" || stop "the .ipa does not unpack."
app="$unpacked/Payload/$scheme.app"
[ -d "$app" ] || stop "the .ipa holds no Payload/$scheme.app."
codesign --verify --deep --strict "$app" || stop "codesign --verify --deep --strict failed on the .ipa's app."
check_app "$app" exported
authority="$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"
say "exported app signed by: $authority"

say "sizes:"
du -sh "$archive" "$ipa" "$app" | sed 's/^/  /' >&2

if [ "$upload" = 0 ]; then
    say "done: $ipa (unpacked app: $app)"
    say "nothing was uploaded (TestFlight is release-ios.sh --upload, with MIDGARD_APPLE_UPLOAD_OK=1)."
    exit 0
fi

size="$(stat -f %z "$ipa")"
sum="$(shasum -a 256 "$ipa" | awk '{ print $1 }')"
say "about to send to App Store Connect (TestFlight):"
say "  file    $ipa"
say "  size    $size bytes"
say "  sha256  $sum"
say "  app     io.veldris.midgardmaps.ios, signed by: $authority"
say "  how     xcrun altool --upload-app -t ios --apiKey \$MIDGARD_ASC_KEY_ID --apiIssuer \$MIDGARD_ASC_ISSUER_ID"
printf 'release-ios: type upload to send it, anything else to stop: ' >&2
read -r word || word=""
[ "$word" = upload ] || stop "not confirmed."
xcrun altool --upload-app -f "$ipa" -t ios --apiKey "$MIDGARD_ASC_KEY_ID" --apiIssuer "$MIDGARD_ASC_ISSUER_ID" \
    || { say "altool failed; check App Store Connect before trying again."; exit 1; }
say "uploaded: $ipa"
