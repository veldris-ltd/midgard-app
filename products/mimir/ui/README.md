# Mimir console (`@midgard/console-mimir`)

The local agentic AI studio — Mimir's operator UI. Grounded (OAG) agents,
no-code Logic, plan–act–observe session traces, Evals with regression gates,
self-defending guardrails, governed feedback, and an in-enclosure model plane.
Built on the ported design system (well-teal accent, `theme-mimir` scope). Vite +
React + TypeScript. Nothing leaves the enclosure — `INFERENCE · LOCAL`, `NO EGRESS`.

## Run

```bash
pnpm install                                 # from the repo root (once)
pnpm --filter @midgard/console-mimir dev     # http://localhost:5175
pnpm --filter @midgard/console-mimir build   # typecheck + production bundle
pnpm --filter @midgard/console-mimir preview # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites and no outbound calls.

## Screens (7)

`/` Agent Studio (flagship OAG chat) · `/logic` Logic Builder (no-code) ·
`/sessions` Session Traces · `/evals` Evals · `/guardrails` Guardrails ·
`/feedback` Governed Feedback · `/models` Model Plane. Nav, collapse-to-rail, and
`<900px` drawers come from the shared `ConsoleShell` (see `NAV.mimir`).

Two flows are interactive against the gateway:

- **Evals** — select a suite and Run it. `runEval(id)` returns per-case pass/fail
  and a score, rendered live with a `MeterBar` and a 90%-gate verdict.
- **Guardrails** — the live interception demo. Submit an agent input (or pick a
  preset) and `simulateGuardrail(input)` blocks hostile prompts at a rail with a
  refusal code (`REFUSED · PROMPT-INJECTION`, `REFUSED · EXFILTRATION`) before any
  tool call or egress.

## Data flows — the swap-later seam

Every screen reads data through the `MimirGateway` interface (`@midgard/sdk`) via
the `DataProvider` hooks (`useAsync`, `useGateway`) — never a concrete client:

```ts
const g = useGateway<MimirGateway>();
const { data } = useAsync<MimirEval[], MimirGateway>((c) => c.listEvals(), []);
```

The app is wired to `MimirMock` in [`src/main.tsx`](src/main.tsx):

```ts
const gateway: MimirGateway = new MimirMock();
```

The mock models real agent-plane behaviour: grounded agents with citations, evals
gated at 90%, self-defending guardrail interception with refusal codes, governed
feedback with poisoning defence, and a live ML-DSA-signed audit stream. When a
browser-reachable bridge exists (a runnable Rust inference/agent plane over the
enclosure's PQ-mTLS crates), swap that one line for the HTTP-backed gateway. No
screen changes.

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`.
- `src/screens/` — one component per screen; the flagship reference is `AgentStudioScreen.tsx`.
