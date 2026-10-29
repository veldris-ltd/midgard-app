# play/

The Midgard playground: the seven consoles on sample data in the browser, with a guided tour, published at https://play.veldris.dev. Design, decisions and acceptance criteria are in [docs/play](../docs/play/README.md).

| Path | What it is |
|---|---|
| `build.mjs` | Builds the shell into `dist/` and each console, from its own Vite config, into `dist/consoles/<id>/`; pins each console page's CSP |
| `src/tour.ts` | The guided tour, one record per step |
| `src/consoles.ts` | The seven consoles; their sections come from `ui/shared/src/nav.ts` |
| `src/App.tsx`, `src/highlight.ts`, `src/play.css` | The shell: launcher, tour panel, frame and highlight ring |
| `scripts/serve.mjs` | Loopback-only static server for `dist/` |
| `scripts/check.mjs` | Static and browser verification of a build |

```bash
pnpm play            # from the repository root: build, then serve on http://127.0.0.1:4173
```

There is deliberately no `vite.config.*` here, so the playground is not picked up as a release artefact (ADR-PLAY-001).
