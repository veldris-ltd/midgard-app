import * as React from 'react';
import {
  Badge,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { MimirFeedbackItem, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Governed feedback — self-learning bounded by evals and provenance. Poisoning
 * attempts fail the provenance check and never enter retrieval or policy state;
 * the attempt itself is signed lineage. `[FR-MI-5, NFR-AI-1]` */

export function FeedbackScreen() {
  const { data: feedback } = useAsync<MimirFeedbackItem[], MimirGateway>((c) => c.listFeedback(), []);
  const rows: MimirFeedbackItem[] = feedback ?? [];

  const [sel, setSel] = React.useState(3);
  const f = rows[sel];

  const aside = f ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="refresh-cw" size={17} color={f.status === 'danger' ? 'var(--danger)' : 'var(--accent)'} />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{f.id}</h2>
        <StatusPill status={f.status} pulse={false} label={f.statusLabel} />
      </div>
      <Card size="sm">
        <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6 }}>{f.note}</p>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', marginTop: 8 }}>{f.kind.toUpperCase()} · FROM {f.src.toUpperCase()}</div>
      </Card>
      {f.status === 'danger' ? (
        <Card size="sm" style={{ borderColor: 'var(--danger)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--danger)', marginBottom: 6 }}>POISONING DEFENCE · DISCARDED</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>Source identity could not be provenance-checked and the correction pattern matches a known poisoning vector. Nothing entered retrieval or policy state; the attempt is signed lineage.</p>
        </Card>
      ) : (
        <div>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PATH TO POLICY</div>
          {([
            ['Provenance check', true],
            ['Aggregation window', f.statusLabel !== 'Queued'],
            ['Evals gate (no regression)', f.statusLabel === 'Applied'],
            ['Author approval', f.statusLabel === 'Applied'],
            ['Versioned + reversible', f.statusLabel === 'Applied'],
          ] as [string, boolean][]).map(([label, done], i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '6px 0', borderBottom: '1px solid var(--divider)' }}>
              <Icon name={done ? 'check' : 'clock'} size={14} color={done ? 'var(--secure)' : 'var(--text-faint)'} />
              <span style={{ fontSize: 'var(--fs-xs)', color: done ? 'var(--text-body)' : 'var(--text-faint)' }}>{label}</span>
            </div>
          ))}
        </div>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-MI-5" title="Self-learning from governed, provenance-checked feedback only" />
        <ProvenanceTag id="NFR-AI-1" title="Traceable, evaluated, reversible" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="feedback"
      context="MIMIR / GOVERNED FEEDBACK"
      toolbar={<Badge tone="accent">SELF-LEARNING · EVALS-GATED</Badge>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={8}>FEEDBACK</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Governed feedback</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="212" label="RECORDS · 30D" size="sm" />
        <StatBlock value="100%" label="PROVENANCE-CHECKED" size="sm" accent />
        <StatBlock value="1" label="POISONING ATTEMPTS BLOCKED" size="sm" />
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<MimirFeedbackItem>
          columns={[
            { header: 'Record', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: r.status === 'danger' ? 'var(--danger)' : 'var(--accent-hover)' }}>{r.id}</span>, width: 90 },
            { header: 'Source', key: 'src', mono: true, width: 140 },
            { header: 'Kind', key: 'kind', mono: true, width: 100 },
            { header: 'Agent', key: 'target', mono: true, width: 120 },
            { header: 'Feedback', key: 'note' },
            { header: 'Provenance', cell: (r) => <Badge tone={r.prov === 'FAILED' ? 'danger' : 'secure'}>{r.prov === 'FAILED' ? 'FAILED' : r.prov.toUpperCase()}</Badge>, width: 100 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={false} label={r.statusLabel} />, width: 120 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={setSel}
          style={{ minWidth: 820 }}
        />
      </div>

      <Terminal title="LEARNING LEDGER" lines={[
        '> mimir feedback ledger --window 30d',
        'received   212 · verified 211 · discarded 1',
        'updates    3 policy versions · all evals-gated',
        'reverts    0 · every update reversible',
        '# self-learning is bounded autonomy',
      ]} />
    </ConsolePage>
  );
}
