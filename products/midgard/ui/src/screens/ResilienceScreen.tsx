import * as React from 'react';
import {
  Badge,
  Card,
  Icon,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { RolloutState } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

const STATE_PILL: Record<RolloutState['state'], { status: React.ComponentProps<typeof StatusPill>['status']; label: string; pulse: boolean }> = {
  pending: { status: 'offline', label: 'Pending', pulse: false },
  'in-progress': { status: 'live', label: 'In progress', pulse: true },
  healthy: { status: 'healthy', label: 'Healthy', pulse: false },
  'rolled-back': { status: 'danger', label: 'Rolled back', pulse: false },
};

const GATE_TONE: Record<RolloutState['healthGate'], { tone: React.ComponentProps<typeof Badge>['tone']; label: string }> = {
  pass: { tone: 'secure', label: 'GATE PASS' },
  watch: { tone: 'warn', label: 'GATE WATCH' },
  fail: { tone: 'danger', label: 'GATE FAIL' },
};

export function ResilienceScreen() {
  const { data, loading } = useAsync((c) => c.getResilience(), []);
  const rows: RolloutState[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const r = rows[sel];

  const healthy = rows.filter((x) => x.state === 'healthy').length;
  const inProgress = rows.filter((x) => x.state === 'in-progress').length;
  const watches = rows.filter((x) => x.healthGate === 'watch').length;

  const aside = r ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="git-branch" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1 }}>{r.id}</h2>
        <StatusPill status={STATE_PILL[r.state].status} pulse={STATE_PILL[r.state].pulse} label={STATE_PILL[r.state].label} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={r.version} label="TARGET VERSION" size="sm" />
          <StatBlock value={`${r.progress}%`} label={`RING · ${r.ring.toUpperCase()}`} size="sm" accent={r.state === 'healthy'} />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>HEALTH GATE</div>
        <Badge tone={GATE_TONE[r.healthGate].tone}>{GATE_TONE[r.healthGate].label}</Badge>
        <p style={{ margin: '10px 0 0', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
          Each ring advances only while its health gate holds. A failing gate halts the fleet and rolls the ring back to the last-good version — zero-downtime, no operator in the loop.
        </p>
      </div>
      <Terminal title="ROLLOUT · HEALTH-GATED" lines={[
        `> midgard rollout status ${r.id}`,
        `ring       ${r.ring} · ${r.progress}%`,
        `gate       ${r.healthGate}`,
        r.healthGate === 'fail'
          ? '# gate FAIL → auto rollback to last-good'
          : '# advancing under gate · signed lineage',
      ]} />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="NFR-AVA-1" title="Zero-downtime, health-gated progressive rollouts" />
        <ProvenanceTag id="DD-6" title="Ring-based fleet reconciliation across the enclosure" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="resilience"
      context="ENCLOSURE / RESILIENCE"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search rollouts, rings, versions…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={12}>RESILIENCE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Rollouts & health gates</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={healthy} label="ROLLOUTS HEALTHY" size="sm" accent />
        <StatBlock value={inProgress} label="IN PROGRESS" size="sm" />
        <StatBlock value={watches} label="GATE WATCHES" size="sm" />
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<RolloutState>
          columns={[
            { header: 'Rollout', cell: (x) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="git-branch" size={14} color="var(--accent)" />{x.id}</span> },
            { header: 'Ring', cell: (x) => <Badge tone={x.ring === 'canary' ? 'info' : 'neutral'}>{x.ring.toUpperCase()}</Badge>, width: 110 },
            { header: 'Version', key: 'version', mono: true, width: 120 },
            { header: 'Progress', cell: (x) => <MeterBar value={x.progress} width={90} warnAt={x.healthGate === 'watch' ? 0 : null} format={(v) => `${v}%`} />, width: 190 },
            { header: 'State', cell: (x) => <StatusPill status={STATE_PILL[x.state].status} pulse={STATE_PILL[x.state].pulse} label={STATE_PILL[x.state].label} />, width: 150 },
            { header: 'Health gate', cell: (x) => <Badge tone={GATE_TONE[x.healthGate].tone}>{GATE_TONE[x.healthGate].label}</Badge>, width: 140 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RECONCILING RINGS…</div>}
      </div>

      <div>
        <SectionLabel number={13} muted>PRINCIPLE</SectionLabel>
        <Card size="sm" style={{ marginTop: 8 }}>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
            Rollouts move ring by ring — canary before broad — and never without a passing health gate. A gate that trips triggers an automatic, zero-downtime rollback to the last-good version; the decision and its lineage are signed, so every advance and reversal is auditable after the fact.
          </p>
        </Card>
      </div>
    </ConsolePage>
  );
}
