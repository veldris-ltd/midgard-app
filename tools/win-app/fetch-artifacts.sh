#!/usr/bin/env bash
# Fetch the Windows binaries a successful Windows CI run built for one commit, on the dev
# Mac, for packaging (docs/plans/2026-10-23-windows-app-design.md §4):
#   $MIDGARD_BUILD_DIR/win/<sha>/services/  the six services (.exe) and BUILD.txt
#   $MIDGARD_BUILD_DIR/win/<sha>/shell/     Midgard.exe, midgard-frames.exe, BUILD.txt and
#                                           vcruntime/ (the app-local Visual C++ runtime DLLs
#                                           and VERSIONS.txt)
# (MIDGARD_BUILD_DIR defaults to ~/Library/Caches/midgard-build, outside the suite folder.)
#
# The run is the latest successful run of .github/workflows/windows.yml for that commit (a
# run succeeds only when every job is green, not only the ones that built). Its artefacts are
# midgard-services-windows-x64-<sha> and midgard-shell-windows-x64-<sha> (kept 30 days). Each
# is downloaded with `gh run download` into a fresh staging folder, which replaces the
# commit's folder only once both BUILD.txt files name the commit and every binary is there.
# The files are then listed with their sizes and what `file` makes of them. Nothing is run.
#
#   bash tools/win-app/fetch-artifacts.sh <sha>     (a full 40-hex commit, or a prefix
#                                                    this checkout can resolve)
set -euo pipefail
cd "$(dirname "$0")/../.."

REPO=veldris-ltd/midgard-app
WORKFLOW=windows.yml
SERVICES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
SHELL_EXES=(Midgard midgard-frames)
# The app-local Visual C++ runtime in the shell artefact (tools/ci/run.sh VCRUNTIME_DLLS).
VCRUNTIME_DLLS=(vcruntime140 vcruntime140_1 msvcp140 msvcp140_1)
BUILD_ROOT="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"

die() { echo "fetch-artifacts: $*" >&2; exit 1; }

[ $# -eq 1 ] || die "usage: $0 <commit sha>"
command -v gh >/dev/null || die "gh (the GitHub CLI) is needed"
sha="$1"
if ! [[ "$sha" =~ ^[0-9a-f]{40}$ ]]; then
    sha="$(git rev-parse --verify --quiet "$1^{commit}")" || die "$1 is not a commit here"
fi

# The latest run that succeeded as a whole: every job green, not only the ones that built.
run="$(gh run list -R "$REPO" --workflow "$WORKFLOW" --commit "$sha" --status success \
    --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
[ -n "$run" ] || die "no successful (all green) $WORKFLOW run for $sha"
echo "run: https://github.com/$REPO/actions/runs/$run"

mkdir -p "$BUILD_ROOT/win"
dest="$BUILD_ROOT/win/$sha"
stage="$(mktemp -d "$BUILD_ROOT/win/.fetch-$sha.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

# One artefact into $stage/<part>, its BUILD.txt naming the commit and its binaries there.
fetch() {  # part artefact-name exe...
    local part=$1 name=$2 built e
    shift 2
    gh run download "$run" -R "$REPO" -n "$name" -D "$stage/$part" \
        || die "the run has no artefact $name (expired after 30 days, or never uploaded)"
    [ -f "$stage/$part/BUILD.txt" ] || die "$name has no BUILD.txt"
    built="$(sed -n 's/^commit: *//p' "$stage/$part/BUILD.txt" | tr -d '\r' | head -n 1)"
    [ "$built" = "$sha" ] || die "$name: BUILD.txt names commit ${built:-none}, not $sha"
    for e in "$@"; do
        [ -f "$stage/$part/$e.exe" ] || die "$name has no $e.exe"
    done
}
fetch services "midgard-services-windows-x64-$sha" "${SERVICES[@]}"
fetch shell "midgard-shell-windows-x64-$sha" "${SHELL_EXES[@]}"
[ -f "$stage/shell/vcruntime/VERSIONS.txt" ] || die "the shell artefact has no vcruntime/VERSIONS.txt"
for d in "${VCRUNTIME_DLLS[@]}"; do
    [ -f "$stage/shell/vcruntime/$d.dll" ] || die "the shell artefact has no vcruntime/$d.dll"
done

rm -rf "$dest"
mv "$stage" "$dest"
chmod 755 "$dest"
trap - EXIT

echo "into: $dest"
list() {  # part exe...
    local part=$1 e f
    shift
    echo "== $part"
    sed 's/^/   /' "$dest/$part/BUILD.txt"
    for e in "$@"; do
        f="$dest/$part/$e.exe"
        printf '%10s  %s: %s\n' "$(wc -c <"$f" | tr -d ' ')" "$e.exe" "$(file -b "$f")"
    done
}
list services "${SERVICES[@]}"
list shell "${SHELL_EXES[@]}"
echo "== shell/vcruntime"
sed 's/^/   /' "$dest/shell/vcruntime/VERSIONS.txt"
for d in "${VCRUNTIME_DLLS[@]}"; do
    f="$dest/shell/vcruntime/$d.dll"
    printf '%10s  %s: %s\n' "$(wc -c <"$f" | tr -d ' ')" "$d.dll" "$(file -b "$f")"
done
