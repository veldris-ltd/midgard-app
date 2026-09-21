import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  Select,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type {
  BifrostFleetNode,
  BifrostGateway,
  BifrostHealthGate,
  BifrostRelease,
  BifrostRing,
  BifrostRingStage,
} from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Flagship screen — the zero-downtime rollout console. The Promote button
 * advances the broad (PROD-EU) ring canary → broad one health-gated step at a
 * time via `promoteRelease`, animating a per-ring MeterBar and a health-gate
 * StatusPill and surfacing the signed audit id of each step. `[FR-B-2]` */

const GATE_PILL: Record<BifrostHealthGate, { status: React.ComponentProps<typeof StatusPill>['status']; label: string }> = {
  pass: { status: 'healthy', label: 'GATE PASS' },
  watch: { status: 'warn', label: 'GATE WATCH' },
  fail: { status: 'danger', label: 'GATE FAIL' },
};

const PROMOTE_STAGE: BifrostRingStage = 'broad';

export function ReleaseConsoleScreen() {
  const g = useGateway<BifrostGateway>();
  const { data: releases } = useAsync<BifrostRelease[], BifrostGateway>((c) => c.listReleases(), []);
  const { data: ringData, loading } = useAsync<BifrostRing[], BifrostGateway>((c) => c.listRings(), []);
  const { data: fleet } = useAsync<BifrostFleetNode[], BifrostGateway>((c) => c.listFleet(), []);

  const [rings, setRings] = React.useState<BifrostRing[]>([]);
  React.useEffect(() => {
    if (ringData) setRings(ringData);
  }, [ringData]);

  const release = releases?.[0];
  const [gate, setGate] = React.useState<BifrostHealthGate>('watch');
  React.useEffect(() => {
    if (release) setGate(release.healthGate);
  }, [release]);

  const [promoting, setPromoting] = React.useState(false);
  const [lastAudit, setLastAudit] = React.useState<string | null>(null);

  const promote = async () => {
    if (promoting) return;
    setPromoting(true);
    try {
      // Staged promotion: each step advances the ring under its health gate and
      // emits a signed audit id, until the ring is fully rolled out.
      // eslint-disable-next-line no-constant-condition
      while (true) {
        const res = await g.promoteRelease(release?.id ?? 'REL-2050', PROMOTE_STAGE);
        setGate(res.healthGate);
        setLastAudit(res.auditId);
        setRings((rs) => rs.map((r) => (r.stage === PROMOTE_STAGE ? { ...r, done: Math.round((res.progress / 100) * r.total) } : r)));
        if (res.progress >= 100 || res.healthGate === 'fail') break;
      }
    } finally {
      setPromoting(false);
    }
  };

  const rolled = rings.reduce((n, r) => n + r.done, 0);
  const capacity = rings.reduce((n, r) => n + r.total, 0);
  const fleetRows: BifrostFleetNode[] = fleet ?? [];

  const aside = (
    <>
      <SectionLabel number={3} muted>RELEASE INTELLIGENCE</SectionLabel>
      <Card size="sm" accentRule>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10 }}>
          <StatBlock value={release?.riskScore ?? 'LOW'} label="RISK SCORE · MIMIR" size="sm" accent />
          <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)' }}>LOCAL · Mistral Small 3.2</span>
        </div>
        <p style={{ margin: '10px 0 0', fontSize: 'var(--fs-xs)', lineHeight: 1.6, color: 'var(--text-body)' }}>
          Diff surface is 84% config, 16% code; no schema migrations. Historical anomaly match: none. Advisory by default; configurable to gate the rollout. Rule-based health gates always apply, with or without Mimir.
        </p>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>HEALTH GATES</div>
        {([
          ['p99 latency < 250ms', 'secure'],
          ['error rate < 0.1%', 'secure'],
          ['enclave-ams-03 CPU', gate === 'pass' ? 'secure' : 'warn'],
        ] as const).map(([label, s], i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-body)' }}>
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: s === 'secure' ? 'var(--secure)' : 'var(--warn)', flex: 'none' }} />
            {label}
          </div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="FR-B-6" title="AI release intelligence: risk scoring, anomaly detection via Mimir" />
          <ProvenanceTag id="FR-B-2" title="Zero-downtime rolling upgrades, health-gated rollback" />
          <ProvenanceTag id="NFR-PQ-3" title="ML-DSA / SLH-DSA artefact signing" />
          <ProvenanceTag id="FR-B-5" title="Signed offline bundles for air-gapped sites" />
        </div>
      </div>
      {lastAudit && (
        <div className="mg-label" style={{ color: 'var(--text-faint)' }}>
          LAST STEP · AUDIT <span style={{ color: 'var(--accent-hover)' }}>{lastAudit}</span>
        </div>
      )}
      <Button variant="secondary" size="sm" style={{ marginTop: 'auto' }} iconLeft={<Icon name="undo-2" size={14} />}>Prepare rollback plan</Button>
    </>
  );

  return (
    <ConsolePage
      active="releases"
      context="BIFROST / RELEASES"
      asideWidth={340}
      toolbar={
        <>
          <Badge tone="accent">v{release?.version ?? '2.5.0'}</Badge>
          <Select
            size="sm"
            options={['stable', 'beta', 'canary']}
            value={release?.channel ?? 'stable'}
            onChange={() => {}}
          />
          <StatusPill status={GATE_PILL[gate].status} pulse={promoting} label={GATE_PILL[gate].label} />
          <Button variant="primary" size="sm" iconLeft={<Icon name="rocket" size={14} />} onClick={promote} disabled={promoting}>
            {promoting ? 'Promoting…' : 'Promote to Ring 2'}
          </Button>
        </>
      }
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>ROLLOUT · v{release?.version ?? '2.5.0'}</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Zero-downtime rollout</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={`${rolled}/${capacity}`} label="INSTALLATIONS" size="sm" />
        <StatBlock value={String(release?.failedRequests ?? 0)} label="FAILED REQUESTS" size="sm" accent />
        <StatBlock value={release?.rollbackGate ?? 'AUTO'} label="ROLLBACK GATE" size="sm" />
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {rings.map((r) => {
          const pct = Math.round((r.done / r.total) * 100);
          return (
            <Card key={r.id} size="sm" style={{ flex: 1, minWidth: 190 }} accentRule={r.stage === PROMOTE_STAGE && promoting}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.1em', color: 'var(--text-muted)' }}>{r.name}</span>
                {r.offlineBundle && <Badge>OFFLINE BUNDLE</Badge>}
                <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 11, color: pct === 100 ? 'var(--secure)' : 'var(--accent-hover)' }}>{r.done}/{r.total}</span>
              </div>
              <div style={{ marginTop: 12 }}>
                <MeterBar
                  value={pct}
                  width={140}
                  format={(v) => `${v}%`}
                  warnAt={r.stage === PROMOTE_STAGE && promoting && pct < 100 ? 0 : null}
                  style={{ display: 'flex' }}
                />
              </div>
            </Card>
          );
        })}
      </div>

      <div>
        <SectionLabel number={2} muted>FLEET</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<BifrostFleetNode>
            columns={[
              { header: 'Installation', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="server" size={14} color="var(--accent)" />{r.id}</span> },
              { header: 'Ring', key: 'ring', mono: true, width: 110 },
              { header: 'Version', key: 'actual', mono: true, width: 110 },
              { header: 'Target', key: 'desired', mono: true, width: 90 },
              { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 160 },
            ]}
            rows={fleetRows}
            style={{ minWidth: 640 }}
          />
        </div>
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RECONCILING RINGS…</div>}
      </div>

      <Terminal
        title={`SUPPLY CHAIN · v${release?.version ?? '2.5.0'}`}
        lines={[
          `> bifrost verify ${release?.name ?? 'midgard-suite'}:${release?.version ?? '2.5.0'}`,
          'build      reproducible ✓ (bit-identical 2/2)',
          'sbom       CycloneDX attached · 0 critical CVEs',
          'signature  ML-DSA verified · key enclosure-root-03',
          '# verified, promotion permitted [FR-B-4]',
        ]}
      />
    </ConsolePage>
  );
}
