#!/usr/bin/env bash
# The committed Xcode projects of the Apple apps are what their XcodeGen specs generate:
# for each clients/shells/apple/Apps/*/project.yml, the project is regenerated in a
# scratch copy of the package made of its tracked files only (as a fresh checkout has it:
# no build outputs, no Signing.local.xcconfig, no per-user Xcode state, and the committed
# project removed first), and its project.pbxproj and shared schemes must equal the
# committed <App>.xcodeproj's. An edit made in Xcode and not in project.yml fails here,
# naming the app. The copy keeps the package directory's name ("apple"): XcodeGen names
# a local package's folder reference after it. Each project also declares its one app, the
# Maps edition (owner decision, 23 Nov 2026), with its scheme, UI tests, Info.plist and
# entitlements (check_editions).
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

# One app per project, the Maps edition (owner decision, 23 Nov 2026). Each
# Apps/<App>/project.yml declares exactly one application, <App>Maps, with its scheme, its
# UI-test target <App>MapsUITests that links MidgardAuditKit, an Info.plist saying
# MidgardEdition maps, the bundle identifier io.veldris.midgard (both platforms, one App Store record), and MidgardGlobeKit
# linked. Entitlement files: never the network server entitlement; on the Mac every one
# names the client one, and the app names its own (<App>Maps[-AdHoc|-NoTeam].entitlements,
# here or through Config/Signing.xcconfig). tools/check-edition.sh checks what the build
# actually carries.
#
# check_editions <package dir>: one line per app; returns the number of apps that fail.
check_editions() {
    local base="$1" n=0 bad=0 spec dir app json problems platform plist f
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
            ($a + "Maps") as $m |
            def t($n): $root.targets[$n] // {};
            def links($n; $p): ((t($n).dependencies // []) | map(select(.product == $p)) | length) > 0;
            def bid($n): (t($n).settings.base.PRODUCT_BUNDLE_IDENTIFIER // "");
            (if t($m).type != "application" then "no application target \($m)" else empty end),
            (($root.targets // {}) | to_entries[] | select(.value.type == "application" and .key != $m)
                | "\(.key) is an application other than \($m) (one app per platform)"),
            (if t($m + "UITests").settings.base.TEST_TARGET_NAME != $m then "no UI-test target \($m)UITests for \($m)" else empty end),
            (if links($m + "UITests"; "MidgardAuditKit") | not then "\($m)UITests does not link MidgardAuditKit" else empty end),
            (if ($root.schemes // {})[$m] == null then "no scheme \($m)" else empty end),
            (($root.schemes // {}) | keys[] | select(. != $m) | "scheme \(.) is not for \($m)"),
            (if links($m; "MidgardGlobeKit") | not then "\($m) does not link MidgardGlobeKit" else empty end),
            (if bid($m) | test("^io[.]veldris[.]midgard$") | not then "\($m) bundle id \(bid($m)) is not io.veldris.midgard" else empty end)
            ' <<<"$json" 2>&1)"
        platform="$(jq -r --arg t "${app}Maps" '.targets[$t].platform // ""' <<<"$json")"
        plist="$(jq -r --arg t "${app}Maps" '.targets[$t].settings.base.INFOPLIST_FILE // ""' <<<"$json")"
        if [ -z "$plist" ] || [ ! -f "$dir/$plist" ]; then
            problems="$problems${problems:+
}${app}Maps has no Info.plist"
        elif [ "$(plutil -extract MidgardEdition raw -o - "$dir/$plist" 2>/dev/null)" != maps ]; then
            problems="$problems${problems:+
}${app}Maps's Info.plist does not say MidgardEdition maps"
        fi
        for f in "$dir"/Config/*.entitlements; do
            [ -f "$f" ] || continue
            case "$(basename "$f")" in
                "${app}Maps.entitlements" | "${app}Maps-AdHoc.entitlements" | "${app}Maps-NoTeam.entitlements") ;;
                *) problems="$problems${problems:+
}$(basename "$f") is not an entitlements file of ${app}Maps" ;;
            esac
            if [ "$platform" = macOS ]; then
                grep -q 'com.apple.security.network.client' "$f" ||
                    problems="$problems${problems:+
}$(basename "$f") lacks the network client entitlement"
            fi
            ! grep -q 'com.apple.security.network.server' "$f" ||
                problems="$problems${problems:+
}$(basename "$f") has the network server entitlement"
        done
        if [ -n "$problems" ]; then
            echo "FAIL  $app: its one app is not declared as required:"
            sed 's/^/      /' <<<"$problems"
            bad=$((bad + 1))
        else
            echo "ok    $app: declares ${app}Maps only, with its scheme, UI tests, Info.plist and entitlements"
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
    edition_fixture() { # edition_fixture <dir>: a Mac app "Ed" with its one app, EdMaps
        local d="$1/apple/Apps/Ed"
        rm -rf "$1" && mkdir -p "$d/Config" "$d/Sources" "$d/UITests"
        printf '%s\n' '<plist version="1.0"><dict><key>MidgardEdition</key><string>maps</string></dict></plist>' >"$d/Info.plist"
        printf '%s\n' '<plist version="1.0"><dict><key>com.apple.security.app-sandbox</key><true/>' \
            '<key>com.apple.security.network.client</key><true/></dict></plist>' >"$d/Config/EdMaps.entitlements"
        printf '%s\n' 'name: Ed' 'packages:' '  MidgardKit:' '    path: ../..' 'targets:' \
            '  EdMaps:' '    type: application' '    platform: macOS' '    sources: [Sources]' \
            '    settings:' '      base:' '        PRODUCT_BUNDLE_IDENTIFIER: io.veldris.midgard' \
            '        INFOPLIST_FILE: Info.plist' '        CODE_SIGN_ENTITLEMENTS: Config/EdMaps.entitlements' \
            '    dependencies:' '      - package: MidgardKit' '        product: MidgardKit' \
            '      - package: MidgardKit' '        product: MidgardGlobeKit' \
            '  EdMapsUITests:' '    type: bundle.ui-testing' '    platform: macOS' '    sources: [UITests]' \
            '    settings:' '      base:' '        TEST_TARGET_NAME: EdMaps' '    dependencies:' '      - target: EdMaps' \
            '      - package: MidgardKit' '        product: MidgardAuditKit' \
            'schemes:' '  EdMaps:' '    build:' '      targets:' '        EdMaps: all' >"$d/project.yml"
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
    check "editions fixture: the one app as required passes" "check_editions $ed/apple >/dev/null"
    seeded "a second application target" "an application other than EdMaps" \
        "edit_spec '/^  EdMapsUITests:\$/ { print \"  Ed3D:\"; print \"    type: application\"; print \"    platform: macOS\"; print \"    sources: [Sources]\" } { print }'"
    seeded "a second scheme" "scheme Ed3D is not for" \
        "printf '%s\\n' '  Ed3D:' '    build:' '      targets:' '        EdMaps: all' >>project.yml"
    seeded "no scheme" "no scheme EdMaps" \
        "edit_spec '/^schemes:/ { exit } { print }'"
    seeded "the app not linking MidgardGlobeKit" "EdMaps does not link MidgardGlobeKit" \
        "edit_spec '{ sub(/product: MidgardGlobeKit/, \"product: MidgardKit\"); print }'"
    seeded "the UI-test target missing" "no UI-test target EdMapsUITests" \
        "edit_spec '{ sub(/TEST_TARGET_NAME: EdMaps/, \"TEST_TARGET_NAME: Ed\"); print }'"
    seeded "the Info.plist saying 3d" "MidgardEdition maps" \
        "plutil -replace MidgardEdition -string 3d Info.plist"
    seeded "the Info.plist missing its edition" "MidgardEdition maps" \
        "plutil -remove MidgardEdition Info.plist"
    seeded "a bundle id other than io.veldris.midgard" "bundle id" \
        "edit_spec '{ sub(/io.veldris.midgard$/, \"io.veldris.midgard.mac\"); print }'"
    seeded "a leftover entitlements file of another app" "is not an entitlements file of EdMaps" \
        "cp Config/EdMaps.entitlements Config/Ed3D.entitlements"
    seeded "a Mac entitlement without network" "lacks the network client" \
        "printf '%s\\n' '<plist version=\"1.0\"><dict><key>com.apple.security.app-sandbox</key><true/></dict></plist>' >Config/EdMaps.entitlements"
    seeded "an entitlement with the server one" "network server" \
        "sed 's|</dict>|<key>com.apple.security.network.server</key><true/></dict>|' Config/EdMaps.entitlements >x && mv x Config/EdMaps.entitlements"
else
    echo "SKIP  the editions check needs jq and plutil (macOS)"
fi
[ "$failures" -eq 0 ] || exit 1
