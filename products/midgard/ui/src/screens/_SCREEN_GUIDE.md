# Midgard console — screen authoring guide

Every screen is a routed component that renders `<ConsolePage>` (the shared shell)
with a page body and an optional inspector `aside`, sourcing data through SDK hooks
— never inline consts. The flagship reference is `ObjectFabricScreen.tsx`; match its
structure, imports, and density.

## Skeleton

```tsx
import * as React from 'react';
import { /* DS components */ } from '@midgard/design-system';
import { useAsync, useGateway, useAuditStream } from '@midgard/ui-shared';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

export function XScreen() {
  const { data, loading } = useAsync((c) => c.someMethod(), []);
  const rows = data ?? [];
  return (
    <ConsolePage active="<navId>" context="ENCLOSURE / <AREA>" toolbar={<ConsoleToolbar />} aside={/* inspector JSX or omit */}>
      {/* page header: SectionLabel + <h1> + StatBlocks + primary Button */}
      {/* Tabs (local useState) if the prototype has them */}
      {/* Table / Cards from `rows` */}
      {loading && <div style={{ padding: '12px 4px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>LOADING…</div>}
    </ConsolePage>
  );
}
```

## Rules
- Read the matching prototype `docs/design/ui_kits/midgard-console/<file>.html` and reproduce its
  layout, copy, section labels, stat blocks, provenance tags (`<ProvenanceTag id="…">`), and
  governance chrome faithfully — but replace inline `const X = [...]` data with the hook result.
- One accent (midgard) — never hardcode colours; use `var(--…)` tokens only. Never emoji (Unicode
  glyphs like → · ✓ are fine). ALL-CAPS mono for labels via `className="mg-label"` or `<SectionLabel>`.
- Page header pattern (from the reference): a flex row with `<SectionLabel number={n}>` + `<h1 style={{fontSize:'var(--fs-h3)'}}>` on the left, a `flex:1` spacer, right-aligned `<StatBlock size="sm">`s, and a primary `<Button>`.
- Wrap wide tables in `<div style={{ overflowX: 'auto' }}>` and give the Table a `style={{ minWidth: … }}`.
- Keep each screen self-contained; local `useState` for tab/selection/form; no new global state.

## Component signatures (from @midgard/design-system)
- `Table<Row>`: `columns={[{ header, key?: keyof Row & string, cell?: (row,i)=>node, mono?, align?: 'left'|'right'|'center', width? }]} rows selectedIndex? onRowClick={(index,row)=>…} compact?`
- `Tabs`: `tabs={[{label, icon?, badge?}]} active={n} onChange={setN}`
- `Card`: `size?: 'sm'|'md'|'lg' interactive? accentRule? onClick?` (children)
- `StatBlock`: `value label sub? size?: 'sm'|'md'|'lg' accent?`
- `SectionLabel`: `number? muted?` (children)  — renders `NN // CHILDREN`
- `MarkingChip`: `level?: 'confidential'|'restricted'|'internal'|'open' compartment? label?`
- `StatusPill`: `status label pulse?`  · `Badge`: `tone?: 'neutral'|'secure'|'warn'|'danger'|'info'` (children)
- `ProvenanceTag`: `id kind? title?`  · `Terminal`: `title? lines={string[]}`  · `MeterBar`: `value label? warnAt? dangerAt? width?`
- `Button`: `variant?: 'primary'|'secondary'|'outline'|'ghost' size? iconLeft? iconRight? fullWidth?`
- `Icon`: `name size? color?`  · `IconButton`: `label active? onClick?` (children)
- `Select`: `label? options={[{label,value}]} value onChange={(value:string)=>…} size?`
- `Input`: `mono? size? prefix? suffix? label? placeholder`  · `Switch`: `checked onChange={(v:boolean)=>…} label?`
- `HumanGate`: `title? detail? state?: 'pending'|'authorised'|'rejected' auditId? onAuthorise? onReject?`

## GatewayClient methods (from @midgard/sdk) and types
`policy()`, `listObjectTypes()`, `query(req)`, `applyAction(req)`, `searchAround(seedId,hops)`,
`getAudit(limit?)`, `auditStream(cb)`, `listPolicies()`, `simulatePolicy(ctx,marking)`,
`listIdentities()`, `listKeys()`, `listComputeJobs()`, `getObservability()`, `getResilience()`,
`getContractSurface()`. Types: `ObjectType, AuditRecord, PolicyRule, PolicyDecision, PolicyContext,
Identity, KeyRecord, ComputeJob, SloMetric, RolloutState, RpcSurface, SearchGraph, MarkingLevel`.

## Route / nav-id map (the `active` prop MUST match)
| Screen file | route | active id | prototype html | primary data |
|---|---|---|---|---|
| SearchAroundScreen | /search | `search` | search.html | `searchAround(seed,hops)` |
| PoliciesScreen | /policies | `policies` | policies.html | `listPolicies()` + `simulatePolicy(ctx,marking)` |
| AuditScreen | /audit | `audit` | audit.html | `getAudit()` + `useAuditStream()` |
| IdentityScreen | /identity | `identity` | identity.html | `listIdentities()` |
| KeysScreen | /kms | `kms` | kms.html | `listKeys()` |
| ComputeScreen | /compute | `compute` | compute.html | `listComputeJobs()` |
| SdkScreen | /sdk | `sdk` | sdk.html | `getContractSurface()` |
| ObservabilityScreen | /observability | `obs` | observability.html | `getObservability()` |
| ResilienceScreen | /resilience | `resilience` | resilience.html | `getResilience()` |
