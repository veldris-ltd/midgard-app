import * as React from 'react';
import {
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  Select,
  StatBlock,
  StatusPill,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { MimirModel, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Model plane — in-enclosure serving of open-weight families, exposed to other
 * products over internal PQ-mTLS. Weights are ML-DSA-verified on load (fails
 * closed) and never leave the enclosure. `[FR-MI-1, FR-MI-8, DD-4]` */

const CONSUMERS = [
  { c: 'mimir/agents', u: 'OAG chat, dispatch, triage', m: 'Mistral Small 3.2', n: '4,812' },
  { c: 'forge/assist', u: 'NL→transform, drift diagnosis', m: 'Mistral Small 3.2', n: '210' },
  { c: 'bifrost/risk', u: 'Release risk advisories', m: 'Nemotron', n: '38' },
  { c: 'heimdall/options', u: 'Ranked courses of action', m: 'Mistral Small 3.2', n: '96' },
  { c: 'mcp/tools', u: 'Governed agent/tool access to the fabric (MCP)', m: 'Mistral Small 3.2', n: '1,204' },
];

export function ModelPlaneScreen() {
  const { data: models } = useAsync<MimirModel[], MimirGateway>((c) => c.listModels(), []);
  const rows: MimirModel[] = models ?? [];

  const [sel, setSel] = React.useState(0);
  const m = rows[sel];

  const aside = m ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="cpu" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, flex: 1 }}>{m.name}</h2>
        <StatusPill status={m.status} pulse={m.status === 'healthy'} label={m.statusLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={m.tps} label="THROUGHPUT" size="sm" />
          <StatBlock value={m.live ? (m.live.firstTokenMs != null ? `${m.live.firstTokenMs} ms` : '—') : '0.42s'} label={m.live ? 'FIRST TOKEN · AVERAGE' : 'FIRST TOKEN · NFR-PERF-1'} size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>WEIGHTS PROVENANCE</div>
        {(m.live ? [
          ['Source', `${m.live.file ?? 'weights'}${m.live.sizeBytes ? ` · ${(m.live.sizeBytes / 1e9).toFixed(2)} GB` : ''}`],
          ['Digest', `sha256:${m.live.sha256.slice(0, 12)}…${m.live.sha256.slice(-4)}`],
          ['Licence', `${m.live.licence}${m.live.osiApproved ? ' · OSI' : ''}${m.live.redistributable ? ' · redistributable' : ''}`],
          ['Signature', `${m.live.signature} · fails closed`],
          ['Serving', `${m.live.engine} sidecar · ${m.live.sidecarHealthy === false ? 'DOWN' : 'healthy'}`],
          ['Node', m.live.node],
          ['Egress', 'none · weights never leave'],
        ] : [
          ['Source', 'open weights · vendored'],
          ['Digest', 'sha3:77aa…41c9'],
          ['Signature', 'ML-DSA verified on load · fails closed'],
          ['Serving', 'continuous batching · KV-cache'],
          ['Egress', 'none · weights never leave'],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 76, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--grey-100)' }}>{v}</span>
          </div>
        ))}
      </div>
      {m.live ? (
        <Terminal title="LIVE · SINCE THE PLANE STARTED" lines={[
          '> mimir models describe',
          `calls    ${m.live.calls} · served ${m.live.served} · failed ${m.live.failed} · denied ${m.live.denied}`,
          `latency  first token ${m.live.firstTokenMs ?? '—'} ms · total ${m.live.avgTotalMs != null ? (m.live.avgTotalMs / 1000).toFixed(1) + ' s' : '—'} (avg)`,
          `ledger   ${m.live.ledgerEntries} PQ-signed entries · one per call`,
        ]} />
      ) : <Terminal title="CANARY PROMPTS" lines={['> mimir models canary ' + m.name.toLowerCase().replace(' ', '-'), 'grounding   24/24 OK', 'refusals    18/18 OK', 'latency     p99 0.61s', '# canary battery clean']} />}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="DD-4" title="Local AI processing via the Mimir layer" />
        <ProvenanceTag id="FR-MI-8" title="Expose inference to Forge, Bifrost, Heimdall over internal PQ-mTLS" />
        <ProvenanceTag id="FR-MI-1" title="Serve open-weight model families in-enclosure" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="models"
      context="MIMIR / MODEL PLANE"
      toolbar={<Select size="sm" options={['route: capability', 'route: latency', 'route: pinned']} value="route: capability" onChange={() => {}} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={4}>MODEL PLANE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Model serving</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(rows.length || '—')} label={rows[0]?.live ? 'SERVED NOW' : 'FAMILIES'} size="sm" />
        <StatBlock value={rows[0]?.live ? rows[0].gpu : '8×'} label={rows[0]?.live ? 'HARDWARE' : 'GPU POOL'} size="sm" />
        <StatBlock value="0" label="EXTERNAL CALLS" size="sm" accent />
      </div>

      <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
        {rows.map((mm, i) => (
          <Card key={mm.name} size="md" interactive accentRule={i === sel} onClick={() => setSel(i)} style={{ flex: '1 1 220px', borderColor: i === sel ? 'var(--accent)' : undefined }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <Icon name="cpu" size={16} color="var(--accent)" />
              <span style={{ fontFamily: 'var(--font-display)', fontWeight: 700, fontSize: 17, color: 'var(--text-heading)' }}>{mm.name}</span>
              <span style={{ marginLeft: 'auto' }}><StatusPill status={mm.status} pulse={false} label={mm.statusLabel} /></span>
            </div>
            <p style={{ margin: '10px 0 12px', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{mm.role}</p>
            <div style={{ display: 'flex', gap: 16, fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', flexWrap: 'wrap' }}>
              <span>{mm.quant}</span><span>CTX {mm.ctx}</span><span>{mm.gpu}</span><span>{mm.tps}</span>
            </div>
          </Card>
        ))}
      </div>

      <div>
        <SectionLabel number={5} muted>CONSUMERS · BY PRODUCT</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table
            compact
            columns={[
              { header: 'Consumer', key: 'c', mono: true },
              { header: 'Use', key: 'u' },
              { header: 'Model', key: 'm', mono: true, width: 110 },
              { header: rows[0]?.live ? 'Calls · since start' : 'Calls · 24h', key: 'n', mono: true, align: 'right', width: 120 },
            ]}
            rows={rows[0]?.live
              ? rows[0].live.consumers.map((c) => ({ c: c.caller, u: `purpose: ${c.purposes.join(', ')} · last ${new Date(c.lastAt * 1000).toISOString().slice(11, 16)}Z`, m: rows[0].name, n: String(c.calls) }))
              : CONSUMERS}
            style={{ minWidth: 640 }}
          />
        </div>
      </div>
    </ConsolePage>
  );
}
