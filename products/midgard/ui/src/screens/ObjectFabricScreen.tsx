import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Table,
  Tabs,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useAuditStream } from '@midgard/ui-shared';
import type { AuditRecord, ObjectType } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ExplorerBody, ObjectCard } from './ObjectExplorer';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

export function ObjectFabricScreen() {
  const { data: types, loading, reload: reloadTypes } = useAsync((c) => c.listObjectTypes(), []);
  // Types carry their actions; refresh them now and then, so actions added to the
  // ontology appear without reloading the page.
  React.useEffect(() => {
    const t = window.setInterval(reloadTypes, 60_000);
    return () => window.clearInterval(t);
  }, [reloadTypes]);
  const { data: auditSeed } = useAsync((c) => c.getAudit(8), []);
  const live = useAuditStream(auditSeed ?? [], 40);

  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(0);
  // The type whose objects are being explored, and the objects opened in turn (the
  // last is on the card; earlier ones are where Back goes).
  const [explore, setExplore] = React.useState<string | null>(null);
  const [trail, setTrail] = React.useState<string[]>([]);

  const rows: ObjectType[] = types ?? [];
  const o = rows[sel];
  const exploring = rows.find((t) => t.apiName === explore) ?? null;
  const instances = rows.reduce((n, t) => n + (Number(String(t.instanceCount).replace(/[^0-9]/g, '')) || 0), 0);
  const fmt = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n.toLocaleString());

  const openObject = (id: string) => {
    reloadTypes();
    const typeId = id.slice(0, id.indexOf('/'));
    if (typeId && typeId !== explore) setExplore(typeId);
    setTrail((t) => (t[t.length - 1] === id ? t : [...t, id]));
  };
  const startExploring = (apiName: string) => {
    reloadTypes();
    setExplore(apiName);
    setTrail([]);
  };

  const aside = o ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="boxes" size={18} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1 }}>{o.name}</h2>
        <MarkingChip level={o.marking} compartment={o.compartment ?? undefined} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={o.propertyCount} label="PROPERTIES" size="sm" />
          <StatBlock value={o.instanceCount} label="INSTANCES" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>LINK TYPES</div>
        {o.links.map((l, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <Icon name="network" size={13} color="var(--text-faint)" />
            {l.label}
          </div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>ACTION TYPES</div>
        {o.actions.length === 0 && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>NONE DECLARED</div>}
        {o.actions.map((a, i) => (
          <div
            key={i}
            style={{ boxSizing: 'border-box', width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}
          >
            <Icon name="zap" size={13} color="var(--accent)" />
            {a.name}
            {a.humanGated && (
              <span style={{ marginLeft: 'auto' }}>
                <Badge tone="warn">HUMAN GATE</Badge>
              </span>
            )}
          </div>
        ))}
        {o.actions.length > 0 && (
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)', marginTop: 6 }}>EXPLORE INSTANCES AND OPEN ONE TO ACT ON IT</div>
        )}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="DD-1" title="Object fabric at the centre" />
          <ProvenanceTag id="NFR-SEC-2" title="Role/marking/purpose enforced at query time" />
          <ProvenanceTag id="NFR-PQ-2" title="AES-256-GCM under ML-KEM-1024-wrapped keys" />
        </div>
      </div>
      <div style={{ marginTop: 'auto', display: 'flex', gap: 8 }}>
        <Button variant="secondary" size="sm" fullWidth>Edit schema</Button>
        <Button variant="primary" size="sm" fullWidth iconRight={<Icon name="arrow-right" size={14} />} onClick={() => startExploring(o.apiName)}>Explore instances</Button>
      </div>
    </>
  ) : null;

  const card = trail.length > 0 ? (
    <ObjectCard
      id={trail[trail.length - 1]}
      types={rows}
      onOpen={openObject}
      onBack={() => setTrail((t) => t.slice(0, -1))}
    />
  ) : null;

  return (
    <ConsolePage
      active="objects"
      context="ENCLOSURE / OBJECT FABRIC"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={exploring ? card ?? aside : aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>OBJECT FABRIC</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Object types</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="OBJECT TYPES" size="sm" />
        <StatBlock value={fmt(instances)} label="INSTANCES" size="sm" />
        <StatBlock value="100%" label="LINEAGE SIGNED" size="sm" accent />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />}>New object type</Button>
      </div>

      <Tabs
        tabs={[{ label: 'All types', badge: String(rows.length) }, { label: 'Interfaces' }, { label: 'Shared properties' }, { label: 'Change requests' }]}
        active={tab}
        onChange={setTab}
      />

      {/* flexShrink 0: an overflow wrapper may otherwise be squeezed to zero height
       * inside the shell's fixed-height <main> as the live audit list grows; the
       * page scrolls instead. */}
      {exploring ? (
        <>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <Button variant="secondary" size="sm" onClick={() => { setExplore(null); setTrail([]); }}>All object types</Button>
            <h2 style={{ fontSize: 'var(--fs-h4)' }}>{exploring.name}</h2>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>{exploring.instanceCount} INSTANCES · LIVE</span>
          </div>
          <ExplorerBody type={exploring} selectedId={trail[trail.length - 1] ?? null} onOpen={(id) => { reloadTypes(); setTrail([id]); }} />
        </>
      ) : (
      <div style={{ overflowX: 'auto', flexShrink: 0 }}>
        <Table<ObjectType>
          columns={[
            { header: 'Object type', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="boxes" size={15} color="var(--accent)" />{r.name}</span> },
            { header: 'API name', key: 'apiName', mono: true },
            { header: 'Props', key: 'propertyCount', mono: true, align: 'right', width: 70 },
            { header: 'Instances', key: 'instanceCount', mono: true, align: 'right', width: 110 },
            { header: 'Marking', cell: (r) => <MarkingChip level={r.marking} compartment={r.compartment ?? undefined} />, width: 190 },
            { header: 'Updated', key: 'updated', mono: true, width: 100 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 720 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>QUERYING FABRIC…</div>}
      </div>
      )}

      <div>
        <SectionLabel number={3} muted>AUDIT LINEAGE · LIVE</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<AuditRecord>
            compact
            columns={[
              { header: 'Actor', key: 'actor', mono: true },
              { header: 'Operation', key: 'action' },
              { header: 'Resource', key: 'resource', mono: true },
              { header: 'Signature', cell: (r) => <Badge tone="secure">{r.pqSignature}</Badge>, width: 110 },
              { header: 'TS', key: 'ts', mono: true, width: 90 },
            ]}
            rows={live}
            style={{ minWidth: 640 }}
          />
        </div>
      </div>
    </ConsolePage>
  );
}
