#!/usr/bin/env bash
# The Apple audit hook fails closed without an app, reports failed affordances from an
# xcresult summary, needs both apps (owner decision 4), never lets the macOS run claim
# Dynamic Type, refuses UI tests that rewrite their own results, and record-audit.sh
# stamps `overall` on BSD and GNU sed alike, refuses a tree check-edition.sh fails, and
# handles the macOS keyboard navigation setting (turned on for the run on CI and locally,
# with a restore file written first and applied by a later run, a lock, a notice, and the
# MIDGARD_AUDIT_KBD=keep opt-out, under which keyboard_navigation is reported failed).
# Runs on /bin/bash 3.2 and on Linux (the Xcode tools are stubbed; the JavaScript parser
# is checked only where osascript exists).
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
    if [ "$got" = "contrast_aa focus_order reduced_motion keyboard_navigation hit_target_size" ]; then
        pass "parser: failed, skipped, expected-failure, missing and failed-once-then-passed are not passes"
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
#    failures configured for its scheme (STUB_FAIL_MidgardiOSMaps / STUB_FAIL_MidgardMacMaps);
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
if [ "$1" = simctl ]; then
    printf '%s\n' '== Devices ==' '-- iOS 18.5 --' '    iPhone 15 (AAAA) (Shutdown)' '-- iOS 26.2 --' \
        '    iPhone 16 (BBBB) (Shutdown)' '    iPhone 16 Pro (CCCC) (Shutdown)' '    iPhone 17 (DDDD) (Shutdown)'
    exit 0
fi
while [ $# -gt 0 ] && [ "$1" != --path ]; do shift; done
[ -n "${STUB_NO_TESTS:-}" ] && { echo '{"testNodes":[]}'; exit 0; }
echo '{"name":"test_affordance_stub()"}'
cat "$2/failures"
STUB
cat > "$stubs/osascript" <<'STUB'
#!/usr/bin/env bash
sed -n 2p "$4"
STUB
# defaults: AppleKeyboardUIMode in the global domain, kept in the file $STUB_KBD (absent
# file: absent key; default $work/kbd-on, which holds 2). Each call is logged to
# $STUB_KBD.log. It refuses anything else, and never runs the real defaults.
echo 2 > "$work/kbd-on"
cat > "$stubs/defaults" <<STUB
#!/usr/bin/env bash
f="\${STUB_KBD:-$work/kbd-on}"
echo "\$*" >> "\$f.log"
[ "\$2" = -g ] && [ "\$3" = AppleKeyboardUIMode ] || { echo "stub defaults: refused: \$*" >&2; exit 99; }
case "\$1" in
    read) [ -f "\$f" ] || exit 1; cat "\$f" ;;
    write) [ "\$4" = -int ] || exit 99
        # Safeguard 1: the restore file must exist before the setting is turned on.
        [ "\$5" = 2 ] && echo "restore-file-before-write=\$([ -f "\${STUB_RESTORE:-/nonexistent}" ] && echo yes || echo no)" >> "\$f.log"
        echo "\$5" > "\$f" ;;
    delete) [ -f "\$f" ] || exit 1; rm -f "\$f" ;;
    *) exit 99 ;;
esac
STUB
# rustup: audit.sh provisions the toolchain on CI; the CI cases below must not.
printf '#!/usr/bin/env bash\nexit 0\n' > "$stubs/rustup"
printf '#!/usr/bin/env bash\necho 15.5\n' > "$stubs/sw_vers"
printf '#!/usr/bin/env bash\necho arm64\n' > "$stubs/uname"
chmod +x "$stubs/xcodebuild" "$stubs/xcrun" "$stubs/osascript" "$stubs/sw_vers" "$stubs/uname" "$stubs/defaults" "$stubs/rustup"

# apps TREE APP...: an Apple shell holding the given app projects and a stub framework build.
apps() {
    local t="$work/$1"; shift
    mkdir -p "$t/clients/shells/apple/tools"
    cp "$root/clients/shells/apple/audit.sh" "$t/clients/shells/apple/"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$t/clients/shells/apple/tools/build-xcframework.sh"
    # check-edition.sh: passes unless STUB_EDITION_RC says otherwise.
    printf '#!/usr/bin/env bash\nexit "${STUB_EDITION_RC:-0}"\n' > "$t/clients/shells/apple/tools/check-edition.sh"
    local a
    for a in "$@"; do
        mkdir -p "$t/clients/shells/apple/Apps/$a/$a.xcodeproj" "$t/clients/shells/apple/Apps/$a/UITests"
        printf 'final class AccessibilityAudit: AccessibilityAuditBase {}\n' \
            > "$t/clients/shells/apple/Apps/$a/UITests/AccessibilityAudit.swift"
    done
    echo "$t"
}
run_audit() { # run_audit TREE [VAR=value...]: stdout of audit.sh; rc in $rc
    local t="$1"; shift
    out="$(cd "$t" && env -u CI -u GITHUB_ACTIONS "$@" PATH="$stubs:$PATH" STUB_CALLS="$t/calls" MIDGARD_BUILD_CACHE="$t/cache" \
        bash clients/shells/apple/audit.sh 0.0.0-test shell-audit/apple-0.0.0-test.json 2>"$t/stderr")"
    rc=$?
}

for one in MidgardMac MidgardiOS; do
    t="$(apps "only-$one" "$one")"
    run_audit "$t" STUB_FAIL_MidgardMacMaps=dynamic_type
    if [ "$rc" -ne 0 ] && [ -z "$out" ] && [ ! -f "$t/calls" ] && grep -q 'needs both apps' "$t/stderr"; then
        pass "$one alone: refused before anything runs (the record needs both apps)"
    else
        fail "$one alone: rc=$rc out='$out' $(cat "$t/stderr")"
    fi
done

n=0
for hook in 'override func record(_ issue: XCTIssue) {}' 'override func invokeTest() {}' \
    'public override func record(_ issue: XCTIssue) { }'; do
    n=$((n + 1))
    t="$(apps "hook-$n" MidgardiOS MidgardMac)"
    printf 'extension AccessibilityAudit {\n    %s\n}\n' "$hook" >> "$t/clients/shells/apple/Apps/MidgardMac/UITests/AccessibilityAudit.swift"
    run_audit "$t"
    if [ "$rc" -ne 0 ] && [ -z "$out" ] && [ ! -f "$t/calls" ] && grep -q 'rewrite' "$t/stderr"; then
        pass "a UI test that can rewrite its results is refused: $hook"
    else
        fail "hook '$hook': rc=$rc out='$out' $(cat "$t/stderr")"
    fi
done

t="$(apps both MidgardiOS MidgardMac)"
run_audit "$t" STUB_FAIL_MidgardiOSMaps=contrast_aa "STUB_FAIL_MidgardMacMaps=dynamic_type keyboard_navigation"
if [ "$rc" -eq 0 ] && [ "$out" = "contrast_aa keyboard_navigation " ]; then
    pass "iOS and macOS: the iOS run stands for dynamic_type; other failures add up"
else
    fail "iOS and macOS: rc=$rc out='$out' $(cat "$t/stderr")"
fi
ev="$t/shell-audit/apple-0.0.0-test"
if [ -s "$ev-iosmaps.json" ] && [ -s "$ev-macmaps.json" ] && grep -q '^{"iosmaps":' "$ev.json" \
    && grep -q ',"macmaps":' "$ev.json"; then
    pass "the two apps' reports are kept, and merged in the report"
else
    fail "reports: $(ls "$t/shell-audit" 2>/dev/null)"
fi
if [ "$(cat "$ev.device" 2>/dev/null)" = "iPhone 17 simulator (iOS 26.2) + macOS 15.5 (arm64)" ]; then
    pass "the device sidecar names the simulator (iPhone 17 by default), its runtime and the Mac"
else
    fail "device sidecar: '$(cat "$ev.device" 2>/dev/null)'"
fi
t3="$(apps othersim MidgardiOS MidgardMac)"
run_audit "$t3" MIDGARD_AUDIT_SIMULATOR="iPhone 16"
if [ "$rc" -eq 0 ] && [ "$(cat "$t3/shell-audit/apple-0.0.0-test.device" 2>/dev/null)" = "iPhone 16 simulator (iOS 26.2) + macOS 15.5 (arm64)" ]; then
    pass "MIDGARD_AUDIT_SIMULATOR names another simulator"
else
    fail "simulator override: rc=$rc '$(cat "$t3/shell-audit/apple-0.0.0-test.device" 2>/dev/null)'"
fi
t2="$(apps nosim MidgardiOS MidgardMac)"
run_audit "$t2" MIDGARD_AUDIT_SIMULATOR="iPhone 99"
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then
    pass "a simulator simctl does not list: fails closed"
else
    fail "unknown simulator: rc=$rc out='$out'"
fi
if [ "$(tr '\n' ' ' < "$t/calls")" = "MidgardiOSMaps MidgardMacMaps " ]; then
    pass "both apps are tested, the Maps edition only"
else
    fail "tested: $(cat "$t/calls")"
fi

t="$(apps both-pass MidgardiOS MidgardMac)"
run_audit "$t" STUB_FAIL_MidgardMacMaps=dynamic_type
if [ "$rc" -eq 0 ] && [ -z "$(echo "$out" | tr -d ' ')" ]; then
    pass "iOS and macOS, all measured pass: nothing failed"
else
    fail "all pass: rc=$rc out='$out'"
fi

t="$(apps nobundle MidgardiOS MidgardMac)"
run_audit "$t" STUB_NO_BUNDLE=1
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then pass "no result bundle: fails closed"; else fail "no bundle: rc=$rc out='$out'"; fi

t="$(apps notests MidgardiOS MidgardMac)"
run_audit "$t" STUB_NO_TESTS=1
if [ "$rc" -ne 0 ] && [ -z "$out" ]; then
    pass "no affordance test ran: fails closed"
else
    fail "no tests: rc=$rc out='$out'"
fi

t="$(apps edition-fails MidgardiOS MidgardMac)"
run_audit "$t" STUB_EDITION_RC=1
if [ "$rc" -ne 0 ] && [ -z "$out" ] && [ ! -f "$t/calls" ] && grep -q 'check-edition.sh failed' "$t/stderr"; then
    pass "check-edition.sh failing: nothing is audited"
else
    fail "check-edition failing: rc=$rc out='$out' $(cat "$t/stderr")"
fi

# 4b. Keyboard navigation (macOS): the global AppleKeyboardUIMode, through the stub.
kbd_case() { # kbd_case NAME BEFORE [VAR=value...]: BEFORE is a value or "absent"; tree in $t, state in $kbd
    local name="$1" before="$2"; shift 2
    t="$(apps "kbd-$name" MidgardiOS MidgardMac)"
    kbd="$t/kbd"
    [ "$before" = absent ] || echo "$before" > "$kbd"
    run_audit "$t" STUB_KBD="$kbd" STUB_RESTORE="$t/cache/audit-kbd-restore" STUB_FAIL_MidgardMacMaps=dynamic_type "$@"
}
real_before="$(defaults read -g AppleKeyboardUIMode 2>/dev/null || echo absent)"
for ci in GITHUB_ACTIONS=true CI=true; do
    kbd_case "ci-absent-${ci%%=*}" absent "$ci"
    if [ "$rc" -eq 0 ] && [ ! -f "$kbd" ] && grep -q '^write -g AppleKeyboardUIMode -int 2$' "$kbd.log" \
        && [ "$(tail -n 1 "$kbd.log")" = "delete -g AppleKeyboardUIMode" ] && [ -z "$(echo "$out" | tr -d ' ')" ]; then
        pass "$ci, setting absent: turned on for the run, key deleted after"
    else
        fail "$ci, absent: rc=$rc out='$out' kbd=$(cat "$kbd" 2>/dev/null) log=$(tr '\n' ';' < "$kbd.log") $(cat "$t/stderr")"
    fi
done
kbd_case ci-off 0 CI=true
if [ "$rc" -eq 0 ] && [ "$(cat "$kbd" 2>/dev/null)" = 0 ] && grep -q '^write -g AppleKeyboardUIMode -int 2$' "$kbd.log" \
    && [ "$(tail -n 1 "$kbd.log")" = "write -g AppleKeyboardUIMode -int 0" ]; then
    pass "CI, setting off (0): turned on for the run, 0 restored after"
else
    fail "CI, off: rc=$rc kbd=$(cat "$kbd" 2>/dev/null) log=$(tr '\n' ';' < "$kbd.log")"
fi
kbd_case ci-fail absent CI=true STUB_NO_BUNDLE=1
if [ "$rc" -ne 0 ] && [ ! -f "$kbd" ]; then
    pass "CI, the audit fails closed: the setting is still restored"
else
    fail "CI, failing audit: rc=$rc kbd=$(cat "$kbd" 2>/dev/null)"
fi
kbd_case ci-on 3 GITHUB_ACTIONS=true
if [ "$rc" -eq 0 ] && [ "$(cat "$kbd")" = 3 ] && ! grep -qE '^(write|delete)' "$kbd.log"; then
    pass "CI, setting already on (3): left as it is"
else
    fail "CI, on: rc=$rc kbd=$(cat "$kbd" 2>/dev/null) log=$(tr '\n' ';' < "$kbd.log")"
fi
both="MidgardiOSMaps MidgardMacMaps "
for before in absent 0; do
    kbd_case "local-off-$before" "$before"
    after="$(cat "$kbd" 2>/dev/null || echo absent)"
    if [ "$rc" -eq 0 ] && [ -z "$(echo "$out" | tr -d ' ')" ] && [ "$after" = "$before" ] \
        && grep -q '^write -g AppleKeyboardUIMode -int 2$' "$kbd.log" \
        && grep -q '^restore-file-before-write=yes$' "$kbd.log" \
        && grep -q 'turned on Keyboard navigation' "$t/stderr" && grep -q 'put back to' "$t/stderr" \
        && [ ! -e "$t/cache/audit-kbd-restore" ] && [ ! -e "$t/cache/audit-kbd.lock" ] \
        && [ "$(tr '\n' ' ' < "$t/calls")" = "$both" ]; then
        pass "not CI, setting $before: turned on for the run (restore file first), put back after, said so"
    else
        fail "not CI, $before: rc=$rc out='$out' after=$after log=$(tr '\n' ';' < "$kbd.log") $(cat "$t/stderr")"
    fi
done
kbd_case local-keep 0 MIDGARD_AUDIT_KBD=keep
if [ "$rc" -eq 0 ] && [ "$out" = "keyboard_navigation " ] && [ "$(cat "$kbd")" = 0 ] \
    && ! grep -qE '^(write|delete)' "$kbd.log" && grep -q 'MIDGARD_AUDIT_KBD=keep' "$t/stderr" \
    && [ ! -e "$t/cache/audit-kbd.lock" ]; then
    pass "MIDGARD_AUDIT_KBD=keep: left alone, keyboard_navigation reported failed"
else
    fail "keep: rc=$rc out='$out' log=$(tr '\n' ';' < "$kbd.log") $(cat "$t/stderr")"
fi
# A run that did not finish left the setting on and its restore file (holding 0): the
# next run puts 0 back first, turns it on for itself, and puts 0 back again at the end.
t="$(apps kbd-stale MidgardiOS MidgardMac)"
kbd="$t/kbd"; echo 2 > "$kbd"; mkdir -p "$t/cache"; echo 0 > "$t/cache/audit-kbd-restore"
run_audit "$t" STUB_KBD="$kbd" STUB_RESTORE="$t/cache/audit-kbd-restore" STUB_FAIL_MidgardMacMaps=dynamic_type
first_write="$(grep -m 1 -E '^(write|delete)' "$kbd.log")"
if [ "$rc" -eq 0 ] && [ "$first_write" = "write -g AppleKeyboardUIMode -int 0" ] && [ "$(cat "$kbd")" = 0 ] \
    && [ ! -e "$t/cache/audit-kbd-restore" ]; then
    pass "a restore file left by an unfinished run is applied first, and the setting ends as it was"
else
    fail "stale restore: rc=$rc first='$first_write' kbd=$(cat "$kbd") log=$(tr '\n' ';' < "$kbd.log") $(cat "$t/stderr")"
fi
# Safeguard 2: a lock held by a live audit refuses the run; a dead holder's is taken over.
t="$(apps kbd-locked MidgardiOS MidgardMac)"
kbd="$t/kbd"; echo 0 > "$kbd"; mkdir -p "$t/cache/audit-kbd.lock"; echo $$ > "$t/cache/audit-kbd.lock/pid"
run_audit "$t" STUB_KBD="$kbd" STUB_FAIL_MidgardMacMaps=dynamic_type
if [ "$rc" -ne 0 ] && [ -z "$out" ] && [ ! -f "$t/calls" ] && ! grep -qE '^(write|delete)' "$kbd.log" 2>/dev/null \
    && grep -q 'another audit' "$t/stderr" && [ -d "$t/cache/audit-kbd.lock" ]; then
    pass "a lock held by a live audit: refused, setting and lock untouched"
else
    fail "live lock: rc=$rc out='$out' $(cat "$t/stderr")"
fi
t="$(apps kbd-deadlock MidgardiOS MidgardMac)"
kbd="$t/kbd"; echo 0 > "$kbd"; mkdir -p "$t/cache/audit-kbd.lock"; echo 999999 > "$t/cache/audit-kbd.lock/pid"
run_audit "$t" STUB_KBD="$kbd" STUB_RESTORE="$t/cache/audit-kbd-restore" STUB_FAIL_MidgardMacMaps=dynamic_type
if [ "$rc" -eq 0 ] && [ "$(cat "$kbd")" = 0 ] && [ ! -e "$t/cache/audit-kbd.lock" ]; then
    pass "a lock whose holder is gone is taken over, and released after"
else
    fail "dead lock: rc=$rc $(cat "$t/stderr")"
fi
kbd_case local-on 2
if [ "$rc" -eq 0 ] && [ -z "$(echo "$out" | tr -d ' ')" ] && [ "$(cat "$kbd")" = 2 ] \
    && ! grep -qE '^(write|delete)' "$kbd.log" && ! grep -q 'Keyboard navigation' "$t/stderr"; then
    pass "not CI, setting on: no change, nothing reported"
else
    fail "not CI, on: rc=$rc out='$out' log=$(tr '\n' ';' < "$kbd.log") $(cat "$t/stderr")"
fi
if [ "$(defaults read -g AppleKeyboardUIMode 2>/dev/null || echo absent)" = "$real_before" ]; then
    pass "the real global AppleKeyboardUIMode is untouched"
else
    fail "the real global AppleKeyboardUIMode changed from $real_before"
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
    (cd "$t" && env -u CI -u GITHUB_ACTIONS "$@" PATH="$stubs:$PATH" STUB_CALLS="$t/calls" MIDGARD_BUILD_CACHE="$t/cache" \
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
job "$t" STUB_FAIL_MidgardMacMaps=dynamic_type
if [ "$rc" -ne 0 ] && [ -z "$(ls "$t/clients/shells/apple/audits" 2>/dev/null)" ]; then
    pass "CI job, macOS app alone: fails, no record"
else
    fail "CI job, macOS alone: rc=$rc $(ls "$t/clients/shells/apple/audits" 2>/dev/null) $(cat "$t/stdout")"
fi
t="$(job_tree job-fail MidgardiOS MidgardMac)"
job "$t" STUB_FAIL_MidgardiOSMaps=contrast_aa STUB_FAIL_MidgardMacMaps=dynamic_type
rec="$t/clients/shells/apple/audits/0.0.0-test.toml"
if [ "$rc" -ne 0 ] && grep -q '^contrast_aa *= "fail"' "$rec" 2>/dev/null && grep -q '^overall *= "fail"' "$rec" \
    && grep -q '^dynamic_type *= "pass"' "$rec" && grep -q '^tool_version = "Xcode 26.2"' "$rec" \
    && grep -q '^runner *= "test-runner"' "$rec"; then
    pass "CI job, a failed affordance: the record says so, overall fail"
else
    fail "CI job, failed affordance: rc=$rc $(cat "$rec" 2>/dev/null) $(cat "$t/stdout")"
fi
t="$(job_tree job-both MidgardiOS MidgardMac)"
job "$t" STUB_FAIL_MidgardMacMaps=dynamic_type
rec="$t/clients/shells/apple/audits/0.0.0-test.toml"
if [ "$rc" -eq 0 ] && grep -q '^overall *= "pass"' "$rec" 2>/dev/null && [ "$(grep -c '= "pass"$' "$rec")" -eq 8 ] \
    && grep -q '^device *= "iPhone 17 simulator (iOS 26.2) + macOS 15.5 (arm64)"' "$rec"; then
    pass "CI job, both apps measured pass: record says pass for all seven and names both devices"
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
