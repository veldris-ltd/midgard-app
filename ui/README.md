# ui/ — shared frontend

- `design-system/` (`@midgard/design-system`) — the Midgard design system
  consumed by every product UI. The **authority** lives in `docs/design/`
  (tokens, 28 components, theme scopes, governance chrome); this package is the
  bundled TypeScript port for app toolchains. See
  [`design-system/PORTING.md`](design-system/PORTING.md) for the mapping.
- `shared/` (`@midgard/ui-shared`) — the shared `ConsoleShell` + `NAV`,
  responsive drawers, `ModeToggle`, and the `DataProvider` hooks that bridge
  screens to `@midgard/sdk`'s `GatewayClient` seam.

House rules (from GETTING_STARTED §10): one accent per screen (`.theme-*`
scope), never emoji, ALL-CAPS mono labels, provenance tags as chips,
`ProductMark` instead of invented logos, PQ state is visible UI.

> **Status: live.** The design system and shared shell are ported and bundled.
> The first product console — Midgard core — is built on them at
> [`products/midgard/ui`](../products/midgard/ui) (10 screens, wired to the mock
> gateway). Forge / Mimir / Bifrost / Heimdall / Admin / Clients follow the same
> pattern.
