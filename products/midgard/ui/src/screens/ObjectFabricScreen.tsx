import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Badge,
  Button,
  Card,
  Icon,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  Select,
  StatBlock,
  Table,
  Tabs,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useAuditStream, useGateway } from '@midgard/ui-shared';
import { HumanGate } from '@midgard/design-system';
import type { Approval, AuditRecord, ObjectInstance, ObjectType } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/** The outcome of the last action, approval request or decision. */
type Notice = { tone: 'secure' | 'danger'; text: string } | null;

/** The registry views the gateway does not serve yet, each with what it would show. */
const UNSERVED_TABS: Record<number, string> = {
  1: 'Interfaces are not served by this gateway yet. Object types are declared in the ontology file, which has no interfaces section.',
  2: 'Shared properties are not served by this gateway yet. Each object type lists its own properties.',
};

/** "1,445" → 1445. */
const count = (s: string) => Number(s.replace(/[^0-9]/g, '')) || 0;

/** How often to re-read approvals while one of ours waits for a second person. */
const APPROVAL_POLL_MS = 5000;

const instanceLabel = (i: ObjectInstance) => {
  const name = i.propertyValues.name ?? i.propertyValues.tail;
  return name ? `${i.id} · ${name}` : i.id;
};

/* Object fabric · object types. The registry's types, and actions on one object of
 * the selected type at a time. A human-gated action is requested here, approved by a
 * second person, and only then run by the requester with that approval's id; every
 * action, request and decision shows its outcome or the gateway's refusal.
 * [DD-1] [NFR-AI-1] */

export function ObjectFabricScreen() {
  const gateway = useGateway();
  const navigate = useNavigate();
  const { data: types, loading } = useAsync((c) => c.listObjectTypes(), []);
  const { data: auditSeed } = useAsync((c) => c.getAudit(8), []);
  const live = useAuditStream(auditSeed ?? [], 40);

  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(0);
  const [target, setTarget] = React.useState('');
  const [notice, setNotice] = React.useState<Notice>(null);
  const [busy, setBusy] = React.useState(false);

  const rows: ObjectType[] = types ?? [];
  // The header's figures are counted from what the gateway returned for this caller.
  const instanceTotal = rows.reduce((sum, t) => sum + count(t.instanceCount), 0);
  const signed = live.filter((r) => r.pqSignature.includes('✓')).length;
  const signedShare = live.length ? `${Math.round((100 * signed) / live.length)}%` : '—';
  const o = rows[sel];

  // Actions change one object at a time, so the side panel picks it from the type's
  // readable instances.
  const { data: found, reload: reloadInstances } = useAsync(
    (c) => (o ? c.query({ policy: c.policy(), objectType: o.apiName, maxResults: 500 }) : Promise.resolve(null)),
    [o?.apiName],
  );
  const instances = found?.results ?? [];
  const picked = instances.find((i) => i.id === target);

  const { data: approvalList, reload: reloadApprovals } = useAsync((c) => c.listApprovals(), []);
  const approvals = (approvalList ?? []).filter((a) => o && a.target.startsWith(`${o.apiName}/`));
  const waiting = (approvalList ?? []).some((a) => a.mine && a.status === 'pending');
  React.useEffect(() => {
    if (!waiting) return undefined;
    const t = setInterval(reloadApprovals, APPROVAL_POLL_MS);
    return () => clearInterval(t);
  }, [waiting, reloadApprovals]);

  const attempt = async (work: () => Promise<string>) => {
    setBusy(true);
    setNotice(null);
    try {
      setNotice({ tone: 'secure', text: await work() });
    } catch (e) {
      setNotice({ tone: 'danger', text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const runAction = (action: string, on: string, humanGateToken?: string) =>
    attempt(async () => {
      const res = await gateway.applyAction({ policy: gateway.policy(), actionType: action, params: { target: on }, humanGateToken });
      reloadInstances();
      if (humanGateToken) reloadApprovals();
      return `${action} · ${on} · ${res.applied ? 'APPLIED' : 'NOT APPLIED'} · AUDIT #${res.auditId}`;
    });

  const requestApproval = (action: string) =>
    attempt(async () => {
      const a = await gateway.requestApproval(action, target);
      reloadApprovals();
      return `${a.id} REQUESTED · ${action} · ${a.target} · WAITING FOR A SECOND PERSON`;
    });

  const decide = (a: Approval, approve: boolean) =>
    attempt(async () => {
      const res = await gateway.decideApproval(a.id, approve);
      reloadApprovals();
      return `${res.id} ${res.status.toUpperCase()} · ${res.actionType} · ${res.target}`;
    });

  const triggerAction = (action: string, humanGated: boolean) => {
    if (!target) return;
    if (humanGated) requestApproval(action);
    else runAction(action, target);
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
        <Select
          label="Act on"
          size="sm"
          fullWidth
          value={target}
          onChange={(e) => { setTarget(e.target.value); setNotice(null); }}
          options={[
            { value: '', label: instances.length ? 'Choose an object…' : 'No objects you can act on' },
            ...instances.map((i) => ({ value: i.id, label: instanceLabel(i) })),
          ]}
          style={{ marginBottom: 6 }}
        />
        {picked?.propertyValues.status && (
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)', marginBottom: 6 }}>
            STATUS · <span style={{ color: 'var(--text-body)' }}>{picked.propertyValues.status}</span>
          </div>
        )}
        {o.actions.map((a, i) => {
          const disabled = busy || !target;
          return (
            <button
              key={i}
              type="button"
              disabled={disabled}
              title={target ? undefined : 'Choose an object to act on'}
              onClick={() => triggerAction(a.name, a.humanGated)}
              style={{ all: 'unset', boxSizing: 'border-box', cursor: disabled ? 'not-allowed' : 'pointer', opacity: disabled ? 0.5 : 1, width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}
            >
              <Icon name="zap" size={13} color="var(--accent)" />
              {a.name}
              {a.humanGated && (
                <span style={{ marginLeft: 'auto' }}>
                  <Badge tone="warn">HUMAN GATE</Badge>
                </span>
              )}
            </button>
          );
        })}
      </div>
      {notice && (
        <div
          role={notice.tone === 'danger' ? 'alert' : 'status'}
          style={{ padding: '10px 12px', background: 'var(--surface-inset)', border: `1px solid var(--${notice.tone})`, borderRadius: 'var(--radius-md)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.06em', color: `var(--${notice.tone})`, overflowWrap: 'anywhere' }}
        >
          {notice.text}
        </div>
      )}
      {approvals.length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>APPROVALS</div>
          {approvals.map((a) =>
            !a.mine && a.status === 'pending' ? (
              <HumanGate
                key={a.id}
                title={`${a.actionType} · ${a.target}`}
                detail={`${a.id} · REQUESTED BY ${a.requester.toUpperCase()} · YOUR APPROVAL NEEDED`}
                onAuthorise={busy ? undefined : () => decide(a, true)}
                onReject={busy ? undefined : () => decide(a, false)}
              />
            ) : (
              <ApprovalRow key={a.id} approval={a} busy={busy} onRun={() => runAction(a.actionType, a.target, a.id)} />
            ),
          )}
        </div>
      )}
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="DD-1" title="Object fabric at the centre" />
          <ProvenanceTag id="NFR-SEC-2" title="Role/marking/purpose enforced at query time" />
          <ProvenanceTag id="NFR-PQ-2" title="AES-256-GCM under ML-KEM-1024-wrapped keys" />
          <ProvenanceTag id="NFR-AI-1" title="Consequential actions need a second person's approval" />
        </div>
      </div>
      <div style={{ marginTop: 'auto', display: 'flex', gap: 8 }}>
        <Button variant="secondary" size="sm" fullWidth disabled title="Not available yet: the schema comes from the ontology file at boot">Edit schema</Button>
        <Button variant="primary" size="sm" fullWidth iconRight={<Icon name="arrow-right" size={14} />} onClick={() => navigate(`/search?type=${encodeURIComponent(o.apiName)}`)}>Explore instances</Button>
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
        <StatBlock value={types ? String(rows.length) : '—'} label="OBJECT TYPES" size="sm" />
        <StatBlock value={types ? instanceTotal.toLocaleString('en-GB') : '—'} label="INSTANCES" size="sm" />
        <StatBlock value={signedShare} label="LINEAGE SIGNED" size="sm" accent />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />} disabled title="Not available yet: object types come from the ontology file at boot">New object type</Button>
      </div>

      <Tabs
        tabs={[{ label: 'All types', badge: types ? String(rows.length) : undefined }, { label: 'Interfaces' }, { label: 'Shared properties' }]}
        active={tab}
        onChange={setTab}
      />

      {UNSERVED_TABS[tab] ? (
        <div role="status" style={{ padding: '18px 16px', border: '1px dashed var(--border)', borderRadius: 'var(--radius-md)', color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          {UNSERVED_TABS[tab]}
        </div>
      ) : (
      <div style={{ overflowX: 'auto' }}>
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
          onRowClick={(i) => { setSel(i); setTarget(''); setNotice(null); }}
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

const STATUS_TONE: Record<Approval['status'], 'neutral' | 'secure' | 'warn' | 'danger'> = {
  pending: 'warn',
  approved: 'secure',
  rejected: 'danger',
  used: 'neutral',
  expired: 'neutral',
};

/** One approval in the side panel: the caller's own (waiting, or ready to run), or
 *  someone else's that is already decided. */
function ApprovalRow({ approval: a, busy, onRun }: { approval: Approval; busy: boolean; onRun: () => void }) {
  let who = `REQUESTED BY ${a.requester.toUpperCase()}`;
  if (a.mine) {
    if (a.status === 'pending') who = 'WAITING FOR A SECOND PERSON';
    else who = a.approver ? `DECIDED BY ${a.approver.toUpperCase()}` : '';
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 10px', background: 'var(--surface-inset)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-md)' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-heading)', overflowWrap: 'anywhere' }}>
          {a.actionType} · {a.target}
        </div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)', marginTop: 2 }}>
          {a.id}{who ? ` · ${who}` : ''}
        </div>
      </div>
      {a.mine && a.status === 'approved' ? (
        <Button variant="primary" size="sm" disabled={busy} onClick={onRun}>Run</Button>
      ) : (
        <Badge tone={STATUS_TONE[a.status]}>{a.status.toUpperCase()}</Badge>
      )}
    </div>
  );
}
