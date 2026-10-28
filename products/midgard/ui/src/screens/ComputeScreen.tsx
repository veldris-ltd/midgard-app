import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  MeterBar,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { ComputeJob } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { SampleData } from '../app/SampleData';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Compute: the enclosure workload scheduler — typed queries, function applies,
 * and projection rebuilds dispatched to in-process or forge backends, with live
 * progress and state, sourced from `listComputeJobs()`.
 * The gateway serves this view from fixed example data, so the page is labelled
 * SAMPLE DATA, and its controls are disabled and say why. [AC-M-4] */
const STATE_PILL: Record<ComputeJob['state'], { status: 'live' | 'offline' | 'secure' | 'danger'; label: string }> = {
  running: { status: 'live', label: 'Running' },
  queued: { status: 'offline', label: 'Queued' },
  done: { status: 'secure', label: 'Done' },
  failed: { status: 'danger', label: 'Failed' },
};

const STATE_ICON: Record<ComputeJob['state'], string> = {
  running: 'cpu',
  queued: 'clock',
  done: 'check',
  failed: 'x',
};

export function ComputeScreen() {
  const { data, loading } = useAsync((c) => c.listComputeJobs(), []);
  const rows: ComputeJob[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const j = rows[sel];

  const running = rows.filter((r) => r.state === 'running').length;
  const queued = rows.filter((r) => r.state === 'queued').length;
  const failed = rows.filter((r) => r.state === 'failed').length;

  const aside = j ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name={STATE_ICON[j.state]} size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{j.id}</h2>
        <StatusPill status={STATE_PILL[j.state].status} pulse={false} label={STATE_PILL[j.state].label} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={j.progress + '%'} label="PROGRESS" size="sm" accent={j.state === 'running'} />
          <StatBlock value={j.backend} label="BACKEND" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PLACEMENT</div>
        {([['Kind', j.kind], ['Backend', j.backend], ['Isolation', 'enclave · no host net'], ['Egress', 'none · enclosure only'], ['Submitted', j.submitted]] as [string, string][]).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 84, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      <Terminal title="SCHEDULER" lines={['> midgard compute describe ' + j.id, 'anti-affinity  zone-spread', 'restart        health-gated', '# self-healing: reroute on fault']} style={{ marginTop: 'auto' }} />
    </>
  ) : null;

  return (
    <ConsolePage
      active="compute"
      context="ENCLOSURE / COMPUTE"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search jobs, backends, kinds…" />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={10}>COMPUTE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>Jobs <SampleData /></h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={running} label="RUNNING" size="sm" accent />
        <StatBlock value={queued} label="QUEUED" size="sm" />
        <StatBlock value={failed} label="FAILED" size="sm" />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />} disabled title="Not available yet: jobs are scheduled by the compute service, not from here">Schedule job</Button>
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<ComputeJob>
          columns={[
            { header: 'Job', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name={STATE_ICON[r.state]} size={14} color="var(--accent)" />{r.id}</span> },
            { header: 'Kind', key: 'kind', width: 160 },
            { header: 'Backend', key: 'backend', mono: true, width: 130 },
            { header: 'Progress', cell: (r) => <MeterBar value={r.progress} width={90} />, width: 170 },
            { header: 'Submitted', key: 'submitted', mono: true, width: 110 },
            { header: 'State', cell: (r) => <StatusPill status={STATE_PILL[r.state].status} pulse={false} label={STATE_PILL[r.state].label} />, width: 130 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 760 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>DISPATCHING SCHEDULER…</div>}
      </div>
    </ConsolePage>
  );
}
