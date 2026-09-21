# Incident Response & On-Call

In-enclosure, sovereign incident handling: everything below runs inside the customer
enclosure. Signals come from the local telemetry stack (RT-33) — `/metrics`,
`/api/v1/ready`, structured logs, traces — never from an external APM.

## Severity levels

| Sev | Definition | Examples | Page |
|-----|------------|----------|------|
| **Sev-1** | Platform down or integrity/security compromise | enclosure unreachable, audit-chain verification fails, suspected key compromise, break-glass abuse | immediate, 24×7 |
| **Sev-2** | Major degradation or SLO breach | availability SLO breaching (`slo_breaching{...} 1`), a subsystem `/ready` = 503, first-token SLO breach | immediate, business hours + on-call |
| **Sev-3** | Minor / single-tenant / no SLO impact | elevated latency within budget, non-critical component flapping | next business day |

## On-call roster & escalation

- **Primary on-call SRE** — owns triage and the runbooks. First responder for all Sevs.
- **Secondary SRE** — backup; auto-paged if primary does not ack within **10 min**.
- **Duty Security Officer (SO)** — paged for **any** Sev-1, and for anything touching
  key custody, audit integrity, break-glass, or DR (trust-root recovery is SO-gated).
- **Engineering lead** — escalation for a Sev-1 unresolved after **60 min**, or any
  incident needing a code change.

Escalation ladder: Primary → (10 min no-ack) Secondary → (60 min unresolved, Sev-1) Eng
lead. The **SO is a parallel page, not a ladder rung** — they are always in the loop for
Sev-1 and security-relevant events, because dual-control (RT-31) means no single
responder can unilaterally mutate high-risk config even during an incident.

## Response workflow

1. **Acknowledge** the page (within the SLA above) and open an incident channel.
2. **Assess & classify** severity from telemetry: `/api/v1/ready` per service,
   `/metrics` (`slo_breaching`, error counters), recent structured logs (filter by
   `trace_id` to follow one request end-to-end).
3. **Stabilise** using the matching runbook ([RUNBOOKS.md](RUNBOOKS.md)); for data/node
   loss follow the [DR_PLAN.md](DR_PLAN.md) restore runbook.
4. **Communicate** status on the incident channel at a cadence matched to severity
   (Sev-1: every 30 min).
5. **Resolve**: confirm `/ready` healthy and the breaching SLO recovered in `/metrics`;
   downgrade/close.
6. **Post-mortem** (blameless), required for every Sev-1/2 within 3 business days.

## Guardrails during an incident

- **Dual-control still applies.** High-risk config changes go through the guarded
  orchestrator (RT-31) even under pressure — a distinct approver + single-use token +
  audit. Emergency access uses **break-glass** (RT-32): time-boxed, alerted, and
  reviewed; it is not a way around the controls.
- **No egress.** Do not exfiltrate logs/metrics/traces for "external analysis" — that
  violates NFR-PRV-1. All investigation is in-enclosure.
- **Audit everything.** Break-glass grants, config commits, and DR actions are sealed
  into the audit chain; the post-mortem reads that chain, not memory.

## Post-mortem template (minimum)

- Timeline (from the audit chain + traces, not recollection).
- Severity, detection source, time-to-ack, time-to-resolve.
- RPO/RTO achieved (for DR incidents) vs the stated objectives.
- Root cause, contributing factors.
- Action items with owners + due dates; at least one must be a **test or runbook
  improvement** that would have caught or shortened this incident.
