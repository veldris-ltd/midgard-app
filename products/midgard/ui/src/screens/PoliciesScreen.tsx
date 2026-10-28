import * as React from 'react';
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
  StatusPill,
  Table,
  Tabs,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { MarkingLevel, PolicyDecision, PolicyRule } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Policies & markings · governance. Role / marking / purpose policy evaluated at
 * query time, deny-by-default. The rules Table comes from `listPolicies()`; the key
 * flow is the policy dry-run — pick a purpose + marking against the session's policy
 * context and evaluate `simulatePolicy(ctx, marking)` for a signed grant/deny.
 * Rules come from the policy file at boot, so New policy and Edit clearances are
 * disabled and say so. [NFR-SEC-2] */

const MARKINGS: MarkingLevel[] = ['confidential', 'restricted', 'internal', 'open'];

export function PoliciesScreen() {
  const gateway = useGateway();
  const { data, loading } = useAsync((c) => c.listPolicies(), []);
  const rules: PolicyRule[] = data ?? [];
  const ctx = gateway.policy();

  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(0);

  // Dry-run form state.
  const purposes = Array.from(new Set([ctx.purpose, ...rules.map((r) => r.purpose)]));
  const [purpose, setPurpose] = React.useState(ctx.purpose);
  const [marking, setMarking] = React.useState<MarkingLevel>('confidential');
  const [decision, setDecision] = React.useState<PolicyDecision | null>(null);
  const [busy, setBusy] = React.useState(false);

  const runDryRun = async () => {
    setBusy(true);
    try {
      const res = await gateway.simulatePolicy({ ...ctx, purpose }, marking);
      setDecision(res);
    } finally {
      setBusy(false);
    }
  };

  const p = rules[sel];
  const granted = decision?.decision === 'grant';

  const aside = p ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="shield-check" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{p.name}</h2>
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={p.effect.toUpperCase()} label="EFFECT" size="sm" accent={p.effect !== 'allow'} />
          <StatBlock value={p.marking.toUpperCase()} label="MARKING" size="sm" />
        </div>
      </Card>
      <Terminal
        title="POLICY · DECLARATIVE"
        lines={[
          `${p.effect} when {`,
          `  role    in [${p.roles.join(', ')}]`,
          `  marking <= ${p.marking.toUpperCase()}`,
          `  purpose == ${p.purpose}`,
          '}',
        ]}
      />
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>EVALUATED AT</div>
        {['Query time · row and column level', 'Action invocation · pre-execution', 'Agent retrieval · OAG scope'].map((l, i) => (
          <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '6px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <Icon name="check" size={13} color="var(--secure)" />{l}
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="NFR-SEC-2" title="Role, marking and purpose enforced at query time" />
        <ProvenanceTag id="DD-8" title="Standards-aligned assurance: NIST SP 800-53, OWASP ASVS, ISO/IEC 27001" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="policies"
      context="ENCLOSURE / GOVERNANCE"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={8}>GOVERNANCE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Policies &amp; markings</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rules.length)} label="POLICIES" size="sm" />
        <StatBlock value="4" label="MARKING LEVELS" size="sm" />
        <StatBlock value="0" label="UNGOVERNED TYPES" size="sm" accent />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />} disabled title="Not available yet: policies come from the policy file at boot">New policy</Button>
      </div>

      <Tabs
        tabs={[{ label: 'Policies', badge: String(rules.length) }, { label: 'Markings' }, { label: 'Purposes', badge: String(purposes.length) }]}
        active={tab}
        onChange={setTab}
      />

      {tab === 1 ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {MARKINGS.map((lvl) => {
            const count = rules.filter((r) => r.marking === lvl).length;
            return (
              <Card key={lvl} size="sm">
                <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
                  <MarkingChip level={lvl} />
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{count} polic{count === 1 ? 'y' : 'ies'} reference this marking</span>
                  <div style={{ flex: 1 }} />
                  <Button variant="ghost" size="sm" disabled title="Not available yet: clearances are set when an identity is enrolled">Edit clearances</Button>
                </div>
              </Card>
            );
          })}
        </div>
      ) : tab === 2 ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {purposes.map((pu) => {
            const count = rules.filter((r) => r.purpose === pu).length;
            return (
              <Card key={pu} size="sm">
                <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
                  <Icon name="target" size={15} color="var(--accent)" />
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-sm)', color: 'var(--grey-100)' }}>{pu}</span>
                  <div style={{ flex: 1 }} />
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{count} rule{count === 1 ? '' : 's'}</span>
                </div>
              </Card>
            );
          })}
        </div>
      ) : (
        <div style={{ overflowX: 'auto' }}>
          <Table<PolicyRule>
            columns={[
              { header: 'Policy', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="shield-check" size={14} color="var(--accent)" />{r.name}</span> },
              { header: 'Effect', cell: (r) => <Badge tone={r.effect === 'deny' ? 'danger' : 'secure'}>{r.effect.toUpperCase()}</Badge>, width: 110 },
              { header: 'Roles', cell: (r) => r.roles.join(', '), mono: true, width: 160 },
              { header: 'Marking', key: 'marking', mono: true, width: 130 },
              { header: 'Purpose', key: 'purpose', mono: true, width: 140 },
              { header: 'State', cell: (r) => <StatusPill status={r.effect === 'deny' ? 'gated' : 'secure'} pulse={false} label={r.effect === 'deny' ? 'Deny' : 'Enforced'} />, width: 130 },
            ]}
            rows={rules}
            selectedIndex={sel}
            onRowClick={(i) => setSel(i)}
            style={{ minWidth: 720 }}
          />
          {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING POLICY SET…</div>}
        </div>
      )}

      <div>
        <SectionLabel number={9} muted>POLICY DRY-RUN · DENY BY DEFAULT</SectionLabel>
        <Card size="md" accentRule>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
            <div style={{ display: 'flex', alignItems: 'flex-end', gap: 12, flexWrap: 'wrap' }}>
              <Select
                label="Purpose"
                options={purposes.map((pu) => ({ value: pu, label: pu }))}
                value={purpose}
                onChange={(e) => setPurpose(e.target.value)}
                size="sm"
              />
              <Select
                label="Marking"
                options={MARKINGS.map((m) => ({ value: m, label: m }))}
                value={marking}
                onChange={(e) => setMarking(e.target.value as MarkingLevel)}
                size="sm"
              />
              <div style={{ flex: 1 }} />
              <Button variant="primary" iconLeft={<Icon name="play" size={15} />} onClick={busy ? undefined : runDryRun}>
                Evaluate
              </Button>
            </div>

            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 18, fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
              <span>SUBJECT&nbsp;&nbsp;<span style={{ color: 'var(--grey-100)' }}>{ctx.subject}</span></span>
              <span>CLEARANCE&nbsp;&nbsp;<span style={{ color: 'var(--grey-100)' }}>{ctx.clearance.toUpperCase()}</span></span>
              <span>ROLES&nbsp;&nbsp;<span style={{ color: 'var(--grey-100)' }}>[{ctx.roles.join(', ')}]</span></span>
            </div>

            {decision ? (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 10, padding: 14, borderRadius: 'var(--radius-md)', background: granted ? 'var(--surface-inset)' : 'var(--surface-inset)', border: `1px solid ${granted ? 'var(--secure)' : 'var(--danger)'}` }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                  <Badge tone={granted ? 'secure' : 'danger'}>{decision.decision.toUpperCase()}</Badge>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
                    {marking.toUpperCase()} · {purpose}
                  </span>
                  <div style={{ flex: 1 }} />
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>MATCHED RULE&nbsp;&nbsp;{decision.matchedRule ?? 'none — deny-by-default'}</span>
                </div>
                <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}>{decision.reason}</div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.1em', color: 'var(--text-faint)' }}>
                  DECISION AUDITED · ML-DSA SIGNED · #{decision.auditId}
                </div>
              </div>
            ) : (
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>
                NO ALLOW RULE MATCHES UNLESS PROVEN · EVALUATE TO SEE THE SIGNED DECISION
              </div>
            )}
          </div>
        </Card>
      </div>
    </ConsolePage>
  );
}
