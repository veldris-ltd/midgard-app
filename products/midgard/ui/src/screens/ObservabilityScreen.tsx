import * as React from 'react';
import {
  Card,
  Icon,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { SloMetric } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { SampleData } from '../app/SampleData';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/** Parse the leading numeric out of an SLO readout ("99.97%", "100%") for the meter. */
function pct(value: string): number | null {
  if (!value.includes('%')) return null;
  const n = parseFloat(value);
  return Number.isFinite(n) ? n : null;
}

/* Observability · SLOs and traces. The gateway serves this view from fixed example
 * data until telemetry is wired to it, so the page is labelled SAMPLE DATA. [AC-M-4] */
export function ObservabilityScreen() {
  const { data, loading } = useAsync((c) => c.getObservability(), []);
  const metrics: SloMetric[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const s = metrics[sel];

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="activity" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1 }}>{s.label}</h2>
        <StatusPill status={s.healthy ? 'healthy' : 'warn'} pulse={false} label={s.healthy ? 'Met' : 'Degraded'} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.value} label={`TARGET ${s.target}`} size="sm" accent={s.healthy} />
          <StatBlock value={s.healthy ? 'MET' : 'BURN'} label="STATUS" size="sm" />
        </div>
      </Card>
      <Terminal title="TRACE · CORRELATION-TAGGED" lines={[
        '> midgard trace 8f2c-44e1',
        'gateway     2ms   identity  4ms',
        'policy      1ms   fabric   38ms',
        'audit       9ms   # every hop in-enclosure',
      ]} />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="NFR-AVA-1" title="99.9% control-plane availability target" />
        <ProvenanceTag id="NFR-PRV-1" title="No external telemetry; in-enclosure sinks only" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="obs"
      context="ENCLOSURE / OBSERVABILITY"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search SLOs, traces, sinks…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={12}>OBSERVABILITY</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>SLOs & traces <SampleData /></h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="99.9%" label="CONTROL-PLANE SLO" size="sm" accent />
        <StatBlock value={metrics.length} label="SLOS TRACKED" size="sm" />
        <StatBlock value="0" label="EXTERNAL SINKS" size="sm" />
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))', gap: 12 }}>
        {metrics.map((m, i) => {
          const meter = pct(m.value);
          return (
            <Card key={m.label} accentRule={i === sel} interactive onClick={() => setSel(i)}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
                <span className="mg-label" style={{ color: 'var(--text-faint)', flex: 1 }}>{m.label}</span>
                <StatusPill status={m.healthy ? 'healthy' : 'warn'} pulse={false} label={m.healthy ? 'Met' : 'Degraded'} />
              </div>
              <StatBlock value={m.value} label={`TARGET ${m.target}`} accent={m.healthy} />
              {meter != null && (
                <div style={{ marginTop: 12 }}>
                  <MeterBar value={meter} width={140} format={(v) => `${v}%`} />
                </div>
              )}
            </Card>
          );
        })}
      </div>
      {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>SAMPLING SLOS…</div>}

      <div>
        <SectionLabel number={13} muted>TELEMETRY · IN-ENCLOSURE SINKS</SectionLabel>
        <div style={{ display: 'flex', gap: 12, marginTop: 8, flexWrap: 'wrap' }}>
          {([
            ['LOGS', 'structured · correlation-tagged', 'in-enclosure aggregation'],
            ['METRICS', 'control plane + inference', 'SLO dashboards, burn alerts'],
            ['TRACES', 'distributed · mesh-wide', 'request + action paths'],
            ['BACKUP', 'encrypted snapshots · nightly', 'restore tested 6d ago'],
          ] as [string, string, string][]).map(([t, v, d]) => (
            <Card key={t} size="sm" style={{ flex: 1, minWidth: 200 }}>
              <div className="mg-label" style={{ color: 'var(--text-faint)' }}>{t}</div>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', margin: '8px 0 4px' }}>{v}</div>
              <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{d}</div>
            </Card>
          ))}
        </div>
      </div>
    </ConsolePage>
  );
}
