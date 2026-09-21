import * as React from 'react';
import {
  BreakGlass,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { AdminGateway, AdminOverview, AdminSubsystem } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Overview — unified oversight: subsystem health, SLOs, security events,
 * pending controls. Reproduces docs/design/ui_kits/admin/index.html; data from
 * `getOverview()`. Governance invariant on show: super-admin is NOT DEFINED. */
export function OverviewScreen() {
  const g = useGateway<AdminGateway>();
  const { data, loading } = useAsync<AdminOverview, AdminGateway>((c) => c.getOverview(), []);
  const [sel, setSel] = React.useState(4);

  const subs: AdminSubsystem[] = data?.subsystems ?? [];
  const s = subs[sel];

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="server" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1 }}>{s.name}</h2>
        <StatusPill status={s.status} pulse={false} label={s.statusLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.slo} label="SLO · 30D" size="sm" />
          <StatBlock value={String(s.openFindings)} label="OPEN FINDINGS" size="sm" accent={s.openFindings > 0} />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PANEL BINDING · LEAST PRIVILEGE</div>
        {([
          ['Identity', 'svc:admin/' + s.api],
          ['Scope', 'admin API only · no data plane'],
          ['Transport', 'PQ-mTLS · admin zone'],
          ['Blast radius', 'this subsystem only'],
        ] as const).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 90, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--text-heading)' }}>{v}</span>
          </div>
        ))}
      </div>
      <Terminal
        title="AR-SEC · SELF-GOVERNANCE"
        lines={[
          '> admin policy self-check',
          'policy point   shared · no bypass',
          'audit path     shared ledger',
          'super-admin    NOT DEFINED',
          '# the panel is governed by what it governs',
        ]}
        style={{ marginTop: 'auto' }}
      />
    </>
  ) : null;

  return (
    <ConsolePage
      active="overview"
      context="ADMIN / OVERVIEW"
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> admin status', '# 7 SUBSYSTEMS · SELF-GOVERNED']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>ADMIN</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Suite oversight</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(subs.length || 7)} label="SUBSYSTEMS" size="sm" />
        <StatBlock value={String(data?.pendingApprovals ?? 0)} label="PENDING APPROVALS" size="sm" accent />
        <StatBlock value={String(data?.breakGlassReview ?? 0)} label="BREAK-GLASS · REVIEW" size="sm" />
        <StatBlock value={String(data?.superAdminRoles ?? 0)} label="SUPER-ADMIN ROLES" size="sm" />
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        <BreakGlass
          state="review"
          holder="k.osei"
          scope="KMS · unwrap only"
          justification="HSM failover during rotation window · expired 06:12 · review due in 18h"
          style={{ flex: 2, minWidth: 320 }}
        />
        <Card size="sm" style={{ flex: 1, minWidth: 220 }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>SECURITY EVENTS · 24H</div>
          <div style={{ display: 'flex', gap: 16, marginTop: 10 }}>
            <StatBlock value={String(data?.security.deniedActions ?? 0)} label="DENIED ACTIONS" size="sm" />
            <StatBlock value={String(data?.security.selfApprovals ?? 0)} label="SELF-APPROVALS" size="sm" />
            <StatBlock value={String(data?.security.anomalyFlags ?? 0)} label="ANOMALY FLAGS" size="sm" accent={(data?.security.anomalyFlags ?? 0) > 0} />
          </div>
        </Card>
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<AdminSubsystem>
          columns={[
            { header: 'Subsystem', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="server" size={14} color="var(--accent)" />{r.name}</span> },
            { header: 'Admin surface', key: 'api', mono: true, width: 160 },
            { header: 'SLO · 30d', key: 'slo', mono: true, align: 'right', width: 100 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={false} label={r.statusLabel} />, width: 130 },
          ]}
          rows={subs}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 640 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>AGGREGATING SUBSYSTEMS…</div>}
      </div>

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
        <ProvenanceTag id="FR-AD-1" title="One console composing existing subsystem admin APIs" />
        <ProvenanceTag id="FR-AD-9" title="Aggregated cross-subsystem observability" />
        <ProvenanceTag id="AC-AD-10" title="Panel outage pauses administration only, never the data plane" />
      </div>

      <Button variant="secondary" size="sm" iconLeft={<Icon name="search" size={14} />} style={{ alignSelf: 'flex-start' }}>Audit search</Button>
    </ConsolePage>
  );
}
