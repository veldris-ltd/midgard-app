#!/usr/bin/env bash
# Build the suite's services as universal macOS binaries (Intel + Apple Silicon) into
# dist/mac/bin. Uses the pinned toolchain (rust-toolchain.toml) and the vendored crates;
# build output lives outside the repo, in $MIDGARD_BUILD_DIR (default
# ~/Library/Caches/midgard-build), so the suite folder stays small.
#
#   bash tools/mac-app/build-services.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

# Homebrew's rustup is keg-only, so its cargo is not on PATH by default.
if ! command -v cargo >/dev/null 2>&1 && [ -x /opt/homebrew/opt/rustup/bin/cargo ]; then
  PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi

BUILD="${MIDGARD_BUILD_DIR:-$HOME/Library/Caches/midgard-build}"
OUT=dist/mac/bin
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
PACKAGES=(-p http-gateway -p heimdall-feed-server -p mimir-server -p forge-worker -p midgard-replay)
BINARIES=(http-gateway heimdall-feed heimdall-source mimir-server forge-worker midgard-replay)

for t in "${TARGETS[@]}"; do
  echo "== building for $t"
  CARGO_TARGET_DIR="$BUILD" cargo build --offline --locked --release --target "$t" "${PACKAGES[@]}" --bins
done

mkdir -p "$OUT"
for b in "${BINARIES[@]}"; do
  lipo -create -output "$OUT/$b" \
    "$BUILD/aarch64-apple-darwin/release/$b" "$BUILD/x86_64-apple-darwin/release/$b"
  echo "$b: $(lipo -archs "$OUT/$b")"
done
