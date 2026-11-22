#!/usr/bin/env bash
# clients/shells/apple/tools/check-edition.sh, against bundles made here (Task B7), for
# the three editions. Bundles shaped as each edition requires pass: a release app with no
# globe and no network entitlement, a 3D app with the globe and no network entitlement, a
# maps app with the globe and the client network entitlement (on the Mac). Each seeded
# failure fails naming its reason: in a release app a MidgardGlobeKit symbol, a Globe type,
# a MidgardGlobeKit resource bundle or a network entitlement; a 3D app with a network
# entitlement or without the globe; a Mac maps app without the client entitlement or with
# the server one; and any bundle whose MidgardEdition is not the one it is checked as. A
# tree with no app fails closed. The bundles are built with cc and signed ad hoc with codesign, so the
# bundle checks run on macOS only; elsewhere they say SKIP. [FR-C-1] [DD-6]
#
#   bash tools/ci/tests/apple-edition.test.sh
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
script="$root/clients/shells/apple/tools/check-edition.sh"
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "parses under /bin/bash" "/bin/bash -n $0"
check "no bash-4 builtins" "! grep -Eq 'map''file|read''array|declare -''A' $0"
check "check-edition.sh parses under /bin/bash" "/bin/bash -n $script"
check "check-edition.sh has no bash-4 builtins" "! grep -Eq 'map''file|read''array|declare -''A' $script"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# A tree with no release app: nothing was checked, so it fails closed.
mkdir -p "$work/tree/clients/shells/apple/tools"
cp "$script" "$work/tree/clients/shells/apple/tools/"
check "no app: fails closed, saying so" \
    "! out=\$(bash $work/tree/clients/shells/apple/tools/check-edition.sh 2>&1) && grep -q 'no app' <<<\"\$out\""
check "an unknown option is refused" "! bash $script --nonsense >/dev/null 2>&1"
for opt in --release --3d --maps; do
    check "$opt with no bundle is refused" "! bash $script $opt >/dev/null 2>&1"
done

if [ "$(uname -s)" != Darwin ] || ! command -v cc >/dev/null 2>&1 || ! command -v codesign >/dev/null 2>&1; then
    echo "SKIP  the bundle checks need macOS (cc, codesign, plutil)"
    [ "$failures" -eq 0 ] || exit 1
    exit 0
fi

# make_app <name> <layout mac|ios> <edition or -> <C source> <entitlements or ->
make_app() {
    local name="$1" layout="$2" edition="$3" source="$4" ents="$5" app info exe
    app="$work/$name.app"
    rm -rf "$app"
    if [ "$layout" = mac ]; then
        mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
        info="$app/Contents/Info.plist"; exe="$app/Contents/MacOS/$name"
    else
        mkdir -p "$app"
        info="$app/Info.plist"; exe="$app/$name"
    fi
    printf '%s\n' "$source" >"$work/$name.c"
    cc -o "$exe" "$work/$name.c" || return 1
    {
        printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict>\n'
        printf '<key>CFBundleIdentifier</key><string>test.edition.%s</string>\n' "$name"
        printf '<key>CFBundleExecutable</key><string>%s</string>\n' "$name"
        [ "$edition" = - ] || printf '<key>MidgardEdition</key><string>%s</string>\n' "$edition"
        printf '</dict></plist>\n'
    } >"$info"
    if [ "$ents" = - ]; then
        codesign --force --sign - "$app" >/dev/null 2>&1 || return 1
    else
        printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' \
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
            "<plist version=\"1.0\"><dict>$ents</dict></plist>" >"$work/$name.entitlements"
        codesign --force --sign - --entitlements "$work/$name.entitlements" "$app" >/dev/null 2>&1 || return 1
    fi
}

clean='int main(void) { return 0; }'
sandbox='<key>com.apple.security.app-sandbox</key><true/>'
client='<key>com.apple.security.network.client</key><true/>'
server='<key>com.apple.security.network.server</key><true/>'

globe='void MidgardGlobeKit_GlobeRegistration(void) {} int main(void) { MidgardGlobeKit_GlobeRegistration(); return 0; }'

# passes <edition> <name>: check-edition.sh passes the bundle checked as <edition>.
passes() { bash "$script" "--$1" "$work/$2.app" >"$work/$2.out" 2>&1; }
# fails <edition> <name> <reason>: it fails the bundle, and its output says <reason>.
fails() { ! bash "$script" "--$1" "$work/$2.app" >"$work/$2.out" 2>&1 && grep -q "$3" "$work/$2.out"; }
# For the release seeds below, the edition is release.
fails_release() { fails release "$@"; }

make_app Release mac release "$clean" "$sandbox" && check "a release Mac bundle passes" "passes release Release"
make_app ReleaseFlat ios release "$clean" - && check "a release iPhone bundle passes" "passes release ReleaseFlat"
make_app ThreeD mac 3d "$globe" "$sandbox" && check "a 3D Mac bundle (globe, no network) passes" "passes 3d ThreeD"
make_app ThreeDFlat ios 3d "$globe" - && check "a 3D iPhone bundle passes" "passes 3d ThreeDFlat"
make_app Maps mac maps "$globe" "$sandbox$client" && check "a maps Mac bundle (globe, client network) passes" "passes maps Maps"
make_app MapsFlat ios maps "$globe" - && check "a maps iPhone bundle (iOS has no network entitlement) passes" "passes maps MapsFlat"

make_app SeedSymbol mac release 'void MidgardGlobeKit_seed(void) {} int main(void) { MidgardGlobeKit_seed(); return 0; }' "$sandbox"
check "seeded: a MidgardGlobeKit symbol in a release app fails" "fails_release SeedSymbol 'MidgardGlobeKit'"
make_app SeedType ios release 'void GlobeView_seed(void) {} int main(void) { GlobeView_seed(); return 0; }' -
check "seeded: a Globe type in a release app fails" "fails_release SeedType 'Globe'"
make_app SeedResource mac release "$clean" "$sandbox" &&
    mkdir -p "$work/SeedResource.app/Contents/Resources/MidgardKit_MidgardGlobeKit.bundle" &&
    codesign --force --sign - --entitlements "$work/SeedResource.entitlements" "$work/SeedResource.app" >/dev/null 2>&1
check "seeded: a MidgardGlobeKit resource bundle fails" "fails_release SeedResource 'MidgardGlobeKit'"
make_app SeedClient mac release "$clean" "$sandbox$client"
check "seeded: the network client entitlement in a release app fails" "fails_release SeedClient 'network'"
make_app SeedServer mac release "$clean" "$sandbox$server"
check "seeded: the network server entitlement in a release app fails" "fails_release SeedServer 'network'"
make_app SeedEdition mac 3d "$clean" "$sandbox"
check "seeded: a release app saying edition 3d fails" "fails_release SeedEdition 'MidgardEdition'"
make_app SeedNoEdition ios - "$clean" -
check "seeded: no edition fails" "fails_release SeedNoEdition 'MidgardEdition'"
make_app Seed3DClient mac 3d "$globe" "$sandbox$client"
check "seeded: a 3D app with the network client entitlement fails" "fails 3d Seed3DClient 'network'"
make_app Seed3DServer mac 3d "$globe" "$sandbox$server"
check "seeded: a 3D app with the network server entitlement fails" "fails 3d Seed3DServer 'network'"
make_app Seed3DNoGlobe mac 3d "$clean" "$sandbox"
check "seeded: a 3D app without the globe fails" "fails 3d Seed3DNoGlobe 'MidgardGlobeKit'"
make_app SeedMapsNoNet mac maps "$globe" "$sandbox"
check "seeded: a Mac maps app without the client entitlement fails" "fails maps SeedMapsNoNet 'network client'"
make_app SeedMapsServer mac maps "$globe" "$sandbox$client$server"
check "seeded: a maps app with the server entitlement fails" "fails maps SeedMapsServer 'server'"
make_app SeedMapsNoGlobe mac maps "$clean" "$sandbox$client"
check "seeded: a maps app without the globe fails" "fails maps SeedMapsNoGlobe 'MidgardGlobeKit'"
check "seeded: a 3D app checked as maps fails on its edition" "fails maps ThreeD 'MidgardEdition'"
check "seeded: a maps app checked as 3D fails" "fails 3d Maps 'network'"
check "seeded: a maps app checked as release fails" "fails release Maps 'MidgardGlobeKit'"
check "several editions in one run pass together" \
    "bash $script --release $work/Release.app $work/ReleaseFlat.app --3d $work/ThreeD.app --maps $work/Maps.app >/dev/null 2>&1"
check "one failing bundle among passing ones fails the run" \
    "! bash $script --release $work/Release.app $work/SeedClient.app --3d $work/ThreeD.app >/dev/null 2>&1"
check "a path that is not a bundle fails" "! bash $script --release $work/missing.app >/dev/null 2>&1"
[ "$failures" -eq 0 ] || exit 1
