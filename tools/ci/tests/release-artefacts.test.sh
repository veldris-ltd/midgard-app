#!/usr/bin/env bash
# Tests for tools/ci/release-artefacts.sh, against throwaway workspaces.
# [AC-RF-SC-5a] [RC-07]
#
#   bash tools/ci/tests/release-artefacts.test.sh
#
# The claim under test: the release matrix is derived from the workspace, so a binary
# added to it is gated with no edit anywhere, and the annotation list can neither hide
# a binary nor invent one.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../release-artefacts.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# workspace NAME MEMBER...   a fresh workspace root with the given member directories
workspace() {
    local root="$work/$1" member list=""
    shift
    mkdir -p "$root/supply-chain"
    for member in "$@"; do list="$list    \"$member\",\n"; done
    printf '[workspace]\nmembers = [\n%b]\n\n[workspace.package]\nversion = "1.2.3"\n' "$list" > "$root/Cargo.toml"
    echo "$root"
}

# crate ROOT DIR PACKAGE [EXTRA-MANIFEST]   a member with a manifest and src/lib.rs
crate() {
    mkdir -p "$1/$2/src"
    printf '[package]\nname = "%s"\nversion = "1.2.3"\n%b' "$3" "${4:-}" > "$1/$2/Cargo.toml"
    : > "$1/$2/src/lib.rs"
}

run() { RELEASE_ARTEFACTS_ROOT="$1" bash "$script" "${@:2}"; }

# --- a workspace whose list annotates everything ------------------------------
root="$(workspace annotated svc tool lib)"
crate "$root" svc gateway
: > "$root/svc/src/main.rs"
crate "$root" tool mg-tool
: > "$root/tool/src/main.rs"
crate "$root" lib a-library
printf 'gateway  gateway  service  gateway\nmg-tool  mg-tool  tool     -\n' > "$root/supply-chain/release-artefacts.txt"

if out="$(run "$root" --rows 2>/dev/null)" &&
   [ "$out" = "$(printf 'gateway gateway service gateway\nmg-tool mg-tool tool -')" ]; then
    pass "an annotated workspace yields its annotations"
else
    fail "an annotated workspace yields its annotations: $out"
fi
[ "$(run "$root" --version)" = "1.2.3" ] && pass "the workspace version is read" || fail "the workspace version is read"
[ "$(run "$root" --images 2>/dev/null)" = '["gateway"]' ] && pass "images come from the annotations" || fail "images come from the annotations"
[ "$(run "$root" --bins gateway)" = "gateway" ] && pass "an image lists its binaries" || fail "an image lists its binaries"
if run "$root" --matrix 2>/dev/null | grep -q '{"package":"gateway","bin":"gateway","component":"service","image":"gateway"}'; then
    pass "the matrix is JSON a pipeline can consume"
else
    fail "the matrix is JSON a pipeline can consume"
fi

# --- AC-RF-SC-5a: a new binary is in the matrix with no edit ------------------
crate "$root" extra newcomer
: > "$root/extra/src/main.rs"
sed -i 's|^members = \[|members = [\n    "extra",|' "$root/Cargo.toml"
if out="$(run "$root" --rows 2>"$work/err")" && grep -qx 'newcomer newcomer tool -' <<<"$out"; then
    pass "a binary added to the workspace is gated with no list edit"
else
    fail "a binary added to the workspace is gated with no list edit: $out"
fi
grep -q 'WARNING.*does not annotate' "$work/err" && grep -q 'newcomer newcomer' "$work/err" \
    && pass "and the script says it is shipping unannotated" || fail "and the script says it is shipping unannotated"

# --- every way Cargo finds a binary --------------------------------------------
root="$(workspace layouts multi)"
crate "$root" multi multi '\n[[bin]]\nname = "declared"\npath = "src/entry.rs"\n'
: > "$root/multi/src/entry.rs"
: > "$root/multi/src/main.rs"
mkdir -p "$root/multi/src/bin/in-a-directory"
: > "$root/multi/src/bin/flat.rs"
: > "$root/multi/src/bin/in-a-directory/main.rs"
expected="$(printf 'multi declared tool -\nmulti flat tool -\nmulti in-a-directory tool -\nmulti multi tool -')"
if out="$(run "$root" --rows 2>/dev/null)" && [ "$out" = "$expected" ]; then
    pass "declared, implicit, src/bin/x.rs and src/bin/x/main.rs binaries are all found"
else
    fail "declared, implicit, src/bin/x.rs and src/bin/x/main.rs binaries are all found: $out"
fi

# --- what the list must not do --------------------------------------------------
root="$(workspace stale one)"
crate "$root" one one
: > "$root/one/src/main.rs"
printf 'one   one   tool  -\ngone  gone  tool  -\n' > "$root/supply-chain/release-artefacts.txt"
if run "$root" --check >/dev/null 2>"$work/err"; then
    fail "an entry for a binary the workspace does not build fails the check"
else
    grep -q 'gone gone' "$work/err" && pass "an entry for a binary the workspace does not build fails the check" \
        || fail "the stale entry is named: $(cat "$work/err")"
fi
run "$root" --matrix >/dev/null 2>&1 && fail "and no matrix is emitted from a wrong list" || pass "and no matrix is emitted from a wrong list"

printf 'one  one  daemon  -\n' > "$root/supply-chain/release-artefacts.txt"
run "$root" --check >/dev/null 2>&1 && fail "an unknown component kind fails the check" || pass "an unknown component kind fails the check"
printf 'one  one  tool\n' > "$root/supply-chain/release-artefacts.txt"
run "$root" --check >/dev/null 2>&1 && fail "a short entry fails the check" || pass "a short entry fails the check"

root="$(workspace clash a b)"
crate "$root" a pkg-a '\n[[bin]]\nname = "same"\npath = "src/main.rs"\n'
: > "$root/a/src/main.rs"
crate "$root" b pkg-b '\n[[bin]]\nname = "same"\npath = "src/main.rs"\n'
: > "$root/b/src/main.rs"
if run "$root" --check >/dev/null 2>"$work/err"; then
    fail "one binary name from two packages fails the check"
else
    grep -q 'more than one package' "$work/err" && pass "one binary name from two packages fails the check" \
        || fail "the clash is explained: $(cat "$work/err")"
fi

root="$(workspace empty lib)"
crate "$root" lib only-a-library
run "$root" --check >/dev/null 2>&1 && fail "a workspace with no binaries is not a pass" || pass "a workspace with no binaries is not a pass"

# --- the real workspace ----------------------------------------------------------
if [ "${RELEASE_ARTEFACTS_SKIP_REAL:-}" = 1 ]; then
    echo "skip  the real workspace (RELEASE_ARTEFACTS_SKIP_REAL=1)"
elif bash "$script" --check >/dev/null 2>"$work/err"; then
    pass "this workspace's own list checks clean"
else
    fail "this workspace's own list checks clean: $(cat "$work/err")"
fi

if [ "$failures" -gt 0 ]; then
    echo "release-artefacts tests: $failures failure(s)"
    exit 1
fi
echo "release-artefacts tests: all passed"
