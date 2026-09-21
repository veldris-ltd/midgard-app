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
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { BifrostBundle, BifrostGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Offline bundles — signed media cut for air-gapped sites: cut, export,
 * courier, verify offline, apply, receipt. `[FR-B-5]` */

const PIPELINE: Array<[string, string]> = [
  ['package', 'Cut & sign'],
  ['download', 'Export media'],
  ['arrow-right', 'Courier'],
  ['shield-check', 'Verify offline'],
  ['refresh-cw', 'Apply · rolling'],
  ['upload', 'Receipt returns'],
];

export function BundlesScreen() {
  const { data, loading } = useAsync<BifrostBundle[], BifrostGateway>((c) => c.listBundles(), []);
  const rows: BifrostBundle[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const b = rows[sel];

  const awaiting = rows.filter((r) => r.state === 'gated').length;

  const aside = b ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="download" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{b.id}</h2>
        <StatusPill status={b.state} pulse={false} label={b.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={b.release} label="RELEASE" size="sm" />
          <StatBlock value={b.size} label="SIZE" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>CONTENTS</div>
        {([
          ['Platform artefacts', '14 images · signed'],
          ['Model weights', 'GLM 5.2.1 · digest-pinned'],
          ['SBOMs', 'CycloneDX · every artefact'],
          ['Rollback set', 'previous version included'],
        ] as const).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-faint)', width: 110, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--text-body)' }}>{v}</span>
          </div>
        ))}
      </div>
      <Terminal
        title="OFFLINE VERIFICATION · AT SITE"
        lines={[
          `> bifrost bundle verify ${b.id}`,
          'manifest   ML-DSA verified',
          'artefacts  14/14 digests OK',
          'weights    sha3 match · sig OK',
          '# apply permitted · rollback staged',
        ]}
      />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="DD-7" title="Sovereignty: on-prem, self-hosted, air-gapped by design" />
        <ProvenanceTag id="FR-B-5" title="Signed offline bundles for air-gapped sites" />
        <ProvenanceTag id="AC-M-5" title="Full air-gap acceptance criterion" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="bundles"
      context="BIFROST / OFFLINE BUNDLES"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search bundles, sites…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={4}>AIR-GAP</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Offline bundles</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="9" label="AIR-GAPPED SITES" size="sm" />
        <StatBlock value="100%" label="RECEIPTS RETURNED" size="sm" accent />
        <StatBlock value={String(awaiting)} label="AWAITING COURIER" size="sm" />
        <Button variant="primary" size="sm" iconLeft={<Icon name="package" size={14} />}>Cut new bundle</Button>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: 18, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflowX: 'auto' }}>
        {PIPELINE.map(([ic, lb], i) => (
          <React.Fragment key={lb}>
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, padding: '8px 12px', borderRadius: 6, background: 'var(--surface-card)', border: '1px solid ' + (i <= 1 && b?.state === 'gated' ? 'var(--accent)' : 'var(--border)'), whiteSpace: 'nowrap' }}>
              <Icon name={ic} size={13} color="var(--accent)" />
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--grey-100)' }}>{lb}</span>
            </span>
            {i < PIPELINE.length - 1 && <Icon name="chevron-right" size={13} color="var(--text-faint)" />}
          </React.Fragment>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<BifrostBundle>
          columns={[
            { header: 'Bundle', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--accent-hover)' }}>{r.id}</span>, width: 120 },
            { header: 'Release', key: 'release', mono: true, width: 90 },
            { header: 'Target sites', key: 'sites', mono: true, width: 160 },
            { header: 'Size', key: 'size', mono: true, align: 'right', width: 80 },
            { header: 'Cut', key: 'cut', mono: true, width: 110 },
            { header: 'Status', key: 'status' },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 110 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING BUNDLES…</div>}
      </div>
    </ConsolePage>
  );
}
