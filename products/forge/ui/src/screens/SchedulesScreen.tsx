import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { ForgeGateway, ForgeSchedule } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Schedules & triggers: batch and streaming pipelines with cadence, triggers,
 * and run history. Sourced from `listSchedules()`. Reproduces forge/schedules.html. */
export function SchedulesScreen() {
  const { data, loading } = useAsync<ForgeSchedule[], ForgeGateway>((c) => c.listSchedules(), []);
  const rows = data ?? [];

  const [sel, setSel] = React.useState(0);
  const s = rows[sel];

  const rules: Array<[string, string]> = s
    ? [
        ['Trigger', s.trigger],
        ['Backfill', s.backfill],
        ['Concurrency', s.concurrency],
        ['On failure', s.onFailure],
      ]
    : [];

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name={s.mode === 'stream' ? 'radio-tower' : 'clock'} size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{s.pipeline}</h2>
        <StatusPill status={s.state} pulse={s.state === 'online'} label={s.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.mode.toUpperCase()} label="MODE" size="sm" />
          <StatBlock value={s.mode === 'stream' ? '0.4s' : '5m'} label={s.mode === 'stream' ? 'FUSION LAG' : 'CADENCE'} size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>TRIGGER RULES</div>
        {rules.map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 92, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      {s.note && (
        <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--warn)', marginBottom: 6 }}>RETRY 2/3</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>{s.note}</p>
        </Card>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-F-5" title="Batch and streaming pipelines with schedule/trigger management" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="schedules"
      context="DATA OPS / SCHEDULES & TRIGGERS"
      toolbar={<ConsoleToolbar placeholder="Search schedules, triggers, pipelines…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={6}>SCHEDULES</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Schedules &amp; triggers</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="38" label="SCHEDULED" size="sm" />
        <StatBlock value="4" label="STREAMING" size="sm" accent />
        <StatBlock value="1" label="RETRYING" size="sm" />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />}>New schedule</Button>
      </div>

      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
        <Tag active>all</Tag>
        <Tag>batch</Tag>
        <Tag>streaming</Tag>
        <Tag>event-driven</Tag>
        <Tag>failing</Tag>
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<ForgeSchedule>
          columns={[
            { header: 'Pipeline', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name={r.mode === 'stream' ? 'radio-tower' : 'workflow'} size={14} color="var(--accent)" />{r.pipeline}</span> },
            { header: 'Mode', key: 'mode', mono: true, width: 90 },
            { header: 'Trigger', key: 'trigger', mono: true, width: 190 },
            { header: 'Last run', key: 'last', mono: true, width: 150 },
            { header: 'Next', key: 'next', mono: true, width: 90 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={r.state === 'online'} label={r.stateLabel} />, width: 150 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING SCHEDULES…</div>}
      </div>

      <Terminal
        title="RUN LEDGER · TODAY"
        lines={[
          '02:00  crew_roster_sync      FAIL  conn refused · retry queued',
          '13:55  flight_tracks_eu      OK    1m42s · v41 → v42 · sealed',
          '14:00  maintenance_rollup    OK    22s · triggered by v42',
          '# every run recorded with lineage',
        ]}
      />
    </ConsolePage>
  );
}
