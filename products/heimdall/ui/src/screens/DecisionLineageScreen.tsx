import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useAuditStream } from '@midgard/ui-shared';
import type { AuditRecord, HeimdallDecision, HeimdallGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

export function DecisionLineageScreen() {
  const { data, loading } = useAsync<HeimdallDecision[], HeimdallGateway>((c) => c.listDecisions(), []);
  const { data: auditSeed } = useAsync<AuditRecord[], HeimdallGateway>((c) => c.getAudit(8), []);
  const live = useAuditStream(auditSeed ?? [], 40);

  const rows: HeimdallDecision[] = data ?? [];
  const [sel, setSel] = React.useState(0);
  const idx = Math.min(sel, rows.length - 1);
  const d = rows[idx];

  const aside = d ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="scroll-text" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 15, flex: 1 }}>#{d.id}</h2>
        <Badge tone="secure">{d.signature}</Badge>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>REPLAY · EVIDENCE AT DECISION TIME</div>
        {([
          ['Picture state', 'snapshot #S-88121 · 11 feeds'],
          ['Entity', 'E-0871 · UNKNOWN · conf 54%'],
          ['Options shown', '3 · all cited · ranks preserved'],
          ['Chosen', 'opt 01 · re-task NORTH-2'],
          ['Authoriser', d.authoriser + ' · WebAuthn verified'],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-faint)', width: 100, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--text-body)', lineHeight: 1.5 }}>{v}</span>
          </div>
        ))}
      </div>
      <Card size="sm">
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--secure)', marginBottom: 6 }}>OUTCOME</div>
        <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>{d.outcome} · The decision, its evidence, and its result are one chain: audit, review, and after-action all read the same record.</p>
      </Card>
      <Button variant="primary" size="sm" iconLeft={<Icon name="play" size={13} />}>Replay picture at decision time</Button>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-5" title="Full audit lineage on decision workflows" />
        <ProvenanceTag id="NFR-SEC-2" title="Full audit lineage enforced at the data and object layer" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="lineage"
      context="DECIDE / DECISION LINEAGE"
      asideWidth={360}
      toolbar={<Button variant="secondary" size="sm" iconLeft={<Icon name="download" size={14} />}>Export for review</Button>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={5}>LINEAGE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Decision lineage</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="312" label="DECISIONS · 90D" size="sm" />
        <StatBlock value="100%" label="SIGNED + REPLAYABLE" size="sm" accent />
        <StatBlock value="0" label="CHAIN BREAKS" size="sm" />
      </div>
      <div style={{ overflowX: 'auto' }}>
        <Table<HeimdallDecision>
          columns={[
            { header: 'Decision', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--accent-hover)' }}>#{r.id}</span>, width: 90 },
            { header: 'Action', key: 'action' },
            { header: 'Authoriser', key: 'authoriser', mono: true, width: 110 },
            { header: 'Basis', key: 'basis', mono: true, width: 150 },
            { header: 'Outcome', key: 'outcome', width: 170 },
            { header: 'Signature', cell: (r) => <Badge tone="secure">{r.signature}</Badge>, width: 100 },
            { header: 'When', key: 'when', mono: true, width: 100 },
          ]}
          rows={rows}
          selectedIndex={idx}
          onRowClick={setSel}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>VERIFYING CHAIN…</div>}
      </div>

      <div>
        <SectionLabel number={6} muted>DECISION LINEAGE · LIVE</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<AuditRecord>
            compact
            columns={[
              { header: 'Authoriser', key: 'actor', mono: true },
              { header: 'Decision', key: 'action' },
              { header: 'Resource', key: 'resource', mono: true },
              { header: 'Signature', cell: (r) => <Badge tone="secure">{r.pqSignature}</Badge>, width: 110 },
              { header: 'TS', key: 'ts', mono: true, width: 90 },
            ]}
            rows={live}
            style={{ minWidth: 660 }}
          />
        </div>
      </div>

      <Terminal
        title="CHAIN VERIFICATION"
        lines={[
          '> heimdall lineage verify --range 90d',
          'records    312 · signatures 312/312 OK',
          'chain      contiguous · no gaps',
          'replays    snapshots resolvable 312/312',
          '# lineage intact',
        ]}
      />
    </ConsolePage>
  );
}
