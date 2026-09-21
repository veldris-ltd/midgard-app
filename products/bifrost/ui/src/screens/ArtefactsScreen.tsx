import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  Input,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { BifrostArtefact, BifrostGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Artefact registry — reproducible builds + CycloneDX SBOMs + ML-DSA
 * signatures, verified fail-closed before promotion. `[FR-B-4]` */

const FILTERS = ['all', 'images', 'bundles', 'weights', 'blocked'];

export function ArtefactsScreen() {
  const { data, loading } = useAsync<BifrostArtefact[], BifrostGateway>((c) => c.listArtefacts(), []);
  const rows: BifrostArtefact[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const [filter, setFilter] = React.useState(0);
  const a = rows[sel];

  const signed = rows.length ? '100%' : '—';
  const blocked = rows.filter((r) => r.state === 'danger').length;

  const aside = a ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="package" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{a.name}:{a.version}</h2>
        <StatusPill status={a.state} pulse={false} label={a.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={a.reproducible} label="REPRODUCIBLE" size="sm" accent={a.reproducible.includes('⚠')} />
          <StatBlock value={String(a.criticalCves)} label="CRITICAL CVES" size="sm" />
        </div>
      </Card>
      <Terminal
        title="VERIFICATION"
        lines={a.state === 'danger'
          ? [
              `> bifrost verify ${a.name}:${a.version}`,
              'build      NOT bit-identical (1/2)',
              'diff       embedded timestamp in layer 4',
              '# promotion BLOCKED · fail closed [FR-B-4]',
            ]
          : [
              `> bifrost verify ${a.name}:${a.version}`,
              'build      reproducible ✓',
              'sbom       CycloneDX attached',
              'signature  ML-DSA verified · artefact-sign-01',
              '# promotion permitted',
            ]}
      />
      {a.state === 'danger' && (
        <Card size="sm" style={{ borderColor: 'var(--danger)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--danger)', marginBottom: 6 }}>BLOCKED · NOTHING WAS PROMOTED</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>Rebuild determinism failed: a timestamp leaks into layer 4. Fix the build recipe; verification will clear automatically on the next registration.</p>
        </Card>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-B-4" title="Reproducible builds + SBOM + PQ signature, verified before promotion" />
        <ProvenanceTag id="NFR-PQ-3" title="ML-DSA / SLH-DSA artefact signing (NIST FIPS 204/205)" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="artefacts"
      context="BIFROST / ARTEFACTS"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search by name, digest, CVE…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>ARTEFACTS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Artefact registry</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="58" label="ARTEFACTS" size="sm" />
        <StatBlock value={signed} label="SIGNED" size="sm" accent />
        <StatBlock value={String(blocked)} label="BLOCKED" size="sm" />
        <Button variant="secondary" size="sm" iconLeft={<Icon name="upload" size={14} />}>Register artefact</Button>
      </div>

      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
        <Input size="sm" mono placeholder="Search by name, digest, CVE…" prefix={<Icon name="search" size={14} color="var(--text-faint)" />} style={{ width: 300 }} />
        {FILTERS.map((f, i) => (
          <Tag key={f} active={i === filter} onClick={() => setFilter(i)}>{f}</Tag>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<BifrostArtefact>
          columns={[
            { header: 'Artefact', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="package" size={14} color="var(--accent)" />{r.name}</span> },
            { header: 'Version', key: 'version', mono: true, width: 100 },
            { header: 'Kind', key: 'kind', width: 130 },
            { header: 'Size', key: 'size', mono: true, align: 'right', width: 90 },
            { header: 'Reproducible', key: 'reproducible', mono: true, width: 110 },
            { header: 'Crit. CVEs', key: 'criticalCves', mono: true, align: 'right', width: 90 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 150 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>VERIFYING ARTEFACTS…</div>}
      </div>
    </ConsolePage>
  );
}
