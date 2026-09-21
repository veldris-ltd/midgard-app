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
import { useAsync } from '@midgard/ui-shared';
import type { BifrostFleetNode, BifrostGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Fleet & drift — desired vs actual state across every installation, with
 * least-privilege signed on-host agents and reconciliation. `[FR-B-3, NFR-SEC-1]` */

export function FleetScreen() {
  const { data, loading } = useAsync<BifrostFleetNode[], BifrostGateway>((c) => c.listFleet(), []);
  const rows: BifrostFleetNode[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const f = rows[sel];

  const converged = rows.filter((x) => x.drift === 'none').length;
  const versionDrift = rows.filter((x) => x.drift.startsWith('version') || x.drift.includes('bundle')).length;
  const configDrift = rows.filter((x) => x.drift.startsWith('config')).length;

  const aside = f ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="server" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{f.id}</h2>
        <StatusPill status={f.state} pulse={false} label={f.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={f.desired} label="DESIRED" size="sm" />
          <StatBlock value={f.actual} label="ACTUAL" size="sm" accent={f.drift !== 'none'} />
        </div>
      </Card>
      {f.drift.startsWith('config') && (
        <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--warn)', marginBottom: 6 }}>CONFIG DRIFT</div>
          <Terminal lines={['- fusion.batch_window: 200ms', '+ fusion.batch_window: 350ms  # local edit']} />
          <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
            <Button variant="primary" size="sm" fullWidth>Revert to desired</Button>
            <Button variant="ghost" size="sm">Adopt</Button>
          </div>
        </Card>
      )}
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>ON-HOST AGENT</div>
        {([
          ['Identity', f.agent === 'offline' ? 'signed bundle only' : 'workload · ML-DSA'],
          ['Instructions', 'signed · least-privilege'],
          ['Heartbeat', f.agent === 'offline' ? 'courier cycle' : '10s · healthy'],
        ] as const).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 100, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-B-3" title="Declarative release channels and fleet/installation management" />
        <ProvenanceTag id="NFR-SEC-1" title="Least-privilege on-host agents, signed instructions" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="fleet"
      context="BIFROST / FLEET"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search installations, rings…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={6}>FLEET</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Fleet &amp; drift</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="INSTALLATIONS" size="sm" />
        <StatBlock value={String(converged)} label="CONVERGED" size="sm" accent />
        <StatBlock value={String(rows.length - converged)} label="DRIFTED / HELD" size="sm" />
        <Button variant="primary" size="sm" iconLeft={<Icon name="refresh-cw" size={14} />}>Reconcile all</Button>
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {([
          ['CONVERGED', converged, 'var(--secure)'],
          ['VERSION DRIFT', versionDrift, 'var(--warn)'],
          ['CONFIG DRIFT', configDrift, 'var(--warn)'],
        ] as const).map(([t, v, col]) => (
          <Card key={t} size="sm" style={{ flex: 1, minWidth: 180 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ width: 8, height: 8, borderRadius: '50%', background: col }} />
              <span style={{ fontFamily: 'var(--font-display)', fontWeight: 800, fontSize: 22, color: 'var(--text-heading)' }}>{v}</span>
              <span className="mg-label" style={{ color: 'var(--text-faint)' }}>{t}</span>
            </div>
          </Card>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<BifrostFleetNode>
          columns={[
            { header: 'Installation', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="server" size={14} color="var(--accent)" />{r.id}</span> },
            { header: 'Ring', key: 'ring', mono: true, width: 100 },
            { header: 'Desired', key: 'desired', mono: true, width: 90 },
            { header: 'Actual', key: 'actual', mono: true, width: 100 },
            { header: 'Drift', key: 'drift', mono: true, width: 140 },
            { header: 'Agent', cell: (r) => <Badge tone={r.agent === 'offline' ? 'neutral' : 'secure'}>{r.agent}</Badge>, width: 110 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 160 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 860 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RECONCILING FLEET…</div>}
      </div>
    </ConsolePage>
  );
}
