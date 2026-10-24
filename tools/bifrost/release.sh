#!/usr/bin/env bash
# Build, sign and register the suite's services for one git commit as release X.Y.Z in
# bifrost-server's artefact registry.
#
#   bash tools/bifrost/release.sh <commit> --version X.Y.Z [--bad-build]
#        [--server HOST:PORT] [--identity DIR] [--service NAME] [--key DIR]
#        [--trust-dir DIR] [--out DIR]
#   bash tools/bifrost/release.sh --init-key [--key DIR] [--trust-dir DIR]
#
# 1. Build. <commit> is checked out in a temporary git worktree outside the suite folder
#    ($CACHE/worktree; removed afterwards) and its services are built for Apple Silicon
#    and Intel and joined with lipo, as tools/mac-app/build-services.sh does, into
#    $CACHE/target. The worktree path is fixed, so a rebuild of the same commit is
#    incremental and the binaries do not vary with where they were built.
# 2. SBOM and sign. Per binary (http-gateway, heimdall-feed, heimdall-source,
#    mimir-server, forge-worker, midgard-replay): a CycloneDX SBOM from the commit's
#    Cargo.lock (mg-sbom), then the Bifrost manifest {id, version, component,
#    payloadSha256, sbomSha256} is signed with ML-DSA-65 inside the release keystore's
#    custodian (bifrost-release sign, as mg-register signs).
# 3. Register. Each is uploaded to bifrost-server in chunks (bifrost-release upload:
#    RegisterArtefact, PutArtefactChunk, CommitArtefact) as the release-manager identity;
#    the registry's gate verifies it before it is stored. The result is printed per
#    artefact, then the registry's listing for X.Y.Z.
#
# --bad-build replaces forge-worker, the canary, with tools/bifrost/unready-forge-worker:
# a std-only stand-in built from this checkout's release tooling that takes forge-worker's
# command line, stays alive (/healthz 200) and is never ready (/readyz 503; it never logs
# "job run"). It is registered, signed and SBOM'd like the rest (its SBOM names the stub),
# for the B2 demo in which the canary gate fails and the release rolls back. The services'
# own code is not touched.
#
# The release key is a dedicated ML-DSA-65 keystore, made once by --init-key (or on the
# first release) in --key (default $BIFROST_HOME/release-key, never in the repository).
# Its public half (<key>/release.pub) must be in the server's trust/release-signers/;
# --init-key copies it to --trust-dir (default $BIFROST_HOME/fleet-state/trust/
# release-signers, when that exists). bifrost-server reads its signers when it starts,
# so restart it after a new key is installed. Before building, the release checks that
# the server answers and trusts the key.
#
# Defaults: BIFROST_HOME=~/Desktop/Midgard/bifrost-local, --server $BIFROST_MESH or
# 127.0.0.1:7750, --identity the clients= folder in $BIFROST_HOME/mesh.conf (run-local.sh
# setup --real-enclosure) or else $BIFROST_HOME/fleet-clients, --service release-manager.
# --out keeps the binaries, SBOMs and signed manifests (default: a temporary folder,
# removed). Build output: $MIDGARD_BUILD_DIR (default ~/Library/Caches/midgard-build/
# bifrost-release/target); the tools are built in $CARGO_TARGET_DIR (default
# ~/Library/Caches/midgard-build/bifrost-target). Needs 5 GB free.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
H="${BIFROST_HOME:-$HOME/Desktop/Midgard/bifrost-local}"
CACHE="$HOME/Library/Caches/midgard-build/bifrost-release"
BUILD="${MIDGARD_BUILD_DIR:-$CACHE/target}"
TOOLS="${CARGO_TARGET_DIR:-$HOME/Library/Caches/midgard-build/bifrost-target}"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
PACKAGES=(-p http-gateway -p heimdall-feed-server -p mimir-server -p forge-worker -p midgard-replay)
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)
CANARY=forge-worker
KEY_ID=bifrost-release-1
MIN_FREE_KB=$((5 * 1024 * 1024))

die() { echo "release: $*" >&2; exit 1; }
usage() { sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

COMMIT="" VERSION="" BAD=0 INIT_KEY=0 OUT=""
SERVER="${BIFROST_MESH:-127.0.0.1:7750}"
# The clients' identities: where run-local.sh setup --real-enclosure recorded them, or the
# private enclosure's.
IDENTITY="$(sed -n 's/^clients=//p' "$H/mesh.conf" 2>/dev/null | head -1 || true)"
IDENTITY="${IDENTITY:-$H/fleet-clients}"
SERVICE=release-manager
KEY="$H/release-key"
TRUST=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
        --bad-build) BAD=1; shift ;;
        --init-key) INIT_KEY=1; shift ;;
        --server) SERVER="${2:?--server needs a value}"; shift 2 ;;
        --identity) IDENTITY="${2:?--identity needs a value}"; shift 2 ;;
        --service) SERVICE="${2:?--service needs a value}"; shift 2 ;;
        --key) KEY="${2:?--key needs a value}"; shift 2 ;;
        --trust-dir) TRUST="${2:?--trust-dir needs a value}"; shift 2 ;;
        --out) OUT="${2:?--out needs a value}"; shift 2 ;;
        -h|--help) usage ;;
        --*) echo "release: unknown option $1" >&2; usage ;;
        *) [ -z "$COMMIT" ] || die "one commit only (got $COMMIT and $1)"; COMMIT="$1"; shift ;;
    esac
done
[ -n "$TRUST" ] || { [ -d "$H/fleet-state/trust/release-signers" ] && TRUST="$H/fleet-state/trust/release-signers"; } || true

command -v cargo >/dev/null 2>&1 || PATH="/opt/homebrew/opt/rustup/bin:$PATH"

# --- the tools, from this checkout ---------------------------------------------------
tools() {
    (cd "$ROOT" && CARGO_TARGET_DIR="$TOOLS" cargo build --offline --locked --release -q \
        -p bifrost-fleet-server -p mg-sign -p mg-sbom \
        --bin bifrost-release --bin mg-sign --bin mg-sbom)
    RELEASE="$TOOLS/release/bifrost-release"
    MG_SIGN="$TOOLS/release/mg-sign"
    MG_SBOM="$TOOLS/release/mg-sbom"
}

# --- the release key ----------------------------------------------------------------
init_key() {
    local ks="$KEY/release.keystore" pub="$KEY/release.pub"
    if [ ! -f "$ks" ]; then
        mkdir -p "$KEY" && chmod 700 "$KEY"
        "$MG_SIGN" keygen --alg ML-DSA-65 --key-id "$KEY_ID" --keystore "$ks" \
            --export-verifying-key "$pub"
        echo "release key: made $ks ($KEY_ID, ML-DSA-65)"
    fi
    [ -f "$pub" ] || die "$pub is missing; it is the public half of $ks"
    if [ -n "$TRUST" ]; then
        if cmp -s "$pub" "$TRUST/$KEY_ID.pub"; then
            echo "release key: already trusted in $TRUST"
        else
            mkdir -p "$TRUST" && cp "$pub" "$TRUST/$KEY_ID.pub"
            echo "release key: installed $TRUST/$KEY_ID.pub; restart bifrost-server to trust it"
        fi
    else
        echo "release key: put $pub into the server's trust/release-signers/ and restart it"
    fi
}

tools
if [ "$INIT_KEY" = 1 ]; then
    [ -z "$COMMIT$VERSION" ] || die "--init-key takes no commit or version"
    init_key
    exit 0
fi
[ -n "$COMMIT" ] && [ -n "$VERSION" ] || usage
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]] || die "--version must be X.Y.Z (got $VERSION)"
SHA="$(git -C "$ROOT" rev-parse --verify --quiet "$COMMIT^{commit}")" || die "no commit $COMMIT"
[ -f "$KEY/release.keystore" ] || init_key

client() { "$RELEASE" "$1" --identity-dir "$IDENTITY" --service "$SERVICE" --addr "$SERVER" "${@:2}"; }
client preflight --keystore "$KEY/release.keystore"

free_kb="$(df -k "$HOME" | awk 'NR==2 {print $4}')"
[ "$free_kb" -ge "$MIN_FREE_KB" ] || die "only $((free_kb / 1024)) MB free; a release build needs 5 GB"

# --- one release at a time, in a worktree outside the suite folder ------------------
mkdir -p "$CACHE"
LOCK="$CACHE/lock"
WT="$CACHE/worktree"
KEEP_OUT=1
[ -n "$OUT" ] || KEEP_OUT=0
mkdir "$LOCK" 2>/dev/null || die "another release is running (or remove $LOCK if not)"
cleanup() {
    if [ -e "$WT" ]; then git -C "$ROOT" worktree remove --force "$WT" >/dev/null 2>&1 || true; fi
    git -C "$ROOT" worktree prune >/dev/null 2>&1 || true
    [ "$KEEP_OUT" = 1 ] || [ -z "$OUT" ] || rm -rf "$OUT"
    rmdir "$LOCK" 2>/dev/null || true
}
trap cleanup EXIT
[ -n "$OUT" ] || OUT="$(mktemp -d "${TMPDIR:-/tmp}/bifrost-release.XXXXXX")"
if [ -e "$WT" ]; then git -C "$ROOT" worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; fi
git -C "$ROOT" worktree prune
git -C "$ROOT" worktree add --detach --quiet "$WT" "$SHA"
mkdir -p "$OUT"

echo "== release $VERSION from ${SHA:0:12}$([ "$BAD" = 1 ] && echo ' (bad build: forge-worker never ready)')"
start=$(date +%s)
for t in "${TARGETS[@]}"; do
    echo "== building for $t"
    (cd "$WT" && CARGO_TARGET_DIR="$BUILD" cargo build --offline --locked --release --target "$t" \
        "${PACKAGES[@]}" --bins)
done
mkdir -p "$WT/dist/mac/bin"
for b in "${BINARIES[@]}"; do
    lipo -create -output "$WT/dist/mac/bin/$b" \
        "$BUILD/aarch64-apple-darwin/release/$b" "$BUILD/x86_64-apple-darwin/release/$b"
    # The SBOM is the commit's (its Cargo.lock), named by the binary's place in the build.
    (cd "$WT" && "$MG_SBOM" --out "$OUT/$b.sbom.cdx.json" "dist/mac/bin/$b" 2>/dev/null)
    cp "$WT/dist/mac/bin/$b" "$OUT/$b"
done

if [ "$BAD" = 1 ]; then
    STUB="$ROOT/tools/bifrost/unready-forge-worker"
    for t in "${TARGETS[@]}"; do
        CARGO_TARGET_DIR="$BUILD/unready" cargo build --offline --locked --release -q \
            --manifest-path "$STUB/Cargo.toml" --target "$t"
    done
    STAGE="$OUT/unready-sbom"
    mkdir -p "$STAGE"
    cp "$STUB/Cargo.toml" "$STUB/Cargo.lock" "$STAGE/"
    lipo -create -output "$STAGE/$CANARY" \
        "$BUILD/unready/aarch64-apple-darwin/release/$CANARY" \
        "$BUILD/unready/x86_64-apple-darwin/release/$CANARY"
    (cd "$STAGE" && "$MG_SBOM" --out "$OUT/$CANARY.sbom.cdx.json" "$CANARY" 2>/dev/null)
    mv "$STAGE/$CANARY" "$OUT/$CANARY"
    rm -rf "$STAGE"
fi
echo "== built in $(( $(date +%s) - start )) s"

failed=0
for b in "${BINARIES[@]}"; do
    "$RELEASE" sign --keystore "$KEY/release.keystore" --id "$b" --version "$VERSION" \
        --component service --payload "$OUT/$b" --sbom "$OUT/$b.sbom.cdx.json" \
        --out "$OUT/$b.bifrost.json" >/dev/null
    if line="$(client upload --signed "$OUT/$b.bifrost.json" --payload "$OUT/$b" \
            --sbom "$OUT/$b.sbom.cdx.json" 2>&1)"; then
        echo "registered $line"
    else
        echo "REFUSED $line"
        failed=$((failed + 1))
    fi
done

echo "== the registry for $VERSION"
client list --version "$VERSION"
[ "$KEEP_OUT" = 0 ] || echo "== kept in $OUT"
[ "$failed" = 0 ] || die "$failed of ${#BINARIES[@]} artefacts were refused"
