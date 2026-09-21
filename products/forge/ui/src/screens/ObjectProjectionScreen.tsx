import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { ForgeGateway, ForgeProjection, ForgeProjectionMapping } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

const KIND_TONE: Record<ForgeProjectionMapping['kind'], 'neutral' | 'accent' | 'warn'> = {
  key: 'neutral',
  property: 'neutral',
  struct: 'neutral',
  link: 'accent',
  governance: 'warn',
};

/* Object projection: mapping curated dataset columns onto Midgard fabric
 * objects, links, and markings — governance carried through at query time.
 * Sourced from `listProjections()`. Reproduces forge/projection.html. */
export function ObjectProjectionScreen() {
  const { data, loading } = useAsync<ForgeProjection[], ForgeGateway>((c) => c.listProjections(), []);
  const projections = data ?? [];
  const p = projections[0];

  const [sel, setSel] = React.useState(0);
  const rows = p?.mappings ?? [];
  const m = rows[sel];

  const aside = p ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="boxes" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, flex: 1 }}>{p.name}</h2>
        <StatusPill status="healthy" pulse={false} label="In sync" />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={p.objects} label="OBJECTS" size="sm" />
          <StatBlock value={p.syncLag} label="SYNC LAG" size="sm" />
        </div>
      </Card>
      {m && (
        <div>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>SELECTED MAPPING</div>
          <Terminal lines={[m.column + '  →  ' + m.to, '# ' + m.note]} />
        </div>
      )}
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>GOVERNANCE CARRY-THROUGH</div>
        <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>Row markings survive projection: a CONFIDENTIAL source row yields a CONFIDENTIAL object. Policy applies at query time in the fabric, not at copy time.</p>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 10 }}>
          <ProvenanceTag id="FR-F-4" title="Project curated datasets into Midgard object/link types via a mapping layer" />
          <ProvenanceTag id="NFR-SEC-2" title="Markings enforced at query time" />
        </div>
      </div>
      <Button variant="secondary" size="sm" style={{ marginTop: 'auto' }} iconLeft={<Icon name="arrow-up-right" size={14} />}>Open type in Midgard</Button>
    </>
  ) : null;

  return (
    <ConsolePage
      active="projection"
      context="DATA OPS / OBJECT PROJECTION"
      toolbar={<ConsoleToolbar placeholder="Search mappings, columns, object types…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={5}>PROJECTION</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>flight_tracks_eu → Aircraft</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={p?.columnsMapped ?? '—'} label="COLUMNS MAPPED" size="sm" accent />
        <StatBlock value={String(p?.linkTypes ?? '—')} label="LINK TYPES" size="sm" />
        <StatBlock value={String(p?.unmarkedRows ?? '—')} label="UNMARKED ROWS" size="sm" />
        <Button variant="primary" iconLeft={<Icon name="boxes" size={15} />}>Sync now</Button>
      </div>

      {p && (
        <div className="mg-grid-bg" style={{ display: 'flex', alignItems: 'center', gap: 10, padding: 18, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', flexWrap: 'wrap' }}>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8, padding: '9px 13px', borderRadius: 8, background: 'var(--surface-card)', border: '1px solid var(--border)' }}>
            <Icon name="table" size={15} color="var(--accent)" />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)' }}>{p.source}</span>
          </span>
          <Icon name="chevron-right" size={15} color="var(--text-faint)" />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', letterSpacing: '0.1em' }}>MAPPING · {rows.length} RULES</span>
          <Icon name="chevron-right" size={15} color="var(--text-faint)" />
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8, padding: '9px 13px', borderRadius: 8, background: 'var(--surface-card)', border: '1px solid var(--accent)' }}>
            <Icon name="boxes" size={15} color="var(--accent)" />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)' }}>{p.target}</span>
          </span>
          <span style={{ marginLeft: 'auto' }}><MarkingChip level={p.marking} compartment={p.compartment ?? undefined} /></span>
        </div>
      )}

      <div style={{ overflowX: 'auto' }}>
        <Table<ForgeProjectionMapping>
          columns={[
            { header: 'Source column(s)', key: 'column', mono: true },
            { header: 'Maps to', key: 'to', mono: true },
            { header: 'Kind', cell: (r) => <Badge tone={KIND_TONE[r.kind]}>{r.kind.toUpperCase()}</Badge>, width: 130 },
            { header: 'Transform', key: 'note', width: 220 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 720 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING MAPPING…</div>}
      </div>
    </ConsolePage>
  );
}
