#!/usr/bin/env bash
# The CI tooling never runs a package fetched by name at run time: no `npx --yes`, no
# `npx -y`, and no `npx <name>` without an exact `@<version>` unless `--no-install`
# forbids the fetch. Scans tools/, .github/ and .gitlab-ci.yml. [VLD-ANB-MIDGARD-0058]
# [AC-68.2] Nor does it run a dependency's lifecycle scripts: every `pnpm install` it
# runs passes --ignore-scripts, so an install on the signing runner executes nothing it
# fetched.
#
#   bash tools/ci/tests/npx-pin.test.sh
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# lint FILE...: print FILE:LINE: TEXT for every offending npx call; fail if there is any.
lint() {
    awk '
        {
            line = $0
            while (match(line, /(^|[^A-Za-z0-9_.\/-])npx[ \t]+/)) {
                line = substr(line, RSTART + RLENGTH)
                n = split(line, tok, /[ \t]+/)
                bad = 0; noinstall = 0; name = ""
                for (i = 1; i <= n; i++) {
                    t = tok[i]
                    if (t ~ /^(--yes|-y)(=|$)/) { bad = 1; break }
                    if (t == "--no-install") { noinstall = 1; continue }
                    if (t ~ /^-/) continue
                    name = t; break
                }
                gsub(/^["'\''(]+/, "", name)
                if (!bad && !noinstall && name !~ /^@?[^@]+@[0-9]/) bad = 1
                if (bad) { print FILENAME ":" FNR ": " $0; found = 1; break }
            }
        }
        END { exit found }
    ' "$@"
}

# --- the lint itself, against fixtures ------------------------------------------------
fixture() { printf '%s\n' "$@" > "$work/fixture"; }
refused() { # refused NAME LINE
    fixture "echo before" "$2"
    local out
    if out="$(lint "$work/fixture")"; then fail "$1 is refused"; else
        grep -qF "fixture:2: $2" <<<"$out" && pass "$1 is refused, naming the line" ||
            fail "$1 names the line: $out"
    fi
}
allowed() { # allowed NAME LINE
    fixture "$2"
    local out
    out="$(lint "$work/fixture")" && pass "$1 is allowed" || fail "$1 is allowed: $out"
}

refused "npx --yes" 'npx --yes foo'
refused "npx -y" '  (cd web && exec npx -y http-server dist)'
refused "an unversioned npx" 'npx wait-on http://127.0.0.1:4173'
refused "a dist-tag is not a version" 'npx foo@latest'
refused "an unversioned scoped package" 'npx @scope/tool'
refused "--yes even with an exact version" 'npx --yes foo@1.2.3'
allowed "--no-install with an exact version" 'npx --no-install foo@1.2.3'
allowed "--no-install, which cannot fetch" 'npx --no-install foo'
allowed "an exact version" 'npx foo@1.2.3 --port 1'
allowed "an exact scoped version" 'npx @scope/tool@2.0.0'
allowed "prose naming the tool" 'the audit never calls `npx`; see npx-pin.test.sh'

# --- the repository -------------------------------------------------------------------
self="$(cd "$here" && pwd)/$(basename "$0")"
files=()
while IFS= read -r f; do
    [ "$(cd "$(dirname "$f")" && pwd)/$(basename "$f")" = "$self" ] || files+=("$f")
done < <(cd "$repo" && grep -rIl --exclude-dir=node_modules -e npx tools .github .gitlab-ci.yml 2>/dev/null |
    sed "s|^|$repo/|")
if [ "${#files[@]}" -eq 0 ]; then
    pass "the repository runs no npx at all"
elif out="$(lint "${files[@]}")"; then
    pass "the repository's npx calls are all pinned (${#files[@]} file(s) mention npx)"
else
    fail "the repository runs an unpinned npx:"
    echo "$out"
fi

# --- installs run no lifecycle scripts -------------------------------------------------
installs="$(cd "$repo" && grep -rInE --exclude-dir=node_modules     '^[[:space:]]*(- )?(run: *)?pnpm (install|i)([[:space:]]|$)' tools .github .gitlab-ci.yml 2>/dev/null)"
unscripted="$(grep -v -e '--ignore-scripts' <<<"$installs")"
if [ -z "$installs" ]; then
    fail "the scan finds the js-build job's pnpm install"
elif [ -z "$unscripted" ]; then
    pass "every pnpm install the CI runs passes --ignore-scripts ($(wc -l <<<"$installs") found)"
else
    fail "a pnpm install runs lifecycle scripts:"$'
'"$unscripted"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all npx pin tests passed"
