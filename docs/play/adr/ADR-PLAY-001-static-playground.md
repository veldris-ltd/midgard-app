# ADR-PLAY-001: The playground is the shipping consoles on their built-in gateways, built into one static site

| | |
|---|---|
| Status | Accepted |
| Date | 29 October 2026 |
| Owner | Veldris Ltd |
| Related | ADR-PLAY-002, ADR-PLAY-003, ADR-PLAY-004, ADR-PLAY-005 |

## Context

Visitors to the public repository can read the source, but trying Midgard today means installing Node and pnpm, building a console, and for the Midgard console also building and running the Rust edge. Most people who arrive from veldris.io, a talk or a recording want to click through the software before deciding to build it.

The seven console UIs already have what a hosted, read-only trial needs:

- Six consoles run on an in-browser gateway with seeded sample data and no network: `products/admin/ui/src/main.tsx:17`, `products/bifrost/ui/src/main.tsx:18`, `products/clients/ui/src/main.tsx:18`, `products/forge/ui/src/main.tsx:19`, `products/heimdall/ui/src/main.tsx:17`, `products/mimir/ui/src/main.tsx:18`.
- The Midgard console talks to the real edge by default and switches to the same kind of in-browser gateway when built with `VITE_GATEWAY=mock` (`products/midgard/ui/src/main.tsx:29-33`).
- Every console builds with a relative base (`base: './'`, for example `products/forge/ui/vite.config.ts:11`) and routes with a hash router (`createHashRouter`, for example `products/forge/ui/src/app/routes.tsx:12`), so a built console works from any static path with no server rewrites.

## Decision

1. A new top-level directory, `play/`, holds the playground: a small shell (launcher and guided tour, see ADR-PLAY-002) and a build script, `play/build.mjs`.
2. `play/build.mjs` builds each of the seven consoles **from its own source and its own Vite config**, unchanged, into `play/dist/consoles/<product>/`. The Midgard console is built with `VITE_GATEWAY=mock`; the other six need no flag. No console source is forked, patched or copied.
3. The shell is built into `play/dist/` beside them. The result is one directory of static files with no server-side code and no runtime network dependency.
4. `play/` is a pnpm workspace member so it shares the lockfile, but it has **no `vite.config.*` file**: its Vite configuration lives inside `play/build.mjs`. Under `tools/ci/js-artefacts.sh:12-16` a member with a Vite config is a `ui-bundle` release artefact; `play/` is deliberately not one. The playground is a showcase of the release artefacts, not an artefact itself, and must not enter the signed release manifest.

## Alternatives considered

- **A recorded walkthrough only** (screenshots and video). Rejected: the recordings already exist on veldris.io and demo.veldris.dev; the gap is hands-on use.
- **Hosting the real edge for the public.** Rejected: development mode has one published credential and an in-memory store (`README.md:357`) and must not be exposed beyond loopback. A public multi-tenant edge is a production deployment, out of scope here.
- **A separate demo build of the consoles with tour hooks inside them.** Rejected: it forks the consoles, so the playground would drift from what ships. Same-origin composition (ADR-PLAY-002) needs no console change.
- **Making `play/` a `ui-bundle`.** Rejected for the reason in Decision 4.

## Consequences

- What a visitor clicks is the console code in this repository at the deployed commit, on sample data. When a console changes, the playground changes at its next build.
- The playground shows only what the in-browser gateways implement. Screens that depend on the real edge (sign-in, live audit from a real ledger) are not represented, and the tour says so (ADR-PLAY-003).
- Build time is roughly seven console builds plus the shell; on a laptop this is well under a minute.
- A console that stops building breaks the playground build, which is the intended early signal.
