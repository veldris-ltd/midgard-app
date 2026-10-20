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
import type { HeimdallFeed, HeimdallGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

export function FeedsScreen() {
  const { data, loading } = useAsync<HeimdallFeed[], HeimdallGateway>((c) => c.listFeeds(), []);
  const rows: HeimdallFeed[] = data ?? [];
  const [sel, setSel] = React.useState(5);
  const f = rows[Math.min(sel, rows.length - 1)];

  const aside = f ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="radio-tower" size={17} color={f.status === 'danger' ? 'var(--danger)' : 'var(--accent)'} />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 15, flex: 1 }}>{f.name}</h2>
        <StatusPill status={f.status} pulse={f.status === 'online'} label={f.statusLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={f.latency} label="LATENCY" size="sm" />
          <StatBlock value={f.weight.toUpperCase()} label="FUSION WEIGHT" size="sm" />
        </div>
      </Card>
      {f.status === 'danger' ? (
        <Card size="sm" style={{ borderColor: 'var(--danger)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--danger)', marginBottom: 6 }}>EXCLUDED FROM FUSION</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>PQ signature check failed on the bridge relay at 13:58. The feed is quarantined; the picture runs without it. Nothing unauthenticated reaches fusion.</p>
          <Button variant="outline" size="sm" style={{ marginTop: 10 }}>Re-key bridge</Button>
        </Card>
      ) : (
        <Terminal
          title="AUTHENTICATION"
          lines={['> heimdall feed verify ' + f.name, 'transport  PQ-mTLS OK', 'payload    ' + f.scheme + ' per frame OK', '# admitted to fusion']}
        />
      )}
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>CONTRIBUTION</div>
        {([
          ['Entities fed', f.weight === 'high' ? '412' : f.weight === 'medium' ? '188' : '9'],
          ['Unique coverage', f.name === 'SAT-IR-7' ? '31%' : '12%'],
          ['Marking', 'CONFIDENTIAL // OPS-N'],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 110, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-1" title="Fuse multiple live feeds into one common picture" />
        <ProvenanceTag id="FR-H-3" title="Authenticate feed sources with PQ signatures; reject unverified feeds" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="feeds"
      context="PICTURE / FEED MANAGER"
      toolbar={<Button variant="primary" size="sm" iconLeft={<Icon name="plus" size={14} />}>Register feed</Button>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>FEEDS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Feed manager</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="FEEDS" size="sm" />
        <StatBlock value={String(rows.filter((r) => r.status === 'online').length)} label="IN FUSION" size="sm" />
        <StatBlock value={String(rows.filter((r) => r.status !== 'online').length)} label="STALE" size="sm" accent />
      </div>
      <div style={{ overflowX: 'auto' }}>
        <Table<HeimdallFeed>
          columns={[
            { header: 'Feed', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="radio-tower" size={14} color={r.status === 'danger' ? 'var(--danger)' : 'var(--accent)'} />{r.name}</span> },
            { header: 'Kind', key: 'kind', width: 140 },
            { header: 'Rate', key: 'rate', mono: true, width: 80 },
            { header: 'Latency', key: 'latency', mono: true, align: 'right', width: 80 },
            { header: 'Weight', key: 'weight', mono: true, width: 80 },
            { header: 'PQ auth', cell: (r) => <Badge tone={r.auth ? 'secure' : 'danger'}>{r.auth ? `${r.scheme} ✓` : 'FAILED'}</Badge>, width: 120 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={r.status === 'online'} label={r.statusLabel} />, width: 130 },
          ]}
          rows={rows}
          selectedIndex={Math.min(sel, rows.length - 1)}
          onRowClick={setSel}
          style={{ minWidth: 760 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>VERIFYING FEEDS…</div>}
      </div>
    </ConsolePage>
  );
}
