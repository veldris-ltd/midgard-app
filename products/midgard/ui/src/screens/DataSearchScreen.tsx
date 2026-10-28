import * as React from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import {
  Badge,
  Button,
  Card,
  Icon,
  IconButton,
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
import type { CompareOp, ObjectInstance, ObjectType, QueryCondition } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Data search. Pick an object type, add conditions on its properties — equality,
 * ranges, or text that contains a value — order the matches and page through them,
 * with every property shown. Each match opens in Search-around. With `?q=` (the top
 * bar's search box) it instead looks for that text in every type at once. Every
 * search is authorised and audited by the gateway, and every count covers only what
 * the signed-in user may read. [DD-1] [NFR-SEC-2] */

const PAGE = 50;
/** Matches shown per type when searching every type for text. */
const PER_TYPE = 20;
/** The pseudo-property for "any text property". */
const ANY_TEXT = '*';

const OP_LABEL: Record<CompareOp, string> = {
  eq: 'is',
  ne: 'is not',
  lt: '<',
  lte: '≤',
  gt: '>',
  gte: '≥',
  contains: 'contains',
};

/** The comparisons that make sense for a property of `type`. */
function opsFor(type: string | undefined): CompareOp[] {
  if (type === undefined) return ['contains'];
  if (type === 'text') return ['contains', 'eq', 'ne', 'lt', 'lte', 'gt', 'gte'];
  if (type === 'bool') return ['eq', 'ne'];
  return ['eq', 'ne', 'lt', 'lte', 'gt', 'gte'];
}

/** One search as it was run, so editing the form changes nothing until Search. */
interface Spec {
  objectType: string;
  where: QueryCondition[];
  orderBy: string;
  descending: boolean;
  offset: number;
}

const errorStyle: React.CSSProperties = {
  padding: '12px 14px',
  background: 'var(--surface-inset)',
  border: '1px solid var(--danger)',
  borderRadius: 'var(--radius-md)',
  fontFamily: 'var(--font-mono)',
  fontSize: 'var(--fs-xs)',
  color: 'var(--danger)',
  overflowWrap: 'anywhere',
};

export function DataSearchScreen() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const text = params.get('q')?.trim() ?? '';

  const { data: types } = useAsync((c) => c.listObjectTypes(), []);
  const objectTypes: ObjectType[] = types ?? [];

  const [typeName, setTypeName] = React.useState('');
  const type = objectTypes.find((t) => t.apiName === typeName) ?? objectTypes[0];
  const properties = type?.properties ?? [];
  const [rows, setRows] = React.useState<QueryCondition[]>([]);
  const [orderBy, setOrderBy] = React.useState('');
  const [descending, setDescending] = React.useState(false);
  const [spec, setSpec] = React.useState<Spec | null>(null);
  const [sel, setSel] = React.useState(0);

  const pickType = (apiName: string) => {
    setTypeName(apiName);
    setRows([]);
    setOrderBy('');
  };
  const typeOf = (property: string) => properties.find((p) => p.name === property)?.type;
  const setRow = (i: number, patch: Partial<QueryCondition>) =>
    setRows((rs) =>
      rs.map((r, j) => {
        if (j !== i) return r;
        const next = { ...r, ...patch };
        const ops = opsFor(next.property === ANY_TEXT ? undefined : typeOf(next.property));
        return ops.includes(next.op) ? next : { ...next, op: ops[0] };
      }),
    );
  const run = (offset = 0) => {
    if (!type) return;
    setSpec({ objectType: type.apiName, where: rows.filter((r) => r.value !== ''), orderBy, descending, offset });
    setSel(0);
  };

  // One type, with conditions.
  const { data: page, loading, error } = useAsync(
    (c) =>
      spec
        ? c.query({
            policy: c.policy(),
            objectType: spec.objectType,
            where: spec.where,
            orderBy: spec.orderBy || undefined,
            descending: spec.descending,
            offset: spec.offset,
            maxResults: PAGE,
          })
        : Promise.resolve(null),
    [spec],
  );
  // Every type, for text from the top bar.
  const { data: textHits, loading: textLoading } = useAsync(
    (c) =>
      text && types
        ? Promise.all(
            types.map((t) =>
              c
                .query({ policy: c.policy(), objectType: t.apiName, where: [{ property: ANY_TEXT, op: 'contains', value: text }], maxResults: PER_TYPE })
                .then((r) => ({ type: t, results: r.results, total: r.total ?? r.results.length })),
            ),
          )
        : Promise.resolve(null),
    [text, types],
  );

  const shown = loading || error ? null : page;
  const results: ObjectInstance[] = shown?.results ?? [];
  const total = shown?.total ?? results.length;
  const specType = objectTypes.find((t) => t.apiName === spec?.objectType);
  const columns = specType?.properties?.map((p) => p.name) ?? [];
  const picked = results[Math.min(sel, Math.max(0, results.length - 1))];
  const from = spec ? spec.offset + 1 : 0;
  const to = spec ? spec.offset + results.length : 0;

  const aside = picked ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="boxes" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1, overflowWrap: 'anywhere' }}>{picked.id}</h2>
        {picked.markings[0] && <MarkingChip level={picked.markings[0].level} />}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>PROPERTIES</div>
        {Object.entries(picked.propertyValues).map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ color: 'var(--text-faint)', width: 120, flex: 'none', overflowWrap: 'anywhere' }}>{k}</span>
            <span style={{ color: 'var(--grey-100)', overflowWrap: 'anywhere' }}>{v}</span>
          </div>
        ))}
      </div>
      <Button variant="primary" size="sm" fullWidth iconRight={<Icon name="arrow-right" size={14} />} onClick={() => navigate(`/search?seed=${encodeURIComponent(picked.id)}`)}>
        Open in Search-around
      </Button>
      <Terminal
        title="QUERY · AUDITED"
        lines={[
          `> fabric.search("${spec?.objectType ?? ''}")`,
          ...(spec?.where.map((w) => `where     ${w.property === ANY_TEXT ? 'any text' : w.property} ${OP_LABEL[w.op]} ${w.value}`) ?? []),
          `matches   ${total} readable`,
          `# decision audited · #${shown?.auditId ?? '—'}`,
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
      active="data"
      context="ENCLOSURE / DATA SEARCH"
      asideWidth={340}
      toolbar={<ConsoleToolbar />}
      navFooter={<Terminal lines={['> midgard status', '# ALL SYSTEMS NOMINAL']} style={{ fontSize: 10 }} />}
      aside={text ? null : aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={3}>QUERY ENGINE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>{text ? `Data search · “${text}”` : 'Data search'}</h1>
        </div>
        <div style={{ flex: 1 }} />
        {!text && spec && <StatBlock value={String(total)} label="MATCHES" size="sm" />}
      </div>

      {text ? (
        <>
          {textLoading && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>SEARCHING EVERY TYPE…</div>}
          {textHits && textHits.every((h) => h.total === 0) && (
            <div role="status" style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
              Nothing you can read contains “{text}”.
            </div>
          )}
          {textHits
            ?.filter((h) => h.total > 0)
            .map((h) => (
              <div key={h.type.apiName}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8 }}>
                  <SectionLabel muted>{`${h.type.name.toUpperCase()} · ${h.total}`}</SectionLabel>
                  <div style={{ flex: 1 }} />
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      const where: QueryCondition[] = [{ property: ANY_TEXT, op: 'contains', value: text }];
                      setTypeName(h.type.apiName);
                      setRows(where);
                      setOrderBy('');
                      setSpec({ objectType: h.type.apiName, where, orderBy: '', descending: false, offset: 0 });
                      navigate('/data');
                    }}
                  >
                    {h.total > h.results.length ? `See all ${h.total}` : 'Refine'}
                  </Button>
                </div>
                <div style={{ overflowX: 'auto' }}>
                  <Table<ObjectInstance>
                    compact
                    columns={[
                      { header: 'Object', key: 'id', mono: true },
                      { header: 'Name', cell: (r) => r.propertyValues.name ?? '—' },
                      { header: 'Marking', cell: (r) => (r.markings[0] ? <MarkingChip level={r.markings[0].level} /> : null), width: 160 },
                    ]}
                    rows={h.results}
                    onRowClick={(_, r) => navigate(`/search?seed=${encodeURIComponent(r.id)}`)}
                    style={{ minWidth: 520 }}
                  />
                </div>
              </div>
            ))}
        </>
      ) : (
        <>
          <Card size="sm">
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              <div style={{ display: 'flex', alignItems: 'flex-end', gap: 12, flexWrap: 'wrap' }}>
                <Select
                  label="Object type"
                  size="sm"
                  options={objectTypes.map((t) => ({ value: t.apiName, label: t.name }))}
                  value={type?.apiName ?? ''}
                  onChange={(e) => pickType(e.target.value)}
                />
                <Select
                  label="Order by"
                  size="sm"
                  options={[{ value: '', label: 'Object id' }, ...properties.map((p) => ({ value: p.name, label: p.name }))]}
                  value={orderBy}
                  onChange={(e) => setOrderBy(e.target.value)}
                />
                <Select
                  label="Direction"
                  size="sm"
                  options={[{ value: 'asc', label: 'Smallest first' }, { value: 'desc', label: 'Largest first' }]}
                  value={descending ? 'desc' : 'asc'}
                  onChange={(e) => setDescending(e.target.value === 'desc')}
                />
              </div>
              {rows.map((r, i) => {
                const ops = opsFor(r.property === ANY_TEXT ? undefined : typeOf(r.property));
                const valueType = r.property === ANY_TEXT ? 'text' : typeOf(r.property);
                return (
                  <div key={i} style={{ display: 'flex', alignItems: 'flex-end', gap: 10, flexWrap: 'wrap' }}>
                    <Select
                      label={i === 0 ? 'Where' : 'And'}
                      size="sm"
                      options={[{ value: ANY_TEXT, label: 'any text' }, ...properties.map((p) => ({ value: p.name, label: `${p.name} · ${p.type}` }))]}
                      value={r.property}
                      onChange={(e) => setRow(i, { property: e.target.value })}
                    />
                    <Select
                      label="Is"
                      size="sm"
                      options={ops.map((o) => ({ value: o, label: OP_LABEL[o] }))}
                      value={r.op}
                      onChange={(e) => setRow(i, { op: e.target.value as CompareOp })}
                    />
                    {valueType === 'bool' ? (
                      <Select
                        label="Value"
                        size="sm"
                        options={[{ value: 'true', label: 'true' }, { value: 'false', label: 'false' }]}
                        value={r.value || 'true'}
                        onChange={(e) => setRow(i, { value: e.target.value })}
                      />
                    ) : (
                      <Input
                        label="Value"
                        mono
                        size="sm"
                        value={r.value}
                        inputMode={valueType === 'int' || valueType === 'float' || valueType === 'timestamp' ? 'decimal' : undefined}
                        onChange={(e) => setRow(i, { value: e.target.value })}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') run();
                        }}
                        style={{ width: 200 }}
                      />
                    )}
                    <IconButton label="Remove condition" onClick={() => setRows((rs) => rs.filter((_, j) => j !== i))}>
                      <Icon name="x" size={15} />
                    </IconButton>
                  </div>
                );
              })}
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <Button
                  variant="secondary"
                  size="sm"
                  iconLeft={<Icon name="plus" size={14} />}
                  disabled={!type}
                  onClick={() => {
                    const first = properties[0];
                    setRows((rs) => [...rs, first ? { property: first.name, op: opsFor(first.type)[0], value: first.type === 'bool' ? 'true' : '' } : { property: ANY_TEXT, op: 'contains', value: '' }]);
                  }}
                >
                  Add condition
                </Button>
                <div style={{ flex: 1 }} />
                <Button variant="primary" iconLeft={<Icon name="search" size={15} />} disabled={!type} onClick={() => run(0)}>
                  Search
                </Button>
              </div>
            </div>
          </Card>

          {loading && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>SEARCHING · POLICY AT QUERY TIME…</div>}
          {error && (
            <div role="alert" style={errorStyle}>
              {error.message}
            </div>
          )}

          {shown && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
                  {total === 0 ? 'No matches you can read' : `${from}–${to} of ${total}`}
                </span>
                {spec?.where.map((w, i) => (
                  <Badge key={i} tone="info">{`${w.property === ANY_TEXT ? 'any text' : w.property} ${OP_LABEL[w.op]} ${w.value}`}</Badge>
                ))}
                <div style={{ flex: 1 }} />
                <Button variant="secondary" size="sm" disabled={!spec || spec.offset === 0} onClick={() => spec && setSpec({ ...spec, offset: Math.max(0, spec.offset - PAGE) })}>
                  Previous
                </Button>
                <Button variant="secondary" size="sm" disabled={!spec || to >= total} onClick={() => spec && setSpec({ ...spec, offset: spec.offset + PAGE })}>
                  Next
                </Button>
              </div>
              {results.length > 0 && (
                <div style={{ overflowX: 'auto' }}>
                  <Table<ObjectInstance>
                    compact
                    columns={[
                      { header: 'Object', key: 'id', mono: true },
                      ...columns.map((name) => ({ header: name, mono: true, cell: (r: ObjectInstance) => r.propertyValues[name] ?? '—' })),
                    ]}
                    rows={results}
                    selectedIndex={Math.min(sel, results.length - 1)}
                    onRowClick={(i) => setSel(i)}
                    style={{ minWidth: Math.max(560, 160 + columns.length * 130) }}
                  />
                </div>
              )}
            </>
          )}
        </>
      )}
    </ConsolePage>
  );
}
