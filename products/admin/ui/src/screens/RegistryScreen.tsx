import * as React from 'react';
import {
  Badge,
  Card,
  GuardedAction,
  Icon,
  ProvenanceTag,
  RiskBadge,
  SectionLabel,
  StatBlock,
  Table,
  Tabs,
  Terminal,
  type BadgeProps,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { AdminGateway, AdminRegistryItem, AdminRegistryState } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Registry & Suite Governance — model registry, agent scopes, Evals
 * gates, release administration; lifecycle changes are dual-controlled and
 * verified (fails closed, FR-AD-4). Reproduces docs/design/ui_kits/admin/registry.html;
 * model data from `listRegistry()`. */

const STATE_TONE: Record<AdminRegistryState, NonNullable<BadgeProps['tone']>> = {
  approved: 'secure',
  revoked: 'danger',
  pending: 'warn',
};

const AGENTS = [
  { a: 'dispatch-01', s: 'fabric read + schedule_maintenance action', g: 'human', e: '96%' },
  { a: 'feed-triage', s: 'fabric read only', g: 'pre', e: '94%' },
  { a: 'release-risk', s: 'artefact metadata read only', g: 'pre', e: '91%' },
];

const GATES: Array<readonly [string, string, string]> = [
  ['dispatch agents', '92%', 'raised from 90% · CFG-2209'],
  ['triage agents', '90%', 'floor'],
  ['advisory (risk, options)', '88%', 'advisory-only surfaces'],
];

const RELEASES = [
  { s: 'Channels', v: 'stable 2.4.1 · beta 2.5.0', a: 'Edit channel policy', dual: false },
  { s: 'Rollout', v: 'v2.5.0 · ring 2 of 4', a: 'Pause / resume / rollback', dual: false },
  { s: 'Fleet-wide release', v: '73 installations', a: 'Promote to all rings', dual: true },
];

export function RegistryScreen() {
  const { data } = useAsync<AdminRegistryItem[], AdminGateway>((c) => c.listRegistry(), []);
  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(4);
  const [approved, setApproved] = React.useState(false);

  const models: AdminRegistryItem[] = data ?? [];
  const m = models[sel];

  const lifecycle: Array<readonly [string, string, boolean]> = m
    ? [
        ['Approve', 'evals gate + licence + hardware eligibility', m.state === 'pending'],
        ['Deprecate', 'consumers warned · grace window', m.state === 'approved'],
        ['Revoke', 'immediate · fleet stops serving it', m.state !== 'revoked'],
      ]
    : [];

  const aside = m ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="cpu" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], fontSize: 14, flex: 1 }}>{m.name}</h2>
        <Badge tone={STATE_TONE[m.state]}>{m.state.toUpperCase()}</Badge>
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={m.family} label="FAMILY" size="sm" />
          <StatBlock value={m.signature ? 'ML-DSA ✓' : 'FAILED'} label="WEIGHTS SIGNATURE" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>LIFECYCLE · ALL DUAL-CONTROL</div>
        {lifecycle.map(([a, d, avail]) => (
          <div key={a} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', opacity: avail ? 1 : 0.45 }}>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', width: 76, flex: 'none' }}>{a.toUpperCase()}</span>
            <span style={{ fontSize: 'var(--fs-micro)', fontFamily: 'var(--font-mono)', color: 'var(--text-faint)' }}>{d.toUpperCase()}</span>
          </div>
        ))}
      </div>
      {m.state === 'pending' ? (
        <GuardedAction
          action={'Approve ' + m.name}
          subsystem="MIMIR REGISTRY"
          risk={4}
          state={approved ? 'approved' : 'awaiting'}
          requester="p.iwu"
          approver="j.benjamin"
          justification="Evals 94.1% (gate 90%) · KAT + licence verified · hardware: GPU pool only"
          onApprove={() => setApproved(true)}
          onDeny={() => setApproved(false)}
        />
      ) : (
        <Terminal
          title="VERIFICATION"
          lines={[
            '> admin registry verify ' + m.name,
            'weights   sha3 match · ML-DSA ✓',
            'licence   policy OK',
            'hardware  eligibility verified',
            m.state === 'revoked' ? '# REVOKED · serving blocked fleet-wide' : '# approved for serving',
          ]}
        />
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-AD-4" title="Administer the model registry, agent governance, Evals gates, releases and fleet" />
        <ProvenanceTag id="AC-MI-7" title="Weights verified: signature, licence, hardware eligibility · fails closed" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="registry"
      context="ADMIN / REGISTRY & SUITE GOVERNANCE"
      toolbar={<><Badge tone="warn">1 CANDIDATE IN EVALS</Badge><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={10}>REGISTRY</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Suite governance</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="3" label="MODELS APPROVED" size="sm" />
        <StatBlock value="12" label="AGENT SCOPES" size="sm" />
        <StatBlock value="90%" label="EVALS GATE · FLOOR" size="sm" accent />
      </div>

      <Tabs
        tabs={[{ label: 'Model registry', badge: String(models.length || 5) }, { label: 'Agent governance', badge: '12' }, { label: 'Evals gates' }, { label: 'Releases & fleet' }]}
        active={tab}
        onChange={setTab}
      />

      {tab === 0 && (
        <div style={{ overflowX: 'auto' }}>
          <Table<AdminRegistryItem>
            columns={[
              { header: 'Model', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="cpu" size={14} color="var(--accent)" />{r.name}</span> },
              { header: 'Family', key: 'family', mono: true, width: 100 },
              { header: 'Signature', cell: (r) => <Badge tone={r.signature ? 'secure' : 'danger'}>{r.signature ? 'ML-DSA ✓' : 'FAILED'}</Badge>, width: 110 },
              { header: 'Use', key: 'uses', width: 220 },
              { header: 'Lifecycle', cell: (r) => <Badge tone={STATE_TONE[r.state]}>{r.state.toUpperCase()}</Badge>, width: 110 },
              { header: 'Risk', cell: (r) => <RiskBadge level={r.risk} dualControl={false} />, width: 110 },
            ]}
            rows={models}
            selectedIndex={sel}
            onRowClick={(i) => setSel(i)}
            style={{ minWidth: 820 }}
          />
        </div>
      )}
      {tab === 1 && (
        <div style={{ overflowX: 'auto' }}>
          <Table<typeof AGENTS[number]>
            columns={[
              { header: 'Agent', key: 'a', mono: true },
              { header: 'Tool scope', key: 's' },
              { header: 'Gate', cell: (r) => <Badge tone={r.g === 'human' ? 'warn' : 'secure'}>{r.g === 'human' ? 'HUMAN GATE' : 'PRE-AUTH · NARROW'}</Badge>, width: 160 },
              { header: 'Evals', key: 'e', mono: true, align: 'right', width: 80 },
            ]}
            rows={AGENTS}
            style={{ minWidth: 620 }}
          />
        </div>
      )}
      {tab === 2 && (
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          {GATES.map(([grp, v, d]) => (
            <Card key={grp} size="sm" style={{ flex: 1, minWidth: 220 }}>
              <StatBlock value={v} label={('GATE · ' + grp).toUpperCase()} size="sm" accent />
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', marginTop: 8 }}>{d.toUpperCase()}</div>
            </Card>
          ))}
        </div>
      )}
      {tab === 3 && (
        <div style={{ overflowX: 'auto' }}>
          <Table<typeof RELEASES[number]>
            columns={[
              { header: 'Surface', key: 's' },
              { header: 'State', key: 'v', mono: true, width: 200 },
              { header: 'Admin action', key: 'a', width: 240 },
              { header: 'Control', cell: (r) => <Badge tone={r.dual ? 'warn' : 'neutral'}>{r.dual ? 'DUAL-CONTROL' : 'STANDARD'}</Badge>, width: 140 },
            ]}
            rows={RELEASES}
            style={{ minWidth: 780 }}
          />
        </div>
      )}

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        <ProvenanceTag id="FR-AD-4" />
        <ProvenanceTag id="FR-AD-5" title="Model revocation and fleet-wide release are dual-controlled" />
        <ProvenanceTag id="AC-AD-3" />
      </div>
    </ConsolePage>
  );
}
