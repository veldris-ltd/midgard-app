import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { ForgeGateway, ForgeHealthCheck } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Data health: expectations, drift monitors, and freshness SLOs that gate every
 * projection. Sourced from `listHealthChecks()`. Reproduces forge/health.html. */
export function DataHealthScreen() {
  const live = useGateway<ForgeGateway>().live === true;
  const { data, loading } = useAsync<ForgeHealthCheck[], ForgeGateway>((c) => c.listHealthChecks(), []);
  const rows = data ?? [];
  const failing = rows.filter((r) => r.state === 'warn').length;
  const blocked = rows.filter((r) => r.projectionGate === 'BLOCKS').length;

  const [sel, setSel] = React.useState(0);
  const c = rows[sel];

  const summary: Array<[string, string, string]> = live
    ? [
        ['PASSING', String(rows.length - failing), 'var(--secure)'],
        ['STALE OR FAILING', String(failing), 'var(--warn)'],
        ['BLOCKING', String(blocked), 'var(--danger)'],
      ]
    : [
        ['PASSING', '1,199', 'var(--secure)'],
        ['DRIFTING', '3', 'var(--warn)'],
        ['BLOCKING', '2', 'var(--danger)'],
      ];

  const aside = c ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="activity" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, fontFamily: 'var(--font-mono)', fontWeight: 600, flex: 1 }}>{c.kind}</h2>
        <StatusPill status={c.state} pulse={false} label={c.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', lineHeight: 1.7 }}>{c.check}</div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', marginTop: 6 }}>ON {c.dataset.toUpperCase()}</div>
      </Card>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={c.passRate} label="PASS RATE" size="sm" accent={c.state === 'warn'} />
          <StatBlock value={c.projectionGate === 'BLOCKS' ? 'BLOCKS' : 'CLEAR'} label="PROJECTION GATE" size="sm" />
        </div>
        <div style={{ marginTop: 10 }}>
          <MeterBar value={c.passValue} label="PASS" width={150} warnAt={98} dangerAt={95} invert />
        </div>
      </Card>
      {c.diagnosis && (
        <Card size="sm" style={{ borderColor: 'var(--accent)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--accent-hover)', marginBottom: 6 }}>{live ? 'LAST RUN SAID' : 'MIMIR DIAGNOSIS · LOCAL'}</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55, overflowWrap: 'anywhere' }}>{c.diagnosis}</p>
          {!live && (
            <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
              <Button variant="outline" size="sm" fullWidth>Preview quarantine</Button>
              <Button variant="ghost" size="sm">Dismiss</Button>
            </div>
          )}
        </Card>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-F-6" title="Expectations and health checks gate every build" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="health"
      context="DATA OPS / DATA HEALTH"
      toolbar={<ConsoleToolbar placeholder="Search expectations, datasets, kinds…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={4}>DATA HEALTH</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Expectations &amp; drift</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={live ? String(rows.length) : '1,204'} label={live ? 'CHECKS' : 'EXPECTATIONS'} size="sm" />
        <StatBlock value={live ? String(failing) : '3'} label="FAILING" size="sm" accent />
        <StatBlock value={live ? String(blocked) : '2'} label="PROJECTIONS BLOCKED" size="sm" />
        {!live && <Button variant="primary" iconLeft={<Icon name="plus" size={15} />}>New expectation</Button>}
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {summary.map(([t, v, col]) => (
          <Card key={t} size="sm" style={{ flex: '1 1 160px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ width: 8, height: 8, borderRadius: '50%', background: col }} />
              <span style={{ fontFamily: 'var(--font-display)', fontWeight: 800, fontSize: 22, color: 'var(--text-heading)' }}>{v}</span>
              <span className="mg-label" style={{ color: 'var(--text-faint)' }}>{t}</span>
            </div>
          </Card>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<ForgeHealthCheck>
          columns={[
            { header: 'Dataset', key: 'dataset', mono: true, width: 170 },
            { header: 'Expectation', key: 'check' },
            { header: 'Kind', key: 'kind', mono: true, width: 100 },
            { header: 'Pass rate', key: 'passRate', mono: true, align: 'right', width: 90 },
            { header: 'Trend', key: 'trend', mono: true, width: 130 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 120 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 780 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>EVALUATING EXPECTATIONS…</div>}
      </div>
    </ConsolePage>
  );
}
