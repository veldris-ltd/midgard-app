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
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { Identity } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { SampleData } from '../app/SampleData';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Identity & access: users, service identities, and agent identities with their
 * clearance marking, role/group grants, and session state — sourced from
 * `listIdentities()`. Zero standing privilege; every call re-authorised.
 * The gateway serves this view from fixed example data, so the page is labelled
 * SAMPLE DATA, and its controls are disabled and say why. [AC-M-4] */
const KIND_ICON: Record<Identity['kind'], string> = { user: 'user', service: 'server', agent: 'bot' };

export function IdentityScreen() {
  const { data, loading } = useAsync((c) => c.listIdentities(), []);
  const rows: Identity[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const u = rows[sel];

  const total = rows.length;
  const active = rows.filter((r) => r.status === 'active').length;
  const agents = rows.filter((r) => r.kind === 'agent').length;

  const aside = u ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name={KIND_ICON[u.kind]} size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{u.subject}</h2>
        <StatusPill status={u.status === 'active' ? 'online' : 'warn'} pulse={false} label={u.status === 'active' ? 'Active' : 'Suspended'} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={u.kind.toUpperCase()} label="KIND" size="sm" />
          <StatBlock value={u.roles.length} label="ROLES" size="sm" accent={u.kind === 'agent'} />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>CLEARANCE</div>
        <MarkingChip level={u.clearance} />
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>GRANTS · ROLES</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {u.roles.map((r) => <Badge key={r} tone="info">{r}</Badge>)}
        </div>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>GROUPS</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {u.groups.map((g) => <Tag key={g}>{g}</Tag>)}
        </div>
      </div>
      <Terminal title="TOKEN" lines={['> midgard token inspect', 'alg     ML-DSA-65', 'subject ' + u.subject, '# zero-trust: re-authorised per call']} />
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="NFR-PRV-1" title="Every identity and access decision is provenance-tracked" />
          <ProvenanceTag id="NFR-SEC-2" title="Role/marking/purpose enforced at query time" />
        </div>
      </div>
      <div style={{ display: 'flex', gap: 8, marginTop: 'auto' }}>
        <Button variant="secondary" size="sm" fullWidth disabled title="Not available yet: grants are managed with mg-admin">Edit grants</Button>
        <Button variant="outline" size="sm" fullWidth disabled title="Not available yet: sessions end when their token expires">Revoke session</Button>
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="identity"
      context="ENCLOSURE / IDENTITY & ACCESS"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={5}>IDENTITY & ACCESS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>Identities <SampleData /></h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={total} label="SUBJECTS" size="sm" />
        <StatBlock value={active} label="ACTIVE" size="sm" accent />
        <StatBlock value={agents} label="AGENTS" size="sm" />
        <Button variant="primary" iconLeft={<Icon name="plus" size={15} />} disabled title="Not available yet: identities are enrolled with mg-admin user add">New identity</Button>
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<Identity>
          columns={[
            { header: 'Identity', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name={KIND_ICON[r.kind]} size={14} color="var(--accent)" />{r.subject}</span> },
            { header: 'Kind', key: 'kind', mono: true, width: 90 },
            { header: 'Clearance', cell: (r) => <MarkingChip level={r.clearance} />, width: 170 },
            { header: 'Roles', cell: (r) => <span style={{ display: 'flex', gap: 5, flexWrap: 'wrap' }}>{r.roles.map((x) => <Badge key={x} tone="info">{x}</Badge>)}</span>, width: 190 },
            { header: 'Groups', cell: (r) => r.groups.join(' · '), mono: true, width: 130 },
            { header: 'Last seen', key: 'lastSeen', mono: true, width: 100 },
            { header: 'Status', cell: (r) => <StatusPill status={r.status === 'active' ? 'online' : 'warn'} pulse={false} label={r.status === 'active' ? 'Active' : 'Suspended'} />, width: 130 },
          ]}
          rows={rows}
          selectedIndex={sel}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 840 }}
        />
        {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RESOLVING IDENTITIES…</div>}
      </div>
    </ConsolePage>
  );
}
