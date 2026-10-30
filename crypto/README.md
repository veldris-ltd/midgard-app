# crypto/ — cross-cutting PQ primitives (Phase 0.c)

The primitives every phase reuses, built **before** any product code so no
product re-implements cryptography (KDD-1). Rust crates over a vetted PQC
backend — pure-Rust, ACVP-validated **FIPS 203/204/205 + X25519** (chosen over
liboqs in Phase 1 to keep builds C-free, offline-vendorable, and bit-reproducible).

| Crate | Responsibility | Provenance |
|---|---|---|
| `pqc-agility/` | ML-KEM-768/1024, ML-DSA, SLH-DSA behind a negotiated, versioned, hot-swappable shim | `[NFR-PQ-4]` |
| `pq-mtls/` | Zero-trust mTLS sidecar + client for inter-service calls | `[NFR-SEC-1]` |
| `audit-writer/` | PQ-signed, append-only audit ledger writer | `[NFR-PQ-3]` |
| `policy/` | Role / marking / purpose policy-decision interface | `[NFR-SEC-2]` |
| `attest/` | **The** acceptance decision — domain-separated ML-DSA signature verification — plus the release-artefact bundle format | `[NFR-PQ-3]` `[AC-B-1]` `[AC-B-4]` |

`attest` is the one implementation of *reject unless proven*. Every construct
that signs something signs under its own domain and is accepted through
`attest::accept`, so "acceptable" cannot come to mean different things in
different places:

| Construct | Domain | Call site |
|---|---|---|
| CI release bundle | `midgard-artefact-v1` | `attest::verify_artefact` (`mg-verify`) |
| Bifrost artefact manifest | `midgard-bifrost-artefact-v1` | `bifrost_artefact::verify` |
| Bifrost offline-bundle manifest | `midgard-bifrost-bundle-v1` | `bifrost_bundle::import` |
| Mimir model manifest | `midgard-mimir-model-v1` | `ModelLoader::verify_and_load` |
| Mimir model manifest file (`model.manifest`, format 2) | `midgard-mimir-model-file-v2` | `model_server::verify_manifest_file` (`ModelLoader::verify_dir`) |

A signature only proves where a release came from, not that it is current, so the
places that accept signed software also keep a **release floor**
(`attest::ReleaseFloor`): per artefact id, the highest version accepted and the digest
of what was signed. An older version, or the same version with different content, is
refused. Bifrost's artefact registry and `mimir-server`'s model load keep one.
[MG-SEC-14]

Its `custody` feature (the KMS-backed signer and the sealed keystore) is for
*signers* only; verifiers take `default-features = false` and never pull `kms`
in merely to check a signature.

> **Status (Phase 1):** the real lattice crypto sits behind a trait. With no
> features the crate is std-only and dependency-free. Two backends, mutually
> exclusive by a `compile_error`:
> - **`test-backend`** — deterministic, **INSECURE** (SHA-256-built); exercises the
>   agility shim, handshake, and audit chain without real key sizes' cost.
> - **`backend-fips`** — production: pure-Rust FIPS 203 (ML-KEM-768/1024),
>   FIPS 204 (ML-DSA-44/65/87), FIPS 205 (SLH-DSA-SHA2-128s), plus X25519 for the
>   hybrid transit suites. Dependencies are committed under `/vendor` for fully
>   offline, reproducible builds. `OsRng` draws from the platform CSPRNG
>   (`getrandom`, a syscall — no network). `tools/mg-sign` and `tools/mg-verify`
>   are thin CLIs over `attest`, so the CI signer, the CI verifier, Bifrost's
>   pre-deploy check and Mimir's model load all share one ML-DSA implementation
>   and one acceptance decision.
>
> **Feature groupings matter for tooling too.** Because `test-backend` and
> `backend-fips` are mutually exclusive behind a `compile_error!`, neither tests
> nor lints can be a single `--workspace` run — `cargo clippy --workspace` would
> unify both features and fail to compile. `just test` and `just lint` both use
> the same per-group invocations for this reason.
>
> Run the tests with `just test`, or directly:
> `cargo test -p pqc-agility -p pq-mtls -p audit-writer --features test-backend`,
> `cargo test -p pqc-agility --features backend-fips --test fips_backend`, and
> `cargo test -p policy`.
