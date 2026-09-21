{
  # Midgard suite — EXPERIMENTAL Nix build umbrella. NOT the reproducibility anchor.
  #
  # ⚠ The authoritative offline/reproducible build path is the pinned, digest-locked
  #   Docker toolchain image (deploy/acceptance/Dockerfile) driven by CI and
  #   docs/dev/BUILD_ENV.md Option B; bit-reproducibility (AC-M-6) and the air-gap
  #   posture (AC-M-5, NFR-PRV-1) are proven there, from committed /vendor sources.
  #
  #   This flake is kept for contributors who prefer Nix, but it is BLOCKED and does
  #   not yet build the workspace: it has no committed `flake.lock` (inputs would
  #   resolve from the network, defeating the offline claim), and nixpkgs 24.05's
  #   rustc predates the 1.82 MSRV. To make it usable: pin Rust 1.82 via the
  #   rust-overlay below AND commit a `flake.lock` (`nix flake lock`, the only step
  #   that touches the network). Until both land, do not treat Nix as reproducible.
  #   [RT-04]
  description = "Midgard — sovereign, post-quantum platform suite (experimental Nix umbrella)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    flake-utils.url = "github:numtide/flake-utils";
    # Pins Rust to the exact channel in rust-toolchain.toml (1.82.0). nixpkgs
    # 24.05's own rustc predates 1.82 and is below the workspace's crate MSRV, so
    # the devshell's Rust MUST come from this overlay, not nixpkgs. [AC-M-6]
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # The single source of truth for the Rust toolchain: read channel +
        # components (clippy, rustfmt) straight from rust-toolchain.toml so Nix and
        # non-Nix (Docker) builds use byte-identical rustc. [NFR-OSS-1]
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # Toolchains for the polyglot workspace. Each is pinned via flake.lock.
        toolchains = with pkgs; [
          # Rust — crypto spine, core services, tools, client core (pinned 1.82.0)
          rustToolchain
          # Go — compute scheduler, Bifrost, admin backend
          go gopls
          # Python — Mimir model plane (via uv, fully locked)
          python312 uv
          # TypeScript / JS — UIs, SDKs, extension
          nodejs_22 nodePackages.pnpm
          # Contracts + supply chain
          protobuf buf
          # Reproducibility + SBOM + signing helpers
          just cyclonedx-cli
          # NB: the post-quantum backend is pure-Rust (FIPS 203/204/205 + X25519,
          # vendored under /vendor) — no system liboqs / C toolchain is required.
        ];
      in
      {
        # `nix develop` — the one shell every contributor uses. No global installs.
        devShells.default = pkgs.mkShell {
          name = "midgard-dev";
          packages = toolchains;
          shellHook = ''
            echo "Midgard dev shell — run 'just' for available tasks."
          '';
        };

        # `nix build` entry points are declared per-artefact in later phases.
        # The scaffold exposes the toolchain and the task runner only.
        packages = { };

        # `nix run .#<tool>` — the supply-chain toolchain (stubs in Phase 0.b).
        apps = {
          # Convenience passthroughs to the justfile targets.
          ci = {
            type = "app";
            program = "${pkgs.just}/bin/just";
          };
        };
      });
}
