#!/usr/bin/env bash
# The committed Xcode projects of the Apple apps are what their XcodeGen specs generate:
# for each clients/shells/apple/Apps/*/project.yml, the project is regenerated in a
# scratch copy of the package made of its tracked files only (as a fresh checkout has it:
# no build outputs, no Signing.local.xcconfig, no per-user Xcode state, and the committed
# project removed first), and its project.pbxproj and shared schemes must equal the
# committed <App>.xcodeproj's. An edit made in Xcode and not in project.yml fails here,
# naming the app. The copy keeps the package directory's name ("apple"): XcodeGen names
# a local package's folder reference after it. macOS only (XcodeGen): elsewhere it says
# SKIP. [DD-6]
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
[ "$failures" -eq 0 ] || exit 1
