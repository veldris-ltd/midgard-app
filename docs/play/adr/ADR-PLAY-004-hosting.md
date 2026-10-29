# ADR-PLAY-004: play.veldris.dev is served from the sub-domain_play branch on Veldris-controlled hosting

| | |
|---|---|
| Status | Accepted |
| Date | 29 October 2026 |
| Owner | Veldris Ltd |
| Related | ADR-PLAY-001, ADR-PLAY-003 |

## Context

The playground is published at **https://play.veldris.dev** from the `sub-domain_play` branch of this repository. The site is static (ADR-PLAY-001). Veldris's existing sites are deployed by GitHub Actions over SSH with rsync to Veldris-controlled hosting, with secrets scoped per site (veldris.io and demo.veldris.dev follow this pattern), and veldris.io's privacy policy commits to UK hosting with no transfers outside the UK and EEA.

## Decision

1. `.github/workflows/play.yml` runs on every push to `sub-domain_play` and on manual dispatch. It refuses to run from any other branch.
2. It installs the workspace from the lockfile (`pnpm install --frozen-lockfile`), builds the playground (`node play/build.mjs`), checks the result (`node play/scripts/check.mjs --static`), and rsyncs `play/dist/` to the web root with `--delete`, so the server mirrors the build exactly.
3. It uses its own secrets, `PLAY_DEPLOY_HOST`, `PLAY_DEPLOY_USER`, `PLAY_DEPLOY_KEY`, `PLAY_DEPLOY_PATH` and optional `PLAY_DEPLOY_PORT`, in a GitHub environment named `play`, so it cannot write to any other site. A required reviewer can be added to that environment if deploys should need approval.
4. The web server must serve `index.html` at `/`, send correct MIME types for `.js`, `.css`, `.svg`, `.ttf` and `.woff2`, and should cache `consoles/*/assets/` and `assets/` for a long time (file names are content-hashed) and `index.html` files briefly.
5. If the host is the Plesk server, the subdomain is created in Plesk, its certificate issued there, and `PLAY_DEPLOY_PATH` points at the document root Plesk assigns. Plesk owns the vhost configuration.

## Alternatives considered

- **GitHub Pages.** Simple and free, with HTTPS. Not chosen as the default because visitor requests would be served and logged by a US provider under a Veldris domain, which the veldris.io privacy posture avoids. It remains a one-file change to the workflow if that trade-off is accepted later.
- **Building on the server.** Rejected: the server would need Node and the workspace; a static upload keeps it a plain file host.
- **Committing the built site to the branch.** Rejected: built files in git drift from source and bloat history. The branch holds source; the workflow builds.

## Consequences

- The first run fails clearly until the five secrets are set and DNS for `play.veldris.dev` points at the host.
- Every deploy is the exact output of one commit on `sub-domain_play`, and rollback is a revert on that branch.
