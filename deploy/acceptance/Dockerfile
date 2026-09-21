# Air-gapped acceptance toolchain for the Midgard suite.
#
# WHY THIS EXISTS
# The base `rust:1.82` image ships cargo/rustc/rust-std but NOT rustfmt or clippy.
# The workspace's rust-toolchain.toml pins channel 1.82.0 AND requests
# components = ["rustfmt", "clippy"]. So the first cargo invocation makes rustup
# try to fetch those missing components from static.rust-lang.org — which fails
# closed under `docker run --network none`, aborting before anything compiles.
#
# Baking the components in (the ONLY online step) means an offline rustup finds
# every component already present, so the entire acceptance suite — build, tests,
# and reproducibility check — runs with NO outbound connectivity. That is the
# definitive demonstration of the sovereign / air-gap posture. [NFR-PRV-1] [AC-M-5]
#
# Multi-arch: the digest below is the manifest-LIST (index) digest, so it still
# resolves to the host architecture (x86_64 or aarch64) while pinning the exact
# toolchain image — the Docker-world analogue of a lock file. This is what makes
# "no input resolution on a clean runner" true: the toolchain is content-addressed,
# not a floating tag. Refresh it deliberately (and record the new digest) when
# bumping the base image. [AC-M-6] [NFR-PRV-1]
FROM rust:1.82@sha256:d9c3c6f1264a547d84560e06ffd79ed7a799ce0bff0980b26cf10d29af888377

# rust-toolchain.toml resolves "1.82.0" to the host-triple toolchain that is the
# image default; adding the components here shares them with that resolution.
RUN rustup component add rustfmt clippy && rustup show

# The task runner the build/test/repro recipes use. Installed here (the online bake
# step) so the image is a COMPLETE offline build environment — the role the Nix
# devshell used to play. Pinned for a reproducible image. [NFR-OSS-1]
RUN cargo install just --version 1.37.0 --locked

# Guard against toolchain drift from the workspace pin.
LABEL org.midgard.toolchain="1.82.0" \
      org.midgard.purpose="offline build + acceptance verification (--network none)"
