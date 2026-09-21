# Security Policy

Midgard is security-critical infrastructure: a sovereign, post-quantum, in-enclosure
platform. We take vulnerability reports seriously and operate a coordinated,
safe-harbour disclosure process.

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability.**

- **Email:** security@veldris.io
- **PGP:** encrypt sensitive reports to the key published at
  `https://veldris.io/.well-known/security.txt` (and mirrored in-repo as the release
  trust material). Please encrypt any exploit details or proof-of-concept.
- Alternatively, use GitHub **private vulnerability reporting** on
  `veldris-ltd/midgard-app` (Security → Report a vulnerability).

Please include: affected component/version (commit or release tag), a description,
reproduction steps or PoC, and the impact you believe it has.

### Our commitment (safe harbour)

- We **acknowledge** within **1 working day** and give an initial assessment within
  **5 working days**.
- We will not pursue or support legal action against good-faith researchers who
  follow this policy, avoid privacy violations and service degradation, and give us
  reasonable time to remediate before public disclosure.
- We practise **coordinated disclosure**: we agree a disclosure date with you,
  default target **90 days** from report (sooner if a fix ships and users can update;
  longer only by mutual agreement for exceptionally complex fixes).
- Where you wish, we **credit** you in the advisory and CHANGELOG.

## Scope

In scope: the code in this repository — the crypto crates, services, product
subsystems (Forge/Mimir/Bifrost/Heimdall), the Admin control plane, client crates,
the supply-chain tools, the gateway edges, and the build/release pipeline.

Out of scope: third-party vendored crates (report upstream; tell us so we can pin a
fix), findings requiring a compromised HSM/host, and social-engineering of Veldris
staff.

## Post-quantum-specific considerations

Because Midgard's threat model explicitly includes **"harvest-now, decrypt-later"**
adversaries, we are especially interested in:

- **Downgrade / hybrid-negotiation flaws** in the PQ-mTLS handshake
  (`crypto/pq-mtls`) — anything that could force a classical-only suite, strip the
  ML-KEM half of the hybrid KEM, or bypass the fail-closed PQ requirement.
- **Signature-verification bypass** or fail-open in ML-DSA/SLH-DSA paths
  (`crypto/pqc-agility`, audit chain, token/cert verification, `mg-verify`).
- **Key-custody weaknesses** — the KMS trust root (`services/kms`), envelope
  encryption, rotation, and the software-vs-hardware root residual risk
  (`services/kms/RESIDUAL_RISK.md`).
- **Non-constant-time** operations on secret material, and gaps in zeroization
  (`crypto/pqc-agility/src/secret.rs`).
- **AEAD nonce reuse / record-layer** issues in the PQ-mTLS record layer.
- **Governance / policy-enforcement bypass** (deny-by-default PDP, Bell–LaPadula
  markings, dual-control, break-glass).

A cryptographic weakness that undermines the PQ guarantees is treated as **Critical**
regardless of current exploitability, given the long-lived-secret exposure.

## Severity & handling

We classify with CVSS plus a PQ-impact overlay. Critical/High fixes are prioritised;
we ship the fix, an advisory (GHSA), and — where a signed release is affected — a new
PQ-signed release with an updated SBOM (`mg-sign`/`mg-sbom`/`mg-verify`). Fixed
versions and credits are recorded in [CHANGELOG.md](CHANGELOG.md).

## Supported versions

Pre-1.0: the latest tagged release and `main` receive fixes. A supported-versions
table will be published at the 1.0 release.
