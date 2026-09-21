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
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { BifrostChannel, BifrostGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Release channels — cadence, subscribers, promotion policy. Declarative
 * channels drive fleet convergence. `[FR-B-3]` */

interface Promotion {
  release: string;
  path: string;
  ok: boolean;
  when: string;
}

const HISTORY: Promotion[] = [
  { release: '2.5.0', path: 'beta → stable · staged', ok: true, when: 'in progress' },
  { release: '2.4.1', path: 'beta → stable', ok: true, when: '32d ago' },
  { release: '2.4.0', path: 'beta → stable', ok: false, when: '61d ago' },
];

export function ChannelsScreen() {
  const { data, loading } = useAsync<BifrostChannel[], BifrostGateway>((c) => c.listChannels(), []);
  const rows: BifrostChannel[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const c = rows[sel];

  const totalSubs = rows.reduce((n, x) => n + x.subscribers, 0);

  const aside = c ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="git-branch" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 15, flex: 1 }}>{c.name}</h2>
        <StatusPill status={c.state} pulse={c.state === 'online'} label={c.stateLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={c.current} label="CURRENT" size="sm" />
          <StatBlock value={String(c.subscribers)} label="SUBSCRIBERS" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROMOTION POLICY</div>
        {([
          ['Verification', 'reproducible + SBOM + ML-DSA'],
          ['Rings', 'canary → staging → prod'],
          ['Gates', c.policy],
          ['Rollback', 'automatic on health failure'],
        ] as const).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 90, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>SUBSCRIBER MIX</div>
        {([
          ['Connected enclaves', c.name === 'stable' ? 52 : Math.max(0, c.subscribers - 1)],
          ['Air-gapped sites', c.name === 'stable' ? 9 : 1],
        ] as const).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '6px 0', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <Icon name={k.includes('gap') ? 'download' : 'server'} size={13} color="var(--accent)" />{k}
            <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)' }}>{v}</span>
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        <ProvenanceTag id="FR-B-3" title="Declarative release channels and fleet/installation management" />
      </div>
      <Button variant="secondary" size="sm" style={{ marginTop: 'auto' }} iconLeft={<Icon name="rocket" size={14} />}>Open release console</Button>
    </>
  ) : null;

  return (
    <ConsolePage
      active="channels"
      context="BIFROST / CHANNELS"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search channels, subscribers…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>CHANNELS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Release channels</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="CHANNELS" size="sm" />
        <StatBlock value={String(totalSubs)} label="TOTAL SUBSCRIBERS" size="sm" />
        <StatBlock value="7d" label="STABLE SOAK" size="sm" accent />
        <Button variant="primary" size="sm" iconLeft={<Icon name="plus" size={14} />}>New channel</Button>
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {rows.map((ch, i) => (
          <Card key={ch.name} size="md" interactive accentRule={i === sel} onClick={() => setSel(i)} style={{ flex: 1, minWidth: 220, borderColor: i === sel ? 'var(--accent)' : undefined }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 14, fontWeight: 600, color: 'var(--text-heading)' }}>{ch.name}</span>
              <Badge tone={i === 0 ? 'accent' : 'neutral'}>{ch.cadence.toUpperCase()}</Badge>
              <span style={{ marginLeft: 'auto' }}><StatusPill status={ch.state} pulse={false} label={ch.stateLabel} /></span>
            </div>
            <div style={{ display: 'flex', gap: 18, marginTop: 14 }}>
              <StatBlock value={ch.current} label="CURRENT" size="sm" />
              <StatBlock value={String(ch.subscribers)} label="SUBS" size="sm" />
            </div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', marginTop: 12 }}>NEXT · {ch.next.toUpperCase()}</div>
          </Card>
        ))}
      </div>

      <div>
        <SectionLabel number={3} muted>PROMOTION HISTORY</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<Promotion>
            compact
            columns={[
              { header: 'Release', key: 'release', mono: true, width: 110 },
              { header: 'Path', key: 'path', mono: true },
              { header: 'Verification', cell: () => <Badge tone="secure">ML-DSA ✓</Badge>, width: 120 },
              { header: 'Outcome', cell: (r) => <Badge tone={r.ok ? 'secure' : 'danger'}>{r.ok ? 'COMPLETED' : 'ROLLED BACK'}</Badge>, width: 130 },
              { header: 'When', key: 'when', mono: true, width: 110 },
            ]}
            rows={HISTORY}
            style={{ minWidth: 620 }}
          />
        </div>
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING CHANNELS…</div>}
      </div>
    </ConsolePage>
  );
}
