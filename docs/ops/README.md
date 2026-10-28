# Operations

In-enclosure operational documentation for running Midgard. Everything here assumes
the sovereign posture: no cloud dependency, no telemetry or data egress (NFR-PRV-1).

| Doc | Purpose |
|-----|---------|
| [RUNBOOKS.md](RUNBOOKS.md) | Step-by-step procedures for recurring ops tasks (RB-1..RB-7). |
| [DR_PLAN.md](DR_PLAN.md) | Disaster recovery: RTO/RPO, the restore runbook, drill cadence. |
| [BACKUP_RESTORE.md](BACKUP_RESTORE.md) | Backup/snapshot + restore for every durable store (RT-09..14). |
| [INCIDENT_RESPONSE.md](INCIDENT_RESPONSE.md) | Severity levels, on-call roster, escalation, post-mortems. |
| [KEY_CEREMONY.md](KEY_CEREMONY.md) | The release key ceremony: held with the Dvalin app (veldris-ltd/dvalin-app); what it leaves in this repository. |
| [PKI.md](PKI.md) | CA/PKI bootstrap, cert issuance/renewal/revocation, HSM key ceremony, rotation, signing runner. |

## Tested, not just written

The DR restore procedure is exercised on every CI run by the automated drill
`admin/config/tests/dr_drill.rs`: it simulates total node/disk loss and follows the DR
runbook against the real KMS-sealed durable store, asserting a verified restore within
the stated RTO/RPO and that a tampered backup fails closed. Because every durable store
shares the same `SealedJournal` primitive, the drill result generalises across them.

## Observability inputs

All runbooks read from the in-enclosure telemetry stack (`services/telemetry`, RT-33):
`GET /api/v1/health` (liveness), `GET /api/v1/ready` (readiness), `GET /metrics`
(Prometheus text incl. `slo_breaching`), structured JSON logs correlated by `trace_id`,
and buffered traces. Nothing is pushed out; a scraper/collector inside the enclosure
pulls it.
