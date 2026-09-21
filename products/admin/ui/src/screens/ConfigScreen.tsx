import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  RiskBadge,
  SectionLabel,
  StatBlock,
  Table,
  Terminal,
  type BadgeProps,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { AdminConfigCommit, AdminConfigSimulation, AdminConfigState, AdminGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Config & Policy — GitOps-style versioned config: diff → simulate →
 * review → commit → rollback, every step signed (FR-AD-6/7, AC-AD-7). Reproduces
 * docs/design/ui_kits/admin/config.html; data from `listConfigCommits()` +
 * `simulateConfig/commitConfig/rollbackConfig`. */

const STATE_TONE: Record<AdminConfigState, NonNullable<BadgeProps['tone']>> = {
  committed: 'secure',
  'in review': 'warn',
  simulated: 'info',
  'rolled-back': 'neutral',
};

export function ConfigScreen() {
  const g = useGateway<AdminGateway>();
  const { data } = useAsync<AdminConfigCommit[], AdminGateway>((c) => c.listConfigCommits(), []);
  const [sel, setSel] = React.useState(0);
  const [sim, setSim] = React.useState<AdminConfigSimulation | null>(null);
  const [committedAt, setCommittedAt] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);

  const rows: AdminConfigCommit[] = data ?? [];
  const c = rows[sel];

  // Reset the simulate/commit surface whenever the selected change changes.
  React.useEffect(() => {
    setSim(null);
    setCommittedAt(null);
  }, [sel]);

  const runSim = async () => {
    if (!c) return;
    setBusy(true);
    try {
      setSim(await g.simulateConfig(c.id));
    } finally {
      setBusy(false);
    }
  };

  const commit = async () => {
    if (!c) return;
    setBusy(true);
    try {
      const res = await g.commitConfig(c.id);
      setCommittedAt(res.version);
    } finally {
      setBusy(false);
    }
  };

  const aside = c ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="git-branch" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], fontSize: 14, flex: 1 }}>{c.id}</h2>
        <RiskBadge level={c.risk} />
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>DIFF · VERSIONED LIKE SOURCE</div>
        <Terminal lines={c.diff} />
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)', marginTop: 6 }}>CONTENT-HASH · {c.contentHash}</div>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>SIMULATION · BEFORE COMMIT</div>
        <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--warn)', marginBottom: 6 }}>EFFECT · {sim ? 'SIMULATED' : 'WOULD GRANT'}</div>
          {sim ? (
            <>
              <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.6 }}>{sim.summary}</p>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)', marginTop: 8, lineHeight: 1.7 }}>
                {sim.grants.map((x, i) => <div key={'g' + i} style={{ color: 'var(--secure)' }}>+ {x}</div>)}
                {sim.revokes.map((x, i) => <div key={'r' + i} style={{ color: 'var(--danger)' }}>- {x}</div>)}
              </div>
            </>
          ) : (
            <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.6 }}>
              Grants <b style={{ color: 'var(--text-heading)' }}>14 users</b> read on <b style={{ color: 'var(--text-heading)' }}>INTERNAL-marked</b> maintenance data. No marking exposure above clearance. Reversible. Blast radius: one dataset family.
            </p>
          )}
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 8.5, color: 'var(--text-faint)', marginTop: 7 }}>SIM ENGINE · EXPLAINED BY COPILOT · ADVISORY [FR-AD-10]</div>
        </Card>
      </div>
      {committedAt ? (
        <div style={{ padding: '10px 12px', background: 'var(--surface-inset)', border: '1px solid var(--secure)', borderRadius: 'var(--radius-md)', fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', color: 'var(--secure)' }}>COMMITTED · {committedAt.toUpperCase()} · SIGNED · ROLLBACK AVAILABLE</div>
      ) : (
        <div style={{ display: 'flex', gap: 8 }}>
          <Button variant="secondary" size="sm" fullWidth onClick={busy ? undefined : runSim}>{sim ? 'Re-simulate' : 'Simulate'}</Button>
          <Button variant="primary" size="sm" fullWidth onClick={busy ? undefined : commit}>Commit</Button>
        </div>
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-AD-6" title="Policy simulation and dry-run before commit" />
        <ProvenanceTag id="FR-AD-7" title="Versioned config with diff, review, rollback, signed audit" />
        <ProvenanceTag id="AC-AD-7" title="Committed changes are versioned and roll-backable" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="config"
      context="ADMIN / CONFIG & POLICY"
      asideWidth={360}
      toolbar={<><Button variant="secondary" size="sm" iconLeft={<Icon name="undo-2" size={14} />} onClick={busy || !c ? undefined : () => c && g.rollbackConfig(c.id)}>Rollback…</Button><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={5}>CONFIG</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Config as code</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="v481" label="CURRENT · ALL SUBSYSTEMS" size="sm" />
        <StatBlock value="100%" label="CHANGES SIMULATED FIRST" size="sm" accent />
        <StatBlock value="4" label="ROLLBACKS · 90D" size="sm" />
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<AdminConfigCommit>
          columns={[
            { header: 'Change', cell: (x) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="git-commit" size={14} color="var(--accent)" />{x.title}</span> },
            { header: 'Id', key: 'id', mono: true, width: 100 },
            { header: 'Subsystem', key: 'subsystem', mono: true, width: 120 },
            { header: 'Risk', cell: (x) => <RiskBadge level={x.risk} dualControl={false} />, width: 110 },
            { header: 'Author', key: 'author', mono: true, width: 110 },
            { header: 'State', cell: (x) => <Badge tone={STATE_TONE[x.state]}>{x.state.toUpperCase()}</Badge>, width: 120 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 760 }}
        />
      </div>

      <Terminal
        title="DRIFT RECONCILIATION"
        lines={[
          '> admin config drift --all',
          'iam         in sync · v481',
          'governance  in sync · v481',
          'bifrost     DRIFT · channel beta-eu manual edit → reconcile queued',
          '# drift alerts page the owning admin [FR-AD-7]',
        ]}
      />
    </ConsolePage>
  );
}
