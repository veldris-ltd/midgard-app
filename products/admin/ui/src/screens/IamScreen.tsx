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
  StatusPill,
  Table,
  Tabs,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { AdminBinding, AdminGateway, AdminRole } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Identity & Roles — role-separated admin RBAC: scoped, expiring, NO
 * super-admin (AC-AD-2). Reproduces docs/design/ui_kits/admin/iam.html; data
 * from `listRoles()` + `listBindings()`. */
export function IamScreen() {
  const { data: roles } = useAsync<AdminRole[], AdminGateway>((c) => c.listRoles(), []);
  const { data: bindings } = useAsync<AdminBinding[], AdminGateway>((c) => c.listBindings(), []);
  const [tab, setTab] = React.useState(0);
  const [sel, setSel] = React.useState(1);

  const roleRows: AdminRole[] = roles ?? [];
  const bindingRows: AdminBinding[] = bindings ?? [];
  const r = roleRows[sel];

  const canCannot: Array<readonly [string, boolean]> = r
    ? r.readonly
      ? [['Read all audit + config history', true], ['Export evidence for review', true], ['Mutate any state', false]]
      : [['Administer ' + r.scope, true], ['Approve as second controller', true], ['Act outside scope', false], ['Self-approve own requests', false]]
    : [];

  const aside = r ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="fingerprint" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], fontSize: 14, flex: 1 }}>{r.name}</h2>
        {r.readonly ? <Badge tone="info">READ-ONLY</Badge> : <RiskBadge level={r.ceiling} />}
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={String(r.holders)} label="HOLDERS" size="sm" />
          <StatBlock value={r.readonly ? 'NONE' : 'R' + r.ceiling} label="RISK CEILING" size="sm" accent={r.ceiling >= 5} />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>CAPABILITY SPLIT · NO SUPER-ADMIN</div>
        <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>No role administers more than its column. Cross-column actions require the other column&apos;s holder as second approver, so compromise of one admin never spans the suite.</p>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>{r.name.toUpperCase()} · CAN / CANNOT</div>
        {canCannot.map(([c, ok]) => (
          <div key={c} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '6px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <Icon name={ok ? 'check' : 'x'} size={13} color={ok ? 'var(--secure)' : 'var(--danger)'} />{c}
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-AD-3" title="Manage users, groups, roles, markings, purposes and policies with validation" />
        <ProvenanceTag id="FR-AD-2" title="Role separation, expiring bindings, read-only Auditor, no all-powerful role" />
        <ProvenanceTag id="AC-AD-2" title="No single role grants all capabilities" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="iam"
      context="ADMIN / IDENTITY & ROLES"
      toolbar={<><Button variant="primary" size="sm" iconLeft={<Icon name="plus" size={14} />}>New binding</Button><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={4}>IDENTITY</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Admin RBAC</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(roleRows.length || 5)} label="SEPARATED ROLES" size="sm" />
        <StatBlock value="13" label="LIVE BINDINGS · ALL EXPIRING" size="sm" />
        <StatBlock value="0" label="SUPER-ADMIN ROLES" size="sm" accent />
      </div>

      <Tabs
        tabs={[{ label: 'Roles', badge: String(roleRows.length || 5) }, { label: 'Bindings', badge: '13' }]}
        active={tab}
        onChange={setTab}
      />

      <div style={{ overflowX: 'auto' }}>
        {tab === 0 ? (
          <Table<AdminRole>
            columns={[
              { header: 'Role', cell: (x) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="shield-check" size={14} color="var(--accent)" />{x.name}</span> },
              { header: 'Scope', key: 'scope', width: 240 },
              { header: 'Risk ceiling', cell: (x) => (x.readonly ? <Badge tone="info">READ-ONLY</Badge> : <RiskBadge level={x.ceiling} dualControl={false} />), width: 150 },
              { header: 'Holders', key: 'holders', mono: true, align: 'right', width: 70 },
              { header: 'State', cell: (x) => <StatusPill status={x.status} pulse={false} label={x.statusLabel} />, width: 130 },
            ]}
            rows={roleRows}
            selectedIndex={sel}
            onRowClick={(i) => setSel(i)}
            style={{ minWidth: 760 }}
          />
        ) : (
          <Table<AdminBinding>
            columns={[
              { header: 'Admin', key: 'user', mono: true },
              { header: 'Role', key: 'role', mono: true, width: 150 },
              { header: 'Scope', key: 'scope', mono: true, width: 140 },
              { header: 'Expires', key: 'expires', mono: true, align: 'right', width: 80 },
              { header: 'MFA', key: 'mfa', mono: true, width: 160 },
              { header: 'State', cell: (x) => <StatusPill status={x.status} pulse={false} label="Bound" />, width: 100 },
            ]}
            rows={bindingRows}
            style={{ minWidth: 760 }}
          />
        )}
      </div>

      <Terminal
        title="STEP-UP · AC-AD-1"
        lines={[
          '> session inspect j.benjamin',
          'login      WebAuthn passkey ✓',
          'admin scope entered → step-up re-auth ✓ (2m ago)',
          'high-risk approval → fresh re-auth REQUIRED at moment of approval',
        ]}
      />
    </ConsolePage>
  );
}
