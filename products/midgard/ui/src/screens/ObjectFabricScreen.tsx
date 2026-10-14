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
import { useAsync, useAuditStream, useGateway } from '@midgard/ui-shared';
import { HumanGate } from '@midgard/design-system';
import type { AuditRecord, ObjectType } from '@midgard/sdk';
import { HumanGateRequiredError } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

type GateState = { action: string; state: 'pending' | 'authorised' | 'rejected'; auditId?: string } | null;

export function ObjectFabricScreen() {
  const gateway = useGateway();
  const { data: types, loading } = useAsync((c) => c.listObjectTypes(), []);
  const { data: auditSeed } = useAsync((c) => c.getAudit(8), []);
  const live = useAuditStream(auditSeed ?? [], 40);

  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(0);
  const [gate, setGate] = React.useState<GateState>(null);
  const [busy, setBusy] = React.useState(false);

  const rows: ObjectType[] = types ?? [];
  const o = rows[sel];

  const triggerAction = async (action: string, humanGated: boolean) => {
    if (!o) return;
    if (humanGated) {
      setGate({ action, state: 'pending' });
      return;
    }
    setBusy(true);
    try {
      await gateway.applyAction({ policy: gateway.policy(), actionType: action, params: { target: `${o.apiName}/*` } });
    } finally {
      setBusy(false);
    }
  };

  const authorise = async () => {
    if (!gate || !o) return;
    setBusy(true);
    try {
      const res = await gateway.applyAction({
        policy: gateway.policy(),
        actionType: gate.action,
        params: { target: `${o.apiName}/*` },
        humanGateToken: `HG-${Date.now().toString(36)}`,
      });
      setGate({ action: gate.action, state: 'authorised', auditId: res.auditId });
    } catch (e) {
      if (e instanceof HumanGateRequiredError) setGate({ action: gate.action, state: 'pending' });
    } finally {
      setBusy(false);
    }
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
        {o.actions.map((a, i) => (
          <button
            key={i}
            type="button"
            onClick={() => triggerAction(a.name, a.humanGated)}
            style={{ all: 'unset', boxSizing: 'border-box', cursor: 'pointer', width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}
          >
            <Icon name="zap" size={13} color="var(--accent)" />
            {a.name}
            {a.humanGated && (
              <span style={{ marginLeft: 'auto' }}>
                <Badge tone="warn">HUMAN GATE</Badge>
              </span>
            )}
          </button>
        ))}
      </div>
      {gate && (
        <HumanGate
          title={`${gate.action} · ${o.name}`}
          state={gate.state}
          auditId={gate.auditId}
          onAuthorise={busy ? undefined : authorise}
          onReject={() => setGate(null)}
        />
      )}
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
        <Button variant="primary" size="sm" fullWidth iconRight={<Icon name="arrow-right" size={14} />}>Explore instances</Button>
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="objects"
      context="ENCLOSURE / OBJECT FABRIC"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={2}>OBJECT FABRIC</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Object types</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="312" label="OBJECT TYPES" size="sm" />
        <StatBlock value="1.2M" label="INSTANCES" size="sm" />
        <StatBlock value="100%" label="LINEAGE SIGNED" size="sm" accent />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />}>New object type</Button>
      </div>

      <Tabs
        tabs={[{ label: 'All types', badge: '312' }, { label: 'Interfaces' }, { label: 'Shared properties' }, { label: 'Change requests', badge: '4' }]}
        active={tab}
        onChange={setTab}
      />

      {/* flexShrink 0: an overflow wrapper may otherwise be squeezed to zero height
       * inside the shell's fixed-height <main> as the live audit list grows; the
       * page scrolls instead. */}
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
          onRowClick={(i) => { setSel(i); setGate(null); }}
          style={{ minWidth: 720 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>QUERYING FABRIC…</div>}
      </div>

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
