import * as React from 'react';
import {
  Badge,
  Button,
  Icon,
  Input,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useAuditStream, useGateway } from '@midgard/ui-shared';
import type { AdminGateway, AdminLedgerEntry, AuditRecord } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Audit Ledger — one append-only, ML-DSA-signed, tamper-evident ledger
 * across all subsystems (NFR-PQ-3); denied + simulated actions are entries too
 * (AC-AD-6). Cross-subsystem search + read-only investigation assistant (FR-AD-9).
 * Reproduces docs/design/ui_kits/admin/ledger.html; data from `searchLedger()`
 * and the live `useAuditStream()`. */

const FILTERS = ['all', 'approvals', 'denials', 'break-glass', 'simulations'];

export function LedgerScreen() {
  const g = useGateway<AdminGateway>();
  const [draft, setDraft] = React.useState('');
  const [applied, setApplied] = React.useState('');
  const [filter, setFilter] = React.useState('all');
  const [sel, setSel] = React.useState(1);
  const { data } = useAsync<AdminLedgerEntry[], AdminGateway>((c) => c.searchLedger(applied), [applied]);
  const live = useAuditStream([], 24);

  const rows: AdminLedgerEntry[] = data ?? [];
  const r = rows[sel] ?? rows[0];

  const aside = (
    <>
      <SectionLabel number={12} muted>INVESTIGATION ASSISTANT · READ-ONLY</SectionLabel>
      <div style={{ padding: '11px 13px', background: 'var(--accent-tint)', border: '1px solid var(--accent)', borderRadius: 'var(--radius-md)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>Who touched the KMS in the last 24 hours, and was anything unusual?</div>
      <div style={{ padding: '12px 14px', background: 'var(--surface-card)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)' }}>
        <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.65 }}>
          Two KMS events in 24h: a dual-controlled rotation approval <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-hover)' }}>[audit:A-9108]</span> and a break-glass session <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-hover)' }}>[audit:B-0331]</span>, both by k.osei with a.novak as second officer.
        </p>
        <div style={{ marginTop: 9, padding: '8px 10px', background: 'var(--surface-inset)', border: '1px solid var(--warn)', borderRadius: 'var(--radius-sm)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--warn)' }}>PATTERN FOR HUMAN REVIEW</div>
          <div style={{ fontSize: 'var(--fs-micro)', fontFamily: 'var(--font-mono)', color: 'var(--text-body)', marginTop: 3, lineHeight: 1.6 }}>SAME REQUESTER/APPROVER PAIR ON BOTH EVENTS · NO INTENT ASSERTED</div>
        </div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 8.5, color: 'var(--text-faint)', marginTop: 8 }}>EVERY FINDING CITED · OBSERVE-ONLY · CANNOT MUTATE [FR-AD-10]</div>
      </div>
      {r && (
        <div style={{ marginTop: 'auto' }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>SELECTED ENTRY · #{r.id}</div>
          <Terminal
            lines={[
              'actor     ' + r.actor,
              'action    ' + r.action,
              'approver  ' + r.approver,
              'sig       ML-DSA ✓ · chain intact',
              '# denied + simulated actions are entries too [AC-AD-6]',
            ]}
          />
        </div>
      )}
    </>
  );

  return (
    <ConsolePage
      active="ledger"
      context="ADMIN / AUDIT LEDGER"
      asideWidth={360}
      toolbar={<><Button variant="secondary" size="sm" iconLeft={<Icon name="download" size={14} />}>Export for auditors</Button><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={11}>LEDGER</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Audit search</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="7" label="SUBSYSTEMS · ONE LEDGER" size="sm" />
        <StatBlock value="100%" label="SIGNED · TAMPER-EVIDENT" size="sm" accent />
        <StatBlock value="0" label="ENTRIES ESCAPED" size="sm" />
      </div>

      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
        <Input
          size="sm"
          mono
          placeholder="actor:k.osei subsystem:kms since:24h…"
          prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
          style={{ width: 320 }}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { setApplied(draft); setSel(0); } }}
        />
        {FILTERS.map((f) => (
          <Tag key={f} active={filter === f} onClick={() => setFilter(f)}>{f}</Tag>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<AdminLedgerEntry>
          columns={[
            { header: 'Entry', cell: (x) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: x.action.includes('DENIED') ? 'var(--danger)' : 'var(--accent-hover)' }}>#{x.id}</span>, width: 90 },
            { header: 'Actor', key: 'actor', mono: true, width: 110 },
            { header: 'Action', key: 'action' },
            { header: 'Subsystem', key: 'subsystem', mono: true, width: 120 },
            { header: 'Justification', key: 'justification', width: 180 },
            { header: 'Approver', key: 'approver', mono: true, width: 100 },
            { header: 'When', key: 'ts', mono: true, width: 110 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 880 }}
        />
      </div>

      <div>
        <SectionLabel number={13} muted>LEDGER LINEAGE · LIVE</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<AuditRecord>
            compact
            columns={[
              { header: 'Actor', key: 'actor', mono: true },
              { header: 'Operation', key: 'action' },
              { header: 'Resource', key: 'resource', mono: true },
              { header: 'Signature', cell: (x) => <Badge tone="secure">{x.pqSignature}</Badge>, width: 110 },
              { header: 'TS', key: 'ts', mono: true, width: 90 },
            ]}
            rows={live}
            style={{ minWidth: 640 }}
          />
        </div>
      </div>

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        <ProvenanceTag id="FR-AD-9" title="Aggregated observability: health, SLOs, security events, audit search" />
        <ProvenanceTag id="AC-AD-6" title="Every admin action including denied and simulated is ledgered" />
        <ProvenanceTag id="NFR-PQ-3" title="Append-only, ML-DSA-signed, tamper-evident, exportable" />
      </div>
    </ConsolePage>
  );
}
