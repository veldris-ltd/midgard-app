# Compliance & Standards Matrix

Every standards claim Midgard makes traces to a concrete, implemented control. This
matrix is the authoritative map; the inline `[STD:...]` tags in the source point back
here. It is a self-attestation of design conformance, **not** a certification —
formal FIPS 140-3 / Common Criteria / ISO 27001 certification is a deployment-and-lab
activity on top of these controls.

## Cryptographic standards (NIST FIPS)

| Standard | Requirement | Control (where implemented) |
|----------|-------------|------------------------------|
| **FIPS 203** (ML-KEM) | PQ key encapsulation | `crypto/pqc-agility` KEM; ML-KEM-1024 envelope in `services/kms`; hybrid X25519+ML-KEM-768 in `crypto/pq-mtls` |
| **FIPS 204** (ML-DSA) | PQ digital signatures | `crypto/pqc-agility` sign; IdP tokens + mesh certs (`services/iam`); audit chain (`crypto/audit-writer`); release signing (`tools/mg-sign`) |
| **FIPS 205** (SLH-DSA) | Stateless hash-based signatures | `crypto/pqc-agility` (alternative signature scheme, agility-selected) |
| **FIPS 197** (AES) | Symmetric AEAD | AES-256-GCM in `crypto/pqc-agility` (aead); KMS envelope + PQ-mTLS record layer |
| **FIPS 180-4 / RFC 2104 / RFC 5869** | Hash, MAC, key derivation | SHA-256, HMAC-SHA256, HKDF-SHA256, PBKDF2-HMAC-SHA256 and constant-time compare in `crypto/pqc-agility` (hash) — wrappers over the vendored RustCrypto `sha2`, `hmac`, `hkdf`, `pbkdf2` and `subtle` crates, pinned by NIST, RFC 4231 and RFC 5869 known-answer tests (`tests/hash_kat.rs`). Used by the PQ-mTLS handshake and resumption, KMS, TOTP and the connectors [MG-SEC-10] |
| **FIPS 140-3** | Cryptographic module / key custody | Hardware TrustRoot (HSM/PKCS#11 / TPM 2.0) seam in `services/kms` (RT-28); software root documented residual risk. *Module certification is a lab activity.* |
| **NIST SP 800-132 / RFC 9106** | Password storage | Argon2id (PHC string, vendored RustCrypto `argon2`) for every new password in `services/iam` (password); legacy PBKDF2-HMAC-SHA256 entries still verify and are re-hashed with Argon2id on sign-in (`UpgradingAuthenticator`). Enforced cost floors — Argon2id m ≥ 19 MiB, t ≥ 2, p ≥ 1; PBKDF2 ≥ 600 000 iterations — and ceilings, so a tampered entry cannot be weakened [MG-SEC-11] |
| **NIST SP 800-90A** | Deterministic RNG / entropy | `OsRng` (OS CSPRNG) for all production randomness |

## Access control & security controls (NIST SP 800-53)

| Control | Requirement | Where |
|---------|-------------|-------|
| **AC-3** (Access Enforcement) | Deny-by-default enforcement | `crypto/policy` PDP; `services/governance` |
| **AC-16** (Security Attributes) | Mandatory markings/classification | `crypto/policy` markings + Bell–LaPadula lattice; projection propagates markings |
| **AU-9 / AU-10** (Audit protection / non-repudiation) | Tamper-evident, signed audit | PQ-signed append-only audit chain (`crypto/audit-writer`) |
| **SC-8 / SC-12 / SC-13** (Transmission / key mgmt / crypto) | Encrypted transit, PQ crypto | PQ-mTLS; KMS envelope; PQ primitives |
| **SC-28** (Protection at rest) | Encrypted at rest | KMS-sealed journals (RT-09..14, RT-42) |
| **SA-12** (Supply chain protection) | Trusted build/release | Reproducible builds, SBOM, PQ signature + attestation, air-gap bundle (`tools/*`, `deploy/`) |
| **IA-2 / IA-5** (Identification & authenticators) | MFA, credential mgmt | Password + TOTP + WebAuthn (`services/iam`, `admin/identity`) |
| **CP-9 / CP-10** (Backup / recovery) | Backup + tested restore | `docs/ops/{BACKUP_RESTORE,DR_PLAN}.md`; tested DR drill (RT-34) |

## Application security (OWASP ASVS)

| ASVS | Area | Where |
|------|------|-------|
| **V2** (Authentication) | MFA, phishing-resistant step-up | `services/iam`, `admin/identity` (WebAuthn, TOTP, Argon2id) |
| **V5** (Validation/encoding) | Fail-closed decoding | Hand-rolled codecs fail closed on malformed input (fabric, pq-mtls, audit) |
| **V14.2 / V14.3** (Build / secrets) | No secrets in source; secure build | CI secret scan; keys never in repo (KMS/OIDC identity); provenance gate |

## Management systems (ISO/IEC 27001)

Midgard supplies the technical controls an ISMS relies on: access control (A.9 → PDP,
RBAC, dual-control), cryptography (A.10 → PQ crypto + key mgmt), operations security
(A.12 → runbooks, logging, backup), communications security (A.13 → PQ-mTLS, no
egress), and supplier/supply-chain (A.15 → SBOM, signed releases). Certification is
organisational and out of repo scope; these controls are the evidence base.

## Identity standards

| Standard | Where |
|----------|-------|
| **FIDO2 / WebAuthn** | Assertion verification for principals and admins (`services/iam/mfa`, `admin/identity/webauthn`) |
| **OIDC (JWKS + token)** | JWKS publishes the IdP ML-DSA key; token endpoint = the authenticator flow (`services/iam/oidc`); `/authorize` explicitly SAD-rescoped |
| **RFC 6238 (TOTP)** | HMAC-SHA256 TOTP (`services/iam/mfa`) |

## Notes on honesty

- Claims marked as **lab/deployment activities** (FIPS 140-3 module cert, ISO/CC
  certification, real HSM/GPU/cluster) are *controls implemented in code* here; the
  formal certification runs on a deployed system (see RT-40 for the deployment-tier
  acceptance plan).
- Where a standard is approximated offline (the WebAuthn assertion core without
  full COSE/CBOR parsing), the seam is designed for
  the certified implementation to drop in — documented at the call site.
