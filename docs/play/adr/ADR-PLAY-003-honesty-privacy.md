# ADR-PLAY-003: Sample data is labelled as such, and the playground sends nothing anywhere

| | |
|---|---|
| Status | Accepted |
| Date | 29 October 2026 |
| Owner | Veldris Ltd |
| Related | ADR-PLAY-001, ADR-PLAY-004 |

## Context

The consoles on their in-browser gateways show seeded sample data: aircraft, crew, feeds, releases and audit entries that look real. A visitor who does not know this could read the figures as a live system or as customer data. Midgard's own posture is that every figure is real or labelled (see the `console-midgard` change "every figure is real or labelled, every control works or says why"), and the launch kit already separates "real in the demo" from "sample data in the demo" (`docs/launch/site/index.html`, section "What is real").

The playground is also a public page under a Veldris domain. Veldris's sites make no third-party requests, set no cookies and run no analytics.

## Decision

1. **Labelling.** A persistent strip above every console frame reads: *Sample data. Runs entirely in your browser. Nothing you do here is sent anywhere.* The launcher repeats it, and the tour's first and last steps state what is real (the console code at this commit) and what is not (the data, and the absence of the Rust edge).
2. **No network.** The site makes no request outside its own origin and no request at all after loading, apart from loading console files when the visitor opens a console. No analytics, no fonts or scripts from a CDN, no embedded players. The design system's fonts are self-hosted OFL files already in the repository.
3. **No cookies, minimal storage.** The only storage is the existing `mg-mode` key (`ui/design-system/src/mode.ts:16,43`) and, for the shell, the last tour step under `mg-play-step`, so a reload resumes the tour. Both are functional and hold no personal data.
4. **Content Security Policy.** The shell sets `default-src 'self'` with no inline script; its colour-mode bootstrap is a file (`play/public/mode.js`). The consoles' `index.html` carries one inline script, the colour-mode bootstrap that runs before first paint; `play/build.mjs` writes a CSP meta tag into each built console that allows exactly that script by its SHA-256 hash and no other inline script. `connect-src` is `'self'`, so a console cannot reach the network even if a future change tried to. `style-src` allows `'unsafe-inline'` in both: the shared design-system components set inline styles, and the browser check (`play/scripts/check.mjs`) showed a strict `style-src` blocks them. Inline styles cannot run code; tightening this needs the components to move their styles to classes first.
5. **Human gates stay human gates.** Actions behind a human gate in the consoles behave exactly as their gateways implement them. The tour never presses a button for the visitor.

## Alternatives considered

- **No banner, rely on the tour text.** Rejected: free-mode visitors may never read the tour.
- **First-party analytics to measure use.** Deferred: not needed to ship, and adding it would require a privacy notice for the subdomain. If it is wanted later it must be first-party, cookieless and documented, per the Veldris standard.

## Consequences

- The playground can be described truthfully in one sentence: the shipping console code, on sample data, in your browser.
- The CSP hash ties each console's built `index.html` to its bootstrap script. If the bootstrap changes, the build recomputes the hash; nothing is hand-maintained.
