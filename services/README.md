# services/ — Midgard core (the enclosure), Phase 1

The sovereign substrate every product runs on. AI-free. Built on the Phase-0.c
`crypto/` primitives. Source: `SAD_00_Midgard_Core.md`.

| Directory | Container | Stack | Provenance |
|---|---|---|---|
| `gateway/` | Public ingress; hybrid-PQ TLS 1.3, identity assertion, rate limiting | Rust | `[NFR-PQ-1]` `[NFR-SEC-1]` |
| `identity/` | Self-contained OIDC IdP; PQ-signed tokens; offline-capable | Rust | `[NFR-SEC-2]` `[NFR-PRV-1]` |
| `object-fabric/` | Semantic + kinetic core; typed query / action APIs | Rust | `[DD-1]` |
| `object-store/` | Columnar + inverted-index store; incremental indexing | Rust (Arrow/DataFusion) | `[DD-1]` |
| `governance-audit/` | Policy enforcement at query time; PQ-signed append-only lineage | Rust | `[NFR-SEC-2]` `[NFR-PQ-3]` |
| `kms/` | PQ key custody; AES-256-GCM under ML-KEM-1024; HSM/TPM sealing | Rust | `[NFR-PQ-2]` |
| `compute-scheduler/` | K8s autoscaling mesh; zero-downtime rolling upgrades | Go | `[NFR-AVA-1]` |
| `gen/` | Generated gRPC bindings from `contracts/` (gitignored) | — | `[DD-6]` |

> **Status: scaffold only.** Directories are created as each container is built
> in Phase 1 (Prompts 1.a–1.f). Acceptance: AC-M-1..9.
