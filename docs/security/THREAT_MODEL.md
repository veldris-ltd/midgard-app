# Midgard Threat Model

Scope: the Midgard enclosure — its crypto, transport, identity, key custody,
governance, data platform, admin plane, and supply chain. Methodology: STRIDE over
the trust boundaries, with each threat mapped to the implemented, tested control.

## Assets

- **Classified data** in the object fabric, datasets, and the common operating
  picture (Heimdall).
- **Long-lived secrets**: the KMS trust root, the IdP/CA signing key, workload keys,
  audit-signing keys, credentials.
- **Integrity of record**: the PQ-signed audit chain and config history.
- **Availability** of the enclosure services.

## Adversaries

1. **Harvest-now, decrypt-later (HNDL)** — records traffic/ciphertext today to break
   with a future quantum computer. *This is the defining adversary; it is why every
   cryptographic primitive is post-quantum.*
2. **Malicious/compromised insider** — an operator or admin abusing access.
3. **On-path network attacker** inside the enclosure network.
4. **Supply-chain attacker** — tampering with builds, dependencies, or releases.
5. **Host/memory attacker** — reading process memory, swap, or snapshots.

## Trust boundaries

- Browser/console ⇄ **HTTP/JSON gateway** (session termination, auth).
- Service ⇄ service over **PQ-mTLS** (mutual auth, hybrid KEM).
- Process ⇄ **KMS trust root** (ideally an HSM/TPM device boundary).
- Enclosure ⇄ **outside world** — *closed by default* (NFR-PRV-1: no egress).
- Build/release runner ⇄ artefacts (supply chain).

## STRIDE

### Spoofing (identity)
- *Threat:* impersonating a workload, user, or the CA.
- *Controls:* PQ-mTLS mutual auth with IdP-issued certs (`crypto/pq-mtls`,
  `services/iam`); ML-DSA-signed tokens verified on every request
  (`services/http-gateway`); real WebAuthn assertion + password + TOTP for principals
  and admins (RT-29/30); federation verifies upstream assertions before minting local
  tokens. **HNDL:** hybrid X25519+ML-KEM handshake, fail-closed if PQ is unavailable —
  no classical-only downgrade.

### Tampering (integrity)
- *Threat:* altering data at rest, audit records, config, or releases.
- *Controls:* KMS-sealed append-only journals with the sequence bound into the AEAD
  AAD — a reordered/spliced/edited record fails authentication on replay (RT-09..14);
  PQ-signed audit chain (`crypto/audit-writer`, `verify_chain`); signed, hash-linked
  config history; reproducible builds + PQ-signed artefacts + SBOM
  (`mg-sign`/`mg-verify`/`mg-sbom`), verified fail-closed. A tampered backup is
  rejected on restore (RT-34 drill).
- *Threat:* loading signed software that is old, swapped, or checked against the wrong
  key. *Controls:* signatures are checked over the raw bytes of a model manifest or a
  bundle manifest before either is parsed; Bifrost's artefact registry and
  `mimir-server` keep a persisted release floor (`attest::ReleaseFloor`) and refuse an
  older signed release, or the same version with different content; model weights are
  hashed through the handle that is then read, never re-opened by path; and the
  release image verifies against the pinned `/trust/release-trust.pub` only, with no
  environment override. [MG-SEC-14]

### Repudiation
- *Threat:* denying an action was taken.
- *Controls:* every consequential action (query/apply, admin commit, dual-control
  approval, break-glass grant/expiry/review, DR action) is sealed into the PQ-signed
  audit chain as one linked transaction (RT-31/32). Post-mortems read the chain, not
  memory.

### Information disclosure (confidentiality)
- *Threat:* leaking classified data or secrets; exfiltration; HNDL capture.
- *Controls:* **no egress** — the enclosure makes no outbound calls; telemetry is
  zero-dependency and pull-only (RT-33). Data at rest is AES-256-GCM under an
  ML-KEM-1024-wrapped key sealed to the trust root (`services/kms`). PQ-mTLS record
  layer protects data in transit. Deny-by-default Bell–LaPadula markings + purpose
  gate filter every read (`crypto/policy`, `services/governance`). *Residual:* secret
  zeroization is best-effort in pure Rust (RT-39 tracks `zeroize`/mlock); the
  software KMS root exposes the root in memory unless a hardware root is used
  (`services/kms/RESIDUAL_RISK.md`).

### Denial of service
- *Threat:* exhausting the gateway or a service.
- *Controls:* per-identity token-bucket rate limiting + per-connection quotas
  (RT-25); readiness gating and SLO breach observability (RT-33); DR restore within
  RTO/RPO (RT-34). *Residual:* full multi-node clustering DoS resilience is a
  deployment-tier concern (RT-40).

### Elevation of privilege
- *Threat:* gaining capabilities beyond one's role.
- *Controls:* no all-powerful role; strictly read-only Auditor; scoped, expiring
  role bindings; step-up + fresh re-auth for admin scopes; **dual-control** for
  high-risk actions via the guarded-action orchestrator, gated on a single-use
  time-boxed token (RT-31); **break-glass** is time-boxed, alerted, capability-scoped,
  and auto-expiring with mandatory review (RT-32). Deny-by-default PDP; read-up
  blocked by clearance dominance.

## Cross-cutting posture

- **Fail closed**: every verification, authorisation, durability replay, and
  revocation check denies on error rather than proceeding.
- **Post-quantum throughout**: FIPS 203/204/205 for KEM/signatures; hybrid KEM in
  transit; no classical-only fallback.
- **Sovereign / air-gapped**: builds are vendored and run with `--network none`; the
  runtime makes no outbound connection.

## Known residual risks (accepted / tracked)

| Risk | Status | Reference |
|------|--------|-----------|
| Software KMS root secret in process memory | Documented; use hardware root in prod | `services/kms/RESIDUAL_RISK.md` (RT-28) |
| Best-effort secret zeroization (no mlock/guard pages) | Tracked | RT-39 |
| No deployment-tier (real cluster/HSM/GPU) acceptance yet | Tracked | RT-40 |
| Product-console HTTP transports (non-Midgard) not built | Tracked | RT-37/45 |

Review this document each release and whenever a trust boundary changes.
