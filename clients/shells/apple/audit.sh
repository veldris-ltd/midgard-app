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
# app's UI-test target runs `AccessibilityAudit` (a subclass of MidgardAuditKit's
# AccessibilityAuditBase); tools/audit-failures.js reads the result bundle. An
# affordance whose test failed, was skipped or did not run is reported failed. UI-test
# sources that override `record(_:)` or `invokeTest` could rewrite their own results,
# so they are refused. Dynamic Type is measured on iOS only: the macOS run never claims
# it, and the iOS result stands for it. Next to <report> (both apps' result summaries,
# merged) it keeps <report>-ios.json, <report>-mac.json and <report>.device, the
# devices audited, which `run.sh` records. Derived data and result bundles go to
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
sim="${MIDGARD_AUDIT_SIMULATOR:-iPhone 16}"
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

mkdir -p "$derived"
failed=""
for app in $apps; do
    case "$app" in
        ios) proj="$here/Apps/MidgardiOS/MidgardiOS.xcodeproj"; scheme=MidgardiOS
             dest="platform=iOS Simulator,name=$sim" ;;
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
    # Dynamic Type is measured on iOS only; the macOS run does not claim it, and the
    # iOS app, always audited in the same run, stands for the affordance.
    [ "$app" = mac ] && f="$(echo " $f " | sed 's/ dynamic_type / /')"
    failed="$failed $f"
done

# Evidence: each app's summary, and both merged at <report>.
base="$root/${report%.json}"
mkdir -p "$(dirname "$base")"
cp "$derived/ios-tests.json" "$base-ios.json"
cp "$derived/mac-tests.json" "$base-mac.json"
{ printf '{"ios":'; cat "$base-ios.json"; printf ',"mac":'; cat "$base-mac.json"; printf '}\n'; } > "$root/$report"
echo "$device" > "$base.device"
# Each affordance once, in a stable order.
for a in $failed; do echo "$a"; done | sort -u | tr '\n' ' '
