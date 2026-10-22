import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { ForgeConnector, ForgeGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Connector catalogue: typed inbound-pull sources over PQ-mTLS, no egress.
 * Sourced from `listConnectors()`. Reproduces forge/connectors.html. */
export function ConnectorsScreen() {
  const live = useGateway<ForgeGateway>().live === true;
  const { data, loading } = useAsync<ForgeConnector[], ForgeGateway>((c) => c.listConnectors(), []);
  const all = data ?? [];
  const FILTERS = ['all', 'streaming', 'batch', 'needs attention'] as const;
  const [filter, setFilter] = React.useState<(typeof FILTERS)[number]>('all');
  const rows = all.filter((r) =>
    filter === 'streaming' ? /stream/i.test(r.cadence) : filter === 'batch' ? !/stream/i.test(r.cadence) : filter === 'needs attention' ? r.state === 'warn' : true,
  );

  const [sel, setSel] = React.useState(0);
  const c = rows[sel];

  const trust: Array<[string, string]> = c
    ? [
        ['Direction', 'inbound pull only'],
        ['Transport', c.transport],
        ['Egress', c.egress],
        ['Schema', c.schema],
      ]
    : [];

  const aside = c ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="database" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{c.name}</h2>
        <StatusPill status={c.state} pulse={c.state === 'online'} label={c.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={String(c.datasetsFed)} label="DATASETS FED" size="sm" />
          <StatBlock value={c.volume} label="VOLUME" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>TRUST BOUNDARY</div>
        {trust.map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 80, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      {c.action && (
        <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--warn)', marginBottom: 6 }}>ACTION REQUIRED</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>{c.action}</p>
          {!live && <Button variant="outline" size="sm" style={{ marginTop: 10 }}>Rotate certificate</Button>}
        </Card>
      )}
      <Terminal title="LAST RUN" lines={c.lastRun ?? ['> forge connector test ' + c.name, 'handshake  PQ-mTLS OK', 'rows       48,112 · schema OK', '# contract satisfied']} style={{ marginTop: 'auto' }} />
    </>
  ) : null;

  return (
    <ConsolePage
      active="connectors"
      context="DATA OPS / CONNECTORS"
      toolbar={<ConsoleToolbar placeholder="Search connectors, kinds, schemas…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>CONNECTORS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Connector catalogue</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(all.length)} label="CONNECTORS" size="sm" />
        <StatBlock value={String(all.filter((r) => r.state !== 'warn').length)} label="HEALTHY" size="sm" />
        <StatBlock value="0" label="OUTBOUND PATHS" size="sm" accent />
        {!live && <Button variant="primary" iconLeft={<Icon name="plus" size={15} />}>Add connector</Button>}
      </div>

      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
        {FILTERS.map((f) => (
          <Tag key={f} active={filter === f} onClick={() => { setFilter(f); setSel(0); }}>
            {f}{f === 'needs attention' ? ` · ${all.filter((r) => r.state === 'warn').length}` : ''}
          </Tag>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<ForgeConnector>
          columns={[
            { header: 'Connector', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name={r.kind === 'Stream' ? 'radio-tower' : 'database'} size={14} color="var(--accent)" />{r.name}</span> },
            { header: 'Kind', key: 'kind', width: 120 },
            { header: 'Cadence', key: 'cadence', mono: true, width: 130 },
            { header: 'Datasets', key: 'datasetsFed', mono: true, align: 'right', width: 90 },
            { header: 'Volume', key: 'volume', mono: true, align: 'right', width: 100 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={r.state === 'online'} label={r.stateLabel} />, width: 150 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 760 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>PROBING CONNECTORS…</div>}
      </div>
    </ConsolePage>
  );
}
