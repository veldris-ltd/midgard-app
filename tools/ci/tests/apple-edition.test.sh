#!/usr/bin/env bash
# clients/shells/apple/tools/check-edition.sh, against bundles made here, for the one
# edition, Maps (owner decision, 23 Nov 2026). Maps bundles as required pass: on the Mac
# sandboxed, with the edition library and the client network entitlement; on the iPhone
# with the edition library. Each seeded failure fails naming its reason: no edition or
# another one, no strong hook (the edition library not linked, or only the weak default),
# a Mac app without the sandbox or without the client entitlement, the server, a
# developer networking or a temporary-exception entitlement. A tree with no app fails
# closed. The bundles are built with cc and signed ad hoc with codesign, so the bundle
# checks run on macOS only; elsewhere they say SKIP. [FR-C-1] [DD-6]
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

# A tree with no app: nothing was checked, so it fails closed.
mkdir -p "$work/tree/clients/shells/apple/tools"
cp "$script" "$work/tree/clients/shells/apple/tools/"
check "no app: fails closed, saying so" \
    "! out=\$(bash $work/tree/clients/shells/apple/tools/check-edition.sh 2>&1) && grep -q 'no app' <<<\"\$out\""
check "an unknown option is refused" "! bash $script --nonsense >/dev/null 2>&1"
check "--maps with no bundle is refused" "! bash $script --maps >/dev/null 2>&1"
check "a retired edition option is refused" "! bash $script --release /nonexistent.app >/dev/null 2>&1"

if [ "$(uname -s)" != Darwin ] || ! command -v cc >/dev/null 2>&1 || ! command -v codesign >/dev/null 2>&1; then
    echo "SKIP  the bundle checks need macOS (cc, codesign, plutil)"
    [ "$failures" -eq 0 ] || exit 1
    exit 0
fi

# make_app <name> <layout mac|ios> <edition or -> <C source> <entitlements or ->
# (CC_EXTRA: extra cc flags; PLIST_EXTRA: extra Info.plist XML)
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
    # shellcheck disable=SC2086
    cc ${CC_EXTRA:-} -o "$exe" "$work/$name.c" || return 1
    {
        printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict>\n'
        printf '<key>CFBundleIdentifier</key><string>test.edition.%s</string>\n' "$name"
        printf '<key>CFBundleExecutable</key><string>%s</string>\n' "$name"
        [ "$edition" = - ] || printf '<key>MidgardEdition</key><string>%s</string>\n' "$edition"
        printf '%s\n' "${PLIST_EXTRA:-}"
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

# The app links the edition library, whose strong midgard_edition_hook replaces the weak
# default in MidgardHooks.
globe='const void *midgard_edition_hook(void) { return 0; } void MidgardGlobeKit_GlobeRegistration(void) {} int main(void) { MidgardGlobeKit_GlobeRegistration(); return midgard_edition_hook() != 0; }'
weakhook='__attribute__((weak)) const void *midgard_edition_hook(void) { return 0; } int main(void) { return midgard_edition_hook() != 0; }'
temp='<key>com.apple.security.temporary-exception.files.absolute-path.read-only</key><array><string>/</string></array>'
wifi='<key>com.apple.developer.networking.wifi-info</key><true/>'

# passes <name>: check-edition.sh passes the bundle.
passes() { bash "$script" "$work/$1.app" >"$work/$1.out" 2>&1; }
# fails <name> <reason>: it fails the bundle, and its output says <reason>.
fails() { ! bash "$script" "$work/$1.app" >"$work/$1.out" 2>&1 && grep -q "$2" "$work/$1.out"; }

make_app Maps mac maps "$globe" "$sandbox$client" && check "a Mac bundle (globe, sandbox, client network) passes" "passes Maps"
make_app MapsFlat ios maps "$globe" - && check "an iPhone bundle (iOS has no network entitlement) passes" "passes MapsFlat"
check "--maps before the bundles is accepted" "bash $script --maps $work/Maps.app >/dev/null 2>&1"
make_app MapsTiles mac maps "void MidgardTileKit_loader(void) {} $globe" "$sandbox$client"
check "the app may carry MidgardTileKit (the tile loader)" "passes MapsTiles"

make_app SeedNoEdition ios - "$globe" -
check "seeded: no edition fails" "fails SeedNoEdition 'MidgardEdition'"
for other in release 3d; do
    make_app "Seed$other" mac "$other" "$globe" "$sandbox$client"
    check "seeded: an app saying edition $other fails" "fails Seed$other 'MidgardEdition'"
done
make_app SeedNoGlobe mac maps "$clean" "$sandbox$client"
check "seeded: an app without the edition library fails" "fails SeedNoGlobe 'defines midgard_edition_hook strongly'"
make_app SeedWeakHook mac maps "$weakhook" "$sandbox$client"
check "seeded: an app with the weak hook only fails" "fails SeedWeakHook 'defines midgard_edition_hook strongly'"
make_app SeedNoNet mac maps "$globe" "$sandbox"
check "seeded: a Mac app without the client entitlement fails" "fails SeedNoNet 'network client'"
make_app SeedServer mac maps "$globe" "$sandbox$client$server"
check "seeded: the server entitlement fails" "fails SeedServer 'beyond the network client'"
make_app SeedFlatServer ios maps "$globe" "$server"
check "seeded: the server entitlement fails on the iPhone too" "fails SeedFlatServer 'beyond the network client'"
make_app SeedWifi mac maps "$globe" "$sandbox$client$wifi"
check "seeded: a com.apple.developer.networking entitlement fails" "fails SeedWifi 'beyond the network client'"
make_app SeedTemp mac maps "$globe" "$sandbox$client$temp"
check "seeded: a temporary-exception entitlement fails" "fails SeedTemp 'beyond the network client'"
make_app SeedNoSandbox mac maps "$globe" "$client"
check "seeded: a Mac app without the sandbox fails" "fails SeedNoSandbox 'not sandboxed'"
check "both apps in one run pass together" "bash $script $work/Maps.app $work/MapsFlat.app >/dev/null 2>&1"
check "one failing bundle among passing ones fails the run" \
    "! bash $script $work/Maps.app $work/SeedServer.app >/dev/null 2>&1"
check "a path that is not a bundle fails" "! bash $script $work/missing.app >/dev/null 2>&1"
[ "$failures" -eq 0 ] || exit 1
