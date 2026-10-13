#!/usr/bin/env bash
# Tests for tools/ci/claims.sh, against throwaway trees.
# [AC-RF-CA-1b] [AC-RF-CA-1c] [AC-RF-CA-13b]
#
#   bash tools/ci/tests/claims.test.sh
#
# The claim under test: a ledger row is held to what it cites. A path, line, test or
# criterion that does not exist fails and names the row; a row at `operable` without a
# service test fails; a row below `operable` without its fallback wording fails; an
# external row that cites something, or whose name occurs in the code, fails; and
# CLAIMS.md that is not what the ledger renders fails.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
script="$here/../claims.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# tree NAME: a root with source, a test, a smoke script, a criterion and a valid ledger
tree() {
    local root="$work/$1"
    mkdir -p "$root/docs/compliance" "$root/services/edge/src" "$root/services/edge/tests" \
        "$root/deploy/compose" "$root/deploy/acceptance"
    printf 'pub fn serve() {}\n// second line\n// third line\n' > "$root/services/edge/src/lib.rs"
    printf '#[test]\nfn it_serves() {}\n\nfn helper() {}\n\n#[test]\n#[ignore]\nfn it_restarts() {}\n' \
        > "$root/services/edge/tests/serve.rs"
    printf '#!/usr/bin/env bash\necho "--- the edge answers"\ncurl -fsS http://127.0.0.1/health\n' \
        > "$root/deploy/compose/smoke.sh"
    echo '# AC-X-1 the edge answers' > "$root/deploy/acceptance/verify-core.sh"
    cat > "$root/docs/compliance/claims.toml" <<'LEDGER'
# a ledger

[[claim]]
id = "edge"
area = "Core"
claim = "The edge answers."
status = "operable"
criteria = ["AC-X-1"]
evidence = ["services/edge/src/lib.rs:1"]
tests = ["services/edge/tests/serve.rs::it_serves"]
service_tests = ["deploy/compose/smoke.sh:2", "services/edge/tests/serve.rs::it_restarts"]

[[claim]]
id = "routing"
area = "Core"
claim = "Routing between regions."
status = "reference"
tests = ["services/edge/tests/serve.rs::it_serves"]
fallback = "Routing is implemented and tested as a library; no service uses it."
milestone = "A service uses it."

[[claim]]
id = "otherwise"
area = "Outside"
claim = "Otherwise is a separate tool."
status = "external"
LEDGER
    echo "$root"
}

run() { MIDGARD_CLAIMS_ROOT="$1" bash "$script" "${@:2}" 2>&1; }
rendered() { run "$1" --render >/dev/null; }

# edit ROOT SED-EXPRESSION: change the ledger, and render so only the change is at fault
edit() {
    sed -i "$2" "$1/docs/compliance/claims.toml"
    rendered "$1"
}

# expect_fail LABEL ROOT TEXT [ARG...]: the check fails and names TEXT
expect_fail() {
    local out
    if out="$(run "$2" --check "${@:4}")"; then
        fail "$1: passed, expected a failure"
    elif grep -qF -- "$3" <<<"$out"; then
        pass "$1"
    else
        fail "$1: failed without naming '$3': $out"
    fi
}

root="$(tree clean)"
rendered "$root"
if out="$(run "$root" --check)" && grep -q '3 claims' <<<"$out"; then
    pass "a ledger whose citations all exist passes"
else
    fail "a clean ledger: $out"
fi
grep -q '^### `edge` (operable)' "$root/docs/compliance/CLAIMS.md" \
    && grep -q 'Until it is operable, say:' "$root/docs/compliance/CLAIMS.md" \
    && pass "the rendering shows each row, and the fallback wording of a row below operable" \
    || fail "the rendering lacks a row or its fallback wording"

root="$(tree stale)"
rendered "$root"
echo "hand edit" >> "$root/docs/compliance/CLAIMS.md"
expect_fail "a hand-edited CLAIMS.md fails" "$root" "is not what docs/compliance/claims.toml renders"

root="$(tree unrendered)"
expect_fail "a missing CLAIMS.md fails" "$root" "CLAIMS.md is missing"

root="$(tree path)"
edit "$root" 's#services/edge/src/lib.rs:1#services/edge/src/gone.rs:1#'
expect_fail "an evidence path that does not exist fails, with the row named" "$root" \
    "claim 1 (edge): evidence services/edge/src/gone.rs:1 names a file that does not exist"

root="$(tree line)"
edit "$root" 's#services/edge/src/lib.rs:1#services/edge/src/lib.rs:40#'
expect_fail "a line anchor past the end of the file fails" "$root" "points past the end of the file"

root="$(tree test)"
edit "$root" 's#tests = \["services/edge/tests/serve.rs::it_serves"\]#tests = ["services/edge/tests/serve.rs::it_flies"]#'
expect_fail "a test that does not exist fails" "$root" "it_flies is not a #[test] function"

root="$(tree helper)"
edit "$root" '0,/::it_serves/s//::helper/'
expect_fail "a function that is not a test does not count as one" "$root" "helper is not a #[test] function"

root="$(tree criterion)"
edit "$root" 's#AC-X-1#AC-X-9#'
expect_fail "an unknown criterion id fails" "$root" "criterion AC-X-9 is not an id"

root="$(tree library-only)"
edit "$root" '/^service_tests/d'
expect_fail "an operable row with only a library test fails" "$root" \
    "a row at operable must name a service test"

root="$(tree not-a-service-test)"
edit "$root" 's#"deploy/compose/smoke.sh:2", ##; s#serve.rs::it_restarts"\]#serve.rs"]#'
expect_fail "a bare path is not a service test" "$root" "is neither a script step"

root="$(tree no-fallback)"
edit "$root" '/^fallback/d'
expect_fail "a row below operable without fallback wording fails" "$root" "needs \"fallback\""

root="$(tree untested-library)"
edit "$root" '/^status = "reference"/{n;d}'
expect_fail "a reference row with no test fails" "$root" "must name the test that exercises the library"

root="$(tree status)"
edit "$root" 's#status = "reference"#status = "shipping"#'
expect_fail "an unknown status fails" "$root" "\"status\" must be one of"

root="$(tree duplicate)"
edit "$root" 's#id = "routing"#id = "edge"#'
expect_fail "a repeated id fails" "$root" "the id is used twice"

root="$(tree unknown-key)"
edit "$root" 's#^milestone = #owner = #'
expect_fail "an unknown key fails" "$root" "unknown key \"owner\""

root="$(tree unreadable)"
edit "$root" 's#^claim = "Routing between regions."#claim = Routing between regions#'
expect_fail "a line the reader cannot parse fails" "$root" "cannot read"

root="$(tree external-cites)"
printf 'evidence = ["services/edge/src/lib.rs:1"]\n' >> "$root/docs/compliance/claims.toml"
rendered "$root"
expect_fail "an external row that cites evidence fails" "$root" "an external row cites nothing"

root="$(tree external-in-code)"
rendered "$root"
echo '// calls Otherwise for its schema' >> "$root/services/edge/src/lib.rs"
expect_fail "an external tool named in the code fails" "$root" "the name occurs in the workspace"

root="$(tree overstated)"
printf '# Edge\nThe edge routes between regions today.\n' > "$root/README.md"
sed -i 's#^milestone = "A service uses it."#milestone = "A service uses it."\noverstated_in = ["README.md:2"]#' \
    "$root/docs/compliance/claims.toml"
rendered "$root"
if out="$(run "$root" --check)" && grep -q '1 place(s) still word' <<<"$out"; then
    pass "wording above a row's status is reported, and does not fail the plain check"
else
    fail "an overstated wording in the plain check: $out"
fi
expect_fail "and it fails the strict check" "$root" "1 place(s) still word a capability above" --strict

echo
if [ "$failures" -eq 0 ]; then
    echo "claims: all tests passed"
else
    echo "claims: $failures test(s) failed"
    exit 1
fi
