import * as React from 'react';
import {
  Badge,
  Card,
  GuardedAction,
  ProvenanceTag,
  RiskBadge,
  SectionLabel,
  StatBlock,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { AdminApproval, AdminGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Dual-Control Approvals — the flagship guarded-action queue. Each item
 * routes a SECOND approver; `approve()` enforces separation of duties, DENYING
 * self-approval live (requester == approver → AC-AD-4). Denials are audited too
 * (AC-AD-6). Reproduces docs/design/ui_kits/admin/approvals.html; data from
 * `listApprovals()` + `approve(id, approver)`. */

const GUARDED_PATH = [
  'Risk classified (1–5)',
  'Justification captured',
  'Second approver routed out-of-band',
  'Separation of duties verified',
  'Step-up re-auth at approval',
  'Time-boxed single-use token',
  'PQ-signed audit · incl. denials',
];

export function ApprovalsScreen() {
  const g = useGateway<AdminGateway>();
  const { data, loading } = useAsync<AdminApproval[], AdminGateway>((c) => c.listApprovals(), []);
  const [items, setItems] = React.useState<AdminApproval[]>([]);
  const [busy, setBusy] = React.useState<string | null>(null);
  const [tail, setTail] = React.useState<string[]>([
    '> admin audit tail --class approvals',
    '#A-9107  denied   fleet-release   m.silva  self-approval · separation check',
    '#A-9104  approved model-revoke    p.iwu → j.benjamin · token 10m',
    '#A-9101  approved kms-rotation    drill · k.osei → a.novak',
  ]);

  React.useEffect(() => {
    if (data) setItems(data);
  }, [data]);

  // Dual-control decision: routed through the gateway so separation-of-duties is
  // enforced server-side. The self-approval item returns decision='denied'.
  const decide = async (item: AdminApproval, choice: 'approve' | 'deny') => {
    if (choice === 'deny') {
      setItems((xs) => xs.map((x) => (x.id === item.id ? { ...x, state: 'denied', deniedReason: 'DENIED · SECOND CONTROLLER REJECTED' } : x)));
      return;
    }
    setBusy(item.id);
    try {
      const res = await g.approve(item.id, item.approver);
      setItems((xs) =>
        xs.map((x) =>
          x.id === item.id
            ? res.decision === 'granted'
              ? { ...x, state: 'approved' }
              : { ...x, state: 'denied', deniedReason: 'DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)' }
            : x,
        ),
      );
      setTail((t) => [
        t[0],
        `${res.auditId}  ${res.decision === 'granted' ? 'approved' : 'denied  '} ${item.action}  ·  ${res.reason}`,
        ...t.slice(1, 3),
      ]);
    } finally {
      setBusy(null);
    }
  };

  const awaiting = items.filter((i) => i.state === 'awaiting').length;
  const approved = items.filter((i) => i.state === 'approved').length;
  const denied = items.filter((i) => i.state === 'denied').length;

  const aside = (
    <>
      <SectionLabel number={3} muted>THE GUARDED PATH</SectionLabel>
      {GUARDED_PATH.map((step, i) => (
        <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '6px 0', borderBottom: '1px solid var(--divider)' }}>
          <span style={{ fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-black)' as React.CSSProperties['fontWeight'], fontSize: 13, color: 'var(--text-faint)', width: 18 }}>{String(i + 1).padStart(2, '0')}</span>
          <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>{step}</span>
        </div>
      ))}
      <Card size="sm" style={{ marginTop: 4 }}>
        <div className="mg-label" style={{ color: 'var(--text-faint)' }}>APPROVER POOL HEALTH</div>
        <div style={{ display: 'flex', gap: 14, marginTop: 8 }}>
          <StatBlock value="8" label="ELIGIBLE" size="sm" />
          <StatBlock value="31d" label="LAST ROTATION" size="sm" />
          <StatBlock value="0" label="COLLUSION FLAGS" size="sm" />
        </div>
      </Card>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-AD-5" title="Dual-control on key rotation, escalation, model revocation, fleet release, break-glass" />
        <ProvenanceTag id="AC-AD-4" title="Self-approval denied by separation-of-duties check" />
        <ProvenanceTag id="AC-AD-6" title="Denied and simulated actions are audited too" />
      </div>
    </>
  );

  return (
    <ConsolePage
      active="approvals"
      context="ADMIN / DUAL-CONTROL"
      asideWidth={330}
      toolbar={<><Badge tone="warn">{awaiting} AWAITING</Badge><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>DUAL-CONTROL</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Approval queue</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(awaiting)} label="AWAITING" size="sm" accent />
        <StatBlock value={String(14 + approved)} label="APPROVED · 7D" size="sm" />
        <StatBlock value={String(2 + denied)} label="DENIED · 7D" size="sm" />
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {items.map((it) => (
          <GuardedAction
            key={it.id}
            action={it.action}
            subsystem={it.subsystem}
            risk={it.risk}
            requester={it.requester}
            approver={it.approver}
            state={it.state}
            justification={it.justification}
            deniedReason={it.deniedReason}
            onApprove={busy === it.id ? undefined : () => decide(it, 'approve')}
            onDeny={() => decide(it, 'deny')}
          />
        ))}
        {loading && !items.length && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>ROUTING SECOND APPROVERS…</div>}
      </div>

      <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
        <span className="mg-label" style={{ color: 'var(--text-faint)' }}>RISK LEGEND</span>
        <RiskBadge level={3} dualControl={false} />
        <RiskBadge level={4} />
        <RiskBadge level={5} />
      </div>

      <Terminal title="AUDIT · INCLUDING DENIALS" lines={tail} />
    </ConsolePage>
  );
}
