#!/usr/bin/env bash
# The committed Xcode projects of the Apple apps are what their XcodeGen specs generate:
# for each clients/shells/apple/Apps/*/project.yml, the project is regenerated in a
# scratch copy of the package (tracked-looking files only: no build outputs, no
# Signing.local.xcconfig, no per-user Xcode state) and its project.pbxproj and shared
# schemes must equal the committed <App>.xcodeproj's. An edit made in Xcode and not in
# project.yml fails here, naming the app. macOS only (XcodeGen): elsewhere it says SKIP.
# [DD-6]
#
#   bash tools/ci/tests/apple-projects.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
apple=clients/shells/apple
fixture=tools/ci/tests/fixtures/apple-projects
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

# compare_projects <package dir>: prints one line per app, "FAIL <app>: ..." on a
# difference; returns the number of apps that differ.
compare_projects() {
    local base="$1" n=0 bad=0 spec app dir rel copy gen com
    for spec in "$base"/Apps/*/project.yml; do
        [ -f "$spec" ] || continue
        n=$((n + 1))
        dir="$(dirname "$spec")"
        app="$(basename "$dir")"
        rel="Apps/$app"
        com="$dir/$app.xcodeproj"
        # Paths in the project are relative to it, so it is generated where it lives, in
        # a copy of the package as a fresh checkout has it, under the package's own
        # directory name: XcodeGen names a local package reference after its folder
        # (`path: ../..` is "apple"), and that name is in the project.
        copy="$tmp/copy-$n/$(basename "$(cd "$base" && pwd)")"
        rm -rf "$tmp/copy-$n" && mkdir -p "$copy"
        tar -C "$base" --exclude .build --exclude .swiftpm --exclude Frameworks --exclude '*.xcodeproj' \
            --exclude xcuserdata --exclude Signing.local.xcconfig --exclude midgard_client_ffi.swift -cf - . |
            tar -C "$copy" -xf -
        if ! xcodegen generate -q --spec "$copy/$rel/project.yml" >"$tmp/log" 2>&1; then
            echo "FAIL  $app: xcodegen generate failed: $(tr '\n' ' ' <"$tmp/log")"
            bad=$((bad + 1))
            continue
        fi
        gen="$copy/$rel/$app.xcodeproj"
        if [ ! -f "$com/project.pbxproj" ]; then
            echo "FAIL  $app: $app.xcodeproj is not committed beside project.yml"
            bad=$((bad + 1))
        elif ! diff -u "$com/project.pbxproj" "$gen/project.pbxproj" >"$tmp/diff" ||
            ! diff -ru "$com/xcshareddata" "$gen/xcshareddata" >>"$tmp/diff" 2>&1; then
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

# The check's own failure path, on the fixture: as committed it passes; with the
# committed project edited (as Xcode would), or a scheme removed, it fails naming the app.
check "fixture: a committed project equal to its spec passes" "compare_projects $fixture >/dev/null"
mkdir -p "$tmp/edited"
cp -R "$fixture/Apps" "$tmp/edited/"
p="$tmp/edited/Apps/Fixture/Fixture.xcodeproj/project.pbxproj"
sed 's/MACOSX_DEPLOYMENT_TARGET = 14.0;/MACOSX_DEPLOYMENT_TARGET = 13.0;/' "$p" >"$tmp/p" && mv "$tmp/p" "$p"
check "fixture: the edit took" "grep -q 'MACOSX_DEPLOYMENT_TARGET = 13.0;' $p"
check "fixture: an edited project.pbxproj fails, naming the app" \
    "! out=\$(compare_projects $tmp/edited) && grep -q '^FAIL  Fixture: Fixture.xcodeproj differs' <<<\"\$out\""
rm -rf "$tmp/edited" && mkdir -p "$tmp/edited" && cp -R "$fixture/Apps" "$tmp/edited/"
rm -f "$tmp/edited/Apps/Fixture/Fixture.xcodeproj/xcshareddata/xcschemes/Fixture.xcscheme"
check "fixture: a missing shared scheme fails" "! compare_projects $tmp/edited >/dev/null"
rm -rf "$tmp/edited" && mkdir -p "$tmp/edited" && cp -R "$fixture/Apps" "$tmp/edited/"
rm -rf "$tmp/edited/Apps/Fixture/Fixture.xcodeproj"
check "fixture: a spec without a committed project fails" \
    "! out=\$(compare_projects $tmp/edited) && grep -q 'is not committed' <<<\"\$out\""
mkdir -p "$tmp/empty"
check "no apps: passes, saying so" "out=\$(compare_projects $tmp/empty) && grep -q 'no app projects' <<<\"\$out\""
[ "$failures" -eq 0 ] || exit 1
