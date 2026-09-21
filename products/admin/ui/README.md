# Midgard Admin console (`@midgard/console-admin`)

The unified, self-governing **Admin Control Panel** for the Midgard suite — one
console composing every subsystem's admin surface (IAM, KMS, governance, fabric,
Mimir registry, Bifrost release, observability). It renders under the Midgard red
scope (`body.theme-midgard`) but with `ConsoleShell product="admin"`: a grey
**ADMIN** ProductMark and a management-zone classification banner. Vite + React +
TypeScript, built on the same ported design system as the reference console.

## Run

```bash
pnpm install                                # from the repo root (once)
pnpm --filter @midgard/console-admin dev    # http://localhost:5176
pnpm --filter @midgard/console-admin build  # typecheck + production bundle
pnpm --filter @midgard/console-admin preview # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so `dist/`
serves from any static path with no server rewrites and no outbound calls.

## Screens (8)

`/` Overview · `/approvals` Dual-control approvals · `/iam` Identity & roles ·
`/config` Config & policy · `/registry` Registry & governance · `/ledger` Audit
ledger · `/breakglass` Break-glass · `/copilot` Advisory copilot. Nav,
collapse-to-rail, and `<900px` drawers come from the shared `ConsoleShell`
(`NAV.admin`).

### Governance invariants on show

- **Dual-control with separation of duties** — `ApprovalsScreen` routes a second
  approver and DENIES self-approval live (requester == approver → `AC-AD-4`).
  Denials are ledgered too (`AC-AD-6`).
- **No super-admin** — `IamScreen` shows column-split, expiring, role-separated
  RBAC; the "SUPER-ADMIN ROLES" stat is a hard `0` (`AC-AD-2`).
- **GitOps config** — `ConfigScreen` walks diff → simulate → commit → rollback,
  every step signed (`FR-AD-6/7`).
- **Break-glass lifecycle** — `BreakGlassScreen` uses the `BreakGlass` DS
  component for request → time-boxed active → forced post-hoc review (`FR-AD-8`).
- **Advisory-only copilot** — `CopilotScreen` drafts, explains and simulates but
  never executes, and refuses with a visible named code (`FR-AD-10`).

## Data flows — the swap-later seam

Every screen reads data through the `AdminGateway` interface (`@midgard/sdk`) via
the `DataProvider` hooks — never a concrete client:

```ts
const g = useGateway<AdminGateway>();
const { data } = useAsync<AdminOverview, AdminGateway>((c) => c.getOverview());
```

The app is wired to `AdminMock` in [`src/main.tsx`](src/main.tsx):

```ts
const gateway: AdminGateway = new AdminMock();
```

The mock models real self-governing behaviour: a dual-control `approve()` that
enforces separation of duties and PQ-signs both granted and denied paths, a
GitOps config lifecycle (`simulateConfig` → `commitConfig` → `rollbackConfig`), a
break-glass request that time-boxes an active session, a live signed audit ledger
(`auditStream`), and an advisory-only copilot thread. When a browser-reachable
admin-zone bridge exists, swap that one line for an HTTP-backed `AdminGateway` —
no screen changes.

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell
  wrapper, `product="admin"`), `ConsoleToolbar`.
- `src/screens/` — one component per screen (named exports). `ApprovalsScreen` is
  the flagship dual-control flow; source data comes from SDK hooks, never inline
  consts.
