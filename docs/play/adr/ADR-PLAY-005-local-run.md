# ADR-PLAY-005: One command to try it locally, with or without the real edge

| | |
|---|---|
| Status | Accepted |
| Date | 29 October 2026 |
| Owner | Veldris Ltd |
| Related | ADR-PLAY-001 |

## Context

Some visitors want to run the playground on their own machine, offline or behind a firewall. Others want the next step: the Midgard console talking to the real Rust edge. The second is already possible in two terminals, `just gateway-http` and `just console-midgard-http` (`justfile:40-45`), with the development credentials in `README.md:357`. Neither path is a single command today.

## Decision

1. **`pnpm play`** (root `package.json`), for anyone with Node 22 and pnpm: builds the playground fresh from the checkout (about ten seconds) and serves `play/dist/` at `http://127.0.0.1:4173` with a dependency-free static server (`play/scripts/serve.mjs`) that binds to loopback only. After `pnpm install`, it needs no network. `just play` is the same command for those who use `just`.
2. **`just try`**, for those with the Rust toolchain as well: builds and starts the development-mode edge (`cargo build -p http-gateway`, then the binary) on `127.0.0.1:7710`, waits until it answers, then starts the Midgard console against it on `http://localhost:5173`, the one origin the development edge allows (`services/http-gateway/src/lib.rs:532`). Stopping the command stops both. It prints the development credentials and the reminder that development mode must not be exposed beyond loopback.
3. Both commands are documented at the top of the README under "Try it", beside the link to play.veldris.dev.

## Alternatives considered

- **A Docker image for the playground.** Deferred: it would be a published image to sign and maintain for something Node already serves. `deploy/compose` remains the path for running services in containers.
- **`npx serve` or `vite preview`.** Rejected for the static server: `serve` is a network fetch at run time, and `vite preview` is meant for previews, not loopback-only serving with known MIME types. The custom server is about 60 lines and has no dependencies.

## Consequences

- `pnpm play` works offline once the workspace is installed, matching Midgard's air-gap-first stance.
- `just try` builds the edge on first run, which takes a few minutes; later runs are fast.
