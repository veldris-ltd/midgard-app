#!/usr/bin/env bash
# The committed Xcode projects of the Apple apps are what their XcodeGen specs generate:
# for each clients/shells/apple/Apps/*/project.yml, the project is regenerated in a
# scratch copy of the package made of its tracked files only (as a fresh checkout has it:
# no build outputs, no Signing.local.xcconfig, no per-user Xcode state, and the committed
# project removed first), and its project.pbxproj and shared schemes must equal the
# committed <App>.xcodeproj's. An edit made in Xcode and not in project.yml fails here,
# naming the app. The copy keeps the package directory's name ("apple"): XcodeGen names
# a local package's folder reference after it. Each project also declares all three editions
# (Task B7): the release, 3D and maps apps, each with its own scheme, UI tests, Info.plist
# and entitlements, and only the 3D and maps apps linking MidgardGlobeKit (check_editions).
# macOS only (XcodeGen, jq, plutil): elsewhere it says SKIP. [DD-6]
#
#   bash tools/ci/tests/apple-projects.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
apple=clients/shells/apple
fixture=tools/ci/tests/fixtures/apple-projects/apple
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "parses under /bin/bash" "/bin/bash -n $0"
check "no bash-4 builtins" "! grep -Eq 'map''file|read''array|declare -''A' $0"

if ! command -v xcodegen >/dev/null 2>&1; then
    echo "SKIP  XcodeGen is not installed (macOS-only check)"
    [ "$failures" -eq 0 ] || exit 1
    exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
ncopy=0

# compare_projects <package dir, in a git work tree>: prints one line per app,
# "FAIL  <app>: ..." on a difference; returns the number of apps that differ.
compare_projects() {
    local base="$1" n=0 bad=0 spec app rel copy gen com
    for spec in "$base"/Apps/*/project.yml; do
        [ -f "$spec" ] || continue
        n=$((n + 1))
        app="$(basename "$(dirname "$spec")")"
        rel="Apps/$app"
        com="$base/$rel/$app.xcodeproj"
        ncopy=$((ncopy + 1))
        copy="$tmp/copy-$ncopy/$(basename "$(cd "$base" && pwd)")"
        mkdir -p "$copy"
        # Tracked files only, NUL-separated, from the working tree.
        if ! git -C "$base" ls-files -z -- . | tar -C "$base" --null -T - -cf - | tar -C "$copy" -xf -; then
            echo "FAIL  $app: could not copy the tracked files of $base"
            bad=$((bad + 1))
            continue
        fi
        rm -rf "$copy/$rel/$app.xcodeproj"
        if ! xcodegen generate -q --spec "$copy/$rel/project.yml" >"$tmp/log" 2>&1; then
            echo "FAIL  $app: xcodegen generate failed: $(tr '\n' ' ' <"$tmp/log")"
            bad=$((bad + 1))
            continue
        fi
        gen="$copy/$rel/$app.xcodeproj"
        : >"$tmp/diff"
        if [ ! -f "$com/project.pbxproj" ]; then
            echo "FAIL  $app: $app.xcodeproj is not committed beside project.yml"
            bad=$((bad + 1))
            continue
        fi
        diff -u "$com/project.pbxproj" "$gen/project.pbxproj" >>"$tmp/diff" 2>&1
        # Shared schemes: compared when either side has them.
        if [ -e "$com/xcshareddata" ] || [ -e "$gen/xcshareddata" ]; then
            diff -ru "$com/xcshareddata" "$gen/xcshareddata" >>"$tmp/diff" 2>&1
        fi
        if [ -s "$tmp/diff" ]; then
            echo "FAIL  $app: $app.xcodeproj differs from what project.yml generates (run xcodegen generate in $rel):"
            head -n 40 "$tmp/diff" | sed 's/^/      /'
            bad=$((bad + 1))
        else
            echo "ok    $app: $app.xcodeproj is what project.yml generates"
        fi
    done
    [ "$n" -gt 0 ] || echo "ok    no app projects"
    return "$bad"
}

compare_projects "$apple"
failures=$((failures + $?))

# The check's own failure path, on the fixture (an app with a local package, as the real
# apps have): as committed it passes; edited as Xcode would edit it, it fails naming the
# app. Each variant is a scratch git repository whose package directory is named "apple".
variant() {
    rm -rf "$tmp/v" && mkdir -p "$tmp/v" && cp -R "$fixture" "$tmp/v/apple"
}
track() { git -C "$tmp/v" init -q && git -C "$tmp/v" add -A; }
check "fixture: a committed project equal to its spec passes" "compare_projects $fixture >/dev/null"
check "fixture: the package's folder reference is named apple" "grep -q 'name = apple; path = ../..;' $fixture/Apps/Fixture/Fixture.xcodeproj/project.pbxproj"
variant && mv "$tmp/v/apple" "$tmp/v/other" && track
check "fixture: a copy under another directory name than apple differs (the reason for the name)" \
    "! out=\$(compare_projects $tmp/v/other) && grep -q '^FAIL  Fixture: Fixture.xcodeproj differs' <<<\"\$out\" && grep -q 'name = other' <<<\"\$out\""
variant
p="$tmp/v/apple/Apps/Fixture/Fixture.xcodeproj/project.pbxproj"
sed 's/MACOSX_DEPLOYMENT_TARGET = 14.0;/MACOSX_DEPLOYMENT_TARGET = 13.0;/' "$p" >"$tmp/p" && mv "$tmp/p" "$p"
track
check "fixture: the edit took" "grep -q 'MACOSX_DEPLOYMENT_TARGET = 13.0;' $p"
check "fixture: an edited project.pbxproj fails, naming the app" \
    "! out=\$(compare_projects $tmp/v/apple) && grep -q '^FAIL  Fixture: Fixture.xcodeproj differs' <<<\"\$out\""
variant
rm -f "$tmp/v/apple/Apps/Fixture/Fixture.xcodeproj/xcshareddata/xcschemes/Fixture.xcscheme"
track
check "fixture: a missing shared scheme fails" "! compare_projects $tmp/v/apple >/dev/null"
variant
mkdir -p "$tmp/v/apple/Apps/Fixture/Fixture.xcodeproj/xcshareddata/xcschemes"
cp "$fixture/Apps/Fixture/Fixture.xcodeproj/xcshareddata/xcschemes/Fixture.xcscheme" \
    "$tmp/v/apple/Apps/Fixture/Fixture.xcodeproj/xcshareddata/xcschemes/Stale.xcscheme"
track
check "fixture: a committed scheme the spec no longer makes fails" "! compare_projects $tmp/v/apple >/dev/null"
variant
rm -rf "$tmp/v/apple/Apps/Fixture/Fixture.xcodeproj"
track
check "fixture: a spec without a committed project fails" \
    "! out=\$(compare_projects $tmp/v/apple) && grep -q 'is not committed' <<<\"\$out\""
variant
echo "BROKEN = 1" >"$tmp/v/apple/Apps/Fixture/Untracked.xcconfig"
track
git -C "$tmp/v" rm -q --cached apple/Apps/Fixture/Untracked.xcconfig
awk '{ print } /^name: Fixture$/ { print "configFiles:"; print "  Debug: Untracked.xcconfig" }' \
    "$tmp/v/apple/Apps/Fixture/project.yml" >"$tmp/y" &&
    mv "$tmp/y" "$tmp/v/apple/Apps/Fixture/project.yml" && git -C "$tmp/v" add apple/Apps/Fixture/project.yml
check "fixture: only tracked files are copied (a spec needing an untracked file fails)" \
    "! out=\$(compare_projects $tmp/v/apple) && grep -q 'xcodegen generate failed' <<<\"\$out\""
mkdir -p "$tmp/empty" && git -C "$tmp/empty" init -q
check "no apps: passes, saying so" "out=\$(compare_projects $tmp/empty) && grep -q 'no app projects' <<<\"\$out\""

# All three editions in every project (Task B7, owner decision 22 Nov 2026). Each
# Apps/<App>/project.yml declares the release app <App>, the 3D app <App>3D and the maps
# app <App>Maps, each an application for the same platform with its own scheme, its own
# UI-test target (<App>UITests, <App>3DUITests, <App>MapsUITests) that links
# MidgardAuditKit, its own Info.plist (MidgardEdition release, 3d, maps) and its own bundle
# identifier (io.veldris.midgard.*, io.veldris.midgard3d.*, io.veldris.midgardmaps.*).
# Only <App>3D and <App>Maps link MidgardGlobeKit. Entitlement files: only a maps one
# (on the Mac) names a network entitlement, the client one and never the server one; each
# 3D and maps target names its own entitlement files. tools/check-edition.sh checks what
# the builds actually carry.
#
# check_editions <package dir>: one line per app; returns the number of apps that fail.
check_editions() {
    local base="$1" n=0 bad=0 spec dir app json problems platform plist want f pair
    for spec in "$base"/Apps/*/project.yml; do
        [ -f "$spec" ] || continue
        n=$((n + 1))
        dir="$(dirname "$spec")"
        app="$(basename "$dir")"
        if ! json="$(cd "$dir" && xcodegen dump --type json --spec project.yml 2>"$tmp/elog")"; then
            echo "FAIL  $app: xcodegen dump failed: $(tr '\n' ' ' <"$tmp/elog")"
            bad=$((bad + 1))
            continue
        fi
        problems="$(jq -r --arg a "$app" '
            . as $root |
            def t($n): $root.targets[$n] // {};
            def links($n; $p): ((t($n).dependencies // []) | map(select(.product == $p)) | length) > 0;
            def bid($n): (t($n).settings.base.PRODUCT_BUNDLE_IDENTIFIER // "");
            def ents($n): [t($n).settings.base // {} | to_entries[] | select(.key | test("ENTITLEMENTS"))
                | .value | tostring | select(test("[.]entitlements"))];
            [$a, $a + "3D", $a + "Maps"] as $apps |
            ($apps[] | select(t(.).type != "application") | "no application target \(.)"),
            ($apps[] | select(t(.).platform != t($a).platform) | "\(.) is not for the platform of \($a)"),
            ($apps[] | select(t(. + "UITests").settings.base.TEST_TARGET_NAME != .) | "no UI-test target \(.)UITests for \(.)"),
            ($apps[] | select(links(. + "UITests"; "MidgardAuditKit") | not) | "\(.)UITests does not link MidgardAuditKit"),
            ($apps[] | select(($root.schemes // {})[.] == null) | "no scheme \(.)"),
            ($apps[1:][] | select(links(.; "MidgardGlobeKit") | not) | "\(.) does not link MidgardGlobeKit"),
            (($root.targets // {}) | to_entries[] | select(.key as $k | $apps[1:] | index($k) | not)
                | select(((.value.dependencies // []) | map(select(.product == "MidgardGlobeKit")) | length) > 0)
                | "\(.key) links MidgardGlobeKit (only \($a)3D and \($a)Maps may)"),
            (if bid($a) | test("^io[.]veldris[.]midgard[.]") | not then "\($a) bundle id \(bid($a)) is not io.veldris.midgard.*" else empty end),
            (if bid($a + "3D") | test("^io[.]veldris[.]midgard3d[.]") | not then "\($a)3D bundle id \(bid($a + "3D")) is not io.veldris.midgard3d.*" else empty end),
            (if bid($a + "Maps") | test("^io[.]veldris[.]midgardmaps[.]") | not then "\($a)Maps bundle id \(bid($a + "Maps")) is not io.veldris.midgardmaps.*" else empty end),
            (if ([$apps[] | t(.).settings.base.INFOPLIST_FILE // ""] | unique | length) != 3
                then "the three apps do not each have their own Info.plist" else empty end),
            (if (ents($a + "3D") | map(select(test("3D[^/]*[.]entitlements"))) | length) == 0
                then "\($a)3D does not name its own 3D entitlements" else empty end),
            (if (ents($a + "Maps") | map(select(test("Maps[^/]*[.]entitlements"))) | length) == 0
                then "\($a)Maps does not name its own Maps entitlements" else empty end)
            ' <<<"$json" 2>&1)"
        platform="$(jq -r --arg a "$app" '.targets[$a].platform // ""' <<<"$json")"
        for pair in "$app:release" "${app}3D:3d" "${app}Maps:maps"; do
            plist="$(jq -r --arg t "${pair%%:*}" '.targets[$t].settings.base.INFOPLIST_FILE // ""' <<<"$json")"
            want="${pair##*:}"
            if [ -z "$plist" ] || [ ! -f "$dir/$plist" ]; then
                problems="$problems${problems:+
}${pair%%:*} has no Info.plist"
            elif [ "$(plutil -extract MidgardEdition raw -o - "$dir/$plist" 2>/dev/null)" != "$want" ]; then
                problems="$problems${problems:+
}${pair%%:*}'s Info.plist does not say MidgardEdition $want"
            fi
        done
        for f in "$dir"/Config/*.entitlements; do
            [ -f "$f" ] || continue
            case "$(basename "$f")" in
                *Maps*)
                    if [ "$platform" = macOS ]; then
                        grep -q 'com.apple.security.network.client' "$f" ||
                            problems="$problems${problems:+
}$(basename "$f") lacks the network client entitlement"
                    fi
                    ! grep -q 'com.apple.security.network.server' "$f" ||
                        problems="$problems${problems:+
}$(basename "$f") has the network server entitlement" ;;
                *)
                    ! grep -q 'com.apple.security.network' "$f" ||
                        problems="$problems${problems:+
}$(basename "$f") names a network entitlement (only the maps edition has one)" ;;
            esac
        done
        if [ -n "$problems" ]; then
            echo "FAIL  $app: the three editions are not all declared as required:"
            sed 's/^/      /' <<<"$problems"
            bad=$((bad + 1))
        else
            echo "ok    $app: declares $app, ${app}3D and ${app}Maps, each with its own scheme, UI tests, Info.plist and entitlements"
        fi
    done
    [ "$n" -gt 0 ] || echo "ok    no app projects"
    return "$bad"
}

if command -v jq >/dev/null 2>&1 && command -v plutil >/dev/null 2>&1; then
    check_editions "$apple"
    failures=$((failures + $?))

    # The check's own failure paths, on a spec-only fixture made here: as written it passes;
    # each seeded edit fails, saying why.
    edition_fixture() { # edition_fixture <dir>: a Mac app "Ed" with all three editions
        local d="$1/apple/Apps/Ed" e
        rm -rf "$1" && mkdir -p "$d/Config" "$d/Sources" "$d/UITests"
        for e in release:Info 3d:Info3D maps:InfoMaps; do
            printf '%s\n' "<plist version=\"1.0\"><dict><key>MidgardEdition</key><string>${e%%:*}</string></dict></plist>" \
                >"$d/${e##*:}.plist"
        done
        for e in Ed Ed3D; do
            printf '%s\n' '<plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/></dict></plist>' \
                >"$d/Config/$e.entitlements"
        done
        printf '%s\n' '<plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/>' \
            '<key>com.apple.security.network.client</key><true/></dict></plist>' >"$d/Config/EdMaps.entitlements"
        {
            printf '%s\n' 'name: Ed' 'packages:' '  MidgardKit:' '    path: ../..' 'targets:'
            for e in :io.veldris.midgard.mac:Info 3D:io.veldris.midgard3d.mac:Info3D Maps:io.veldris.midgardmaps.mac:InfoMaps; do
                local s="${e%%:*}" rest="${e#*:}"
                local id="${rest%%:*}" plist="${rest##*:}"
                printf '%s\n' "  Ed$s:" '    type: application' '    platform: macOS' '    sources: [Sources]' \
                    '    settings:' '      base:' "        PRODUCT_BUNDLE_IDENTIFIER: $id" "        INFOPLIST_FILE: $plist.plist" \
                    "        CODE_SIGN_ENTITLEMENTS: Config/Ed$s.entitlements" '    dependencies:' \
                    '      - package: MidgardKit' '        product: MidgardKit'
                [ -z "$s" ] || printf '%s\n' '      - package: MidgardKit' '        product: MidgardGlobeKit'
                printf '%s\n' "  Ed${s}UITests:" '    type: bundle.ui-testing' '    platform: macOS' '    sources: [UITests]' \
                    '    settings:' '      base:' "        TEST_TARGET_NAME: Ed$s" '    dependencies:' "      - target: Ed$s" \
                    '      - package: MidgardKit' '        product: MidgardAuditKit'
            done
            printf '%s\n' 'schemes:'
            for s in "" 3D Maps; do
                printf '%s\n' "  Ed$s:" '    build:' '      targets:' "        Ed$s: all"
            done
        } >"$d/project.yml"
    }
    ed="$tmp/ed"
    edit_spec() { awk "$1" project.yml >project.new && mv project.new project.yml; }
    # seeded <name> <expected words> <command that edits $ed/apple/Apps/Ed>
    seeded() {
        edition_fixture "$ed"
        (cd "$ed/apple/Apps/Ed" && eval "$3")
        check "editions fixture: $1 fails" \
            "! out=\$(check_editions $ed/apple) && grep -q '^FAIL  Ed:' <<<\"\$out\" && grep -q '$2' <<<\"\$out\""
    }
    edition_fixture "$ed"
    check "editions fixture: all three editions as required pass" "check_editions $ed/apple >/dev/null"
    seeded "no 3D target" "no application target Ed3D" \
        "edit_spec '/^  Ed3D:\$/ && !done { skip = 1; done = 1; next } skip && /^  [A-Za-z]/ { skip = 0 } !skip { print }'"
    seeded "no maps target" "no application target EdMaps" \
        "edit_spec '/^  EdMaps:\$/ && !done { skip = 1; done = 1; next } skip && /^  [A-Za-z]/ { skip = 0 } !skip { print }'"
    seeded "no maps scheme" "no scheme EdMaps" \
        "edit_spec '/^schemes:/ { on = 1 } on && /^  EdMaps:\$/ { skip = 1; next } skip && /^  [A-Za-z]/ { skip = 0 } !skip { print }'"
    seeded "the release app linking MidgardGlobeKit" "Ed links MidgardGlobeKit" \
        "edit_spec '{ print } /^  Ed:\$/ { on = 1 } on && /^    dependencies:\$/ { print \"      - package: MidgardKit\"; print \"        product: MidgardGlobeKit\"; on = 0 }'"
    seeded "the 3D app not linking MidgardGlobeKit" "Ed3D does not link MidgardGlobeKit" \
        "edit_spec '/^  Ed3D:\$/ { on = 1 } /^  EdMaps:\$/ { on = 0 } on && /product: MidgardGlobeKit/ { sub(/MidgardGlobeKit/, \"MidgardKit\") } { print }'"
    seeded "the maps app not linking MidgardGlobeKit" "EdMaps does not link MidgardGlobeKit" \
        "edit_spec '/^  EdMaps:\$/ { on = 1 } /^  EdMapsUITests:\$/ { on = 0 } on && /product: MidgardGlobeKit/ { sub(/MidgardGlobeKit/, \"MidgardKit\") } { print }'"
    seeded "a maps UI-test target missing" "no UI-test target EdMapsUITests" \
        "edit_spec '{ sub(/TEST_TARGET_NAME: EdMaps/, \"TEST_TARGET_NAME: Ed\"); print }'"
    seeded "the 3D Info.plist saying release" "MidgardEdition 3d" \
        "plutil -replace MidgardEdition -string release Info3D.plist"
    seeded "the maps Info.plist saying 3d" "MidgardEdition maps" \
        "plutil -replace MidgardEdition -string 3d InfoMaps.plist"
    seeded "the release Info.plist missing its edition" "MidgardEdition release" \
        "plutil -remove MidgardEdition Info.plist"
    seeded "a 3D bundle id outside io.veldris.midgard3d" "bundle id" \
        "edit_spec '{ sub(/io.veldris.midgard3d.mac/, \"io.veldris.midgard.mac3d\"); print }'"
    seeded "a maps bundle id outside io.veldris.midgardmaps" "bundle id" \
        "edit_spec '{ sub(/io.veldris.midgardmaps.mac/, \"io.veldris.midgard3d.macmaps\"); print }'"
    seeded "a release entitlement with network" "names a network entitlement" \
        "cp Config/EdMaps.entitlements Config/Ed.entitlements"
    seeded "a 3D entitlement with network" "Ed3D.entitlements names a network entitlement" \
        "cp Config/EdMaps.entitlements Config/Ed3D.entitlements"
    seeded "a Mac maps entitlement without network" "lacks the network client" \
        "cp Config/Ed.entitlements Config/EdMaps.entitlements"
    seeded "a maps entitlement with the server one" "network server" \
        "sed 's|</dict>|<key>com.apple.security.network.server</key><true/></dict>|' Config/EdMaps.entitlements >x && mv x Config/EdMaps.entitlements"
    seeded "a maps target naming the 3D entitlements" "own Maps entitlements" \
        "edit_spec '{ sub(/Config\\/EdMaps.entitlements/, \"Config/Ed3D.entitlements\"); print }'"
    seeded "a 3D target naming the release entitlements" "own 3D entitlements" \
        "edit_spec '{ sub(/Config\\/Ed3D.entitlements/, \"Config/Ed.entitlements\"); print }'"
else
    echo "SKIP  the editions check needs jq and plutil (macOS)"
fi
[ "$failures" -eq 0 ] || exit 1
