#!/usr/bin/env bash
# On-device accessibility audit of the Apple shell, run by `tools/ci/run.sh shell-audit
# apple` on a macOS runner. Prints the failed affordances (space-separated) on stdout;
# exits non-zero when no app could be built and tested, so no record is written.
# [RC-49] [AC-C-11]
#
#   bash clients/shells/apple/audit.sh <release> <report-path>
#
# Each app's UI-test target runs `AccessibilityAudit` (a subclass of MidgardAuditKit's
# AccessibilityAuditBase); tools/audit-failures.js reads the result bundle. An
# affordance whose test failed, was skipped or did not run is reported failed. Dynamic
# Type is measured on iOS only: the macOS run never claims it, and its failure there
# is dropped only when the iOS app was audited in the same run. Derived data and result
# bundles go to ~/Library/Caches/midgard-build/clients-derived.
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

# On a CI runner, provision the pinned toolchain's Apple targets (the workflow has no
# build logic of its own; see .github/workflows/shell-audit.yml).
if [ -n "${CI:-}" ]; then
    rustup toolchain install 1.82.0 --profile minimal >&2
    rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim \
        --toolchain 1.82.0 >&2
fi
bash "$here/tools/build-xcframework.sh" >&2

mkdir -p "$derived"
failed=""
first=""
for app in $apps; do
    case "$app" in
        ios) proj="$here/Apps/MidgardiOS/MidgardiOS.xcodeproj"; scheme=MidgardiOS
             dest="platform=iOS Simulator,name=${MIDGARD_AUDIT_SIMULATOR:-iPhone 16}" ;;
        mac) proj="$here/Apps/MidgardMac/MidgardMac.xcodeproj"; scheme=MidgardMac; dest="platform=macOS" ;;
    esac
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
    # Dynamic Type is measured on iOS only; the macOS run does not claim it. When the
    # iOS app was audited too, its result stands for the affordance.
    if [ "$app" = mac ]; then
        case "$apps" in *ios*) f="$(echo " $f " | sed 's/ dynamic_type / /')" ;; esac
    fi
    failed="$failed $f"
    [ -n "$first" ] || first="$derived/$app-tests.json"
done

mkdir -p "$(dirname "$root/$report")"
cp "$first" "$root/$report"
# Each affordance once, in a stable order.
for a in $failed; do echo "$a"; done | sort -u | tr '\n' ' '
