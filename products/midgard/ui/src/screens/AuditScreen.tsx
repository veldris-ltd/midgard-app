import * as React from 'react';
import {
  Badge,
  Button,
  Icon,
  Input,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useAuditStream } from '@midgard/ui-shared';
import type { AuditRecord } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Audit lineage. An append-only, ML-DSA-signed ledger: seeded from `getAudit(40)`
 * then grown live via `useAuditStream`. A client-side substring filter and category
 * tags narrow the view, and Export chain saves what is shown. Every figure is counted
 * from the records on screen. [NFR-SEC-2] [AC-M-4] */

type Category = 'all' | 'actions' | 'queries' | 'writes' | 'sessions' | 'admin' | 'agent';

const CATEGORIES: { id: Category; label: string }[] = [
  { id: 'all', label: 'all events' },
  { id: 'actions', label: 'actions' },
  { id: 'queries', label: 'queries' },
  { id: 'writes', label: 'writes' },
  { id: 'sessions', label: 'sign-ins' },
  { id: 'admin', label: 'administration' },
  { id: 'agent', label: 'agent activity' },
];

/** Edge decisions on these verbs bring data in, rather than act on it. */
const WRITE_VERBS = new Set(['SyncRecords', 'OpenChannel']);

/** The category of one ledger entry. The gateway writes `PERMIT <verb>` / `DENY <verb>`
 *  for each decision (`Read` for a query, `Apply`, `Approve` or an action's api name
 *  for an action), `LOGIN …` for a sign-in, `SYNC records` and `IMPORT data` for data
 *  coming in, and `ENROLL …`, `GRANT …` and `ISSUE …` from mg-admin. The offline mock
 *  writes `Query · …`, `Action · …` and `Write · …`. */
function categoryOf(r: AuditRecord): Exclude<Category, 'all' | 'agent'> | null {
  const [head, verb = ''] = r.action.split(/\s+/, 2);
  if (head === 'PERMIT' || head === 'DENY') {
    if (verb === 'Read') return 'queries';
    return WRITE_VERBS.has(verb) ? 'writes' : 'actions';
  }
  if (head === 'LOGIN') return 'sessions';
  if (head === 'SYNC' || head === 'IMPORT') return 'writes';
  if (head === 'ENROLL' || head === 'GRANT' || head === 'ISSUE') return 'admin';
  if (r.action.startsWith('Query')) return 'queries';
  if (r.action.startsWith('Action')) return 'actions';
  if (r.action.startsWith('Write')) return 'writes';
  return null;
}

function matchesCategory(r: AuditRecord, cat: Category): boolean {
  if (cat === 'all') return true;
  if (cat === 'agent') return r.actor.startsWith('agent:');
  return categoryOf(r) === cat;
}

/** Save `records` as a JSON file the browser downloads. */
function exportChain(records: AuditRecord[]) {
  const blob = new Blob([JSON.stringify(records, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `audit-chain-${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

export function AuditScreen() {
  const { data: seed } = useAsync((c) => c.getAudit(40), []);
  const live = useAuditStream(seed ?? [], 80);

  const [query, setQuery] = React.useState('');
  const [cat, setCat] = React.useState<Category>('all');
  const [sel, setSel] = React.useState(0);

  const q = query.trim().toLowerCase();
  const rows = live.filter(
    (r) => matchesCategory(r, cat) && (!q || r.actor.toLowerCase().includes(q) || r.resource.toLowerCase().includes(q)),
  );

  const total = live.length;
  // The gateway re-verifies each row's hash and ML-DSA signature as it serves it, and
  // marks it ✓ only when both hold.
  const signed = live.filter((r) => r.pqSignature.includes('✓')).length;
  const signedPct = total ? Math.round((signed / total) * 100) : 100;

  const e = rows[Math.min(sel, Math.max(0, rows.length - 1))];
  const selIdx = rows.findIndex((r) => r === e);

  const aside = e ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="scroll-text" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>#{e.id}</h2>
        <Badge tone="secure">{e.pqSignature}</Badge>
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>LINEAGE TRACE</div>
        {([
          ['↑ caused by', selIdx < rows.length - 1 ? `#${rows[selIdx + 1].id} · ${rows[selIdx + 1].action}` : 'origin'],
          ['this event', `${e.action} → ${e.resource}`],
          ['↓ leads to', selIdx > 0 ? `#${rows[selIdx - 1].id} · ${rows[selIdx - 1].action}` : 'chain head'],
        ] as const).map(([k, v], i) => (
          <div key={k} style={{ padding: '8px 10px', marginBottom: 6, borderRadius: 6, background: i === 1 ? 'var(--accent-tint)' : 'var(--surface-inset)', border: `1px solid ${i === 1 ? 'var(--accent)' : 'var(--border)'}` }}>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 8.5, letterSpacing: '0.12em', color: 'var(--text-faint)', textTransform: 'uppercase' }}>{k}</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--grey-100)', marginTop: 3 }}>{v}</div>
          </div>
        ))}
      </div>
      <Terminal
        title="RECORD"
        lines={[
          `actor    ${e.actor}`,
          `action   ${e.action}`,
          `resource ${e.resource}`,
          `sig      ${e.pqSignature}`,
          e.pqSignature.includes('✓') ? '# append-only · verified' : '# signature did not verify',
        ]}
      />
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="NFR-SEC-2" title="Role/marking/purpose access control with full audit lineage" />
        <ProvenanceTag id="NFR-AI-1" title="Every agentic action traceable, evaluated, reversible or human-gated" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="audit"
      context="ENCLOSURE / AUDIT LINEAGE"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={9}>AUDIT</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Audit lineage</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(total)} label="RECORDS · LIVE" size="sm" />
        <StatBlock value={`${signedPct}%`} label="ML-DSA SIGNED" size="sm" accent />
        <StatBlock value={String(total - signed)} label="UNVERIFIED" size="sm" />
        <Button
          variant="secondary"
          iconLeft={<Icon name="download" size={15} />}
          disabled={rows.length === 0}
          title="Download the records shown, as JSON"
          onClick={() => exportChain(rows)}
        >
          Export chain
        </Button>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Input
          mono
          size="sm"
          value={query}
          onChange={(ev) => setQuery(ev.target.value)}
          placeholder="Filter by actor or resource…"
          prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
          style={{ width: 'min(320px, 60vw)' }}
        />
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {CATEGORIES.map((c) => (
            <Tag key={c.id} active={cat === c.id} onClick={() => { setCat(c.id); setSel(0); }}>{c.label}</Tag>
          ))}
        </div>
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<AuditRecord>
          compact
          columns={[
            { header: 'Audit id', cell: (r) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--accent-hover)' }}>#{r.id}</span>, width: 110 },
            { header: 'Actor', key: 'actor', mono: true },
            { header: 'Operation', key: 'action' },
            { header: 'Resource', key: 'resource', mono: true, width: 190 },
            { header: 'Signature', cell: (r) => <Badge tone="secure">{r.pqSignature}</Badge>, width: 110 },
            { header: 'TS', key: 'ts', mono: true, width: 90 },
          ]}
          rows={rows}
          selectedIndex={selIdx >= 0 ? selIdx : undefined}
          onRowClick={(i) => setSel(i)}
          style={{ minWidth: 720 }}
        />
        {rows.length === 0 && (
          <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>
            NO RECORDS MATCH THIS FILTER · APPEND-ONLY LEDGER
          </div>
        )}
      </div>

      <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.12em', color: 'var(--text-faint)' }}>
        APPEND-ONLY · ML-DSA-SIGNED · {signed}/{total} RECORDS VERIFIED · STREAM LIVE
      </div>
    </ConsolePage>
  );
}
