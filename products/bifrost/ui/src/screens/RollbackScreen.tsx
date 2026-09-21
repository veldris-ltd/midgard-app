import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { BifrostGateway, BifrostRollback } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Rollback centre — standing plans, automatic health-gated triggers, drill
 * history. Rollback is rehearsed, not hoped for, and the drill itself is signed
 * lineage. `[FR-B-2]` */

export function RollbackScreen() {
  const g = useGateway<BifrostGateway>();
  const { data, loading } = useAsync<BifrostRollback[], BifrostGateway>((c) => c.listRollbacks(), []);
  const rows: BifrostRollback[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const [drill, setDrill] = React.useState(false);
  const [executing, setExecuting] = React.useState(false);
  const [rbAudit, setRbAudit] = React.useState<string | null>(null);
  const [log, setLog] = React.useState<string[]>([
    '> bifrost rollback drill --plan release-2.5.0',
    '# last drill 2d ago · 41s to healthy',
  ]);
  const p = rows[sel];

  // Health gate: a plan is executable only while armed/available (secure/healthy).
  const gated = !p || (p.state !== 'secure' && p.state !== 'healthy');

  const runDrill = () => {
    if (drill) return;
    setDrill(true);
    setLog((l) => [...l, '> bifrost rollback drill --plan release-2.5.0']);
    ([
      ['freeze     promotions paused', 400],
      ['revert     2.5.0 → 2.4.1 · ring order reversed', 1200],
      ['health     p99 OK · errors 0 · 38s', 2200],
      ['restore    promotions resumed · state intact', 3000],
      ['# drill complete · 41s to healthy · audit #R-2214', 3400],
    ] as const).forEach(([line, t]) => setTimeout(() => setLog((l) => [...l, line]), t));
    setTimeout(() => setDrill(false), 3600);
  };

  const execute = async () => {
    if (!p || gated || executing) return;
    setExecuting(true);
    try {
      const res = await g.rollback(p.id);
      setRbAudit(res.auditId);
      setLog((l) => [
        ...l,
        `> bifrost rollback execute --plan ${p.id}`,
        `# rollback complete · health-gated · audit ${res.auditId}`,
      ]);
    } finally {
      setExecuting(false);
    }
  };

  const aside = p ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="undo-2" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, flex: 1 }}>{p.kind} rollback</h2>
        <StatusPill status={p.state} pulse={false} label={p.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', lineHeight: 1.7 }}>{p.scope}</div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>AUTOMATIC TRIGGERS</div>
        {([
          ['p99 latency > 250ms · 3 min', p.kind === 'release'],
          ['error rate > 0.1% · 3 min', p.kind === 'release'],
          ['eval pass < gate', p.kind === 'agent policy'],
          ['manual only', p.trigger === 'manual only'],
        ] as const)
          .filter(([, on]) => on)
          .map(([l]) => (
            <div key={l} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '6px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
              <Icon name="zap" size={13} color="var(--warn)" />{l}
            </div>
          ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>HEALTH GATE</div>
        <Badge tone={gated ? 'warn' : 'secure'}>{gated ? 'GATE HELD' : 'GATE CLEAR'}</Badge>
        <p style={{ margin: '10px 0 0', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
          Execution is health-gated: a plan runs only while armed and its health gate is clear. The reversal reverses ring order — broad before canary — with zero downtime, and the decision is signed lineage.
        </p>
      </div>
      {rbAudit && (
        <div className="mg-label" style={{ color: 'var(--text-faint)' }}>
          EXECUTED · AUDIT <span style={{ color: 'var(--accent-hover)' }}>{rbAudit}</span>
        </div>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        <ProvenanceTag id="FR-B-2" title="Zero-downtime upgrades with health-gated automatic rollback" />
      </div>
      <Button
        variant="primary"
        size="sm"
        fullWidth
        style={{ marginTop: 'auto' }}
        iconLeft={<Icon name="undo-2" size={14} />}
        onClick={execute}
        disabled={gated || executing}
      >
        {executing ? 'Rolling back…' : gated ? 'Rollback held by gate' : 'Execute rollback'}
      </Button>
    </>
  ) : null;

  return (
    <ConsolePage
      active="rollback"
      context="BIFROST / ROLLBACK CENTRE"
      asideWidth={340}
      toolbar={
        <>
          <ConsoleToolbar placeholder="Search rollback plans…" />
          <Button variant="secondary" size="sm" iconLeft={<Icon name="play" size={14} />} onClick={runDrill} disabled={drill}>
            {drill ? 'Drilling…' : 'Run drill'}
          </Button>
        </>
      }
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={5}>ROLLBACK</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Rollback centre</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="STANDING PLANS" size="sm" />
        <StatBlock value="41s" label="LAST DRILL · TO HEALTHY" size="sm" accent />
        <StatBlock value="0" label="UNPLANNED ROLLBACKS · 90D" size="sm" />
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<BifrostRollback>
          columns={[
            { header: 'Plan', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="undo-2" size={14} color="var(--accent)" />{r.scope}</span> },
            { header: 'Kind', key: 'kind', mono: true, width: 120 },
            { header: 'Trigger', key: 'trigger', width: 170 },
            { header: 'Readiness', key: 'readiness', mono: true, width: 160 },
            { header: 'Last drill', key: 'lastDrill', mono: true, width: 100 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 120 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 840 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING PLANS…</div>}
      </div>

      <Terminal title={drill ? 'DRILL · RUNNING' : 'DRILL LOG'} lines={log} style={{ flex: 1, overflowY: 'auto' }} />
    </ConsolePage>
  );
}
