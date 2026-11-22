#!/usr/bin/env bash
# On-device accessibility audit of the Apple shell, run by `tools/ci/run.sh shell-audit
# apple` on a macOS runner. Prints the failed affordances (space-separated) on stdout;
# exits non-zero when no app could be built and tested, so no record is written.
# [RC-49] [AC-C-11]
#
#   bash clients/shells/apple/audit.sh <release> <report-path>
#
# The record covers both apps (owner decision 4: it is written from a merge of
# clients_mac and clients_iphone), so a tree with only one of them is refused. Each
# project holds the three editions (Task B7): the release app (scheme MidgardiOS,
# MidgardMac), the 3D app (MidgardiOS3D, MidgardMac3D) and the maps app (MidgardiOSMaps,
# MidgardMacMaps), so six apps are audited, and a 3D or maps scheme that does not run
# fails the audit as the release one would. Each
# app's UI-test target runs `AccessibilityAudit` (a subclass of MidgardAuditKit's
# AccessibilityAuditBase); tools/audit-failures.js reads the result bundle. An
# affordance whose test failed, was skipped or did not run is reported failed. UI-test
# sources that override `record(_:)` or `invokeTest` could rewrite their own results,
# so they are refused. Dynamic Type is measured on iOS only: the macOS run never claims
# it, and the iOS results stand for it. Next to <report> (the six apps' result summaries,
# merged) it keeps <report>-ios.json, -ios3d.json, -iosmaps.json, -mac.json, -mac3d.json,
# -macmaps.json and <report>.device, the devices audited, which `run.sh` records. Derived data and result bundles go to
# ~/Library/Caches/midgard-build/clients-derived.
set -euo pipefail
release="${1:?release}"; report="${2:?report path}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
derived="$cache/clients-derived"

apps=""
[ -d "$here/Apps/MidgardiOS/MidgardiOS.xcodeproj" ] && apps="$apps ios"
[ -d "$here/Apps/MidgardMac/MidgardMac.xcodeproj" ] && apps="$apps mac"
if [ -z "$apps" ]; then
    echo "audit: no app project under clients/shells/apple/Apps; nothing was audited" >&2
    exit 1
fi
case "$apps" in
    " ios mac") ;;
    *)
        echo "audit: the record needs both apps (owner decision 4); found:$apps" >&2
        exit 1
        ;;
esac
# A test that overrides record(_:) or invokeTest can drop or rewrite its own failures.
hooks="$(grep -rlE '(func[[:space:]]+record[[:space:]]*\(|invokeTest)' "$here/Apps" --include='*.swift' || true)"
if [ -n "$hooks" ]; then
    echo "audit: UI-test sources could rewrite their own results (record(_:) or invokeTest):" >&2
    echo "$hooks" >&2
    exit 1
fi

# The devices audited, for the record: the simulator with its runtime, and this Mac.
# iPhone 17 by default (the B3 runs); MIDGARD_AUDIT_SIMULATOR names another. iOS's
# keyboard_navigation needs the simulator's hardware keyboard and Full Keyboard Access;
# that step is not automated yet: on the iOS 26.3 simulator, turning Full Keyboard
# Access on made key delivery worse (no Tab or Cmd-digit press reached the app), so the
# iOS keyboard_navigation test is expected to fail until a working setting is found.
sim="${MIDGARD_AUDIT_SIMULATOR:-iPhone 17}"
sims="$(xcrun simctl list devices available)"
runtime="$(awk -v name="$sim" '
    /^-- / { rt = $0; sub(/^-- /, "", rt); sub(/ --$/, "", rt); next }
    { line = $0; sub(/^ +/, "", line); if (index(line, name " (") == 1) { print rt; exit } }' <<<"$sims")"
if [ -z "$runtime" ]; then
    echo "audit: no available simulator named '$sim'" >&2
    exit 1
fi
device="$sim simulator ($runtime) + macOS $(sw_vers -productVersion) ($(uname -m))"

# On a CI runner, provision the pinned toolchain's Apple targets (the workflow has no
# build logic of its own; see .github/workflows/shell-audit.yml).
if [ -n "${CI:-}" ]; then
    rustup toolchain install 1.82.0 --profile minimal >&2
    rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim \
        --toolchain 1.82.0 >&2
fi
bash "$here/tools/build-xcframework.sh" >&2

# Keyboard navigation: Tab moves focus to every control (SwiftUI's included) only with
# the system setting on (System Settings > Keyboard > Keyboard navigation), the global
# default AppleKeyboardUIMode with its 2 bit set (the toggle writes 2; older "All controls"
# wrote 3); a launch argument does not turn it on. A CI runner turns it on for this run
# and puts the previous value back (or removes the key) on exit. Elsewhere the setting
# is the user's: it is not changed, and keyboard_navigation is reported failed, as not
# measured. Every read and write of the global domain goes through kbd_mode.
kbd_mode() { # kbd_mode read | write N | delete: AppleKeyboardUIMode in the global domain
    case "$1" in
        read) defaults read -g AppleKeyboardUIMode 2>/dev/null ;;
        write) defaults write -g AppleKeyboardUIMode -int "$2" ;;
        delete) defaults delete -g AppleKeyboardUIMode 2>/dev/null ;;
    esac
}
kbd_on() { case "$1" in '' | *[!0-9]*) return 1 ;; esac; [ $(($1 & 2)) -ne 0 ]; }
kbd_unmeasured=""
kbd_before="$(kbd_mode read || true)"
if ! kbd_on "$kbd_before"; then
    if [ "${GITHUB_ACTIONS:-}" = true ] || [ "${CI:-}" = true ]; then
        if [ -n "$kbd_before" ]; then
            trap 'kbd_mode write "$kbd_before" || true' EXIT
        else
            trap 'kbd_mode delete || true' EXIT
        fi
        kbd_mode write 2
    else
        echo "audit: turn on System Settings > Keyboard > Keyboard navigation; keyboard_navigation is not measured and reported failed" >&2
        kbd_unmeasured=keyboard_navigation
    fi
fi

mkdir -p "$derived"
failed="$kbd_unmeasured"
runs=""
for app in $apps; do runs="$runs $app ${app}3d ${app}maps"; done
for app in $runs; do
    case "$app" in
        ios | ios3d | iosmaps)
            proj="$here/Apps/MidgardiOS/MidgardiOS.xcodeproj"; scheme=MidgardiOS
            dest="platform=iOS Simulator,name=$sim" ;;
        mac | mac3d | macmaps) proj="$here/Apps/MidgardMac/MidgardMac.xcodeproj"; scheme=MidgardMac; dest="platform=macOS" ;;
    esac
    case "$app" in *3d) scheme="${scheme}3D" ;; *maps) scheme="${scheme}Maps" ;; esac
    bundle="$derived/$app-$release.xcresult"
    rm -rf "$bundle"
    # Test failures are data here, not a reason to stop: the parser reads them.
    xcodebuild test -project "$proj" -scheme "$scheme" -destination "$dest" \
        -only-testing:"${scheme}UITests/AccessibilityAudit" \
        -derivedDataPath "$derived/$app" -resultBundlePath "$bundle" >&2 || true
    [ -d "$bundle" ] || { echo "audit: the $app audit did not run" >&2; exit 1; }
    xcrun xcresulttool get test-results tests --path "$bundle" > "$derived/$app-tests.json"
    # A bundle with no affordance test in it (the UI tests did not build or launch)
    # measured nothing: no record, rather than seven "fail"s that look measured.
    grep -q 'test_affordance_' "$derived/$app-tests.json" || {
        echo "audit: the $app result bundle holds no affordance test; nothing was measured" >&2
        exit 1
    }
    f="$(osascript -l JavaScript "$here/tools/audit-failures.js" "$derived/$app-tests.json")"
    # Dynamic Type is measured on iOS only; the macOS run does not claim it, and the
    # iOS app, always audited in the same run, stands for the affordance.
    case "$app" in mac*) f="$(echo " $f " | sed 's/ dynamic_type / /')" ;; esac
    failed="$failed $f"
done

# Evidence: each app's summary, and both merged at <report>.
base="$root/${report%.json}"
mkdir -p "$(dirname "$base")"
sep='{'
for app in $runs; do
    cp "$derived/$app-tests.json" "$base-$app.json"
    printf '%s"%s":' "$sep" "$app"
    cat "$base-$app.json"
    sep=','
done > "$root/$report"
printf '}\n' >> "$root/$report"
echo "$device" > "$base.device"
# Each affordance once, in a stable order.
for a in $failed; do echo "$a"; done | sort -u | tr '\n' ' '
