#!/usr/bin/env bash
# Tests for tools/ci/js-artefacts.sh, against throwaway workspaces.
# [AC-RF-SC-5a] [AC-RF-SC-5b] [AC-M-6]
#
#   bash tools/ci/tests/js-artefacts.test.sh
#
# The claims under test: the JavaScript artefacts are derived from the workspace, so a
# UI or an SDK added to it is gated with no edit anywhere; and the build proves each
# one reproducible by building it twice, failing when the two results differ.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../js-artefacts.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

run() { JS_ARTEFACTS_ROOT="$1" bash "$script" "${@:2}"; }

# package ROOT DIR NAME [VERSION]
package() {
    mkdir -p "$1/$2/src"
    printf '{\n  "name": "%s",\n  "version": "%s",\n  "private": true\n}\n' "$3" "${4:-1.2.3}" > "$1/$2/package.json"
    echo "export const x = 1;" > "$1/$2/src/index.ts"
}

# --- derivation ----------------------------------------------------------------------
root="$work/derive"
mkdir -p "$root"
cat > "$root/pnpm-workspace.yaml" <<'EOF'
# members
packages:
  - "sdk/ts"
  - "ui/*"
  - 'products/*/ui'
  - "clients/shells/web"
EOF
package "$root" sdk/ts @acme/sdk 2.0.0
package "$root" ui/kit @acme/kit
package "$root" products/console/ui @acme/console
: > "$root/products/console/ui/vite.config.ts"
package "$root" products/portal/ui @acme/portal
: > "$root/products/portal/ui/vite.config.mjs"
mkdir -p "$root/products/empty/ui"   # matches the glob, holds no package: not a member

expected="$(printf 'ui-bundle acme-console products/console/ui acme-console.ui.tar @acme/console\nui-bundle acme-portal products/portal/ui acme-portal.ui.tar @acme/portal\nnpm-package acme-sdk sdk/ts acme-sdk-2.0.0.tgz @acme/sdk')"
if out="$(run "$root" --rows 2>&1)" && [ "$out" = "$expected" ]; then
    pass "UI bundles and SDK packages are derived; internal libraries are not artefacts"
else
    fail "derivation: $out"
fi

package "$root" products/newcomer/ui @acme/newcomer
: > "$root/products/newcomer/ui/vite.config.ts"
run "$root" --rows 2>/dev/null | grep -qx 'ui-bundle acme-newcomer products/newcomer/ui acme-newcomer.ui.tar @acme/newcomer' \
    && pass "a UI added to the workspace is an artefact with no edit anywhere" \
    || fail "a UI added to the workspace is an artefact with no edit anywhere"

package "$root" sdk/py-shim @acme/console
sed -i 's|  - "sdk/ts"|  - "sdk/*"|' "$root/pnpm-workspace.yaml"
: > "$root/sdk/py-shim/vite.config.ts"
if run "$root" --check >/dev/null 2>"$work/err"; then
    fail "two packages that would share a registry id fail the check"
else
    grep -q 'more than one package' "$work/err" && pass "two packages that would share a registry id fail the check" \
        || fail "the clash is explained: $(cat "$work/err")"
fi

root="$work/none"
mkdir -p "$root"
[ -z "$(run "$root" --rows 2>/dev/null)" ] && pass "a repository with no JS workspace ships no JS artefacts" \
    || fail "a repository with no JS workspace ships no JS artefacts"

# --- the reproducible build ------------------------------------------------------------
if ! command -v pnpm >/dev/null 2>&1; then
    echo "skip  the build tests (pnpm is not on PATH)"
else
    root="$work/build"
    mkdir -p "$root"
    printf 'packages:\n  - "sdk/*"\n' > "$root/pnpm-workspace.yaml"
    printf '{ "name": "root", "private": true }\n' > "$root/package.json"
    package "$root" sdk/steady @acme/steady 1.0.0
    out="$work/out"

    if log="$(MIDGARD_SOURCE_REVISION=abc123 run "$root" build acme-steady --out "$out" 2>&1)"; then
        pass "a deterministic package builds twice to the same bytes"
    else
        fail "a deterministic package builds twice to the same bytes: $log"
    fi
    [ -s "$out/acme-steady-1.0.0.tgz" ] && pass "the artefact is emitted" || fail "the artefact is emitted"
    if grep -q '"package": "acme-steady"' "$out/acme-steady.repro.json" 2>/dev/null &&
       grep -q '"reproducible": true, "excepted": false' "$out/acme-steady.repro.json"; then
        pass "the report is in the form mg-sign embeds"
    else
        fail "the report is in the form mg-sign embeds"
    fi
    digest="$( (sha256sum "$out/acme-steady-1.0.0.tgz" 2>/dev/null || shasum -a 256 "$out/acme-steady-1.0.0.tgz") | awk '{print $1}')"
    grep -q "\"sha256\": \"$digest\"" "$out/acme-steady.repro.json" \
        && pass "the report's digest is the emitted artefact's" || fail "the report's digest is the emitted artefact's"
    [ "$(cat "$out/.commit")" = abc123 ] && pass "the build records the revision it was built from" \
        || fail "the build records the revision it was built from: $(cat "$out/.commit" 2>&1)"

    # Injected non-determinism: a prepack step that stamps the clock into the package.
    package "$root" sdk/stamped @acme/stamped 1.0.0
    cat > "$root/sdk/stamped/package.json" <<'EOF'
{
  "name": "@acme/stamped",
  "version": "1.0.0",
  "private": true,
  "scripts": { "prepack": "node -e \"require('fs').writeFileSync('src/stamp.ts', String(process.hrtime.bigint()))\"" }
}
EOF
    if log="$(run "$root" build acme-stamped --out "$out" 2>&1)"; then
        fail "a package that differs between two builds MUST fail: $log"
    else
        grep -q 'acme-stamped (@acme/stamped) is NOT reproducible' <<<"$log" \
            && pass "a package that differs between two builds fails and is named" \
            || fail "the failure names the package: $log"
    fi
    [ ! -e "$out/acme-stamped-1.0.0.tgz" ] && pass "and nothing is emitted for it" || fail "and nothing is emitted for it"
    grep -q '"reproducible": false' "$out/acme-stamped.repro.json" 2>/dev/null \
        && pass "but its report is written, with both digests" || fail "but its report is written, with both digests"

    run "$root" build no-such-artefact --out "$out" >/dev/null 2>&1 \
        && fail "an artefact the workspace does not ship is an error" || pass "an artefact the workspace does not ship is an error"
fi

# --- the real workspace -------------------------------------------------------------------
if bash "$script" --check >/dev/null 2>"$work/err"; then
    pass "this workspace's own artefacts derive cleanly"
else
    fail "this workspace's own artefacts derive cleanly: $(cat "$work/err")"
fi

if [ "$failures" -gt 0 ]; then
    echo "js-artefacts tests: $failures failure(s)"
    exit 1
fi
echo "js-artefacts tests: all passed"
