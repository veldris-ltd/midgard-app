# Midgard core console (`@midgard/console-midgard`)

The reference operator UI for the Midgard core enclosure — the first product
console built on the ported design system. Vite + React + TypeScript.

## Run

```bash
pnpm install                                   # from the repo root (once)
pnpm --filter @midgard/console-midgard dev     # http://localhost:5173
pnpm --filter @midgard/console-midgard build   # typecheck + production bundle
pnpm --filter @midgard/console-midgard preview  # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites and no outbound calls
(fonts are the only CDN import in the token layer — vendor them locally for a
fully offline build; see `ui/design-system/src/tokens/fonts.css`).

## Screens (10)

`/` Object Fabric · `/search` Search-around · `/policies` Policies & Markings ·
`/audit` Audit Lineage · `/identity` Identity & Access · `/kms` Keys & Crypto ·
`/compute` Compute · `/sdk` SDK & APIs · `/observability` Observability ·
`/resilience` Resilience. Nav, collapse-to-rail, and `<900px` drawers come from
the shared `ConsoleShell`.

## Data flows — the swap-later seam

Every screen reads data through the `GatewayClient` interface (`@midgard/sdk`)
via the `DataProvider` hooks (`useAsync`, `useGateway`, `useAuditStream`) — never
a concrete client. [`src/main.tsx`](src/main.tsx) wires it to the HTTP gateway behind
`AuthGate`'s sign-in by default, and to `MockGateway` only when built or served with
`VITE_GATEWAY=mock`:

```ts
const { MockGateway } = await import('@midgard/sdk/mock');
```

The mocks and their fixtures are the separate entry `@midgard/sdk/mock`, loaded by
dynamic import in mock mode only, so an http build does not contain them
(`tools/ci/tests/js-mock-bundle.test.sh`). A mock-mode console shows a demonstration
banner and asks for no sign-in. The mock models real enclosure behaviour:
policy-scoped `query`, **human-gated** `applyAction` that emits an ML-DSA-signed
`AuditRecord`, a live audit stream, and a deny-by-default policy dry-run. No screen
changes between the two. [VLD-ANB-MIDGARD-0086]

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`.
- `src/screens/` — one component per screen; `_SCREEN_GUIDE.md` documents the authoring conventions.
