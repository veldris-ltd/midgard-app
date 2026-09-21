import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  MarkingChip,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tabs,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { ForgeDataset, ForgeGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

const DIFF_COLOR: Record<ForgeDataset['diff'][number]['tone'], string> = {
  add: 'var(--secure)',
  remove: 'var(--danger)',
  change: 'var(--warn)',
};

/* Dataset browser: versioned, branched, sealed datasets with diffs + lineage.
 * Sourced from `listDatasets()`. Reproduces forge/datasets.html. */
export function DatasetsScreen() {
  const { data, loading } = useAsync<ForgeDataset[], ForgeGateway>((c) => c.listDatasets(), []);
  const rows = data ?? [];

  const [sel, setSel] = React.useState(0);
  const [tab, setTab] = React.useState(0);
  const d = rows[sel];
  const prevVersion = d ? 'v' + (parseInt(d.version.slice(1), 10) - 1) : '';

  const aside = d ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="table" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{d.name}</h2>
        <MarkingChip level={d.marking} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={d.version} label={'BRANCH · ' + d.branch.toUpperCase()} size="sm" />
          <StatBlock value={d.rows} label="ROWS" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>VERSION DIFF · {d.version} ← {prevVersion}</div>
        {d.diff.map((line, i) => (
          <div key={i} style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: DIFF_COLOR[line.tone], padding: '5px 0', borderBottom: '1px solid var(--divider)' }}>{line.text}</div>
        ))}
      </div>
      <Terminal title="SEAL" lines={['> forge seal status ' + d.name, 'dek     fabric-dek-ring-a', 'wrap    ML-KEM-1024 · root-03', 'addr    content-hash sha3:22f1…9e04', 'sig     ML-DSA · lineage OK', '# sealed · rebuild reproducible']} />
      <div style={{ display: 'flex', gap: 8, marginTop: 'auto' }}>
        <Button variant="secondary" size="sm" fullWidth>Time travel</Button>
        <Button variant="primary" size="sm" fullWidth iconRight={<Icon name="arrow-right" size={14} />}>Open in builder</Button>
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="datasets"
      context="DATA OPS / DATASETS"
      toolbar={<ConsoleToolbar placeholder="Search datasets, branches, columns…" />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>DATASETS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Dataset browser</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="412" label="DATASETS" size="sm" />
        <StatBlock value="100%" label="SEALED AT REST" size="sm" accent />
        <StatBlock value="31" label="OPEN BRANCHES" size="sm" />
        <Button variant="secondary" iconLeft={<Icon name="git-branch" size={15} />}>New branch</Button>
      </div>

      <Tabs tabs={[{ label: 'All datasets', badge: '412' }, { label: 'My branches', badge: '3' }, { label: 'Pending merges', badge: '1' }]} active={tab} onChange={setTab} />

      <div style={{ overflowX: 'auto' }}>
        <Table<ForgeDataset>
          columns={[
            { header: 'Dataset', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="table" size={14} color="var(--accent)" />{r.name}</span> },
            { header: 'Version', key: 'version', mono: true, width: 80 },
            { header: 'Branch', key: 'branch', mono: true, width: 130 },
            { header: 'Rows', key: 'rows', mono: true, align: 'right', width: 90 },
            { header: 'Columns', key: 'columns', mono: true, align: 'right', width: 90 },
            { header: 'Marking', cell: (r) => <MarkingChip level={r.marking} />, width: 150 },
            { header: 'State', cell: (r) => <StatusPill status={r.state} pulse={false} label={r.stateLabel} />, width: 150 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 820 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LISTING DATASETS…</div>}
      </div>

      {d && (
        <div>
          <SectionLabel number={3} muted>LINEAGE · {d.name}</SectionLabel>
          <div className="mg-grid-bg" style={{ display: 'flex', alignItems: 'center', gap: 8, padding: 16, marginTop: 8, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflowX: 'auto' }}>
            {d.lineage.map((hop, i, arr) => (
              <React.Fragment key={hop.label}>
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, padding: '7px 11px', borderRadius: 6, background: 'var(--surface-card)', border: '1px solid ' + (i === 2 ? 'var(--accent)' : 'var(--border)'), whiteSpace: 'nowrap' }}>
                  <Icon name={hop.icon} size={13} color="var(--accent)" />
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--grey-100)' }}>{hop.label}</span>
                </span>
                {i < arr.length - 1 && <Icon name="chevron-right" size={13} color="var(--text-faint)" />}
              </React.Fragment>
            ))}
          </div>
        </div>
      )}
    </ConsolePage>
  );
}
