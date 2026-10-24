# Bifrost working for real — design

Agreed with the user on 24 Oct 2026. Branch `md_bifrost`, branched from `dev_mac`; worktree `~/Desktop/Midgard/midgard-bifrost`. It follows `docs/dev/markdown/SAD_03_Bifrost.md`: a control plane in the enclosure plus a PQ-mTLS agent on each managed node, where deploys fail closed on trust.

## Decisions

- **Purpose:** demo first, ship later. Build a demo-grade Bifrost now, with an architecture ready for real delivery.
- **Where:** the native dev suite on this Mac. It is not in the presenter installers yet.
- **Installations:** the native suite itself. Bifrost delivers real service builds to the suite running on the Mac and restarts its services.
- **Release contents:** the suite's service builds (http-gateway, heimdall-feed, mimir-server, forge-worker, midgard-replay), each signed and SBOM-attested, versioned together. Console bundles and model weights come later.
- **Architecture:** as the specification says (approach A below).
- Commits are signed and carry no Claude co-author line (an organisation rule).

## 1. Parts and where they run

**`bifrost-server`** is one mesh service in the enclosure. It replaces today's `bifrost-fleet` (CheckIn and FleetStatus, which it keeps) and holds the SAD's five containers as modules over the existing crates:

| Module | Crate | Role |
|---|---|---|
| Artefact registry | `bifrost-artefact` | Signed builds and SBOMs, encrypted at rest; verified on ingest and before every deploy |
| Release orchestrator | `bifrost-rollout` | Ring planner, health gate, rollback controller, state reconciler |
| Fleet manager | `bifrost-fleet` | Installations, channels, desired vs actual, drift |
| Bundle service | `bifrost-bundle` | Signed offline bundles, export and import |
| Risk adapter | `bifrost-risk` | Mimir risk score, advisory or gating; the rule gate works with Mimir down |

**`bifrost-agent` in suite mode** runs beside the native suite. It works through these steps:
1. pulls its desired release's artefacts and verifies them itself;
2. installs each service build into a versioned slot (`releases/<version>/`) with an `active` pointer;
3. restarts that service and reports health;
4. keeps the previous slot for instant rollback.

**Rings on one Mac:**

| Ring | Services |
|---|---|
| Canary | forge-worker |
| Ring 2 | midgard-replay, heimdall-feed |
| Ring 3 | mimir-server |
| Last | http-gateway, which carries the console |

**Console path:** a Bifrost bridge in http-gateway (like `heimdall_bridge.rs`) talks to bifrost-server over PQ-mTLS. A new `sdk/ts/src/products/bifrost-http.ts` implements `BifrostGateway`. `products/bifrost/ui` switches from `BifrostMock` to the HTTP client, behind the same sign-in, policy and audit as the other consoles.

## 2. Release flow

1. **Build.** `tools/bifrost/release.sh <commit>` builds the service binaries for the commit. `build-services.sh` and `run-native.sh` are brought onto this branch from the app branch. A CycloneDX SBOM is made per binary (`mg-sbom`).
2. **Sign and register.**
   - The enclosure's ML-DSA release key signs each binary and SBOM (`mg-sign`), and they are uploaded to the registry.
   - The registry checks the signature, the SBOM and the reproducible-build digest. A failure is refused and shown as Rejected.
3. **Release.**
   - The artefacts are grouped as release `X.Y.Z` on a channel.
   - The risk adapter asks Mimir for a score with reasons.
   - A high score blocks promotion unless a release manager overrides it; the override is audited.
4. **Promote.** The orchestrator raises the desired version ring by ring.
   - The agent sees it on its next check-in (10 s), downloads, verifies, swaps the slot, restarts the service and waits for readiness, then reports.
   - The gate passes after a healthy soak of about 60 s for the demo, and the next ring starts.
5. **Rollback.**
   - If a gate fails (unhealthy, or not ready within 3 minutes), every ring already rolled out goes back to the previous release. Agents swap back to the kept slot.
   - The Rollback centre records the reason. Rollback can also be triggered by hand.
6. **Drift.** Fleet & drift shows desired vs actual per service, live.

The enclosure is the real one (`~/Desktop/Midgard/airports`). Issuing the bifrost-server and agent identities needs one short gateway stop (see the stop-gateway-before-mg-admin note).

A demo "bad build" option registers a build whose health check fails, to show automatic rollback on demand.

## 3. Safety and failure handling

- **Fails closed on trust.** An artefact with a bad signature, SBOM mismatch or unknown key is refused at upload and again by the agent before install. The agent runs only binaries from its verified slots.
- **Never leaves the Mac broken.**
  - The previous version is kept until the next release fully passes.
  - Rings restart the gateway last.
  - The readiness timeout is 3 minutes.
  - The agent journals each swap. After a crash it finishes or reverts the swap before anything else.
- **Control plane down:** the agent keeps the current version, retries, and changes nothing.
- **Mimir down:** the risk score shows "unavailable", and the rule gate still decides.
- **Gateway restart (last ring):** the console reconnects. The rollout runs in bifrost-server, so it isn't interrupted.
- **Roles and audit:** release, promote, roll back and override need the role `release-manager` (gateway policy); each is recorded in the signed audit with actor and reason. Analysts can view only.
- **Out of scope for this phase:** remote sites and network delivery, Kubernetes, console bundles, model updates, and more than one machine. The agent and APIs are designed so these can be added later.

## 4. Testing and done

- **Unit tests:**
  - registry: bad signature, bad SBOM, unknown key;
  - orchestrator: ring order, gate pass, gate fail leading to rollback, manual rollback;
  - fleet: drift and convergence;
  - risk adapter: Mimir up and down;
  - agent: slot swap, crash-mid-swap journal, refusing an unverified artefact. These run against fake services only.
- **End to end in a temporary folder:** bifrost-server plus the agent managing fake services that turn healthy or unhealthy on cue. It checks a full rollout, an automatic rollback, and a refused tampered artefact.
- **Console:** SDK client tests against recorded replies, plus type checks.
- **Live acceptance on this Mac,** with the Bifrost console open:
  1. Register the running suite as 0.1.0.
  2. Build 0.2.0: it is verified, shows its SBOM, and has a risk score.
  3. Promote it ring by ring, gateway last, with Heimdall staying up; Fleet & drift ends converged.
  4. Promote the bad build 0.2.1: the canary gate fails and everything rolls back to 0.2.0 automatically.
  5. A tampered upload is refused and the refusal is audited.
  6. Export 0.2.0 as a bundle and re-import it.
- **Done when:**
  - all six screens are live, with no mock data left in the Bifrost console;
  - the six acceptance steps pass on this Mac;
  - the main enclosure is unharmed;
  - `just verify-bifrost` still passes.

## Phases

1. **B1:** bifrost-server and the artefact registry, with the native tools brought onto the branch.
2. **B2:** the orchestrator and the suite-mode agent.
3. **B3:** the gateway bridge, `bifrost-http.ts` and the live console.
4. **B4:** bundles, Mimir risk, and the live acceptance run.
