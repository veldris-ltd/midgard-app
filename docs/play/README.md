# The Midgard playground

The playground lets anyone try Midgard without installing anything: the seven console UIs from this repository, built for the browser, running on sample data, with an 18-step guided tour. It is published at **https://play.veldris.dev** from the `sub-domain_play` branch, and runs locally with one command.

| | |
|---|---|
| Owner | Veldris Ltd |
| Source | `play/` (shell, tour, build), `.github/workflows/play.yml` (deploy), `justfile` recipes `play` and `try` |
| Hosted at | https://play.veldris.dev, from `sub-domain_play` |
| Status | Built and verified locally; the deploy needs the secrets and DNS in [Deployment](#deployment) |

## Decisions

| ADR | Decision |
|---|---|
| [ADR-PLAY-001](adr/ADR-PLAY-001-static-playground.md) | The playground is the shipping consoles on their built-in gateways, built into one static site; not a release artefact |
| [ADR-PLAY-002](adr/ADR-PLAY-002-tour-composition.md) | One origin, consoles in a frame, a guided tour driven by data, highlights by visible text, deep links |
| [ADR-PLAY-003](adr/ADR-PLAY-003-honesty-privacy.md) | Sample data is labelled as such; no third-party requests, no cookies, a pinned CSP |
| [ADR-PLAY-004](adr/ADR-PLAY-004-hosting.md) | play.veldris.dev is deployed from `sub-domain_play` by rsync to Veldris-controlled hosting, with its own secrets |
| [ADR-PLAY-005](adr/ADR-PLAY-005-local-run.md) | `pnpm play` for the playground offline; `just try` for the Midgard console against the real edge |

## What a visitor gets

- **Launcher** (`#/`): what the playground is, a card for each console, and what is real and what is sample data, with the commit the site was built from.
- **Guided tour** (`#/tour/1` to `#/tour/18`): each step opens a console at the right screen, explains it, suggests one thing to try and rings the control to look at. Midgard (object fabric, human gates, policy dry-run, audit chain), Forge (pipelines, blocking expectations), Mimir (grounded agents, guardrails), Heimdall (operating picture, decision replay), Bifrost (ringed delivery, rollback), Admin (dual control, break-glass) and the clients (least-capability extension, offline-first mobile), then how to run it yourself.
- **Free exploration** (`#/console/<id>`): any console full-size, with its sections listed beside it.
- A colour-mode switch shared with every console, keyboard navigation (left and right arrows in the tour), and a layout that stacks on narrow screens.

## Acceptance criteria

Each criterion names the ADR it realises and how it is verified. `check` is `node play/scripts/check.mjs`: `--static` runs in the deploy workflow; the browser part runs wherever Playwright is installed.

| ID | Given | When | Then | ADR | Verified by |
|---|---|---|---|---|---|
| AC-PLAY-01 | a clean checkout with the workspace installed | `node play/build.mjs` runs | `play/dist/` holds the shell and all seven consoles, each built from its own Vite config, with no console source changed | 001 | check (static); `git diff --stat products/` is empty |
| AC-PLAY-02 | the workspace | `bash tools/ci/js-artefacts.sh --rows` runs | `play/` is not listed as a release artefact | 001 | manual run, recorded below |
| AC-PLAY-03 | a built playground | every tour step is opened through the shell | each step's console route exists and its highlight is found on screen | 002 | check (static: routes; browser: highlights) |
| AC-PLAY-04 | a tour step | the visitor presses the right or left arrow key in the panel, or opens `#/tour/<n>` directly | the tour moves to the next, previous or named step and the frame follows | 002 | browser check (direct links); manual (keys) |
| AC-PLAY-05 | any playground page | it is displayed | a strip states that the data is sample data, runs in the browser and is sent nowhere | 003 | manual; visible in every workspace screenshot |
| AC-PLAY-06 | the shell and all seven consoles | every tour step and every console is opened | no request leaves the playground's origin and no page logs an error, CSP violations included | 003 | check (browser) |
| AC-PLAY-07 | a built console page | it is inspected | it carries a CSP that allows its one inline script by SHA-256 hash and sets `connect-src 'self'` | 003 | check (static) |
| AC-PLAY-08 | the shell in dark and light mode | every launcher, tour and console view is rendered | all shell text meets WCAG 2.1 AA contrast (4.5:1, or 3:1 for large text) | 003 | contrast audit, recorded below |
| AC-PLAY-09 | a push to `sub-domain_play` with the secrets set | the workflow runs | the build and static check pass and `play/dist/` is mirrored to the web root | 004 | the `play.veldris.dev` workflow run |
| AC-PLAY-10 | a push to any other branch, or a manual run from one | the workflow is triggered | it refuses to deploy | 004 | workflow guard step |
| AC-PLAY-11 | Node 22 and pnpm, the workspace installed, no network | `pnpm play` runs | the playground is served at `http://127.0.0.1:4173`, on loopback only, and paths outside `dist` are refused | 005 | manual run, recorded below |
| AC-PLAY-12 | the Rust toolchain as well | `just try` runs | the edge answers on `127.0.0.1:7710`, the console serves on `http://localhost:5173`, `operator` / `midgard-demo` signs in against the real edge, and Ctrl+C stops both | 005 | manual run, recorded below |

### Verification record, 29 October 2026, base commit `e298d665498d`

- AC-PLAY-01, 03, 06, 07: `node play/build.mjs` built the shell and seven consoles in about 9 s; `check` reported 7 consoles, 18 tour steps, every highlight found, no page errors, no requests outside the origin: **OK**.
- AC-PLAY-02: `js-artefacts.sh --rows` lists the seven console bundles and the SDK, 8 artefacts; `play/` is absent.
- AC-PLAY-08: contrast audit over the launcher, 18 tour steps and 7 console views, in dark and light: 0 failures.
- AC-PLAY-11: `pnpm play` served the site on `127.0.0.1:4173`; `/`, a console page, the favicon and `mode.js` returned 200 with the right types; `/../../etc/passwd` returned 404.
- AC-PLAY-04: with focus in the tour panel, the right arrow moved step 1 to 2 and 3, the left arrow back to 2; the launcher then offered "Resume the tour at step 2". Colour mode set in the shell reached the console frame, and mode set inside the Midgard console reached the shell.
- AC-PLAY-12: `just try` started the edge (health `{"status":"ok"}`) and the console; signing in as `operator` loaded the object fabric from the real edge.
- AC-PLAY-09, 10: pending the first deploy.

## Deployment

Set these in the repository (Settings › Secrets and variables › Actions), ideally in an environment named `play`:

| Secret | Value |
|---|---|
| `PLAY_DEPLOY_HOST` | Hostname or IP of the server that serves play.veldris.dev |
| `PLAY_DEPLOY_USER` | SSH user that owns the web root |
| `PLAY_DEPLOY_KEY` | Private key (OpenSSH format, the full file) whose public half is in that user's `authorized_keys`; a dedicated key, not one used for another site |
| `PLAY_DEPLOY_PATH` | The web root, with no trailing slash |
| `PLAY_DEPLOY_PORT` | Optional; defaults to `22` |

Then point DNS for `play.veldris.dev` at that server and issue its certificate. If the server is the Plesk host, create `play.veldris.dev` as a subdomain in Plesk and use the document root it assigns; Plesk owns the vhost configuration.

The web server needs to serve `index.html` at `/` and send `text/javascript`, `text/css`, `image/svg+xml` and `font/ttf` for those extensions. Files under `assets/` and `consoles/*/assets/` are content-hashed and can be cached for a long time; `index.html` files should be cached briefly.

## Working on it

```bash
pnpm install
node play/build.mjs                  # build into play/dist
node play/scripts/serve.mjs          # serve on http://127.0.0.1:4173
node play/scripts/check.mjs          # verify (browser checks need playwright)
pnpm --filter @midgard/play typecheck
```

The tour is `play/src/tour.ts`: one record per step. Change copy, routes or highlight text there; the check fails if a route or highlight no longer exists. Keep every sentence true to what the sample-data consoles show.

---

Midgard is a Veldris Ltd project. Veldris Ltd, registered in England and Wales, company no. 17366869.
