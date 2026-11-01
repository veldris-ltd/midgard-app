# Build environment

Midgard builds **offline and reproducibly** from a pinned toolchain. There are
two supported ways to get that toolchain; both build the *same* vendored sources.

## Toolchain

- **Rust `1.82.0`** — pinned in [`rust-toolchain.toml`](../../rust-toolchain.toml).
- No C toolchain, CMake, or system libraries are required: the post-quantum
  backend is **pure-Rust** (FIPS 203/204/205 + X25519). This is why liboqs was
  dropped in Phase 1.

## Offline / vendored dependencies (sovereign posture) [NFR-PRV-1] [NFR-OSS-1]

Every external crate is committed under [`/vendor`](../../vendor) and wired via
[`.cargo/config.toml`](../../.cargo/config.toml) source-replacement, so the
workspace builds with **no network access**. With no crypto feature enabled the
workspace is std-only and the replacement is inert; under `backend-fips` the
whole FIPS + X25519 graph resolves from `/vendor`.

To add or update a dependency (the only step that touches the network):

```bash
# 1. comment out the [source] block in .cargo/config.toml
cargo update -p <crate> --precise <version>
cargo vendor --versioned-dirs vendor > /tmp/vendor-stanza.toml   # refreshes /vendor
# 2. restore the [source] block; commit /vendor + Cargo.lock
```

Dependencies are pinned to Rust 1.82-compatible versions. Notably `zeroize` is
held at `1.8.1` (1.9's `zeroize_derive` requires edition 2024 / Cargo ≥ 1.85), and
`base64ct` (pulled in by `argon2` / `password-hash`) at `1.6.0`, because later
releases need edition 2024 (`cargo update -p base64ct --precise 1.6.0`).

## Option A — Docker (canonical; used by CI) [AC-M-5] [AC-M-6]

The authoritative offline, reproducible build path. The toolchain lives in a
**digest-pinned** image ([`deploy/acceptance/Dockerfile`](../../deploy/acceptance/Dockerfile))
that bakes rustc 1.82.0 + rustfmt/clippy + `just`; baking is the only step that
touches the network. Everything after runs from committed `/vendor` with no
outbound connectivity — `--network none` proves it.

```bash
# Bake the pinned image once (online), then build and test fully offline:
docker build -t midgard-acceptance:1.82.0 deploy/acceptance
bash tools/ci/in-toolchain.sh just build-rust
bash tools/ci/in-toolchain.sh just test
```

[`tools/ci/in-toolchain.sh`](../../tools/ci/in-toolchain.sh) runs its arguments in
that image with `--network none` and the checkout mounted at `/work`. CI
(`.github/workflows/ci.yml`) runs every Rust step this way: formatting, lints, the
build, the tests, `just repro "" 4` to assert a byte-for-byte identical rebuild, and
the advisory scan. The full acceptance suite runs in the same image via
[`deploy/acceptance/run-airgapped.sh`](../../deploy/acceptance/run-airgapped.sh).
For day-to-day iteration, keep the build output and the cargo registry in named
volumes (Docker creates them on first use) and drop `--network none`:

```bash
docker run --rm \
  -v "$PWD:/work" -v midgard-target:/target -v midgard-cargo:/usr/local/cargo/registry \
  -w /work -e CARGO_TARGET_DIR=/target \
  midgard-acceptance:1.82.0 cargo test --offline -p pqc-agility --features test-backend
```

The `midgard-target` volume grows to tens of gigabytes. `docker volume rm
midgard-target` reclaims the space, and the next build starts from scratch.

> On Windows Git Bash, prefix `docker` with `MSYS_NO_PATHCONV=1` so `/work` is not
> mangled into a host path.

## Option B — Nix (experimental; BLOCKED — do not treat as reproducible)

`flake.nix` is kept for contributors who prefer Nix, but it is **not** the
reproducibility anchor and does not yet build the workspace:

> ⚠ It has **no committed `flake.lock`** (inputs would resolve from the network,
> defeating the offline claim), and the flake tracks `nixpkgs 24.05`, whose `rustc`
> predates 1.82 and is *older* than the vendored crates' MSRV. To make Nix usable,
> pin Rust 1.82 via the `rust-overlay` and commit a `flake.lock`
> (`nix flake lock`). Until both land, use Option A. [RT-04]

**No automated path depends on Nix.** Both workflows build through Option A, the
digest-pinned toolchain image run with `--network none`:
- `ci.yml` builds the image on GitHub's shared runners, then runs formatting, lints,
  the build, the tests, the reproducibility check and the advisory scan in it, through
  `tools/ci/in-toolchain.sh`.
- `release.yml` runs the same image, provisioned in advance, on the self-hosted
  `[kms]` runners.

Docker is authoritative. `flake.nix` is a contributor convenience only, CI does not
exercise it, and a Nix-only result is unproven. [RT-47]

`tools/ci/in-toolchain.sh` also runs any command that way on a Linux machine, for
example `bash tools/ci/in-toolchain.sh just lint`.

## Test matrix

The crypto crates gate their backend tests behind mutually-exclusive features, so
`cargo test --workspace` with no features compiles them out. Run both backends
(see `just test`):

| Command | Exercises |
|---|---|
| `cargo test -p pqc-agility -p pq-mtls -p audit-writer --features test-backend` | agility shim, handshake, audit chain (deterministic INSECURE backend) |
| `cargo test -p pqc-agility --features backend-fips --test fips_backend` | production FIPS 203/204/205 + X25519 backend |
| `cargo test -p policy` | policy decision point |
