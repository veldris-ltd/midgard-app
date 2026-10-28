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
import type { KeyRecord } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { SampleData } from '../app/SampleData';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Keys & crypto: the post-quantum key hierarchy — ML-KEM-1024 envelope wrapping
 * AES-256-GCM data keys, ML-DSA signing keys — with custody, wrap state, and
 * rotation, sourced from `listKeys()`. FIPS 203/204, crypto-agile.
 * The gateway serves this view from fixed example data, so the page is labelled
 * SAMPLE DATA, and its controls are disabled and say why. [AC-M-4] */
const STATUS_PILL: Record<KeyRecord['status'], { status: 'secure' | 'live' | 'offline'; label: string }> = {
  active: { status: 'secure', label: 'Active' },
  rotating: { status: 'live', label: 'Rotating' },
  retired: { status: 'offline', label: 'Retired' },
};

const POSTURE: [string, string, string][] = [
  ['TRANSIT', 'TLS 1.3 · X25519 + ML-KEM-768', 'hybrid, fails closed'],
  ['AT REST', 'AES-256-GCM · ML-KEM-1024 wrap', 'per-dataset DEKs'],
  ['SIGNATURES', 'ML-DSA-65/87 · SLH-DSA', 'lineage, artefacts, firmware'],
  ['AGILITY', 'negotiated · versioned · hot-swap', 'no data migration · KAT-verified'],
];

export function KeysScreen() {
  const { data, loading } = useAsync((c) => c.listKeys(), []);
  const rows: KeyRecord[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const k = rows[sel];

  const total = rows.length;
  const wrappedPct = total ? Math.round((rows.filter((r) => r.wrapped).length / total) * 100) : 0;
  const rotating = rows.filter((r) => r.status === 'rotating').length;

  const aside = k ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="key" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{k.id}</h2>
        <StatusPill status={STATUS_PILL[k.status].status} pulse={false} label={STATUS_PILL[k.status].label} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={k.alg} label="ALGORITHM" size="sm" />
          <StatBlock value={k.custody.toUpperCase()} label="CUSTODY" size="sm" accent={k.custody === 'hsm'} />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>ENVELOPE · WRAP CHAIN</div>
        {['kek-root (ML-KEM-1024)', '└ ' + (k.wrapped ? k.id + ' (' + k.alg + ') · WRAPPED' : k.id + ' (' + k.alg + ') · hardware anchor')].map((l, i) => (
          <div key={i} style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: i === 0 ? 'var(--grey-100)' : 'var(--text-muted)', padding: '5px 0' }}>{l}</div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>DETAIL</div>
        {([['Purpose', k.purpose], ['Custody', k.custody], ['Wrapped', k.wrapped ? 'yes · AES-256-GCM' : 'no · root anchor'], ['Rotated', k.rotated]] as [string, string][]).map(([key, val]) => (
          <div key={key} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 78, flex: 'none' }}>{key.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{val}</span>
          </div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="NFR-PQ-2" title="Data at rest: AES-256-GCM under ML-KEM-1024-wrapped keys" />
          <ProvenanceTag id="NFR-PQ-3" title="Signatures: ML-DSA / SLH-DSA over lineage and artefacts" />
          <ProvenanceTag id="STD:NIST FIPS 203" />
        </div>
      </div>
      <Terminal title="ROTATION DRILL" lines={['> midgard kms rehearse --key ' + k.id, 'unwrap    OK · 1.2s', 'rewrap    OK · 0.9s', '# zero-downtime rotation verified']} style={{ marginTop: 'auto' }} />
    </>
  ) : null;

  return (
    <ConsolePage
      active="kms"
      context="ENCLOSURE / KEYS & CRYPTO"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search keys, algorithms, scopes…" />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={7}>KEYS & CRYPTO</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>Key hierarchy <SampleData /></h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={total} label="KEYS" size="sm" />
        <StatBlock value={wrappedPct + '%'} label="WRAPPED" size="sm" accent />
        <StatBlock value={rotating} label="ROTATING" size="sm" />
        <Button variant="primary" iconLeft={<Icon name="refresh-cw" size={15} />} disabled title="Not available yet: keys are rotated by the KMS on its schedule">Rotate now</Button>
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {POSTURE.map(([t, v, d]) => (
          <Card key={t} size="sm" style={{ flex: 1, minWidth: 180 }}>
            <div className="mg-label" style={{ color: 'var(--text-faint)' }}>{t}</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', margin: '8px 0 4px' }}>{v}</div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{d}</div>
          </Card>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<KeyRecord>
          columns={[
            { header: 'Key', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="key" size={14} color="var(--accent)" />{r.id}</span> },
            { header: 'Purpose', key: 'purpose', width: 160 },
            { header: 'Algorithm', key: 'alg', mono: true, width: 130 },
            { header: 'Custody', cell: (r) => <Badge tone={r.custody === 'hsm' ? 'secure' : 'neutral'}>{r.custody.toUpperCase()}</Badge>, width: 110 },
            { header: 'Wrapped', cell: (r) => <Badge tone={r.wrapped ? 'secure' : 'neutral'}>{r.wrapped ? 'WRAPPED' : 'ANCHOR'}</Badge>, width: 110 },
            { header: 'Rotated', key: 'rotated', mono: true, width: 110 },
            { header: 'Status', cell: (r) => <StatusPill status={STATUS_PILL[r.status].status} pulse={false} label={STATUS_PILL[r.status].label} />, width: 130 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>ENUMERATING KEY RING…</div>}
      </div>
    </ConsolePage>
  );
}
