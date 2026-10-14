#!/usr/bin/env bash
# ============================================================================
# Provenance-tag lint.
#
# "Provenance everywhere" is a design principle, not a convention: every claim
# has a source, every decision cites the requirement it realises. This enforces
# it on the way in — a CHANGED source file under a governed tree must cite at
# least one provenance id.
#
# Scoped to the diff deliberately. Roughly a third of the existing tree predates
# the rule; linting everything would demand a mass retrofit and would be
# switched off within a week. Linting the diff ratchets: untouched files are
# grandfathered, and anything you edit you tag.
#
# Accepted ids are the vocabulary the repo actually uses:
#   [FR-*]  functional requirement      [AC-*]  acceptance criterion
#   [NFR-*] non-functional requirement  [RC-*]  remedial / change item
#   [DD-*]  design driver               [STD:*] external standard
#
# Usage:
#   tools/ci/provenance-lint.sh [BASE_REF]
#
# BASE_REF defaults to the pull-request base, else the pushed-from commit, else
# HEAD~1. With no usable base (an initial or force push) the lint reports and
# passes — it gates changes, and there is no change set to judge.
# ============================================================================
set -uo pipefail

# Trees whose sources must cite provenance. Everything else (tools, docs, the
# design system, generated SDKs) is out of scope.
GOVERNED=(services mimir products clients)

# Source extensions the rule applies to.
EXT_RE='\.(rs|ts|tsx)$'

# Generated or vendored output that happens to sit under a governed tree.
SKIP_RE='(^|/)(node_modules|dist|target|\.vite)/'

TAG_RE='\[(FR|NFR|DD|AC|RC|STD)[-:][^]]*\]'

resolve_base() {
    if [ "$#" -ge 1 ] && [ -n "${1:-}" ]; then
        echo "$1"; return
    fi
    # A pull or merge request compares against the branch being merged into; a
    # push, against the commit the branch moved from. tools/ci/lib.sh knows where
    # each CI system records those.
    local base
    base="$(ci_base_ref)"
    echo "${base:-HEAD~1}"
}

. "$(dirname "$0")/lib.sh"
BASE="$(resolve_base "$@")"

if ! git rev-parse --verify --quiet "$BASE^{commit}" >/dev/null; then
    echo "provenance-lint: no usable base ref ('$BASE') — nothing to compare, skipping."
    exit 0
fi

# --diff-filter=d excludes deletions: a file that no longer exists cannot be
# read, and removing code needs no citation.
mapfile -t CHANGED < <(
    git diff --name-only --diff-filter=d "$BASE"...HEAD -- "${GOVERNED[@]}" 2>/dev/null |
        grep -E "$EXT_RE" |
        grep -Ev "$SKIP_RE" || true
)

if [ "${#CHANGED[@]}" -eq 0 ]; then
    echo "provenance-lint: no governed source files changed since $BASE — nothing to check."
    exit 0
fi

missing=()
for f in "${CHANGED[@]}"; do
    [ -f "$f" ] || continue
    if ! grep -Eq "$TAG_RE" "$f"; then
        missing+=("$f")
    fi
done

checked="${#CHANGED[@]}"
if [ "${#missing[@]}" -eq 0 ]; then
    echo "provenance-lint: $checked changed source file(s) since $BASE, all cite provenance."
    exit 0
fi

echo
echo "provenance-lint: ${#missing[@]} of $checked changed source file(s) cite no provenance id."
echo
for f in "${missing[@]}"; do
    # Rendered as an inline annotation on the offending file where the CI
    # system supports one.
    ci_error "No provenance tag. Cite the requirement this realises, e.g. [FR-B-1], [NFR-PQ-3], [DD-1], [AC-B-1]." "$f"
    # An annotation is attached to the file and its name does not appear in the
    # job's log, so the name is printed as well.
    echo "  $f"
done
echo
echo "Every changed source under ${GOVERNED[*]} must cite at least one of:"
echo "  [FR-*] [NFR-*] [DD-*] [AC-*] [RC-*] [STD:*]"
echo "Put it in the module doc comment, or on the item the change belongs to."
exit 1
