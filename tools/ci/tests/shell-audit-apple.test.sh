#!/usr/bin/env bash
# The Apple audit hook fails closed without an app, reports failed affordances from an
# xcresult summary, never lets the macOS run claim Dynamic Type, and record-audit.sh
# stamps `overall` on BSD and GNU sed alike. Runs on /bin/bash 3.2 and on Linux (the
# Xcode tools are stubbed; the JavaScript parser is checked only where osascript exists).
# [RC-49] [AC-C-11]
#
#   bash tools/ci/tests/shell-audit-apple.test.sh
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
failures=0
pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. No app project: non-zero exit, nothing on stdout (so no record can be written).
mkdir -p "$work/clients/shells/apple/tools"
cp "$root/clients/shells/apple/audit.sh" "$work/clients/shells/apple/"
out="$(env -u CI bash "$work/clients/shells/apple/audit.sh" 0.0.0-test shell-audit/x.json 2>/dev/null)"
rc=$?
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then pass "no app: fails closed"; else fail "no app: rc=$rc out='$out'"; fi

# 2. The xcresult parser (macOS only: JavaScript for Automation).
if command -v osascript >/dev/null 2>&1; then
    got="$(osascript -l JavaScript "$root/clients/shells/apple/tools/audit-failures.js" "$here/fixtures/xcresult-tests.json")"
    if [ "$got" = "contrast_aa keyboard_navigation" ]; then
        pass "parser lists failed and missing affordances"
    else
        fail "parser said '$got'"
    fi
else
    echo "skip  parser (no osascript on this host)"
fi

# 3. record-audit.sh stamps overall with whatever sed this host has.
mkdir -p "$work/r/clients/shells/apple" "$work/r/clients/shells/tools"
cp "$root/clients/shells/tools/record-audit.sh" "$work/r/clients/shells/tools/"
cp "$root/clients/shells/apple/accessibility.toml" "$work/r/clients/shells/apple/"
bash "$work/r/clients/shells/tools/record-audit.sh" apple 0.0.0-test runner-1 "iPhone 16" XCUITest "Xcode 26.2" r.json \
    contrast_aa >/dev/null 2>&1
rc=$?
rec="$work/r/clients/shells/apple/audits/0.0.0-test.toml"
if [ "$rc" -ne 0 ] && grep -q '^overall *= "fail"' "$rec" 2>/dev/null && ! grep -q PLACEHOLDER "$rec" \
    && grep -q '^contrast_aa *= "fail"' "$rec"; then
    pass "record-audit stamps overall = fail"
else
    fail "record-audit (rc=$rc) left $(grep overall "$rec" 2>/dev/null)"
fi
bash "$work/r/clients/shells/tools/record-audit.sh" apple 0.0.0-ok runner-1 "iPhone 16" XCUITest "Xcode 26.2" r.json \
    >/dev/null 2>&1
rc=$?
rec="$work/r/clients/shells/apple/audits/0.0.0-ok.toml"
if [ "$rc" -eq 0 ] && grep -q '^overall *= "pass"' "$rec" 2>/dev/null && ! grep -q PLACEHOLDER "$rec"; then
    pass "record-audit stamps overall = pass"
else
    fail "record-audit (rc=$rc) left $(grep overall "$rec" 2>/dev/null)"
fi
leftover="$(find "$work/r/clients/shells/apple/audits" -type f ! -name '*.toml')"
if [ -z "$leftover" ]; then pass "record-audit leaves no temp or backup file"; else fail "left $leftover"; fi

# 4. audit.sh against stub Xcode tools. xcodebuild writes a result bundle holding the
#    failures configured for its scheme (STUB_FAIL_MidgardiOS / STUB_FAIL_MidgardMac);
#    xcrun prints them after a test-case line; osascript prints that line back.
stubs="$work/stubs"
mkdir -p "$stubs"
cat > "$stubs/xcodebuild" <<'STUB'
#!/usr/bin/env bash
[ "$1" = -version ] && { echo "Xcode 26.2"; echo "Build version stub"; exit 0; }
scheme=""; bundle=""
while [ $# -gt 0 ]; do
    case "$1" in
        -scheme) scheme="$2"; shift ;;
        -resultBundlePath) bundle="$2"; shift ;;
    esac
    shift
done
echo "$scheme" >> "$STUB_CALLS"
[ -n "${STUB_NO_BUNDLE:-}" ] && exit 65
mkdir -p "$bundle"
eval "echo \"\${STUB_FAIL_$scheme:-}\"" > "$bundle/failures"
exit 65
STUB
cat > "$stubs/xcrun" <<'STUB'
#!/usr/bin/env bash
while [ $# -gt 0 ] && [ "$1" != --path ]; do shift; done
[ -n "${STUB_NO_TESTS:-}" ] && { echo '{"testNodes":[]}'; exit 0; }
echo '{"name":"test_affordance_stub()"}'
cat "$2/failures"
STUB
cat > "$stubs/osascript" <<'STUB'
#!/usr/bin/env bash
sed -n 2p "$4"
STUB
chmod +x "$stubs/xcodebuild" "$stubs/xcrun" "$stubs/osascript"

# apps TREE APP...: an Apple shell holding the given app projects and a stub framework build.
apps() {
    local t="$work/$1"; shift
    mkdir -p "$t/clients/shells/apple/tools"
    cp "$root/clients/shells/apple/audit.sh" "$t/clients/shells/apple/"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$t/clients/shells/apple/tools/build-xcframework.sh"
    local a
    for a in "$@"; do mkdir -p "$t/clients/shells/apple/Apps/$a/$a.xcodeproj"; done
    echo "$t"
}
run_audit() { # run_audit TREE [VAR=value...]: stdout of audit.sh; rc in $rc
    local t="$1"; shift
    out="$(cd "$t" && env -u CI "$@" PATH="$stubs:$PATH" STUB_CALLS="$t/calls" MIDGARD_BUILD_CACHE="$t/cache" \
        bash clients/shells/apple/audit.sh 0.0.0-test shell-audit/apple-0.0.0-test.json 2>"$t/stderr")"
    rc=$?
}

t="$(apps maconly MidgardMac)"
run_audit "$t" STUB_FAIL_MidgardMac=dynamic_type
if [ "$rc" -eq 0 ] && [ "$out" = "dynamic_type " ]; then
    pass "macOS alone: dynamic_type is reported failed (not measured there)"
else
    fail "macOS alone: rc=$rc out='$out' $(cat "$t/stderr")"
fi
if [ -f "$t/shell-audit/apple-0.0.0-test.json" ]; then pass "the raw report is kept"; else fail "no report written"; fi

t="$(apps both MidgardiOS MidgardMac)"
run_audit "$t" STUB_FAIL_MidgardiOS=contrast_aa "STUB_FAIL_MidgardMac=dynamic_type keyboard_navigation"
if [ "$rc" -eq 0 ] && [ "$out" = "contrast_aa keyboard_navigation " ]; then
    pass "iOS and macOS: the iOS run stands for dynamic_type; other failures add up"
else
    fail "iOS and macOS: rc=$rc out='$out' $(cat "$t/stderr")"
fi
if [ "$(tr '\n' ' ' < "$t/calls")" = "MidgardiOS MidgardMac " ]; then
    pass "both apps are tested"
else
    fail "tested: $(cat "$t/calls")"
fi

t="$(apps both-pass MidgardiOS MidgardMac)"
run_audit "$t" STUB_FAIL_MidgardMac=dynamic_type
if [ "$rc" -eq 0 ] && [ -z "$(echo "$out" | tr -d ' ')" ]; then
    pass "iOS and macOS, all measured pass: nothing failed"
else
    fail "all pass: rc=$rc out='$out'"
fi

t="$(apps nobundle MidgardiOS)"
run_audit "$t" STUB_NO_BUNDLE=1
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then pass "no result bundle: fails closed"; else fail "no bundle: rc=$rc out='$out'"; fi

t="$(apps notests MidgardiOS)"
run_audit "$t" STUB_NO_TESTS=1
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then
    pass "no affordance test ran: fails closed"
else
    fail "no tests: rc=$rc out='$out'"
fi

# 5. Through the CI job (`run.sh shell-audit apple`): a package without an app records
#    nothing; an audited app gets a record the release gate reads.
job_tree() { # job_tree NAME [APP...]: a checkout with run.sh, the record writer and an Apple shell
    local t
    t="$(apps "$@")"
    mkdir -p "$t/tools/ci" "$t/clients/shells/tools"
    cp "$root/tools/ci/run.sh" "$root/tools/ci/lib.sh" "$t/tools/ci/"
    cp "$root/clients/shells/tools/record-audit.sh" "$t/clients/shells/tools/"
    cp "$root/clients/shells/apple/accessibility.toml" "$t/clients/shells/apple/"
    : > "$t/clients/shells/apple/Package.swift"
    echo "$t"
}
job() { # job TREE [VAR=value...]: run the CI job; rc in $rc
    local t="$1"; shift
    (cd "$t" && env -u CI "$@" PATH="$stubs:$PATH" STUB_CALLS="$t/calls" MIDGARD_BUILD_CACHE="$t/cache" \
        GITHUB_SHA=0123abcd RUNNER_NAME=test-runner bash tools/ci/run.sh shell-audit apple 0.0.0-test \
        >"$t/stdout" 2>&1)
    rc=$?
}
t="$(job_tree job-noapp)"
job "$t"
if [ "$rc" -ne 0 ] && [ -z "$(ls "$t/clients/shells/apple/audits" 2>/dev/null)" ]; then
    pass "CI job, package but no app: fails, no record"
else
    fail "CI job, no app: rc=$rc $(ls "$t/clients/shells/apple/audits" 2>/dev/null) $(cat "$t/stdout")"
fi
t="$(job_tree job-mac MidgardMac)"
job "$t" STUB_FAIL_MidgardMac=dynamic_type
rec="$t/clients/shells/apple/audits/0.0.0-test.toml"
if [ "$rc" -ne 0 ] && grep -q '^dynamic_type *= "fail"' "$rec" 2>/dev/null && grep -q '^overall *= "fail"' "$rec" \
    && grep -q '^tool_version = "Xcode 26.2"' "$rec" && grep -q '^runner *= "test-runner"' "$rec"; then
    pass "CI job, macOS app alone: record says dynamic_type fail, overall fail"
else
    fail "CI job, macOS alone: rc=$rc $(cat "$rec" 2>/dev/null) $(cat "$t/stdout")"
fi
t="$(job_tree job-both MidgardiOS MidgardMac)"
job "$t" STUB_FAIL_MidgardMac=dynamic_type
rec="$t/clients/shells/apple/audits/0.0.0-test.toml"
if [ "$rc" -eq 0 ] && grep -q '^overall *= "pass"' "$rec" 2>/dev/null && [ "$(grep -c '= "pass"$' "$rec")" -eq 8 ]; then
    pass "CI job, both apps measured pass: record says pass for all seven"
else
    fail "CI job, both apps: rc=$rc $(cat "$rec" 2>/dev/null) $(cat "$t/stdout")"
fi

# 6. bash 3.2 and BSD/GNU portability of the scripts this test covers.
for s in clients/shells/apple/audit.sh clients/shells/tools/record-audit.sh; do
    if [ -f "$root/$s" ] && ! grep -Eq 'mapfile|readarray|declare -A|\$\{[A-Za-z_]+,,\}' "$root/$s" \
        && ! grep -Eq '^[^#]*sed -i' "$root/$s" && /bin/bash -n "$root/$s"; then
        pass "$s: bash 3.2 safe, no in-place sed"
    else
        fail "$s: bash-4 builtin, in-place sed or a /bin/bash parse error"
    fi
done

[ "$failures" -eq 0 ] || exit 1
