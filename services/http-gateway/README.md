# http-gateway — the browser-facing HTTP/JSON edge

A runnable Rust service that exposes the `GatewayClient` contract (see
`@midgard/sdk`) over plain **HTTP/JSON**, so the console UIs can reach real data
from a real process instead of the in-browser mock.

It is deliberately **std-only** — a thread-per-connection `std::net::TcpListener`,
a hand-rolled HTTP/1.1 parser (`http.rs`), and a hand-rolled JSON codec
(`json`, shared with the mesh services from `services/runtime`) — mirroring the PQ-mTLS `gateway`'s lean-std transport and the repo's
no-async/serde posture. No external dependencies.

## Why it exists

The enclosure's internal data plane is a custom PQ-mTLS + bespoke-binary protocol
a browser cannot speak. This edge is the browser-friendly termination point.

Object types, query, human-gated actions, search-around, audit and policy
dry-runs are served by the real `fabric` + `governance` crates (`enclosure.rs`).
`Fabric` owns the single governance spine, so edge admission and every fabric
call land in one PQ-signed ledger. The platform read views in `data.rs`
(identities, keys, compute, observability, resilience, SDK surface) are still
fixed data.

There are two startup modes:

- **Development** (`SecuredGateway::dev()`): an in-memory store and ledger, an
  ephemeral IdP key, one built-in login and an illustrative seed.
- **State directory** (`boot::open_state_dir`): serves the enclosure `mg-admin init`
  created, with its persisted issuer and audit keys, durable KMS-sealed object store,
  audit ledger and identity directory, and `policy.json`. Logins go through
  `iam::Authenticator` (password, then any enrolled TOTP). Boot re-verifies the
  recovered ledger and fails closed on anything that does not authenticate. An
  exclusive lock keeps a second gateway or `mg-admin` out while it runs. The schema
  comes from `ontology.json` (`ontology_file`), registered on every boot. Data is added
  offline with `mg-admin import`, checked against the ontology by `import_file`.
  Principals can also be enrolled online under dual control (`admin_api`):
  `POST /api/v1/admin/challenge`, `enrolments`, `enrolments/approve` and
  `enrolments/list` are authenticated by administrators' ML-DSA-65 passkey
  assertions rather than bearer tokens, and a request waits for a second, distinct
  administrator.
- **Mesh door** (`mesh_sync`, `--mesh-bind` with `--mesh-identity-dir`): certified
  workloads — the Forge worker — sync objects and links into the fabric over PQ-mTLS.
  Each `SyncRecords` call is authorised by `policy.json` and audited in the one ledger.
  It is then checked against the ontology by `import_file`, and what it changes is
  recorded before it is applied. Objects are created or replaced, and links are added
  once.

## Run

```bash
cargo run -p http-gateway                 # development mode on 127.0.0.1:7710
cargo run -p http-gateway -- 0.0.0.0:8080 # development mode, custom bind address

# a real enclosure (see the repository README, "Run a real enclosure")
cargo run -p http-gateway -- --state-dir ./enclosure \
  --bind 127.0.0.1:7710 --allow-origin http://localhost:5173
```

Point a console at it (Midgard core surface):

```bash
VITE_GATEWAY=http pnpm --filter @midgard/console-midgard dev
# or set VITE_GATEWAY_URL to a non-default edge, e.g. http://127.0.0.1:8080/api/v1
```

## Endpoints (`/api/v1`)

| Method | Path | GatewayClient method |
|---|---|---|
| GET | `/health` | — |
| GET | `/object-types` | `listObjectTypes` |
| POST | `/query` | `query` |
| POST | `/actions` | `applyAction` — **403** if a gated action lacks `humanGateToken` |
| POST | `/search-around` | `searchAround` |
| GET | `/audit?limit=` | `getAudit` (polled for `auditStream`) |
| GET | `/policies` | `listPolicies` |
| POST | `/policy/simulate` | `simulatePolicy` (deny-by-default PDP) |
| GET | `/identities` `/keys` `/compute` `/observability` `/resilience` `/sdk-surface` | the platform reads |

Every route except `/health`, `/ready`, `/metrics` and `/login` needs a bearer
token from `POST /login`. A missing, wrong or forged token returns `401`.
A permitted request is audited before it is served; a denied one is audited and
returns `403`.
CORS is locked to `http://localhost:5173`.

## Test

```bash
cargo test -p http-gateway   # JSON codec + router logic + a real-socket round-trip
```

Off the native toolchain, build/test via the repo's Docker path:
`docker run --rm -v "$PWD":/work -w /work rust:1.82 cargo test -p http-gateway`.
