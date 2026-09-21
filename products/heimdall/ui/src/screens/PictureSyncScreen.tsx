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
import type { HeimdallGateway, HeimdallSyncSite } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

const roleIcon = (role: HeimdallSyncSite['role']): string =>
  role === 'air-gap' ? 'download' : role === 'primary' ? 'eye' : 'radar';

export function PictureSyncScreen() {
  const { data, loading } = useAsync<HeimdallSyncSite[], HeimdallGateway>((c) => c.listSyncSites(), []);
  const rows: HeimdallSyncSite[] = data ?? [];
  const [sel, setSel] = React.useState(2);
  const idx = Math.min(sel, rows.length - 1);
  const s = rows[idx];

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="network" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{s.name}</h2>
        <StatusPill status={s.status} pulse={s.status === 'online'} label={s.statusLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.entities} label="ENTITIES VISIBLE" size="sm" />
          <StatBlock value={s.lag} label="REPLICATION LAG" size="sm" />
        </div>
      </Card>
      {s.markings === 'RESTRICTED' && s.role === 'replica' && (
        <Card size="sm" style={{ borderColor: 'var(--accent)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--accent-hover)', marginBottom: 6 }}>MARKING FILTER ACTIVE</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>216 CONFIDENTIAL // OPS-N entities are withheld from this site: its operators hold RESTRICTED clearance. The filter is evaluated per object, per replication cycle.</p>
        </Card>
      )}
      <Terminal
        title="REPLICATION"
        lines={[
          '> heimdall sync status ' + s.name.split(' ')[0].toLowerCase(),
          'transport  PQ-mTLS · X25519+ML-KEM-768',
          'mode       ' + (s.role === 'air-gap' ? 'signed snapshot · courier' : 'delta stream · CRDT merge'),
          'markings   filtered per site clearance',
          '# one picture, per-clearance views',
        ]}
      />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-6" title="Synchronise the common picture across nodes/sites over PQ-mTLS" />
        <ProvenanceTag id="NFR-SEC-2" title="Marking-based access enforced at the object layer" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="sync"
      context="PICTURE / SYNC · 4 SITES"
      toolbar={<Button variant="secondary" size="sm" iconLeft={<Icon name="download" size={14} />}>Cut picture snapshot</Button>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={6}>SYNC</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Picture sync</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="4" label="SITES" size="sm" />
        <StatBlock value="0.8s" label="WORST LIVE LAG" size="sm" accent />
        <StatBlock value="216" label="MARKING-FILTERED OBJECTS" size="sm" />
      </div>

      <div className="mg-grid-bg" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 26, padding: 26, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', flexWrap: 'wrap' }}>
        {rows.map((site, i) => (
          <React.Fragment key={site.name}>
            <button onClick={() => setSel(i)} style={{ all: 'unset', cursor: 'pointer', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 }}>
              <span
                style={{
                  width: 54, height: 54, borderRadius: 12, display: 'flex', alignItems: 'center', justifyContent: 'center',
                  background: i === idx ? 'var(--accent-tint)' : 'var(--surface-card)',
                  border: '1px solid ' + (i === idx ? 'var(--accent)' : 'var(--border)'),
                  boxShadow: i === idx ? 'var(--shadow-glow)' : 'none',
                }}
              >
                <Icon name={roleIcon(site.role)} size={20} color={site.status === 'offline' ? 'var(--text-faint)' : 'var(--accent)'} />
              </span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.08em', color: i === idx ? 'var(--grey-100)' : 'var(--text-faint)' }}>{site.name.split(' ')[0]}</span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, color: 'var(--text-faint)' }}>{site.lag}</span>
            </button>
            {i < rows.length - 1 && (
              <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 3 }}>
                <span style={{ width: 44, height: 2, background: rows[i + 1].status === 'offline' ? 'var(--border-strong)' : 'var(--accent)', opacity: rows[i + 1].status === 'offline' ? 1 : 0.6, borderRadius: 1 }} />
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 7.5, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>{rows[i + 1].status === 'offline' ? 'COURIER' : 'PQ-mTLS'}</span>
              </span>
            )}
          </React.Fragment>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<HeimdallSyncSite>
          columns={[
            { header: 'Site', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="radar" size={14} color="var(--accent)" />{r.name}</span> },
            { header: 'Role', key: 'role', mono: true, width: 90 },
            { header: 'Lag', key: 'lag', mono: true, width: 80 },
            { header: 'Entities', key: 'entities', mono: true, align: 'right', width: 90 },
            { header: 'Max marking', key: 'markings', mono: true, width: 130 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={r.status === 'online'} label={r.statusLabel} />, width: 140 },
          ]}
          rows={rows}
          selectedIndex={idx}
          onRowClick={setSel}
          style={{ minWidth: 720 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RESOLVING SITES…</div>}
      </div>
    </ConsolePage>
  );
}
