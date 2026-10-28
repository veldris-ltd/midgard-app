import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { RpcSurface } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { SampleData } from '../app/SampleData';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

type Rpc = RpcSurface['rpcs'][number];
type Lang = 'ts' | 'py' | 'cli';

const SNIPPETS: Record<Lang, string[]> = {
  ts: [
    'import { fabric } from "@midgard/sdk";',
    '',
    'const risky = await fabric.objects("aircraft")',
    '  .where(a => a.riskScore.gt(0.6))',
    '  .searchAround("maintenanceEvents")',
    '  .fetch(); // policy applied at query time',
  ],
  py: [
    'from midgard import fabric',
    '',
    'risky = (fabric.objects("aircraft")',
    '  .where(lambda a: a.risk_score > 0.6)',
    '  .search_around("maintenance_events")',
    '  .fetch())  # policy applied at query time',
  ],
  cli: [
    '> midgard query --type aircraft \\',
    '    --where "risk_score > 0.6" \\',
    '    --search-around maintenance_events',
    '# 14 objects · purpose: maintenance · audited',
  ],
};

/* SDK & API surface. The contract surface is fixed example data from the gateway,
 * labelled SAMPLE DATA; the SDKs ship in the repository, so Download SDKs is disabled
 * and says where they are. [AC-M-4] */
export function SdkScreen() {
  const { data, loading } = useAsync((c) => c.getContractSurface(), []);
  const surfaces: RpcSurface[] = data ?? [];

  const [sel, setSel] = React.useState(0);
  const [lang, setLang] = React.useState<Lang>('ts');

  const rpcCount = surfaces.reduce((n, s) => n + s.rpcs.length, 0);
  const s = surfaces[sel];

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="code" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1 }}>{s.service}</h2>
        <StatusPill status="online" pulse label="Serving" />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.rpcs.length} label="RPCS" size="sm" />
          <StatBlock value="100%" label="POLICY-CHECKED" size="sm" accent />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>EVERY CALL</div>
        {['PQ-mTLS · workload identity', 'Role / marking / purpose evaluated', 'Deny-by-default · audited', 'Generated, typed clients'].map((l, i) => (
          <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '6px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <Icon name="check" size={13} color="var(--secure)" />{l}
          </div>
        ))}
      </div>
      <div style={{ marginTop: 'auto' }}>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROVENANCE</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <ProvenanceTag id="DD-6" title="Ports & adapters: products and developers depend on Midgard contracts, not implementations" />
          <ProvenanceTag id="STD:NIST FIPS 203/204" kind="std" title="ML-KEM-1024 (FIPS 203) key wrap + ML-DSA (FIPS 204) signatures on the wire" />
        </div>
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="sdk"
      context="ENCLOSURE / SDK & APIs"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={11}>SDK & APIS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>SDK & API surface <SampleData /></h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={surfaces.length} label="SERVICES" size="sm" />
        <StatBlock value={rpcCount} label="RPCS" size="sm" />
        <StatBlock value="TS · PY · RS · GO" label="SDK TARGETS" size="sm" accent />
        <Button variant="primary" iconLeft={<Icon name="download" size={15} />} disabled title="Not available yet: the SDKs ship in the repository (sdk/)">Download SDKs</Button>
      </div>

      {surfaces.map((svc, si) => (
        <Card key={svc.service} accentRule={si === sel} onClick={() => setSel(si)} interactive>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 12 }}>
            <Icon name="code" size={15} color="var(--accent)" />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-sm)', color: 'var(--text-heading)' }}>{svc.service}</span>
            <span style={{ flex: 1 }} />
            <StatusPill status="online" pulse={false} label={`${svc.rpcs.length} RPCS`} />
          </div>
          <div style={{ overflowX: 'auto' }}>
            <Table<Rpc>
              compact
              columns={[
                { header: 'RPC', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="zap" size={13} color="var(--accent)" />{r.name}</span>, width: 180 },
                { header: 'Request', key: 'request', mono: true, width: 180 },
                { header: 'Response', key: 'response', mono: true, width: 180 },
                { header: 'Note', key: 'note' },
              ]}
              rows={svc.rpcs}
              style={{ minWidth: 720 }}
            />
          </div>
        </Card>
      ))}
      {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RESOLVING CONTRACT SURFACE…</div>}

      <div>
        <SectionLabel number={12} muted>TYPED CLIENT · EXAMPLE</SectionLabel>
        <div style={{ display: 'flex', gap: 8, margin: '10px 0 8px' }}>
          {(['ts', 'py', 'cli'] as Lang[]).map((k) => (
            <Tag key={k} active={lang === k} onClick={() => setLang(k)}>
              {k === 'ts' ? 'TypeScript' : k === 'py' ? 'Python' : 'CLI'}
            </Tag>
          ))}
        </div>
        <Terminal title={`TYPED CLIENT · ${lang.toUpperCase()}`} lines={SNIPPETS[lang]} />
        <div style={{ marginTop: 8, fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-faint)' }}>
          CODEGEN · buf generate from contracts/proto/** · transport gRPC over PQ-mTLS
        </div>
      </div>
    </ConsolePage>
  );
}
