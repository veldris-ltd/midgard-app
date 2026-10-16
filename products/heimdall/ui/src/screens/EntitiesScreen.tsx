import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Button,
  Card,
  Icon,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { HeimdallEntity, HeimdallGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

function Conf({ v }: { v: number }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
      <span style={{ width: 60, height: 5, background: 'var(--surface-inset)', borderRadius: 3, overflow: 'hidden', display: 'inline-block' }}>
        <span style={{ display: 'block', height: '100%', width: v + '%', background: v < 70 ? 'var(--warn)' : 'var(--secure)', borderRadius: 3 }} />
      </span>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>{v}%</span>
    </span>
  );
}

const CLASS_FILTER: Record<string, string> = { air: 'AIRCRAFT', maritime: 'VESSEL', ground: 'GROUND UNIT' };
const FILTERS = ['all', 'air', 'maritime', 'ground', 'anomalous', 'low confidence'];

export function EntitiesScreen() {
  const { data, loading } = useAsync<HeimdallEntity[], HeimdallGateway>((c) => c.listEntities(), []);
  const all: HeimdallEntity[] = data ?? [];
  const navigate = useNavigate();

  const [filter, setFilter] = React.useState('all');
  const [sel, setSel] = React.useState(0);

  const rows = all.filter((e) => {
    if (filter === 'all') return true;
    if (filter === 'anomalous') return e.track === 'anomalous';
    if (filter === 'low confidence') return e.conf < 70;
    return e.cls === CLASS_FILTER[filter];
  });
  const e = rows[Math.min(sel, rows.length - 1)];

  const aside = e ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="crosshair" size={17} color={e.track === 'anomalous' ? 'var(--warn)' : 'var(--accent)'} />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 15, flex: 1 }}>{e.id}</h2>
        <MarkingChip level={e.marking} compartment={e.compartment ?? undefined} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={e.conf + '%'} label="CLASSIFICATION CONF." size="sm" accent={e.conf < 70} />
          <StatBlock value={String(e.feeds)} label="CORROBORATING FEEDS" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>CLASSIFIER</div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)', paddingBottom: 10 }}>
          rules + mimir · provenance logged
          <br />
          <span style={{ color: 'var(--text-faint)', fontSize: 10 }}>rules-only when Mimir is absent (degraded mode)</span>
        </div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>TRACK HISTORY</div>
        {([
          [e.firstSeen, 'first observed · ' + (e.feeds > 2 ? 'NORTH-2 + ADS-B' : 'NORTH-2')],
          ['13:58', e.track === 'anomalous' ? 'transponder ceased · reclassified UNKNOWN' : 'course steady'],
          ['14:31', e.track === 'anomalous' ? 'descent through FL180 · flagged' : 'position nominal'],
        ] as [string, string][]).map(([t, ev], i) => (
          <div key={i} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-faint)', width: 44, flex: 'none' }}>{t}</span>
            <span style={{ color: 'var(--text-body)', lineHeight: 1.5 }}>{ev}</span>
          </div>
        ))}
      </div>
      {e.track === 'anomalous' && (
        <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--warn)', marginBottom: 6 }}>OPTIONS AVAILABLE</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>Mimir has 3 ranked options for improving classification. Review on the operating picture.</p>
          <Button variant="primary" size="sm" style={{ marginTop: 10 }} iconRight={<Icon name="arrow-right" size={13} />} onClick={() => navigate('/')}>Open picture</Button>
        </Card>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-2" title="Track and classify entities over time against the object fabric" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="entities"
      context="PICTURE / ENTITY BROWSER"
      toolbar={<ConsoleToolbar placeholder="Search entities…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>ENTITIES</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Entity browser</h1>
        </div>
        <div style={{ flex: 1 }} />
        {/* Counted from the picture the gateway serves, not fixed figures. [FR-H-1] [FR-H-2] */}
        <StatBlock value={all.length.toLocaleString()} label="TRACKED" size="sm" />
        <StatBlock
          value={all.length ? `${(all.reduce((s, x) => s + x.conf, 0) / all.length).toFixed(1)}%` : '—'}
          label="MEAN CONFIDENCE"
          size="sm"
        />
        <StatBlock value={String(all.filter((x) => x.track === 'anomalous').length)} label="ANOMALOUS" size="sm" accent />
      </div>
      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
        {FILTERS.map((t) => (
          <Tag key={t} active={filter === t} onClick={() => { setFilter(t); setSel(0); }}>{t}</Tag>
        ))}
      </div>
      <div style={{ overflowX: 'auto' }}>
        <Table<HeimdallEntity>
          columns={[
            { header: 'Entity', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: r.track === 'anomalous' ? 'var(--warn)' : 'var(--accent-hover)' }}>{r.id}{r.track === 'anomalous' ? ' ⚠' : ''}</span>, width: 100 },
            { header: 'Class', key: 'cls', mono: true, width: 120 },
            { header: 'Confidence', cell: (r) => <Conf v={r.conf} />, width: 140 },
            { header: 'Feeds', key: 'feeds', mono: true, align: 'right', width: 60 },
            { header: 'First seen', key: 'firstSeen', mono: true, width: 90 },
            { header: 'Marking', cell: (r) => <MarkingChip level={r.marking} />, width: 150 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={false} label={r.statusLabel} />, width: 120 },
          ]}
          rows={rows}
          selectedIndex={Math.min(sel, rows.length - 1)}
          onRowClick={setSel}
          style={{ minWidth: 780 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RESOLVING ENTITIES…</div>}
      </div>
    </ConsolePage>
  );
}
