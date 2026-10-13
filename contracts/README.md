# contracts/ — the client SDK edge contract

Protobuf definitions that are the cross-language **source of truth for the client SDK
surface**: every RPC a console or generated SDK can call over the browser-facing
HTTP/JSON gateway. The SDKs and product consoles depend on these contracts, not on
implementations — the ports-and-adapters boundary. `[DD-6]`

## What these protos govern (and what they don't)

- **They govern the client SDK edge.** `Gateway` + `ObjectFabric` (below) define every
  RPC the TypeScript/Python/… SDKs expose via the single transport seam
  (`sdk/ts/src/client.ts`'s `GatewayClient`). The `HttpGateway` adapter speaks
  HTTP/JSON to the `services/http-gateway` edge; these messages are that edge's wire
  contract.
- **They do NOT govern internal service-to-service traffic.** Inter-service calls use
  the lean **`pq-mtls` binary codec** (hand-rolled, no gRPC — the sovereign/air-gap
  posture keeps the dependency surface minimal). Its typed source of truth is the
  **Rust type definitions** in each crate (e.g. `heimdall-picture`, `forge-dataset-store`,
  the `audit-writer` record types), not a proto. Transport there is PQ-mTLS
  (hybrid X25519+ML-KEM) with the crate's binary framing.

So **DD-6 holds as stated for the client edge** (a typed contract is the source of
truth for what a client can call), and internal wire formats are typed by the Rust
code that defines and tests them. This file no longer claims "gRPC for every
inter-service call" — that was never the built reality.

## Files

- `proto/midgard/v1/common.proto` — shared types (Marking, PolicyContext, AuditRecord).
- `proto/midgard/v1/object_fabric.proto` — `ObjectFabric`: `Query` / `ApplyAction`.
- `proto/midgard/v1/gateway.proto` — `Gateway`: the remaining client RPCs (object-type
  registry, search-around, audit read/stream, policy list/simulate, and the platform
  read models: identities, keys, compute, observability, resilience, contract surface).
- `buf.yaml` — lint + **breaking-change** policy (`buf breaking`, enforced in CI).
- `buf.gen.yaml` — codegen config for the SDK wire types.

## File formats

Two files an operator or another tool writes have a published JSON Schema here:

- `schema/ontology.v1.schema.json`: the ontology file, `ontology.json`, that the gateway
  registers at every start.
- `schema/import-record.v1.schema.json`: one line of the JSON Lines file that
  `mg-admin import` reads.

The gateway does not read these schemas; it has its own strict parsers. So that the two
cannot drift, `services/http-gateway/tests/contract_schemas.rs` compares the keys each
schema allows with the parser's own constants, checks every enumerated value against the
parser, and runs every file under `fixtures/` through both:

| Directory | The schema | The parser |
|---|---|---|
| `fixtures/*/valid` | accepts | accepts |
| `fixtures/*/invalid` | refuses | refuses |
| `fixtures/*/beyond-schema` | accepts | refuses |

`beyond-schema` holds the rules a schema cannot state, each listed in the schema's
description: an id used twice, a link or action naming a type that is not declared, a
property that is not declared or holds the wrong kind of value. A file that meets the
schema and breaks none of those rules is one the gateway accepts.

An ontology can be written by hand against the schema. Nothing in this repository needs
another tool to produce one.

## Gates (how the contract stays true)

1. **Completeness (offline):** `bash contracts/check-conformance.sh` (also
   `just proto-conformance`) asserts **every `GatewayClient` RPC has a governing proto
   RPC**, failing closed on any uncovered method. This runs in the air-gapped gate — no
   buf/network needed — so a new client RPC without a contract fails CI. [RC-36]
2. **Breaking changes (CI):** the `contracts` job runs `buf lint` + `buf breaking`
   against `main`, so an incompatible change to a governing contract fails the PR.

## Regeneration

`just gen` regenerates the SDK **wire types** from these protos (output gitignored,
reproducible from the protos alone). The per-product convenience wrappers in
`sdk/ts/src/products/*.ts` are hand-authored view helpers over those wire types.
