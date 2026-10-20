/* The object explorer: the real instances of one object type, searchable, and the
 * card of one object — its properties and marking, the objects it links to (click to
 * follow), and the actions a person may take on it. A human-gated action asks for the
 * approver's reason; the enclosure records who approved it and why before it runs,
 * and every read and action lands in the live audit lineage. [DD-1] [NFR-SEC-2]
 * [NFR-AI-1] */
import * as React from 'react';
import { Badge, Button, Card, HumanGate, Icon, Input, MarkingChip, Table } from '@midgard/design-system';
import { useGateway } from '@midgard/ui-shared';
import type { ActionType, GraphNode, ObjectInstance, ObjectType } from '@midgard/sdk';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.06em' };
// Properties worth a column, in order of preference; the rest show on the card.
const PREFERRED = ['name', 'title', 'label', 'status', 'severity', 'callsign', 'icao', 'iata', 'city', 'country', 'kind', 'region', 'value', 'reading'];
const MAX_COLUMNS = 4;

const LINKS_PER_GROUP = 4;

const keyOf = (id: string) => id.slice(id.indexOf('/') + 1);

type Linked = { node: GraphNode; label: string; out: boolean };

/** Links grouped by what they say about this object: "runway at → Airport" seen from
 *  the airport is "← runway at · Runway". Largest groups last, so the specific ones
 *  (its region, its operator) come first. */
function groupLinks(links: Linked[]): [string, Linked[]][] {
  const groups = new Map<string, Linked[]>();
  for (const l of links) {
    const verb = l.label.split(' → ')[0];
    const key = l.out ? `${verb} · ${l.node.typeName}` : `${l.node.typeName} · ${verb} this`;
    groups.set(key, [...(groups.get(key) ?? []), l]);
  }
  return [...groups.entries()].sort((a, b) => a[1].length - b[1].length || a[0].localeCompare(b[0]));
}
const typeOf = (id: string) => id.slice(0, id.indexOf('/'));

/** A property value for people: epoch seconds as a date, long floats rounded. */
export function showValue(name: string, v: string): string {
  if (/^\d{9,11}$/.test(v) && /(_at|_seen|seen|time|timestamp|_from|_to|issued|updated|expires)$/i.test(name)) {
    return new Date(Number(v) * 1000).toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' });
  }
  if (/^-?\d+\.\d{5,}$/.test(v)) return String(Math.round(Number(v) * 1e4) / 1e4);
  return v;
}

const pretty = (s: string) => s.replace(/_/g, ' ');
const titleOf = (o: ObjectInstance) => o.propertyValues.name || o.propertyValues.title || o.propertyValues.label || keyOf(o.id);

/** The searchable table of one type's objects. */
export function ExplorerBody({ type, selectedId, onOpen }: { type: ObjectType; selectedId: string | null; onOpen: (id: string) => void }) {
  const gateway = useGateway();
  const [search, setSearch] = React.useState('');
  const [query, setQuery] = React.useState('');
  const [rows, setRows] = React.useState<ObjectInstance[]>([]);
  const [matched, setMatched] = React.useState<number | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => { setSearch(''); setQuery(''); }, [type.apiName]);
  React.useEffect(() => {
    const t = window.setTimeout(() => setQuery(search.trim()), 250);
    return () => window.clearTimeout(t);
  }, [search]);
  React.useEffect(() => {
    let live = true;
    setLoading(true);
    setError(null);
    gateway
      .query({ policy: gateway.policy(), objectType: type.apiName, maxResults: 200, ...(query ? { search: query } : {}) })
      .then((r) => { if (live) { setRows(r.results); setMatched(r.matched ?? r.results.length); } })
      .catch((e) => { if (live) setError(e instanceof Error ? e.message : String(e)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [gateway, type.apiName, query]);

  const columns = React.useMemo(() => {
    const seen = new Set<string>();
    rows.slice(0, 50).forEach((r) => Object.keys(r.propertyValues).forEach((k) => seen.add(k)));
    const picked = PREFERRED.filter((k) => seen.has(k));
    for (const k of seen) if (picked.length < MAX_COLUMNS && !picked.includes(k)) picked.push(k);
    return picked.slice(0, MAX_COLUMNS);
  }, [rows]);

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={`Search ${type.name.toLowerCase()} by name, code or any text…`}
          prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
          aria-label={`Search ${type.name}`}
          style={{ flex: '1 1 320px' }}
          fullWidth
        />
        <span style={{ ...mono, color: 'var(--text-faint)' }}>
          {loading ? 'QUERYING FABRIC…' : matched === null ? '' : `${rows.length.toLocaleString()} SHOWN · ${matched.toLocaleString()} MATCH${query ? ` “${query.toUpperCase()}”` : ''}`}
        </span>
      </div>
      {error && <div style={{ ...mono, color: 'var(--danger, var(--warn))' }}>QUERY REFUSED · {error.toUpperCase()}</div>}
      <div style={{ overflowX: 'auto', flexShrink: 0 }}>
        <Table<ObjectInstance>
          columns={[
            { header: 'Key', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}><Icon name="package" size={13} color="var(--accent)" />{keyOf(r.id)}</span>, mono: true, width: 210 },
            ...columns.map((c) => ({ header: pretty(c), cell: (r: ObjectInstance) => (r.propertyValues[c] === undefined ? <span style={{ color: 'var(--text-faint)' }}>—</span> : showValue(c, r.propertyValues[c])) })),
            { header: 'Marking', cell: (r) => (r.markings[0] ? <MarkingChip level={r.markings[0].level} compartment={r.markings[0].compartments[0]} /> : null), width: 150 },
          ]}
          rows={rows}
          selectedIndex={rows.findIndex((r) => r.id === selectedId)}
          onRowClick={(_, r) => onOpen(r.id)}
          style={{ minWidth: 760 }}
        />
      </div>
    </>
  );
}

type Run =
  | { state: 'idle' }
  | { state: 'approving'; action: ActionType; reason: string }
  | { state: 'running'; action: ActionType }
  | { state: 'done'; action: ActionType; auditId: string; approvalAuditId?: string; changed?: string }
  | { state: 'failed'; action: ActionType; message: string };

/** One object's card: properties, links to follow, and its actions. */
export function ObjectCard({ id, types, onOpen, onBack }: { id: string; types: ObjectType[]; onOpen: (id: string) => void; onBack?: () => void }) {
  const gateway = useGateway();
  const me = gateway.policy().subject;
  const type = types.find((t) => t.apiName === typeOf(id));
  const [obj, setObj] = React.useState<ObjectInstance | null>(null);
  const [missing, setMissing] = React.useState(false);
  const [links, setLinks] = React.useState<{ node: GraphNode; label: string; out: boolean }[] | null>(null);
  const [run, setRun] = React.useState<Run>({ state: 'idle' });
  const [nonce, setNonce] = React.useState(0);
  const [expanded, setExpanded] = React.useState<Set<string>>(new Set());

  React.useEffect(() => { setRun({ state: 'idle' }); setExpanded(new Set()); }, [id]);
  React.useEffect(() => {
    let live = true;
    setMissing(false);
    gateway
      .query({ policy: gateway.policy(), objectType: typeOf(id), id, maxResults: 1 })
      .then((r) => { if (live) { setObj(r.results[0] ?? null); setMissing(!r.results[0]); } })
      .catch(() => { if (live) { setObj(null); setMissing(true); } });
    return () => { live = false; };
  }, [gateway, id, nonce]);
  React.useEffect(() => {
    let live = true;
    setLinks(null);
    gateway
      .searchAround(id, 1)
      .then((g) => {
        if (!live) return;
        const byId = new Map(g.nodes.map((n) => [n.id, n]));
        const out: { node: GraphNode; label: string; out: boolean }[] = [];
        for (const e of g.edges) {
          const other = e.from === id ? e.to : e.to === id ? e.from : null;
          const node = other ? byId.get(other) : undefined;
          if (node) out.push({ node, label: e.label, out: e.from === id });
        }
        setLinks(out);
      })
      .catch(() => { if (live) setLinks([]); });
    return () => { live = false; };
  }, [gateway, id]);

  const apply = async (action: ActionType, reason?: string) => {
    const before = obj?.propertyValues ?? {};
    setRun({ state: 'running', action });
    try {
      const res = await gateway.applyAction({ policy: gateway.policy(), actionType: action.name, params: { target: id }, ...(reason ? { humanGateToken: reason } : {}) });
      const after = await gateway.query({ policy: gateway.policy(), objectType: typeOf(id), id, maxResults: 1 });
      const now = after.results[0] ?? null;
      setObj(now);
      const changed = now ? Object.keys(now.propertyValues).find((k) => now.propertyValues[k] !== before[k]) : undefined;
      setRun({ state: 'done', action, auditId: res.auditId, approvalAuditId: res.approvalAuditId, changed: changed && now ? `${pretty(changed)} → ${now.propertyValues[changed]}` : undefined });
    } catch (e) {
      setRun({ state: 'failed', action, message: e instanceof Error ? e.message : String(e) });
    }
  };

  const changedKey = run.state === 'done' && run.changed ? run.changed.split(' → ')[0].replace(/ /g, '_') : null;

  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        {onBack && (
          <button type="button" onClick={onBack} aria-label="Back" style={{ all: 'unset', cursor: 'pointer', display: 'inline-flex' }}>
            <span style={{ display: "inline-flex", transform: "rotate(180deg)" }}><Icon name="arrow-right" size={16} color="var(--text-muted)" /></span>
          </button>
        )}
        <Icon name="package" size={18} color="var(--accent)" />
        <h2 style={{ fontSize: 'var(--fs-h4)', flex: 1, minWidth: 0, overflowWrap: 'anywhere' }}>{obj ? titleOf(obj) : keyOf(id)}</h2>
        {obj?.markings[0] && <MarkingChip level={obj.markings[0].level} compartment={obj.markings[0].compartments[0]} />}
      </div>
      <div style={{ ...mono, color: 'var(--text-faint)', marginTop: -6 }}>{(type?.name ?? typeOf(id)).toUpperCase()} · {id}</div>
      {missing && <div style={{ ...mono, color: 'var(--text-muted)' }}>NOT FOUND, OR NOT READABLE AT YOUR CLEARANCE</div>}

      {obj && (
        <Card size="sm">
          <div style={{ display: 'grid', gridTemplateColumns: 'minmax(90px, 40%) 1fr', gap: '5px 10px', fontSize: 'var(--fs-xs)' }}>
            {Object.entries(obj.propertyValues).map(([k, v]) => (
              <React.Fragment key={k}>
                <span style={{ ...mono, color: 'var(--text-faint)', textTransform: 'uppercase' }}>{pretty(k)}</span>
                <span style={{ color: k === changedKey ? 'var(--accent-hover)' : 'var(--text-body)', fontWeight: k === changedKey ? 600 : 400, overflowWrap: 'anywhere', transition: 'color 0.6s' }}>{showValue(k, v)}</span>
              </React.Fragment>
            ))}
          </div>
        </Card>
      )}

      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>ACTIONS</div>
        {(type?.actions ?? []).length === 0 && <div style={{ ...mono, color: 'var(--text-faint)' }}>NONE DECLARED FOR THIS TYPE</div>}
        {(type?.actions ?? []).map((a) => (
          <button
            key={a.name}
            type="button"
            disabled={run.state === 'running' || !obj}
            onClick={() => (a.humanGated ? setRun({ state: 'approving', action: a, reason: '' }) : apply(a))}
            style={{ all: 'unset', boxSizing: 'border-box', cursor: 'pointer', width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}
          >
            <Icon name="zap" size={13} color="var(--accent)" />
            {a.name}
            {a.humanGated && <span style={{ marginLeft: 'auto' }}><Badge tone="warn">HUMAN GATE</Badge></span>}
          </button>
        ))}
        {run.state === 'approving' && (
          <div style={{ display: 'grid', gap: 8, marginTop: 10 }}>
            <Input
              label={`APPROVING AS ${me.toUpperCase()} · REASON`}
              value={run.reason}
              onChange={(e) => setRun({ ...run, reason: e.target.value })}
              placeholder="Why this is needed (recorded in the ledger)"
              maxLength={200}
              autoFocus
              fullWidth
            />
            <HumanGate
              title={`${run.action.name} · ${obj ? titleOf(obj) : keyOf(id)}`}
              detail={run.reason.trim().length < 3 ? 'GIVE A REASON TO AUTHORISE' : `SIGNED AS ${me.toUpperCase()} · RECORDED BEFORE IT RUNS`}
              state="pending"
              onAuthorise={run.reason.trim().length < 3 ? undefined : () => apply(run.action, run.reason.trim())}
              onReject={() => setRun({ state: 'idle' })}
            />
          </div>
        )}
        {run.state === 'running' && <div style={{ ...mono, color: 'var(--accent-hover)', marginTop: 8 }}>APPLYING {run.action.name.toUpperCase()}…</div>}
        {run.state === 'done' && (
          <div style={{ marginTop: 10 }}>
            <HumanGate state="authorised" auditId={run.approvalAuditId ?? run.auditId} />
            <div style={{ ...mono, color: 'var(--text-muted)', marginTop: 6, lineHeight: 1.6 }}>
              {run.changed ? `${run.changed.toUpperCase()} · ` : ''}ACTION {run.auditId}{run.approvalAuditId ? ` · APPROVAL ${run.approvalAuditId}` : ''}
            </div>
          </div>
        )}
        {run.state === 'failed' && <div style={{ ...mono, color: 'var(--warn)', marginTop: 8, overflowWrap: 'anywhere' }}>REFUSED · {run.message.toUpperCase()}</div>}
      </div>

      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>LINKED OBJECTS{links ? ` · ${links.length}` : ''}</div>
        {links === null && <div style={{ ...mono, color: 'var(--text-faint)' }}>WALKING LINKS…</div>}
        {links?.length === 0 && <div style={{ ...mono, color: 'var(--text-faint)' }}>NO LINKS YOU MAY READ</div>}
        {links && groupLinks(links).map(([group, items]) => {
          const open = expanded.has(group);
          const shown = open ? items : items.slice(0, LINKS_PER_GROUP);
          return (
            <div key={group} style={{ marginBottom: 8 }}>
              <div style={{ ...mono, color: 'var(--text-muted)', padding: '4px 0' }}>{group.toUpperCase()} · {items.length}{items.length >= 12 ? '+' : ''}</div>
              {shown.map(({ node }) => (
                <button
                  key={node.id}
                  type="button"
                  onClick={() => onOpen(node.id)}
                  style={{ all: 'unset', boxSizing: 'border-box', cursor: 'pointer', width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '5px 0 5px 4px', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}
                >
                  <Icon name="network" size={12} color="var(--text-faint)" />
                  <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{node.label}</span>
                  <Icon name="chevron-right" size={12} color="var(--text-faint)" />
                </button>
              ))}
              {items.length > LINKS_PER_GROUP && (
                <button
                  type="button"
                  onClick={() => setExpanded((s) => { const n = new Set(s); if (open) n.delete(group); else n.add(group); return n; })}
                  style={{ all: 'unset', cursor: 'pointer', ...mono, color: 'var(--accent-hover)', padding: '4px 0' }}
                >
                  {open ? 'SHOW FEWER' : `SHOW ALL ${items.length}`}
                </button>
              )}
            </div>
          );
        })}
      </div>
      <Button variant="secondary" size="sm" onClick={() => setNonce((n) => n + 1)} iconLeft={<Icon name="refresh-cw" size={13} />}>Refresh</Button>
    </>
  );
}
