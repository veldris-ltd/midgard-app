#!/usr/bin/env bash
# The JavaScript half of the release manifest: which non-Rust artefacts the workspace
# ships, and a reproducible build of each. [RC-07] [AC-M-6] [AC-RF-SC-5a] [AC-RF-SC-5b]
#
#   bash tools/ci/js-artefacts.sh --rows                  "kind id dir file package" lines
#   bash tools/ci/js-artefacts.sh --check                 exit 1 if the derivation is unusable
#   bash tools/ci/js-artefacts.sh build ID... --out DIR   build each twice, compare, emit
#   bash tools/ci/js-artefacts.sh build --all --out DIR
#
# Like the Rust half (release-artefacts.sh), coverage is DERIVED: the artefacts come
# from pnpm-workspace.yaml and each member's own files, so a UI or an SDK added to the
# workspace is gated with no edit anywhere. A workspace member is
#   - a `ui-bundle`   when it has a Vite config: its built `dist/` ships;
#   - an `npm-package` when it lives under `sdk/`: its packed tarball ships;
#   - neither, otherwise — an internal library, which ships inside the bundles that
#     import it and is listed in their bills of materials.
#
# `build` is the reproducibility stage for these artefacts, the counterpart of mg-repro:
# every artefact is built TWICE and the two results must be bit-identical, or the build
# fails and names it. It writes, into DIR:
#   <file>             the artefact — a deterministic tar of dist/, or the packed tarball
#   <id>.repro.json    the report mg-sign embeds, in mg-repro's format
#   .commit            the source revision these were built from, which the release
#                      checks before it signs anything it did not build itself
#
# A ui-bundle needs the workspace installed (`pnpm install --frozen-lockfile`); an
# npm-package does not. Needs bash, sed, awk, GNU tar, and pnpm for `build`.
set -euo pipefail

# JS_ARTEFACTS_ROOT points the script at another workspace (its tests use this).
cd "${JS_ARTEFACTS_ROOT:-$(dirname "$0")/../..}"
WORKSPACE=pnpm-workspace.yaml

sha256() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }

# A string field of a package.json, first occurrence. Enough for the two flat fields
# read here (name, version); nothing is decided from a nested value.
json_field() {
    tr -d '\r' < "$1" | sed -n "s/^ *\"$2\" *: *\"\\([^\"]*\\)\".*/\\1/p" | head -1
}

# The directories the workspace names, globs expanded, that hold a package.json.
members() {
    local pattern dir
    [ -f "$WORKSPACE" ] || return 0
    tr -d '\r' < "$WORKSPACE" | sed -n 's/^ *- *["'\'']\{0,1\}\([^"'\'']*\)["'\'']\{0,1\} *$/\1/p' \
        | while read -r pattern; do
            # Unquoted on purpose: the pattern is a glob.
            for dir in $pattern; do
                if [ -f "$dir/package.json" ]; then echo "$dir"; fi
            done
        done | sort -u
}

# kind id dir file package — one line per artefact, sorted by id.
rows() {
    local dir name version id kind file
    members | while read -r dir; do
        name="$(json_field "$dir/package.json" name)"
        version="$(json_field "$dir/package.json" version)"
        [ -n "$name" ] || { echo "js-artefacts: $dir/package.json has no name" >&2; exit 1; }
        if compgen -G "$dir/vite.config.*" >/dev/null; then
            kind=ui-bundle
        elif [[ "$dir" == sdk/* ]]; then
            kind=npm-package
        else
            continue
        fi
        # `@midgard/console-midgard` → `midgard-console-midgard`: the registry id, and
        # the stem pnpm itself gives a packed tarball.
        id="$(sed 's/^@//; s#/#-#g' <<<"$name")"
        case "$kind" in
            ui-bundle) file="$id.ui.tar" ;;
            npm-package) file="$id-${version:-0.0.0}.tgz" ;;
        esac
        echo "$kind $id $dir $file $name"
    done | sort -k2,2
}

check() {
    local clash
    clash="$(rows | awk '{print $2}' | sort | uniq -d)"
    if [ -n "$clash" ]; then
        echo "js-artefacts: one artefact id from more than one package (the registry id would collide):" >&2
        sed 's/^/  /' <<<"$clash" >&2
        return 1
    fi
}

# The source revision being built: the one the pipeline names, else the checkout's.
# (Inside the toolchain container the checkout belongs to another user and git
# declines to read it, so tools/ci/in-toolchain.sh passes the revision in.)
revision() {
    echo "${MIDGARD_SOURCE_REVISION:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"
}

# Write mg-repro's report for one artefact.
report() { # report OUT ID FILE DIGEST1 DIGEST2
    local same=false
    [ "$4" = "$5" ] && same=true
    printf '{\n  "packages": [\n    { "package": "%s", "artefact": "%s", "sha256": "%s", "digest_run1": "%s", "digest_run2": "%s", "reproducible": %s, "excepted": false }\n  ]\n}\n' \
        "$2" "$3" "$4" "$4" "$5" "$same" > "$1/$2.repro.json"
}

# One build of one artefact into a fresh directory; prints the artefact's path.
build_once() { # build_once KIND DIR FILE SCRATCH
    local kind="$1" dir="$2" file="$3" scratch="$4"
    mkdir -p "$scratch"
    case "$kind" in
        npm-package)
            (cd "$dir" && pnpm pack --pack-destination "$scratch" >/dev/null)
            [ -f "$scratch/$file" ] || { echo "js-artefacts: pnpm pack did not produce $file" >&2; return 1; }
            ;;
        ui-bundle)
            [ -d node_modules ] || {
                echo "js-artefacts: the workspace is not installed; run \`pnpm install --frozen-lockfile\` first" >&2
                return 1
            }
            (cd "$dir" && pnpm exec tsc -p tsconfig.json --noEmit \
                && pnpm exec vite build --outDir "$scratch/dist" --emptyOutDir --logLevel warn) >&2
            # A tar of the bundle with everything that is not content pinned: entries
            # sorted, times, owners and modes fixed. Two builds of the same source are
            # then the same bytes or the build is not reproducible.
            tar --sort=name --mtime=@1 --owner=0 --group=0 --numeric-owner \
                --mode='u=rwX,go=rX' --format=ustar -cf "$scratch/$file" -C "$scratch" dist
            ;;
    esac
    echo "$scratch/$file"
}

build() {
    local out="" all=0 wanted=() failed=0 kind id dir file name a b d1 d2 found
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --out) out="${2:?--out needs a directory}"; shift 2 ;;
            --all) all=1; shift ;;
            -*) echo "js-artefacts: unknown option $1" >&2; exit 2 ;;
            *) wanted+=("$1"); shift ;;
        esac
    done
    [ -n "$out" ] || { echo "usage: $0 build (ID... | --all) --out DIR" >&2; exit 2; }
    [ "$all" -eq 1 ] || [ "${#wanted[@]}" -gt 0 ] || { echo "js-artefacts: name an artefact, or --all" >&2; exit 2; }
    command -v pnpm >/dev/null 2>&1 || { echo "js-artefacts: pnpm is not on PATH" >&2; exit 1; }
    check

    for id in "${wanted[@]}"; do
        rows | awk -v id="$id" '$2 == id { found = 1 } END { exit !found }' \
            || { echo "js-artefacts: the workspace ships no artefact called $id" >&2; exit 1; }
    done

    mkdir -p "$out"
    out="$(cd "$out" && pwd)"
    scratch="$(mktemp -d)"
    trap 'rm -rf "${scratch:-}"' EXIT
    # The same normalisation the Rust build gets, for the same reason.
    export SOURCE_DATE_EPOCH=1 TZ=UTC LC_ALL=C LANG=C

    while read -r kind id dir file name; do
        [ -n "$kind" ] || continue
        if [ "$all" -ne 1 ]; then
            found=0
            for a in "${wanted[@]}"; do [ "$a" = "$id" ] && found=1; done
            [ "$found" -eq 1 ] || continue
        fi
        rm -f "$out/$file" "$out/$id.repro.json"
        a="$(build_once "$kind" "$dir" "$file" "$scratch/$id/1" </dev/null)"
        b="$(build_once "$kind" "$dir" "$file" "$scratch/$id/2" </dev/null)"
        d1="$(sha256 "$a")"
        d2="$(sha256 "$b")"
        report "$out" "$id" "$file" "$d1" "$d2"
        if [ "$d1" = "$d2" ]; then
            cp "$a" "$out/$file"
            echo "ok        $kind  $id  $file  ${d1:0:16}"
        else
            echo "DIFFERS   $kind  $id  $file  ${d1:0:16} != ${d2:0:16}"
            echo "js-artefacts: FAIL — $id ($name) is NOT reproducible (digests differ)" >&2
            failed=1
        fi
    done < <(rows)

    [ "$failed" -eq 0 ] || exit 1
    # What these were built from. A release that did not build them itself checks this
    # against its own checkout before signing them.
    revision > "$out/.commit"
    echo "js-artefacts: OK — every artefact built is bit-reproducible"
}

case "${1:-}" in
    --rows)
        check
        rows
        ;;
    --check)
        check
        echo "js-artefacts: $(rows | wc -l | tr -d ' ') artefact(s) derived from $WORKSPACE"
        ;;
    --commit)
        revision
        ;;
    build)
        shift
        build "$@"
        ;;
    *)
        echo "usage: $0 --rows | --check | --commit | build (ID... | --all) --out DIR" >&2
        exit 2
        ;;
esac
