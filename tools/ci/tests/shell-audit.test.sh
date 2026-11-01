#!/usr/bin/env bash
# Tests for the web and extension forms of the shell-audit job in tools/ci/run.sh,
# against a throwaway tree with stub npm, node and npx. [RC-49] [VLD-ANB-MIDGARD-0058]
# [AC-68.1]
#
#   bash tools/ci/tests/shell-audit.test.sh
#
# The claim under test: the audit server and the wait for it run from the CI-owned
# lockfile's node_modules/.bin, installed with install scripts off, and nothing is
# fetched by name with npx; a shell with no project still fails closed.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
ci="$here/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# tree NAME [no-project]: a checkout holding run.sh, lib.sh, the audit lockfile and a
# web shell; prints its root.
tree() {
    local root="$work/$1"
    mkdir -p "$root/tools/ci/shell-audit" "$root/clients/shells/web"
    cp "$ci/run.sh" "$ci/lib.sh" "$root/tools/ci/"
    cp "$ci/shell-audit/package.json" "$ci/shell-audit/package-lock.json" "$root/tools/ci/shell-audit/"
    echo 'screen_reader = "conformant"' > "$root/clients/shells/web/accessibility.toml"
    [ "${2:-}" = no-project ] || echo '{"private": true}' > "$root/clients/shells/web/package.json"
    echo "$root"
}

# Stubs log "<name> <cwd relative to the tree> <args>" to $CALLS. `npm ci` in the audit
# directory installs logging stubs of the two tools; `npm run build` makes dist/.
stubs="$work/stubs"
mkdir -p "$stubs"
cat > "$stubs/npm" <<'STUB'
#!/usr/bin/env bash
echo "npm ${PWD#"$TREE"/} $*" >> "$CALLS"
if [ "$1" = ci ] && [ "${PWD#"$TREE"/}" = tools/ci/shell-audit ]; then
    mkdir -p node_modules/.bin
    printf '#!/usr/bin/env bash\necho "http-server $0 $*" >> "$CALLS"\nexec sleep 30\n' > node_modules/.bin/http-server
    printf '#!/usr/bin/env bash\nfor _ in $(seq 50); do grep -q ^http-server "$CALLS" && break; sleep 0.1; done\necho "wait-on $0 $*" >> "$CALLS"\n' > node_modules/.bin/wait-on
    chmod +x node_modules/.bin/http-server node_modules/.bin/wait-on
elif [ "$1" = run ] && [ "$2" = build ]; then
    mkdir -p dist
fi
STUB
printf '#!/usr/bin/env bash\necho "node $*" >> "$CALLS"\n' > "$stubs/node"
printf '#!/usr/bin/env bash\necho "${0##*/} $*" >> "$CALLS"\nexit 1\n' > "$stubs/npx"
chmod +x "$stubs/npm" "$stubs/node" "$stubs/npx"

audit() { # audit ROOT SHELL: run the job; the calls go to ROOT/calls
    local root="$1"
    : > "$root/calls"
    TREE="$root" CALLS="$root/calls" PATH="$stubs:$PATH" GITHUB_SHA=0123abcd RUNNER_NAME=test-runner \
        bash "$root/tools/ci/run.sh" shell-audit "$2" 0.1.0-test
}

root="$(tree web)"
if out="$(audit "$root" web 2>&1)"; then
    pass "the web audit runs with stub tools"
else
    fail "the web audit runs with stub tools: $out"
fi
calls="$(cat "$root/calls")"
grep -qx 'npm tools/ci/shell-audit ci --ignore-scripts --no-audit --no-fund' <<<"$calls" &&
    pass "the audit tools are installed from the lockfile with install scripts off" ||
    fail "the audit tools are installed with npm ci --ignore-scripts: $calls"
grep -qx "http-server $root/tools/ci/shell-audit/node_modules/.bin/http-server clients/shells/web/dist -p 4173 -s" <<<"$calls" &&
    pass "http-server runs from tools/ci/shell-audit/node_modules/.bin" ||
    fail "http-server runs from the locked install: $calls"
grep -qx "wait-on $root/tools/ci/shell-audit/node_modules/.bin/wait-on http://127.0.0.1:4173" <<<"$calls" &&
    pass "wait-on runs from tools/ci/shell-audit/node_modules/.bin" ||
    fail "wait-on runs from the locked install: $calls"
grep -q '^node clients/shells/web/a11y/axe-audit.mjs ' <<<"$calls" &&
    pass "the axe audit runs once the server is up" || fail "the axe audit runs: $calls"
grep -q '^npx' <<<"$calls" && fail "npx: never invoked: $calls" || pass "npx: never invoked"
grep -q 'npx' "$ci/run.sh" && fail "run.sh does not mention npx" || pass "run.sh does not mention npx"

root="$(tree bare no-project)"
if out="$(audit "$root" web 2>&1)"; then fail "a shell with no project fails closed"; else
    grep -q 'no buildable project' <<<"$out" && ! grep -q '^npm' "$root/calls" &&
        pass "a shell with no project fails closed, before anything is installed" ||
        fail "a shell with no project fails closed before installing: $out"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all shell-audit tests passed"
