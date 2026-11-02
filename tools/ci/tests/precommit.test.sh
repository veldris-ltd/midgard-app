#!/usr/bin/env bash
# The pre-commit configuration keeps both of its hooks. [AC-25.7] (VLD-ANB-MIDGARD-0091)
#
#   bash tools/ci/tests/precommit.test.sh
#
# .pre-commit-config.yaml mirrors two CI checks on a developer's machine: the gitleaks
# scan of what is staged, and the format check. A hook that silently drops out of the
# file stops guarding anyone, so each one is held here by its id and its exact command.
set -uo pipefail

cd "$(dirname "$0")/../../.."
CONFIG=.pre-commit-config.yaml
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

if [ ! -f "$CONFIG" ]; then
    echo "FAIL  $CONFIG is missing"
    exit 1
fi
text="$(tr -d '\r' < "$CONFIG" | sed 's/[[:space:]]*#.*$//')"

# hook ID ENTRY: the hook is declared, with that command, as a local system hook
hook() {
    local id="$1" entry="$2" block
    block="$(awk -v id="$id" '
        /^[[:space:]]*- id:/ { inblock = ($0 ~ "- id: " id "$") }
        inblock { print }' <<<"$text")"
    if [ -z "$block" ]; then
        fail "hook $id is declared"
        return
    fi
    grep -qF "entry: $entry" <<<"$block" && pass "hook $id runs: $entry" \
        || fail "hook $id runs: $entry"
    grep -q 'language: system' <<<"$block" && pass "hook $id uses the tool on PATH (language: system)" \
        || fail "hook $id uses the tool on PATH (language: system)"
}

grep -q '^[[:space:]]*- repo: local$' <<<"$text" && pass "the hooks are local: nothing is fetched" \
    || fail "the hooks are local: nothing is fetched"
hook gitleaks "gitleaks protect --staged --config .gitleaks.toml --redact --no-banner"
hook cargo-fmt "cargo fmt --all -- --check"
# The pinned version the file tells developers to install is the one CI uses.
grep -q '^GITLEAKS_VERSION=' tools/ci/run.sh && grep -q 'GITLEAKS_VERSION' "$CONFIG" \
    && pass "the file points at the gitleaks version tools/ci/run.sh pins" \
    || fail "the file points at the gitleaks version tools/ci/run.sh pins"

if [ "$failures" -ne 0 ]; then
    echo "precommit: $failures failure(s)"
    exit 1
fi
echo "precommit: all passed"
