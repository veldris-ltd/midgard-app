#!/usr/bin/env bash
# Fetch the Windows services a successful Windows CI run built for one commit, on the dev
# Mac, for packaging (docs/plans/2026-10-23-windows-app-design.md §4):
#   $MIDGARD_BUILD_DIR/win/<sha>/  the six services (.exe) and BUILD.txt
# (MIDGARD_BUILD_DIR defaults to ~/Library/Caches/midgard-build, outside the suite folder.)
#
# The run is the latest successful run of .github/workflows/windows.yml for that commit,
# and the artefact is midgard-services-windows-x64-<sha> (kept 30 days). It is downloaded
# with `gh run download` into a fresh staging folder, which replaces the commit's folder
# only once BUILD.txt names the commit and all six services are there. The files are
# then listed with their sizes and what `file` makes of them. Nothing is run.
#
#   bash tools/win-app/fetch-artifacts.sh <sha>     (a full 40-hex commit, or a prefix
#                                                    this checkout can resolve)
set -euo pipefail
cd "$(dirname "$0")/../.."

REPO=veldris-ltd/midgard-app
WORKFLOW=windows.yml
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"

die() { echo "fetch-artifacts: $*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: $0 <commit sha>"
command -v gh >/dev/null || die "gh (the GitHub CLI) is needed"
sha="$1"
if ! [[ "$sha" =~ ^[0-9a-f]{40}$ ]]; then
    sha="$(git rev-parse --verify --quiet "$1^{commit}")" || die "$1 is not a commit here"
fi

run="$(gh run list -R "$REPO" --workflow "$WORKFLOW" --commit "$sha" --status success \
    --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
[ -n "$run" ] || die "no successful $WORKFLOW run for $sha"
echo "run: https://github.com/$REPO/actions/runs/$run"

mkdir -p "$BUILD_ROOT/win"
dest="$BUILD_ROOT/win/$sha"
stage="$(mktemp -d "$BUILD_ROOT/win/.fetch-$sha.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
gh run download "$run" -R "$REPO" -n "midgard-services-windows-x64-$sha" -D "$stage"

[ -f "$stage/BUILD.txt" ] || die "the artefact has no BUILD.txt"
built="$(sed -n 's/^commit: *//p' "$stage/BUILD.txt" | head -n 1)"
[ "$built" = "$sha" ] || die "BUILD.txt names commit ${built:-none}, not $sha"
for b in "${BINARIES[@]}"; do
    [ -f "$stage/$b.exe" ] || die "the artefact has no $b.exe"
done

rm -rf "$dest"
mv "$stage" "$dest"
chmod 755 "$dest"
trap - EXIT

echo "into: $dest"
cat "$dest/BUILD.txt"
for b in "${BINARIES[@]}"; do
    f="$dest/$b.exe"
    printf '%10s  %s: %s\n' "$(wc -c <"$f" | tr -d ' ')" "$b.exe" "$(file -b "$f")"
done
