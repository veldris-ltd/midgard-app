import * as React from 'react';
import {
  Badge,
  Card,
  HumanGate,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import { HumanGateRequiredError } from '@midgard/sdk';
import type { HeimdallGateway, HeimdallWorkflow } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

type GateState = { state: 'pending' | 'authorised' | 'rejected'; auditId?: string };

export function WorkflowsScreen() {
  const gateway = useGateway<HeimdallGateway>();
  const { data, loading } = useAsync<HeimdallWorkflow[], HeimdallGateway>((c) => c.listWorkflows(), []);
  const rows: HeimdallWorkflow[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const [gate, setGate] = React.useState<GateState | null>(null);
  const [busy, setBusy] = React.useState(false);

  const idx = Math.min(sel, rows.length - 1);
  const w = rows[idx];
  const pendingGate = rows.filter((r) => r.humanGated).length;

  const select = (i: number) => { setSel(i); setGate(null); };

  const authorise = async () => {
    if (!w) return;
    setBusy(true);
    try {
      const res = await gateway.authoriseWorkflow(w.id, `HG-${Date.now().toString(36)}`);
      setGate({ state: 'authorised', auditId: res.auditId });
    } catch (err) {
      if (err instanceof HumanGateRequiredError) setGate({ state: 'pending' });
    } finally {
      setBusy(false);
    }
  };

  const authorised = gate?.state === 'authorised';
  const gatedPending = w?.humanGated && !authorised;

  const stages: [string, boolean][] = w
    ? [
        ['Anomaly detected', true],
        ['Options generated · cited', true],
        ['Operator review', true],
        ['Human authorisation', !w.humanGated || authorised],
        ['Execution', w.stage === 'executing' || w.stage === 'complete' || authorised],
        ['Signed lineage', w.stage === 'complete' || authorised],
      ]
    : [];

  const signedLabel = w
    ? authorised
      ? `AUTHORISED · AUDIT #${gate?.auditId ?? '—'}`
      : w.stage === 'complete'
        ? 'COMPLETE · AUDIT #D-4468'
        : 'AUTHORISED · EXECUTING'
    : '';

  const aside = w ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="workflow" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, flex: 1, lineHeight: 1.3 }}>{w.title}</h2>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>STAGES</div>
        {stages.map(([s, done], i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)' }}>
            <Icon name={done ? 'check' : 'clock'} size={14} color={done ? 'var(--secure)' : 'var(--text-faint)'} />
            <span style={{ fontSize: 'var(--fs-xs)', color: done ? 'var(--text-body)' : 'var(--text-faint)' }}>{s}</span>
          </div>
        ))}
      </div>
      {gatedPending ? (
        <HumanGate
          title={`${w.id} · authorise`}
          detail="HUMAN AUTHORISATION REQUIRED · AI IS ADVISORY"
          state={gate?.state ?? 'pending'}
          onAuthorise={busy ? undefined : authorise}
          onReject={() => setGate({ state: 'rejected' })}
        />
      ) : (
        <div style={{ padding: '12px 14px', background: 'var(--surface-inset)', border: '1px solid var(--secure)', borderRadius: 'var(--radius-md)', display: 'flex', alignItems: 'center', gap: 9 }}>
          <Icon name="shield-check" size={16} color="var(--secure)" />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', color: 'var(--secure)' }}>{signedLabel}</span>
        </div>
      )}
      <Terminal
        title="DECISION RECORD"
        lines={[
          'workflow  ' + w.id,
          'authoriser ' + w.owner,
          'basis     ' + w.opts + ' cited options · reviewed',
          'sig       ML-DSA · decision lineage',
        ]}
        style={{ marginTop: 'auto' }}
      />
    </>
  ) : null;

  return (
    <ConsolePage
      active="workflows"
      context="DECIDE / ACTION WORKFLOWS"
      asideWidth={360}
      toolbar={<Badge tone="warn">{pendingGate} AWAITING AUTHORISATION</Badge>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 24, flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={3}>WORKFLOWS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Action workflows</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length)} label="ACTIVE" size="sm" />
        <StatBlock value="100%" label="HUMAN-AUTHORISED" size="sm" accent />
        <StatBlock value="0" label="AUTO-EXECUTED · EVER" size="sm" />
      </div>
      <div style={{ overflowX: 'auto' }}>
        <Table<HeimdallWorkflow>
          columns={[
            { header: 'Workflow', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--accent-hover)' }}>{r.id}</span>, width: 90 },
            { header: 'Action', key: 'title' },
            { header: 'Stage', key: 'stage', width: 190 },
            { header: 'Owner', key: 'owner', mono: true, width: 110 },
            { header: 'Options', key: 'opts', mono: true, align: 'right', width: 70 },
            { header: 'Age', key: 'age', mono: true, width: 60 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={r.status === 'online'} label={r.statusLabel} />, width: 130 },
          ]}
          rows={rows}
          selectedIndex={idx}
          onRowClick={select}
          style={{ minWidth: 760 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING WORKFLOWS…</div>}
      </div>
      <div>
        <SectionLabel number={4} muted>PRINCIPLE</SectionLabel>
        <Card size="sm" style={{ marginTop: 8 }}>
          <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.65 }}>AI generates and ranks options; it never executes. Every consequential action passes a named human, and the authorisation, the basis for it, and the outcome are one signed record.</p>
          <div style={{ display: 'flex', gap: 6, marginTop: 10, flexWrap: 'wrap' }}>
            <ProvenanceTag id="FR-H-5" title="Human-gated action workflows with full audit lineage" />
            <ProvenanceTag id="NFR-AI-1" title="Traceable, evaluated, reversible or human-gated" />
          </div>
        </Card>
      </div>
    </ConsolePage>
  );
}
