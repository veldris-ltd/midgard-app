# Midgard

> **The sovereign enclosure.** A sovereign, open-source, post-quantum alternative to the Palantir platform family — one core platform and four composable products, built and verified against a complete specification.

Midgard is a **Veldris Ltd** project. This repository holds three things that fit together:

1. **The specification** — the Solutions Architecture Documents (the "what and why").
2. **The design system** — a complete, branded UI system (the "how it looks and behaves").
3. **The implementation** — a post-quantum, air-gap-capable platform built from the spec: a 70-crate Rust workspace (core services, Mimir, Forge, Bifrost, Heimdall, admin, clients) plus the console UIs and gateway edge.

The implementation was produced, phase by phase, from [BUILD_PLAYBOOK.md](BUILD_PLAYBOOK.md); in the full working history, one commit maps to each playbook prompt. This public repository begins from a squashed initial import of that verified state.

- **Just want to see it?** → [Run the design system](#run-the-design-system) (static files, no toolchain).
- **Want to run it?** → [Run the platform locally](#run-the-platform-locally) (gateway edge + console, step by step).
- **Want to verify it?** → [Verify the platform](#verify-the-platform) (build, test, and the 72-criterion air-gapped gate).
- **Want to understand how it was built?** → [GETTING_STARTED.md](GETTING_STARTED.md) and [BUILD_PLAYBOOK.md](BUILD_PLAYBOOK.md).

---

## Status

**Implementation: complete and verified. Release: not yet cut.**

The code is finished and the acceptance gate passes end to end. Most recent full run, at commit `60091a9`, inside the pinned `rust:1.82` container under `docker run --network none`:

| | |
|---|---|
| Acceptance criteria | **72 / 72 pass** |
| Tests executed | **248 passed · 0 failed · 0 ignored** |
| Phase definitions-of-done | **8 / 8 met** |
| Network during the run | **none** (`--network none`) |
| Exit | `0` — `>>> FULL SUITE PASSED, AIR-GAPPED` |

What that does **not** yet mean:

- **No release has been tagged yet.** Version `0.1.0-rc.1` is prepared: the release workflow, the verify-before-run images and the signed air-gap bundle are in place (see [the release candidate notes](docs/releases/v0.1.0-rc.1.md)). Tagging it needs a key ceremony and a provisioned signing runner, which are the owner's to do, so `.github/workflows/release.yml` has not run yet.
- **Most subsystems are still libraries, not services.** Every crate is implemented and tested. Five are running services so far:
  - `http-gateway`, the enclosure's edge
  - `mimir-server`, the inference plane over PQ-mTLS
  - `bifrost-fleet`, the fleet manager agents check in with
  - `heimdall-feed`, where feed sources submit signed observations to the operating picture
  - `forge-worker`, which syncs sources into versioned datasets and on into the enclosure's fabric

  All five run under docker compose. See [Run the services with docker compose](#run-the-services-with-docker-compose) and [What is not yet runnable](#what-is-not-yet-runnable).
- **The gateway edge runs on the real `fabric` + `governance` crates, and can serve a real enclosure.** Queries, actions, search-around, audit and policy dry-runs go through the object fabric and its single PQ-signed ledger. Development mode keeps everything in memory with one built-in login. A state directory created by `mg-admin` gives the gateway persisted keys, a durable, encrypted store, ledger and identity directory, a policy file and enrolled users (optionally with TOTP). Its schema, including the object types, links and human-gated actions, comes from an ontology file. Data is loaded offline with `mg-admin import`, which checks every record against the ontology before anything is written. What a real enclosure does not have yet is Forge connectors syncing from live sources. See [Run a real enclosure](#run-a-real-enclosure-no-demo-data).
- **GA is gated on external work** — FIPS 140-3 module certification and deployment-tier assurance on real HSM / GPU / cluster hardware.

The authoritative go/no-go record is **[docs/compliance/GA_READINESS.md](docs/compliance/GA_READINESS.md)**, which currently reads **NO-GO** and names every blocker. The step-by-step gate procedure is [docs/ops/RUNBOOK_ga_gate.md](docs/ops/RUNBOOK_ga_gate.md).

---

## The suite

Midgard is the core platform — *the enclosure*: identity, the object/ontology fabric, storage, compute, governance, and post-quantum cryptography applied by default. Atop it sit four independently useful, composable products, each a direct competitor to a Palantir counterpart.

| Product | Domain | Competes with | Accent |
|---|---|---|---|
| **Midgard** | Core platform, identity, object fabric, governance, PQ crypto substrate | Palantir platform core (Ontology / Foundry substrate / Apollo infra) | Enclosure red `#CA1D25` |
| **Forge** | Data operations — ingest, transform, version, project to the fabric | Foundry | Ember orange `#D96E1E` |
| **Mimir** | Local agentic AI (GLM / Nemotron / Gemma), OAG grounding, agents, Evals | AIP | Well teal `#23A8B6` |
| **Bifrost** | Continuous delivery — PQ-signed artefacts, rings, fleet, offline bundles | Apollo | Bridge violet `#7D5CE6` |
| **Heimdall** | Operational fusion — feeds, one common picture, decision workflows | Maven Smart System | Watchman gold `#C99A22` |

Two cross-cutting layers complete the suite:

- **Admin Control Panel** — one unified, self-governing console over IAM, Governance, KMS, Fabric, Mimir, Bifrost and Observability. Role-separated RBAC (no super-admin), dual-control on all high-risk actions, time-boxed break-glass, GitOps configuration, and a PQ-signed append-only audit ledger.
- **Client Platforms** — native mobile (iOS / Android / Linux), native desktop (Tauri: macOS / Windows / Linux), and web (WASM app + broker-mode browser extension). Offline-first, key-custody-aware, tiered inference.

### Design principles

- **Sovereign by construction.** No core function requires an outbound network call. The platform runs fully offline / air-gapped — the acceptance suite proves it under `--network none`.
- **Post-quantum by default.** Hybrid X25519 + ML-KEM-768 in transit; AES-256-GCM under ML-KEM-1024-wrapped keys at rest; ML-DSA / SLH-DSA signatures for service auth and artefact signing. `[STD:NIST FIPS 203/204/205]`
- **Object fabric at the centre.** A decision-centric semantic + kinetic layer (objects, properties, links; actions, functions, dynamic security) — the answer to Palantir's Ontology.
- **Local AI native.** Mimir provides all inference in-enclosure; every product calls Mimir, never a hosted LLM.
- **Provenance everywhere.** Every claim has a source, every action has lineage. Provenance tags, append-only audit, and human gates are first-class, not afterthoughts.
- **Open-source core.** Reproducible builds, CycloneDX SBOM per release, OSI-approved licence for the core.

---

## Repository layout

```
midgard/
├── README.md · GETTING_STARTED.md · BUILD_PLAYBOOK.md   ← docs (spec workflow + build playbook)
├── Cargo.toml · justfile · flake.nix · rust-toolchain.toml   ← workspace + hermetic toolchain
│
├── contracts/         ← proto contracts: the cross-SDK source of truth (buf)
├── crypto/            ← cross-cutting: pqc-agility, pq-mtls, audit-writer, policy, attest
├── services/          ← Midgard core: gateway, http-gateway, iam, fabric, kms, governance, scheduler, telemetry, runtime
├── mimir/             ← model-server, mimir-server, bridge, prompts, oag-retriever, agent-runtime, staging
├── forge/             ← connector-sdk, dataset-store, engine, pipeline, health, projection, ai-assist, worker
├── bifrost/           ← artefact, rollout, fleet, fleet-server, bundle, risk
├── heimdall/          ← feed-gateway, feed-server, fusion, classify, picture, picture-sync, decision-support, workflow, store
├── admin/             ← identity, privilege, sim, config, breakglass, audit, ai-assist, service-identity, orchestrator
├── clients/           ← shared core + sync, keystore, store, enclosure, inference, broker, delivery; shells/
├── sdk/ · products/ · ui/   ← generated SDKs, product surfaces, console UIs (Vite/React)
├── tools/             ← CLIs: mg-sign, mg-sbom, mg-scan, mg-verify, mg-repro, mg-bundle, mg-register, mg-admin, mg-model
├── deploy/            ← acceptance gates, container images, k8s + helm, air-gap bundle, trust anchors
├── vendor/            ← vendored crates → fully offline builds
│
└── docs/
    ├── dev/           ← the specification (what to build, and why)
    ├── design/        ← the design system (how it looks & behaves)  → runs as static HTML
    ├── demo/          ← the 1:54 introduction film + its render pipeline
    ├── ops/           ← runbooks: GA gate, DR, backup/restore, incident response, PKI
    └── compliance/    ← GA readiness, residual-risk register, compliance matrix, data residency
```

---

## Prerequisites

| For | You need |
|---|---|
| Design system only | Any static file server (Python, `npx serve`, …) |
| Building / testing the platform | **Docker** (recommended — the pinned image is self-contained), *or* Rust **1.82** + [`just`](https://github.com/casey/just) locally |
| Running a console | **Node ≥ 22** (`engines`) and **pnpm 9.12.0** (`packageManager`) |

The Cargo workspace is pinned to Rust 1.82 (`rust-toolchain.toml`) with every dependency **vendored** (`vendor/` + `.cargo/config.toml`), so builds and tests need no network. Where a command below must reach the network, it says so.

---

## Run the platform locally

This brings up the HTTP/JSON gateway edge and the Midgard console against it. Five steps.

> **Read this first.** These steps start the gateway edge in **development mode**, for trying the consoles. For your own users and no demo data, see [Run a real enclosure](#run-a-real-enclosure-no-demo-data). Development mode means illustrative data seeded into an in-memory object fabric, one built-in credential, and an ephemeral identity key. It is genuinely the real Rust service, with real PQ-signed token issuance, real per-object policy evaluation and a real signed audit ledger — but it is **not** a production deployment and must not be exposed beyond loopback. The gap, and what closing it requires, is spelled out in [What is not yet runnable](#what-is-not-yet-runnable).

### 1 — Install the console dependencies

```bash
pnpm install
```

### 2 — Build the gateway edge

With a local Rust 1.82 toolchain:

```bash
cargo build --release -p http-gateway
```

Without one, use the pinned container (deps resolve offline from `vendor/`):

```bash
docker build -t midgard-acceptance:1.82.0 - < deploy/acceptance/Dockerfile   # one-time, online
docker run --rm -v "$PWD":/work -w /work midgard-acceptance:1.82.0 \
  cargo build --offline --release -p http-gateway
```

### 3 — Start the gateway edge

```bash
just gateway-http                 # equivalent to: cargo run -p http-gateway -- 127.0.0.1:7710
```

Or, from the container:

```bash
docker run --rm -p 7710:7710 -v "$PWD":/work -w /work midgard-acceptance:1.82.0 \
  cargo run --offline --release -p http-gateway -- 0.0.0.0:7710
```

It prints `http-gateway (secured) listening on http://127.0.0.1:7710`. Confirm it is up:

```bash
curl -s http://127.0.0.1:7710/api/v1/health
# {"status":"ok","service":"http-gateway","transport":"HTTP/JSON edge → enclosure"}
```

> Binding anything other than loopback prints a warning and is not supported without PQ-mTLS/TLS-1.3 termination in front of it. The container form above binds `0.0.0.0` inside the container only; the published port still reaches you on loopback.

### 4 — Start the Midgard console

In a second terminal:

```bash
just console-midgard-http         # VITE_GATEWAY=http pnpm --filter @midgard/console-midgard dev
```

Open **http://localhost:5173**. It must be this port — the edge's CORS allowlist contains `http://localhost:5173` and nothing else.

### 5 — Log in

The console presents an auth gate. The edge provisions exactly one login at startup:

| Username | Password |
|---|---|
| `operator` | `midgard-demo` |

These are compile-time constants (`DEMO_USERNAME` / `DEMO_PASSWORD` in `services/http-gateway/src/lib.rs`), published deliberately because this mode is for local development only. There is no user-provisioning interface.

Credentials are exchanged at `POST /api/v1/login` for an **ML-DSA-signed bearer token**, which authorises every subsequent request. A wrong password, a missing token and a forged token all return a uniform `401`.

You can drive the same flow without a browser:

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:7710/api/v1/login \
  -H 'content-type: application/json' \
  -d '{"username":"operator","password":"midgard-demo"}' \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')

curl -s -X POST http://127.0.0.1:7710/api/v1/query \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"objectType":"aircraft"}'
# {"results":[{"id":"aircraft/AC-2198","typeId":"aircraft","propertyValues":{…},"markings":[…]}, …],"auditId":"AUD-…"}
```

The operator is cleared to `confidential` with compartment `OPS-N`, so the four aircraft come back. Objects the caller can't read are filtered out per object (the `SIG`-compartment sensor feed never appears). Every response carries the `auditId` of the signed ledger entry that authorised it; `GET /api/v1/audit` lists those entries, each re-verified (`ML-DSA-65 ✓`).

### What you get

Authenticated requests are authorised through the real policy PDP and audited through the governance spine before anything is served. Available on the edge:

| Endpoint | Serves |
|---|---|
| `GET /api/v1/health` · `/api/v1/ready` · `/metrics` | liveness, readiness, in-enclosure Prometheus metrics |
| `GET /api/v1/object-types` | the object-type registry |
| `POST /api/v1/query` · `/api/v1/search-around` | object-fabric query and link traversal |
| `POST /api/v1/actions` | human-gated action application |
| `GET /api/v1/policies` · `POST /api/v1/policy/simulate` | policy set and dry-run evaluation |
| `GET /api/v1/audit` | audit lineage |
| `GET /api/v1/identities` · `/keys` · `/compute` · `/observability` | IAM, KMS, compute and observability views |

### Known limits of this mode

| Limit | Detail |
|---|---|
| **Illustrative data, real enforcement** | At startup, `Enclosure::seed_dev` (`services/http-gateway/src/enclosure.rs`) registers a small aviation ontology and seeds its objects into the real object fabric. Markings are enforced per object, and gated actions really mutate the fabric. The platform read views (identities, keys, compute, observability, resilience) are still fixed data in `data.rs`. |
| **One credential** | `operator` / `midgard-demo`, hard-coded. No sign-up, no user management, no rotation. |
| **Ephemeral identity** | The IdP keypair is generated fresh on every start, so all tokens are invalidated by a restart. |
| **Reduced work factor** | PBKDF2 runs at 4 096 iterations for a fast dev login — far below a production figure. |
| **Built-in policy** | The dev policy is compiled in (`dev_login_policy()`): role `operator` may read and apply each registered action. Mandatory access control (clearance and compartments) and the purpose gate still run first, and anything not granted is denied. There is no policy file yet. |
| **One console** | Only `console-midgard` (port 5173) is in the CORS allowlist. The other six consoles are mock-only — see below. |
| **No persistence** | Everything is in memory and lost on exit. |

### Troubleshooting

**The console loads but logging in does nothing.** Almost always the port. Vite falls back to the next free port when 5173 is taken, printing only `Port 5173 is in use, trying another one...` — and the edge's CORS allowlist contains `http://localhost:5173` and nothing else. The request still succeeds server-side (`200`), but the response carries no `Access-Control-Allow-Origin` header, so the browser discards it and the UI reports nothing useful. Free port 5173, or start Vite with `--strictPort` so a clash fails loudly instead of drifting:

```bash
VITE_GATEWAY=http pnpm --filter @midgard/console-midgard dev -- --strictPort
```

**`Connection refused` on 7710.** The edge isn't running, or it bound inside a container without `-p 7710:7710`.

**You were logged in, then everything returned 401.** The gateway was restarted. Its IdP keypair is generated fresh on every start, so previously issued tokens can no longer verify. Log in again.

**`401` on every login attempt.** The credentials are exactly `operator` / `midgard-demo`; there is no other account and no way to add one in this mode.

### The other consoles

Six more consoles exist and run, but only against their in-browser `MockGateway` fixtures — the edge exposes neither their routes nor their origins:

```bash
VITE_GATEWAY=mock pnpm --filter @midgard/console-forge    dev   # :5174
VITE_GATEWAY=mock pnpm --filter @midgard/console-mimir    dev   # :5175
VITE_GATEWAY=mock pnpm --filter @midgard/console-heimdall dev   # :5176
VITE_GATEWAY=mock pnpm --filter @midgard/console-bifrost  dev   # :5177
VITE_GATEWAY=mock pnpm --filter @midgard/console-admin    dev   # :5178
VITE_GATEWAY=mock pnpm --filter @midgard/client-shells    dev   # :5179
```

---

## Run a real enclosure (no demo data)

This serves an enclosure from a **state directory**: persisted issuer and audit keys, a durable, encrypted object store, audit ledger and identity directory, a policy file and principals you enrol. Nothing is seeded, and there is no built-in login.

### 1 — Build the gateway and the admin tool

```bash
cargo build --release -p http-gateway -p mg-admin
```

(Or through the pinned container, as in [step 2 above](#2--build-the-gateway-edge), with `-p http-gateway -p mg-admin`.)

### 2 — Create the state directory

```bash
./target/release/mg-admin init --state-dir ./enclosure
```

This writes:

| Path | What it is |
|---|---|
| `keys/idp.keystore`, `keys/audit.keystore` | the ML-DSA-65 issuer and audit keys, each sealed under its own ML-KEM-1024 custody root. **Protect `keys/` as a secret**: whoever can read it can sign as the enclosure and read its data. |
| `trust/idp.pub`, `trust/audit.pub` | their verifying keys, safe to distribute |
| `policy.json` | the discretionary rules the PDP enforces. The starter file lets role `operator` Read and Apply. |
| `ontology.json` | the object types, link types and actions the enclosure serves. The starter file declares none. |

`init` refuses a directory that already has anything in it.

### Declare an ontology

Edit `ontology.json` before serving; the gateway registers it on every start and refuses to start if it is invalid:

```json
{
  "version": 1,
  "objectTypes": [
    { "id": "aircraft", "title": "Aircraft",
      "properties": [ { "name": "tail", "type": "text", "required": true },
                      { "name": "status", "type": "text" } ] }
  ],
  "actions": [
    { "id": "ground_aircraft", "name": "groundAircraft", "title": "Ground aircraft",
      "objectType": "aircraft", "marking": "confidential", "purposes": ["operations"],
      "effect": { "kind": "set-property", "property": "status", "value": "grounded" } }
  ]
}
```

- Property types are `text`, `int`, `float`, `bool` and `timestamp`.
- `linkTypes` entries take `id`, `name`, `from`, `to` and a `cardinality` of `one-to-one`, `one-to-many` or `many-to-many`.
- An action is **human-gated unless it says `"humanGated": false`**. A caller needs clearance for its `marking` (a band, plus any `compartments`) and one of its `purposes`.
- A caller also needs a policy rule granting the action's `name`, for example `{"id":"operator-ground","role":"operator","action":"groundAircraft","resource":"*","effect":"permit"}`.
- Effects are `set-property` (`property`, `value`) and `cycle-property` (`property`, `values`), each on a text property of the action's object type.
- Unknown keys, undeclared types or properties and duplicate ids refuse the whole file.

### Import data

`mg-admin import` adds objects and links from a JSON Lines file, one record per line, to the enclosure's durable store. Run it with the gateway stopped:

```bash
./target/release/mg-admin import --state-dir ./enclosure --file ./fleet.jsonl
```

```text
{"object":"aircraft","key":"AC-2214","marking":"confidential","compartments":["OPS-N"],"purposes":["operations"],"properties":{"tail":"MG-404","status":"active"}}
{"object":"facility","key":"FAC-NORTH","marking":"restricted","purposes":["operations"],"properties":{"name":"Northern operating base"}}
{"link":"aircraft_base","from":"AC-2214","to":"FAC-NORTH"}
```

- An object's id becomes `<object type>/<key>`, for example `aircraft/AC-2214`.
- Every property must be declared by its type in `ontology.json`, as the right kind of JSON value: a string for `text`, an integer for `int`, any number for `float`, `true` or `false` for `bool`, and Unix seconds for `timestamp`. Required properties must be present.
- `marking`, `compartments` and `purposes` decide who can read each object. A caller sees it only with clearance that dominates its marking and one of its purposes.
- A link's `from` and `to` are keys of objects of the link type's declared source and target types, either in the same file or already in the store.
- It is all or nothing. The first bad line refuses the file and names the line, and so does an object that already exists: an import never replaces anything. Nothing is written until every record has passed.
- Each import is recorded in the audit ledger before it is written.

### 3 — Enrol a user

The password is read from standard input, and only from a pipe or a file: `mg-admin` refuses a terminal, because it cannot stop it echoing. Read it without echo first:

```bash
read -rs PASSWORD
printf '%s\n' "$PASSWORD" | ./target/release/mg-admin user add --state-dir ./enclosure \
  --subject olivia --roles operator --clearance internal --purposes operations --totp
unset PASSWORD
```

- `--clearance` is a marking band: `open`, `internal`, `restricted` or `confidential`. `--compartments A,B` adds compartments.
- `--totp` also enrols a TOTP second factor and prints its `otpauth://` URI once, for the user's authenticator app. Their logins then need the current code as well as the password.
- Passwords must be at least 12 characters. An enrolled user is never replaced.
- Every enrolment is recorded in the audit ledger before it takes effect.

`./target/release/mg-admin user list --state-dir ./enclosure` shows who is enrolled.

### 4 — Serve it

```bash
./target/release/http-gateway --state-dir ./enclosure --allow-origin http://localhost:5173
```

At startup the gateway re-verifies every recovered audit entry and refuses to start if anything fails to authenticate. That covers a tampered or mismatched keystore, journal or ledger, and an invalid policy file. It prints how many users are enrolled. `--bind ADDR` changes the address (default `127.0.0.1:7710`), and each `--allow-origin` adds a browser origin (there are none by default).

Log in from the Midgard console ([step 4 above](#4--start-the-midgard-console)) with the user you enrolled. From the command line, add `"totp":"123456"` to the login body when the user has a second factor:

```bash
curl -s -X POST http://127.0.0.1:7710/api/v1/login -H 'content-type: application/json' \
  -d '{"username":"olivia","password":"…","totp":"123456"}'
```

Tokens and the audit ledger survive a restart.

### Administrators and online enrolment

Users can also be enrolled **while the gateway runs**, under dual control. One administrator requests, and a different one approves. Administrators prove who they are with a post-quantum passkey (ML-DSA-65), not a password.

Make two administrators first, with the gateway stopped. Each gets a passkey in a sealed keystore file that is **theirs to keep**. Only its public half is registered in the state directory:

```bash
./target/release/mg-admin admin add --state-dir ./enclosure --admin alice --role IAM-Admin --keystore ./alice.passkey
./target/release/mg-admin admin add --state-dir ./enclosure --admin bob   --role IAM-Admin --keystore ./bob.passkey
```

Roles are `IAM-Admin`, `Security-Officer`, `Model-Steward`, `Release-Manager` and `Auditor`. A binding covers `--scope` (default `iam`) for `--days` (default 365). Enrolling needs `IAM-Admin`.

Then, with the gateway running, alice requests and bob approves:

```bash
read -rs PASSWORD
printf '%s\n' "$PASSWORD" | ./target/release/mg-admin enrol request \
  --gateway http://127.0.0.1:7710 --admin alice --keystore ./alice.passkey \
  --subject olivia --roles operator --clearance internal --purposes operations \
  --justification "new duty operator"
unset PASSWORD

./target/release/mg-admin enrol list    --gateway http://127.0.0.1:7710 --admin bob --keystore ./bob.passkey
./target/release/mg-admin enrol approve --gateway http://127.0.0.1:7710 --admin bob --keystore ./bob.passkey --id <enrolment id>
```

- Until the approval, olivia cannot log in.
- alice cannot approve her own request, and an administrator without the IAM user capability cannot approve it.
- A waiting request survives a gateway restart.
- The password is hashed on alice's machine, and only the hash is sent. Each command signs a fresh single-use challenge.
- Requests, approvals and refusals all appear in the audit ledger.
- `--gateway` must be a loopback address, because administration runs over plain HTTP.

### Limits of a real enclosure today

| Limit | Detail |
|---|---|
| **TLS to sources only through the egress broker** | Data arrives through `mg-admin import` files, with the gateway stopped, or through the Forge worker while it runs (see [Run the services with docker compose](#run-the-services-with-docker-compose)). The worker's connectors speak plaintext themselves. Sources outside the enclosure's network are reached through the egress broker, which originates TLS 1.3 (see [Reach sources outside the enclosure](#reach-sources-outside-the-enclosure)). PostgreSQL cannot go through it, because PostgreSQL before 17 negotiates TLS inside its own protocol. |
| **Imports only add** | An import never replaces or deletes an object. Declared actions change properties of objects that exist. |
| **Schema and policy changes need a restart** | The gateway reads `ontology.json` and `policy.json` at startup. |
| **Most administration is offline** | Enrolment works while the gateway runs, but `init`, `user add`, `import` and `admin add` need `midgard.lock`, which the gateway holds, so stop it first. A gateway killed outright leaves the lock behind. On Linux the next start detects that its holder is gone and takes it over; elsewhere you remove it after confirming nothing is running. |
| **No administration console** | Online enrolment is command-line only. A browser cannot produce the post-quantum passkey assertions administrators sign with. |
| **Software key custody** | The keystores are files. HSM-backed custody is not wired in. |
| **Platform views are still fixed data** | Identities, keys, compute, observability and resilience views come from `data.rs`. |

---

## Run the services with docker compose

Compose runs the enclosure's HTTP edge, the Mimir inference plane, the Bifrost fleet manager, the Heimdall feed gateway and the Forge worker as separate containers. Each runs as a non-root user with a read-only root filesystem and no capabilities, and mounts only its own volumes. The image is built from this checkout, not from a signed release, so use it for development and evaluation.

Build the pinned toolchain image once. It compiles the services without network access, against the vendored crates:

```bash
docker build -t midgard-acceptance:1.82.0 deploy/acceptance
```

Then build and start everything:

```bash
just compose-up
```

- **`init`** runs first. `mg-admin init` creates the enclosure's keys and state. `mg-admin mesh issue` then gives each mesh workload an identity key, a separate audit key and a certificate signed by the enclosure's issuer: `mimir-server`, `mimir-probe`, `bifrost-fleet`, the agent `fleet-agent-1`, the operator `fleet-operator`, `heimdall-feed`, the source `feed-source-1`, the analyst `picture-analyst`, the gateway's own mesh identity `http-gateway`, and `forge-worker`. A fresh enclosure also gets a demonstration ontology (aircraft, facilities and the link between them) and a policy that lets the worker sync into it. `init` never overwrites anything, so it is safe to run again.
- **`gateway`** serves the edge from that state on `http://127.0.0.1:7710`. On the internal network it also opens a PQ-mTLS door, `SyncRecords`, for certified workloads to sync objects and links into the fabric. Each sync is handled in this order:
  1. It is authorised by `policy.json` and audited in the enclosure's one ledger.
  2. It is checked against the ontology, all or nothing, and what it will change is recorded before anything is written.
  3. Objects are created or replaced, and a link that already exists is not added again.
- **`mimir-server`** serves inference over PQ-mTLS only, on an internal network with no host port. A caller needs a certificate from the enclosure's issuer, and `policy.json` must let its roles open the channel and `Infer`. Every call, permitted or denied, is written to the service's own sealed, PQ-signed ledger, which it re-verifies whenever it starts. Inside the network it answers `/healthz`, `/readyz` and `/metrics` on port 9090. Compose waits for readiness.
- **`bifrost-fleet`** is the fleet manager, on the same internal network. Its `channels.json` sets the version each release channel should run. Agents check in over PQ-mTLS with the version they run and their health. An agent can check in only for the installation its certificate names. Operators read every installation's version and liveness, the drift from its channel and whether the fleet has converged. Every check-in and read is authorised by `policy.json` and audited. Accepted check-ins are journalled, sealed, so a restart recovers them.
- **`heimdall-feed`** is the feed gateway, on the same internal network. A source submits observations signed with its certified mesh key. An observation is fused into the operating picture only if all four checks pass:
  - policy lets the source `Ingest`
  - the observation names the source's own feed
  - its signature verifies against the key the source's certificate binds
  - the decision is audited

  Refusals are audited too. Observations that share an entity key fuse into one tracked entity. The picture is sealed at rest after each change and recovered on restart. Analysts read it as JSON.
- **`forge-worker`** runs the jobs in `deploy/compose/forge/jobs.json`: each at start, then at its own interval. A run does four things:
  1. It reads a CSV source through Forge's file connector, under the air-gapped sandbox.
  2. It commits the rows to the worker's sealed, content-addressed dataset store with ingest lineage. An unchanged source is the same version.
  3. It projects the rows onto the ontology, carrying the job's marking and purposes onto every object.
  4. It syncs the objects and links through the gateway's door.

  Every run is audited in the worker's own ledger and logged as a JSON line.

Make one inference call as a certified consumer:

```bash
just compose-probe
```

It prints the model, where it ran, the call's audit sequence number, the first-token latency and the number of tokens generated, for example `model=local-reference locality=InEnclosure audit_seq=0 first_token_ms=13 tokens=5`, followed by the completion.

Check an installation in, then read the fleet as an operator:

```bash
docker compose -f deploy/compose/compose.yaml run --rm fleet-agent --channel stable --version 0.1.0
```

```bash
docker compose -f deploy/compose/compose.yaml run --rm fleet-status
```

The status is JSON: each channel's desired version, each installation's version, status (`live`, `unhealthy`, `stale` or `absent`) and last check-in, the drift, and `converged`.

Submit a signed observation, then read the fused picture as an analyst:

```bash
docker compose -f deploy/compose/compose.yaml run --rm feed-source --kind air-track --entity AC-7 --attrs speed=410,callsign=ALPHA1
```

```bash
docker compose -f deploy/compose/compose.yaml run --rm picture
```

The picture is JSON: its layers, and each tracked entity with its kind, layer, contributing feeds, first and last seen, observation count, classification and latest attributes.

`just compose-smoke` does all of this on throwaway volumes. It restarts `mimir-server` between two inference calls and checks that the second continues the recovered ledger (`audit_seq=1`). It checks an installation in behind its channel, sees the drift, upgrades it and sees the fleet converge. It submits two observations of one entity and sees them fused. It waits for the Forge worker's first sync of the sample aircraft into the fabric. Then it removes everything.

See what the worker synced:

```bash
docker compose -f deploy/compose/compose.yaml logs forge-worker
```

Each run line names the job, the dataset version, whether the source changed, and what the gateway applied, for example `synced: 3 created, 0 updated, 3 link(s) added`. To read the synced objects in a console, enrol an operator (see [Enrol a user](#3--enrol-a-user); stop the gateway first) and log in. `just compose-down` stops the services and keeps their volumes.

### Serve a real model

By default `mimir-server` answers with the deterministic reference engine. It can instead serve IBM Granite 3.3 2B Instruct on llama.cpp's CPU server. The model is Apache-2.0 licensed, and this build is 4-bit GGUF, about 1.5 GB. Midgard ships no weights, so download the file yourself from the pinned revision:

```bash
curl -L -o granite-3.3-2b-instruct-Q4_K_M.gguf https://huggingface.co/ibm-granite/granite-3.3-2b-instruct-GGUF/resolve/7cdf86ccd1f1bb3491c9b7017b033f2e51367397/granite-3.3-2b-instruct-Q4_K_M.gguf
```

Stage it into the `midgard-models` volume (build the image first with `docker compose -f deploy/compose/compose.yaml build init`):

```bash
just model-stage granite-3.3-2b-instruct-Q4_K_M.gguf
```

Staging uses `mg-model`, and does the following:

1. Refuses the file unless its SHA-256 is the one the upstream publishes.
2. Signs a manifest over the weights' digest, file name, format, quantisation and size, the family, the licence and the hardware the model needs.
3. Verifies the model directory the way `mimir-server` will.

The signing key it creates is a development key, kept in the `midgard-model-signing` volume on the same machine. In production the key stays in staging, and only the signed manifest and the public key enter the enclosure.

Then start the enclosure with the Granite override and make a call:

```bash
docker compose -f deploy/compose/compose.yaml -f deploy/compose/compose.granite.yaml up -d --build --wait
```

```bash
docker compose -f deploy/compose/compose.yaml -f deploy/compose/compose.granite.yaml run --rm probe --prompt "What is an audit ledger?" --max-tokens 64
```

What happens when it starts and serves:

- **The model is verified first.** Before it serves, `mimir-server` checks the manifest's ML-DSA-65 signature against the trust key, the family and the hardware the model needs. It then streams the weights through SHA-256 and checks their size, and their format from their own first bytes. Only GGUF and safetensors load. Any failure stops the service.
- **The engine is reachable only from `mimir-server`.** llama.cpp runs in `mimir-server`'s network namespace, listening on its loopback address only, with the models volume mounted read-only by both.
- **Readiness follows the engine.** `mimir-server` is ready only while llama.cpp reports healthy.
- **Calls stay governed.** Every call is still authorised and audited as before, and a failed call to the engine is refused, never answered by another model.

`just compose-granite-smoke` stages the model, starts it on throwaway state and checks that a certified call is answered by Granite. It ran on a 12-core arm64 Docker host:

- `mimir-server` was ready 23 s after start, including verifying the weights.
- A 64-token call was answered with 44 tokens, 325 ms to the first token.

To carry the model into an enclosure with no network, bundle it. `mg-model pack` packs a verified model directory into one signed bundle, streamed a chunk at a time. On the far side, `mg-model unpack` verifies that bundle offline, writes the model out and runs the load gate on it. See [the air-gap bundle README](deploy/air-gap-bundle/README.md#model-bundles).

On Kubernetes, the chart's `modelServing` values for `mimir-server` run the same engine as a sidecar in its pod:

- The staged model directories come from a PersistentVolumeClaim, mounted read-only.
- The trust key comes from a ConfigMap.
- The same values take vLLM's image and arguments, and a GPU resource, instead.

### Limits of compose today

| Limit | Detail |
|---|---|
| **Forge sources** | The compose worker reads sample files. It can also read PostgreSQL queries, S3-compatible object prefixes and Kafka topics, each incrementally from a checkpoint, with secrets from `--secrets-dir`. PostgreSQL and S3 have been checked against real servers (`just live-connectors`), and S3 through the egress broker to a TLS server; Kafka only against a fake broker. Connectors do not speak TLS themselves, and compose runs no egress broker (the Helm chart does). Jobs apply no transforms: rows are projected as they are read. |
| **A sync never deletes** | A row removed from a source stays in the fabric, and so does a link. |
| **The jobs file sets markings** | The gateway checks that policy lets the worker sync, not that the worker may write data at the marking its jobs file declares. |
| **The picture stays in Heimdall** | Fused entities are not projected into the enclosure's object fabric, and no classifier is attached, so entities are unclassified. The whole picture is re-sealed after every observation, which will not scale to a large picture. |
| **A feed is a mesh identity** | A source's feed id is the service its certificate names, and it signs with its mesh key. Separate feed-device keys are not supported. |
| **The fleet manager only records** | It reports drift but does not push upgrades: nothing drives the rollout orchestrator from it yet. A channel removed from `channels.json` stays declared. |
| **One real model, on CPU** | Without the Granite override, `mimir-server` serves the deterministic reference backend. Granite on llama.cpp has run end to end. vLLM on GPUs is wired in the code and the chart but has not been run, because no GPU was available, so the 800 ms first-token target is unproven. The model-signing key `stage-model.sh` creates is a development key. |
| **Nothing calls Mimir yet** | The gateway does not route AI requests to `mimir-server`. Only `mimir-probe` does. |
| **Not the release image** | The image is built from source and unsigned. |
| **Certificates do not expire** | To rotate a service's key, remove its identity files from its volume and run `init` again. |

---

## Run on Kubernetes with kind

The same five services run on a local three-node Kubernetes cluster through the Helm chart in `deploy/helm/midgard`. You need [kind](https://kind.sigs.k8s.io), [Helm](https://helm.sh), `kubectl` and Docker, plus the toolchain image from the compose section.

```bash
just kind-up
```

`kind-up` does the following, and is safe to run again:

1. Creates the cluster, a control plane and two workers.
2. Builds the image from this checkout and loads it into the cluster.
3. Installs the chart into the `midgard-enclosure` namespace.
4. Provisions the secrets: the same `init` as compose mints the enclosure's keys and every mesh identity, and each becomes a Kubernetes Secret.
5. Waits for every service.

What the chart deploys:

- **One single-replica `StatefulSet` per service, on its own volume.** Each service holds a single-writer, locked state directory, so a second replica would be refused, not share the load.
- **Hardened pods.** Every pod runs non-root with a read-only root filesystem, no capabilities and the restricted pod-security profile.
- **Seeded state.** On first start an init container unpacks the service's provisioned state from its seed Secret.
- **Default-deny networking.** The namespace allows nothing in or out except cluster DNS. Each port is opened only to the pods the chart names: `mimir-server` accepts only `mimir-probe`, and the gateway's mesh door only `forge-worker`.

Then run every mesh client against it:

```bash
just kind-smoke
```

The smoke test runs each client as a Job with its own identity:

- an inference call, then a restart of `mimir-server` and a second call that continues the recovered ledger
- a fleet check-in and status read
- two fused observations
- a check that the Forge worker synced into the fabric
- a pod outside `mimir-server`'s ingress policy, which must fail to reach it

`just kind-down` deletes the cluster.

To serve Granite on the cluster instead of the reference engine, stage the model for compose first (see [Serve a real model](#serve-a-real-model)), then:

```bash
just kind-granite
```

`kind-granite` downloads nothing. It does the following:

1. Loads the pinned llama.cpp image into the cluster.
2. Streams the staged model into a PersistentVolumeClaim, on the node that holds `mimir-server`'s state.
3. Runs `mg-model verify` on it there.
4. Turns on the chart's `modelServing`.

`mimir-server` then verifies the model again itself before it serves. To keep serving it when you re-run `just kind-up`, set `MIDGARD_KIND_MODEL=granite`.

The deployment-tier acceptance suite runs its checks against this deployment:

| Check | What it does |
|---|---|
| RC-40.1 | Checks that every service comes back ready from a restart on its recovered state. |
| RC-40.2 | A PQ-mTLS handshake between two nodes. |
| RC-40.4 | One certified PQ-mTLS inference call to the deployed `mimir-server`, which must be answered by a real model. It passes only when the model runs on a GPU inside the 800 ms first-token budget. On kind the CPU completion and its latency are reported, and the check skips. |

The HSM check skips too, so the best result on kind is a partial pass, never a promotion:

```bash
ALLOW_SKIP=1 MIDGARD_PROBE_IMAGE=midgard/dev:local just deploy-accept
```

`deploy/k8s/enclosure.yaml` is the chart rendered with its release values, for `kubectl apply` without Helm. `just render-k8s` regenerates it, and CI fails if it is not exactly what the chart renders. Those values expect signed per-service images, which are not built yet.

### Reach sources outside the enclosure

The worker's connectors speak plaintext, and only to the hosts their jobs name. For a source outside the enclosure's network, turn on the egress broker. It runs HAProxy beside the worker in its pod, pinned by digest. Each route takes a local port and originates TLS 1.3 from it to one upstream, verifying the upstream's certificate against a CA pinned for that route, and its name.

Declare the routes in a ConfigMap (key `routes.json`):

```json
{ "version": 1,
  "routes": [ { "name": "exports", "listen": 19000,
                "upstream": { "host": "minio.example.org", "port": 9000 },
                "ca": "exports-ca.pem" } ] }
```

Then point the job's source at `127.0.0.1:19000`. Put each route's CA file in a Secret, and set the chart's values:

```yaml
egressBroker:
  enabled: true                          # for forge-worker
  routesConfigMap: forge-egress-routes   # routes.json
  caSecret: forge-egress-ca              # exports-ca.pem, ...
  upstreamPorts: [9000]                  # the only ports the pod may reach outside the cluster
```

Set `sourcesSecret` on the `forge-worker` service to the Secret holding what the jobs' sources name (passwords, secret keys), mounted for `--secrets-dir`.

When the pod starts, an init container runs `forge-egress`, which renders HAProxy's whole configuration from the routes. The same check runs again when the worker starts with `--egress-routes`. It refuses a routes file that could inject configuration, and any job whose network source is not one of the broker's local ports. So an undeclared host is unreachable twice: the connector may not dial it, and the broker has no route to it.

S3 and Kafka can go through the broker. PostgreSQL cannot, because PostgreSQL negotiates TLS inside its own protocol, and before PostgreSQL 17 no TLS-originating proxy can speak it. `just live-connectors` checks S3 through the broker against a MinIO server that serves TLS. It also checks that a route pinned to the wrong CA fails, and that plaintext sent straight to that server fails.

---

## What is not yet runnable

The engineering is done and tested; the service packaging is not. Being precise about the difference:

**There are five server binaries.** `services/http-gateway` is the edge, and it composes `fabric`, `kms`, `iam` and `governance` in one process; its mesh door lets the Forge worker write into that fabric. `mimir/mimir-server` is the inference plane, `bifrost/fleet-server` the fleet manager, `heimdall/feed-server` the feed gateway and `forge/worker` the Forge worker. All four are built on `services/runtime`, which loads a mesh identity from files, opens a durable audit ledger, runs the PQ-mTLS accept loop, serves health and metrics, and gives clients a one-call PQ-mTLS request. The rest of Forge, Bifrost and Heimdall, and `scheduler` and the admin crates, are still libraries: implemented and exercised by the 72-criterion gate, but with no `main.rs` and no wire listener. The Helm chart deploys all five services; they have been run on a local kind cluster, not on production infrastructure.

**The edge serves a real enclosure, filled offline.** `http-gateway` serves the object-fabric routes from `fabric::Fabric`, which owns the single `governance` spine, so edge admission and every fabric call are written to one PQ-signed ledger. From a state directory it runs on persisted keys, a durable store, ledger and identity directory, a policy file, an ontology file, users enrolled with `mg-admin`, and data imported with `mg-admin`.

Running the platform on live data therefore still needs, in rough order:

1. Reach PostgreSQL outside the enclosure's network. The egress broker covers S3 and Kafka; PostgreSQL needs its own TLS negotiation in the connector, or PostgreSQL 17's direct TLS behind the broker.
2. Build and sign a release image per service, so the chart's release values can run on a real cluster, and run the deployment tier there with an HSM and a GPU.

None of this is blocked on unsolved problems — the libraries underneath are built and verified.

---

## Verify the platform

### Build and test

```bash
just build      # cargo build --workspace --release; Go/TS/Python members build themselves
just test       # runs BOTH crypto backends explicitly (deterministic test-backend + FIPS 203/204/205)
```

> `just test` deliberately does **not** use `cargo test --workspace --all-features` — the crypto crates gate their backend behind mutually-exclusive features, and a naive workspace test would compile those tests out. Use `just test`.

Without a local toolchain:

```bash
docker run --rm -v "$PWD":/work -w /work midgard-acceptance:1.82.0 just build
```

### The air-gapped acceptance gate

The strongest evidence the platform meets the spec: the **entire 72-criterion suite** built and run with the network cut. One command bakes the pinned image (the only online step) and runs everything under `docker --network none`:

```bash
bash deploy/acceptance/run-airgapped.sh
#   Windows Git Bash:
#   MSYS_NO_PATHCONV=1 REPO='D:\path\to\midgard' bash deploy/acceptance/run-airgapped.sh
```

Expect a tail of `>>> FULL SUITE PASSED, AIR-GAPPED (--network none).` and exit `0`. Allow roughly two hours from cold — it compiles the workspace several times over. Any single failing criterion fails the whole run.

Individual phase gates, each mapping its criteria to concrete tests and reporting PASS/FAIL per id:

```bash
# inside the pinned container, from the workspace root
bash deploy/acceptance/verify-core.sh        # AC-M-1..9  (Midgard core)
bash deploy/acceptance/verify-mimir.sh       # AC-MI-1..8 + AC-MS-1..9
bash deploy/acceptance/verify-forge.sh       # AC-F-1..7
bash deploy/acceptance/verify-bifrost.sh     # AC-B-1..6
bash deploy/acceptance/verify-heimdall.sh    # AC-H-1..7
bash deploy/acceptance/verify-admin.sh       # AC-AD-1..13
bash deploy/acceptance/verify-clients.sh     # AC-C-1..13
bash deploy/acceptance/verify-shells-accessibility.sh   # AC-C-11 (static HIG conformance)
```

`run-airgapped.sh` covers everything above except the **on-device shell audit**, which is device-bound and cannot run in a container:

```bash
bash deploy/acceptance/verify-shell-audits.sh <release>   # e.g. 1.4.0
```

It does not perform an audit — it verifies that a real one ran and was recorded for every shell, and fails closed otherwise. See [BUILD_PLAYBOOK.md → Appendix B](BUILD_PLAYBOOK.md#appendix-b--consolidated-acceptance-criteria-matrix-72) for the full matrix.

### The supply-chain release gate

Every binary the workspace builds passes the same fail-closed chain: reproduce → SBOM → scan → sign → verify.

```bash
just keygen                                   # once: ML-DSA keypair + exported trust anchor
export MIDGARD_SIGNING_KEYSTORE=mg.keystore   # never searched for; unset means refuse to sign
export MIDGARD_TRUST_ANCHORS=mg.pub           # never searched for; unset means refuse to verify
just gate http-gateway                        # the full chain for one package's binary
just gate forge-worker forge-egress           # a package that builds several binaries, one at a time
```

`just gate` builds the package **twice** under a normalised environment, proves the outputs bit-identical, and then SBOMs, signs and verifies *that proven artefact* — signing anything else would produce a signature over a binary nobody can reproduce from source. `supply-chain/repro-exceptions.txt` must be empty at GA; it is.

`supply-chain/release-artefacts.txt` lists every binary the workspace builds and how it ships. `bash tools/ci/release-artefacts.sh --check` fails if any binary is missing from the list, so none can skip the gate. On a provisioned signing runner, the release workflow (`.github/workflows/release.yml`) does the following:

1. Gates each binary.
2. Admits each binary to the Bifrost registry.
3. Builds the verify-before-run images.
4. Packs the signed air-gap bundle.
5. Publishes at most a draft pre-release.

See [the release candidate notes](docs/releases/v0.1.0-rc.1.md) and `docs/ops/PKI.md`.

At run time, `deploy/images/entrypoint.sh` re-verifies the signature, SBOM and provenance against the pinned release trust key before exec'ing any service — a tampered or unsigned binary refuses to start.

---

## Run the design system

Static HTML/CSS/JSX — any static file server works. React/Babel load from CDN on first paint, then cache.

```bash
cd docs/design
python -m http.server 8099        # or: npx serve -l 8099 .
# open http://localhost:8099/docs/index.html
```

| Surface | URL (server root = `docs/design`) |
|---|---|
| **Design-system docs** (foundations, 28 components, patterns, screens, flows) | `/docs/index.html` |
| **Midgard console** — Object Explorer | `/ui_kits/midgard-console/index.html` |
| **Forge** · **Mimir** · **Bifrost** · **Heimdall** | `/ui_kits/forge/index.html` · `/mimir/` · `/bifrost/` · `/heimdall/` (each `…/index.html`) |
| **Admin** — Control Panel | `/ui_kits/admin/index.html` |
| **Clients** — mobile / desktop / web | `/ui_kits/clients/mobile.html` · `desktop.html` · `web.html` |
| **Marketing site** | `/site/index.html` |

> **Troubleshooting.** Blank page / "Failed to fetch" → you opened `file://`; serve over `http://`. 404 on `styles.css` → your server root isn't `docs/design`. Fully offline → vendor `react`, `react-dom` and `babel` locally and repoint the three `unpkg` `<script>` tags.

These UI kits are **reference designs**. The shipping console UIs live under `ui/` and `products/` and bind to the gateway edge.

---

## The specification

`docs/dev/markdown/` holds the Solutions Architecture Documents, authored to the **Imhotep** methodology (arc42 + TOGAF + C4; security aligned to NIST SP 800-53 / OWASP ASVS / ISO 27001). Every architectural decision carries a provenance tag: `[DD-*]` design driver, `[FR-*]`/`[NFR-*]` requirement, `[STD:*]` external standard. The SADs are the design authority; the code realises them, and every acceptance criterion traces back to a requirement here.

| Document | Covers |
|---|---|
| `SAD_00_Midgard_Core` | The sovereign core platform / enclosure |
| `SAD_01_Forge` | Data operations (vs Foundry) |
| `SAD_02_Mimir` | Local agentic AI (vs AIP) |
| `SAD_02b_Mimir_Model_Supply_Resolution` | DCR: multi-model provenance (GLM / Nemotron / Gemma) |
| `SAD_03_Bifrost` | Continuous delivery (vs Apollo) |
| `SAD_04_Heimdall` | Operational fusion (vs Maven) |
| `SAD_05_Master_Prompt_and_Acceptance_Spec` | **Consolidating master:** deployable prompt library + agent-flow architecture + all 72 acceptance criteria + build-capabilities register |
| `SAD_06_Client_Platforms` | Native mobile / desktop + web (browser + extension) |
| `SAD_07_Admin_Control_Panel` | Unified, self-governing admin console |

The directory also carries two **forward-looking** module specifications — `SAD_08_Gungnir` and `SAD_08b_Forseti` — which have no implementation in this workspace, no acceptance coverage, and no place in the build playbook.

The same documents ship as branded `.docx` in `docs/dev/documents/`; the 76 diagrams are rendered three ways each — Mermaid (`.mmd`), editable draw.io (`.drawio`), and PNG — in `docs/dev/diagrams/`.

---

## Caveats

- **Model weights are not vendored.** Mimir serves a real model only through a signed manifest whose weights stream to the signed digest, behind an OpenAI-compatible engine on loopback: llama.cpp on CPU, vLLM on GPU. The operator fetches the weights (see [Serve a real model](#serve-a-real-model)). The acceptance suites still run against the deterministic backend.
- **The TypeScript tier is not gated.** `just build-ts` tolerates failure and `just test` runs no TypeScript, so the consoles are not covered by CI. They typecheck clean today (`pnpm -r typecheck`), but nothing enforces that.
- **No logos exist.** Midgard is built by Veldris Ltd; the SADs were authored by Akuma Engineering Ltd. No marks were provided or invented — wherever a logo would sit, the product name is set in plain type (`ProductMark`). Supply real marks and they can be wired in.

---

## Next steps

➡️ **[GETTING_STARTED.md](GETTING_STARTED.md)** — the build workflow and design-system deep-dive.

➡️ **[BUILD_PLAYBOOK.md](BUILD_PLAYBOOK.md)** — the in-sequence build reference: every build prompt, all 15 runtime AI prompts (verbatim), and all 72 acceptance criteria, phase-by-phase.

➡️ **[docs/compliance/GA_READINESS.md](docs/compliance/GA_READINESS.md)** — the release go/no-go record and its outstanding gates.

➡️ **[docs/design/readme.md](docs/design/readme.md)** — the complete design-system reference.

➡️ **[SECURITY.md](SECURITY.md)** · **[CONTRIBUTING.md](CONTRIBUTING.md)** · **[CHANGELOG.md](CHANGELOG.md)**

---

*Midgard is a Veldris Ltd project. Solutions Architecture by Akuma Engineering Ltd. · Classification of source SADs: CONFIDENTIAL — RECIPIENT EYES ONLY.*
