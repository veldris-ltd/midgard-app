import * as React from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  Badge,
  Button,
  Card,
  Icon,
  Input,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  Select,
  StatBlock,
  Table,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { GraphEdge, GraphNode, ObjectType } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Search-around · query engine. An interactive traversal: pick a seed object and a
 * hop depth, run `searchAround(seedId, hops)` through the gateway, and render the
 * returned SearchGraph grouped by hop with each node's own properties. What is shown
 * is always the traversal that was run: the heading names its seed, a failed run shows
 * the gateway's error instead of the previous results, and nothing on screen is a
 * placeholder. [DD-1] [NFR-SEC-2] */

/** One traversal as it was run, so typing a new seed changes nothing until Run. */
interface Traversal {
  seedType: string;
  seedId: string;
  hops: number;
}

export function SearchAroundScreen() {
  const { data: types } = useAsync((c) => c.listObjectTypes(), []);
  const objectTypes: ObjectType[] = types ?? [];

  // "Explore instances" on Object types opens this page as /search?type=<apiName>:
  // every object of that type, one hop out.
  const [params] = useSearchParams();
  const explore = params.get('type');
  const first: Traversal = explore
    ? { seedType: explore, seedId: '', hops: 1 }
    : { seedType: 'aircraft', seedId: 'AC-2214', hops: 2 };
  const [seedType, setSeedType] = React.useState(first.seedType);
  const [seedId, setSeedId] = React.useState(first.seedId);
  const [hops, setHops] = React.useState(first.hops);
  const [run, setRun] = React.useState<Traversal>(first);
  const [selId, setSelId] = React.useState<string | null>(null);

  // Re-runs only when a traversal is requested: the button drives the query, not
  // every keystroke.
  const seed = `${run.seedType}/${run.seedId || '*'}`;
  const { data: result, loading, error } = useAsync((c) => c.searchAround(seed, run.hops), [run]);
  // While a run is in flight, or after it failed, the previous run's results are not
  // this one's: show none.
  const graph = loading || error ? null : result;

  const nodes: GraphNode[] = graph?.nodes ?? [];
  const edges: GraphEdge[] = graph?.edges ?? [];
  const hopLevels = Array.from(new Set(nodes.map((n) => n.hop))).sort((a, b) => a - b);
  const node = nodes.find((n) => n.id === selId) ?? nodes[0];

  const aside = node ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="boxes" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{node.label}</h2>
        <MarkingChip level={node.marking} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={node.typeName.toUpperCase()} label="OBJECT TYPE" size="sm" />
          <StatBlock value={String(node.hop)} label="HOPS FROM ROOT" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROPERTIES</div>
        {[['object_id', node.id] as const, ...Object.entries(node.propertyValues ?? {})].map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 104, flex: 'none', overflowWrap: 'anywhere' }}>{k}</span>
            <span style={{ color: 'var(--grey-100)', overflowWrap: 'anywhere' }}>{v}</span>
          </div>
        ))}
      </div>
      <Terminal
        title="QUERY · AUDITED"
        lines={[
          `> fabric.searchAround("${seed}")`,
          `hops      ${run.hops} · links ${edges.length} · objects ${nodes.length}`,
          'policy    per object, at query time',
          `# decision audited · #${graph?.auditId ?? '—'}`,
        ]}
      />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="DD-1" title="Typed query, search-around, and action APIs on the object fabric" />
        <ProvenanceTag id="NFR-SEC-2" title="Role/marking/purpose enforced at query time" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="search"
      context="ENCLOSURE / SEARCH-AROUND"
      asideWidth={340}
      toolbar={<ConsoleToolbar placeholder="Search objects, links, actions…" />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={13}>QUERY ENGINE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Search-around · {run.seedId || `every ${run.seedType}`}</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(nodes.length)} label="OBJECTS" size="sm" />
        <StatBlock value={`${edges.length} · ${run.hops} HOPS`} label="LINKS" size="sm" />
      </div>

      <Card size="sm">
        <div style={{ display: 'flex', alignItems: 'flex-end', gap: 12, flexWrap: 'wrap' }}>
          <Select
            label="Seed object type"
            options={objectTypes.map((t) => ({ value: t.apiName, label: t.name }))}
            value={seedType}
            onChange={(e) => setSeedType(e.target.value)}
            size="sm"
          />
          <Input
            label="Seed id"
            mono
            size="sm"
            value={seedId}
            onChange={(e) => setSeedId(e.target.value)}
            placeholder="AC-2214"
            style={{ width: 160 }}
          />
          <Select
            label="Hops"
            options={[{ value: '1', label: '1 hop' }, { value: '2', label: '2 hops' }, { value: '3', label: '3 hops' }]}
            value={String(hops)}
            onChange={(e) => setHops(Number(e.target.value))}
            size="sm"
          />
          <div style={{ flex: 1 }} />
          <Button
            variant="primary"
            iconLeft={<Icon name="search" size={15} />}
            onClick={() => { setRun({ seedType, seedId, hops }); setSelId(null); }}
          >
            Run traversal
          </Button>
        </div>
      </Card>

      {loading && (
        <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>
          EXPANDING LINKS · POLICY AT QUERY TIME…
        </div>
      )}

      {error && (
        <div
          role="alert"
          style={{ padding: '12px 14px', background: 'var(--surface-inset)', border: '1px solid var(--danger)', borderRadius: 'var(--radius-md)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--danger)', overflowWrap: 'anywhere' }}
        >
          {error.message}
        </div>
      )}

      {hopLevels.map((hop) => {
        const group = nodes.filter((n) => n.hop === hop);
        return (
          <div key={hop}>
            <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>
              {hop === 0 ? 'SEED · ROOT' : `HOP ${hop}`} · {group.length} OBJECT{group.length === 1 ? '' : 'S'}
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 10 }}>
              {group.map((n) => (
                <Card
                  key={n.id}
                  size="sm"
                  interactive
                  accentRule={n.id === node?.id}
                  onClick={() => setSelId(n.id)}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 9, marginBottom: 8 }}>
                    <Icon name="boxes" size={14} color="var(--accent)" />
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', flex: 1 }}>{n.label}</span>
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <MarkingChip level={n.marking} />
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)' }}>{n.typeName}</span>
                  </div>
                </Card>
              ))}
            </div>
          </div>
        );
      })}

      <div>
        <SectionLabel number={14} muted>LINK TRAVERSALS · {edges.length}</SectionLabel>
        <div style={{ overflowX: 'auto', marginTop: 8 }}>
          <Table<GraphEdge>
            compact
            columns={[
              { header: 'From', key: 'from', mono: true, width: 200 },
              { header: 'Link', cell: (r) => <Badge tone="info">{r.label}</Badge> },
              { header: 'To', key: 'to', mono: true, width: 160 },
            ]}
            rows={edges}
            style={{ minWidth: 560 }}
          />
        </div>
      </div>

      {graph && (
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.12em', color: 'var(--text-faint)' }}>
          SEARCH-AROUND · {run.hops} HOPS · POLICY AT QUERY TIME · DECISION AUDITED #{graph.auditId}
        </div>
      )}
    </ConsolePage>
  );
}
