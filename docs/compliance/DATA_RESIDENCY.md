# Data Residency & Sovereignty

Midgard is **sovereign by construction**: it runs entirely inside the customer's
enclosure and makes **no outbound network connection** (NFR-PRV-1). Data residency is
therefore not a policy overlay bolted on afterwards — it is a structural property of
the platform, enforced and testable in code.

## The guarantee

| Data class | Where it lives | Leaves the enclosure? |
|------------|----------------|-----------------------|
| Object fabric / datasets / operating picture | In-enclosure KMS-sealed stores | **No** |
| Long-lived secrets (KMS root, CA/IdP keys, workload keys) | HSM/TPM or in-enclosure sealed storage | **No** |
| Audit chain, config history | In-enclosure append-only sealed journals | **No** |
| Credentials / MFA enrolments | In-enclosure durable directory (encrypted) | **No** |
| Telemetry (logs, metrics, traces) | In-enclosure; **pull-only** collector | **No** |
| Model weights / inference | In-enclosure model server / GPU | **No** |

## How residency is enforced (not just promised)

- **No egress client.** The OSS core has no outbound network dependency; the telemetry
  crate is *zero-dependency* pure-std and literally cannot dial out (RT-33). The
  release/build path runs `--network none` over vendored dependencies.
- **Pull, don't push.** Observability is scraped by an in-enclosure collector
  (`/metrics`, `/health`, `/ready`); nothing is exported.
- **Encrypted at rest, in place.** Every durable store is sealed under a KMS-wrapped
  key that never leaves the enclosure (and, with a hardware root, never leaves the
  device).
- **Encrypted in transit, internally.** Service-to-service is PQ-mTLS; there is no
  external transit to reside anywhere else.
- **Sandboxed ingress.** Connectors that would reach out enforce `SandboxPolicy`
  egress checks and are **air-gapped by default** — an egress-violating connector is
  blocked (RT-27).

## Jurisdiction mapping

Because all processing and storage are physically located wherever the enclosure is
deployed, data residency follows the deployment site 1:1:

| If deployed in… | Data resides in… | Relevant regime (customer-assessed) |
|-----------------|------------------|--------------------------------------|
| EU/EEA site | that site | GDPR — Midgard is the processor; controller obligations stay with the operator |
| UK site | that site | UK GDPR / DPA 2018 |
| National / air-gapped enclave | that enclave | national sovereignty / classified-handling rules |

Midgard does not replicate data across sites unless the operator explicitly configures
cross-node replication (Heimdall picture-sync / DR), which itself runs **only over
PQ-mTLS between the operator's own nodes** — the operator controls where those nodes
are, so cross-border transfer never happens without their action.

## Operator responsibilities

- Choose deployment sites to satisfy residency obligations.
- Configure any cross-node replication/DR only between sites in acceptable
  jurisdictions.
- Act as data controller for personal data; Midgard provides the processor-side
  technical controls (access control, encryption, audit, deletion paths).

See [COMPLIANCE_MATRIX.md](COMPLIANCE_MATRIX.md) for the control-level standards
mapping and `docs/security/THREAT_MODEL.md` for the disclosure/exfiltration threats
these residency controls address.
