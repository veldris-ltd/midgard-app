# Claims ledger

Generated from [`claims.toml`](claims.toml) by `bash tools/ci/claims.sh --render`. Do not
edit this file; edit the data and render again. `bash tools/ci/claims.sh --check` fails
if the two differ, or if a row cites a file, line or test that does not exist.

One row per capability this repository states about Midgard. A row gives the claim, its
status, and what supports it. It is the only place a capability's status is decided:
the README, the release notes and the launch material say what their row says.

## Status vocabulary

| Status | Meaning | How it may be worded |
|---|---|---|
| `production` | Operable, and assured on the hardware and at the scale it is claimed for. | Present tense. |
| `operable` | Runs in a service this repository builds, and a test drives that running service. | Present tense: "built", "delivers", "supports". |
| `reference` | Implemented and tested as a library. No running service uses it yet. | "Implemented and tested as a library (test); not yet in a running service." |
| `spec` | Specified. Not implemented, or implemented only as a scaffold. | "Specified in (document); not implemented." |
| `external` | Exists outside this repository, which makes no checkable claim about it. | Named as separate. Never counted among Midgard's capabilities or figures. |

A service test is a step of a script that drives the running services
(`deploy/compose/smoke.sh`, `deploy/kind/smoke.sh`) or a test that starts a service and
talks to it over its own listener.

## Summary

| Status | Rows |
|---|---|
| `production` | 0 |
| `operable` | 23 |
| `reference` | 19 |
| `spec` | 4 |
| `external` | 1 |
| **Total** | **47** |

No document is listed as wording a capability above its status.

## Core: cryptography and transport

### `pq-crypto-core` (operable)

ML-KEM, ML-DSA and SLH-DSA (FIPS 203, 204, 205) with X25519, behind one algorithm-agility layer. The FIPS backend passes the ACVP known-answer tests held in the repository, and the test backend cannot be compiled together with it.

**Limit.** The module has no FIPS 140-3 certificate; that is an external laboratory activity and a GA blocker.

- Source:
  - [`crypto/pqc-agility/src/backend/mod.rs:14`](../../crypto/pqc-agility/src/backend/mod.rs)
- Tests:
  - [`crypto/pqc-agility/tests/fips_backend.rs::kem_roundtrip_every_suite`](../../crypto/pqc-agility/tests/fips_backend.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:63`](../../README.md)
  - [`docs/compliance/GA_READINESS.md:64`](../../docs/compliance/GA_READINESS.md)
  - [`docs/launch/pipeline/build-deck.js:474`](../../docs/launch/pipeline/build-deck.js)

### `pq-mtls` (operable)

Services talk to each other over PQ-mTLS: a mutually authenticated handshake with hybrid X25519 and ML-KEM-768 key agreement, certified workload identities, session resumption and per-identity rate limiting.

- Criteria: RC-40.2
- Source:
  - [`crypto/pq-mtls/src/lib.rs:18`](../../crypto/pq-mtls/src/lib.rs)
- Tests:
  - [`crypto/pq-mtls/tests/real_fips_handshake.rs::real_handshake_negotiates_hybrid_and_agrees`](../../crypto/pq-mtls/tests/real_fips_handshake.rs)
  - [`crypto/pq-mtls/tests/tcp_transport.rs::tcp_handshake_denies_unauthorized`](../../crypto/pq-mtls/tests/tcp_transport.rs)
  - [`services/gateway/tests/session.rs::a_second_connection_resumes_and_the_ticket_is_single_use`](../../services/gateway/tests/session.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:115`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:58`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:17`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:470`](../../docs/launch/pipeline/build-deck.js)

### `mesh-certificate-lifecycle` (reference)

A mesh certificate authority that issues, renews and revokes workload certificates, with a revocation honoured at the next handshake.

**Until it is operable, say:** "Workload certificates carry an expiry and a clearance. mg-admin mesh issue, renew and revoke manage them, and services re-read a signed revocation list every 30 seconds. Renewal is an operator command; services do not renew themselves yet."

**Milestone.** Services renew their own certificates before expiry (RF-IAM-6).

- Source:
  - [`tools/mg-admin/src/mesh.rs:203`](../../tools/mg-admin/src/mesh.rs)
  - [`tools/mg-admin/src/mesh.rs:348`](../../tools/mg-admin/src/mesh.rs)
  - [`crypto/pq-mtls/src/revocation.rs:1`](../../crypto/pq-mtls/src/revocation.rs)
  - [`services/runtime/src/identity.rs:68`](../../services/runtime/src/identity.rs)
- Tests:
  - [`services/iam/tests/rc_35.rs::renewal_is_downtime_free_and_revocation_is_honoured_on_next_handshake`](../../services/iam/tests/rc_35.rs)
  - [`services/iam/tests/rc_35.rs::the_signed_list_carries_the_revocation_to_other_verifiers`](../../services/iam/tests/rc_35.rs)
  - [`services/iam/tests/rc_35.rs::an_expired_certificate_no_longer_handshakes`](../../services/iam/tests/rc_35.rs)
  - [`tools/mg-admin/tests/mesh.rs::mesh_certificates_expire_revoke_and_renew`](../../tools/mg-admin/tests/mesh.rs)

### `sealed-durable-state` (operable)

The five running services keep their state in sealed, append-only journals: AES-256-GCM under ML-KEM-1024-wrapped keys, replayed at start. A journal or ledger that has been altered stops the service from starting.

- Criteria: RC-40.1
- Source:
  - [`services/kms/src/lib.rs:85`](../../services/kms/src/lib.rs)
- Tests:
  - [`services/kms/tests/envelope.rs::seal_is_ml_kem_1024_and_carries_no_plaintext`](../../services/kms/tests/envelope.rs)
  - [`services/kms/tests/file_ledger_restart.rs::tampered_on_disk_entry_is_detected_on_load`](../../services/kms/tests/file_ledger_restart.rs)
  - [`services/http-gateway/tests/state_dir_boot.rs::a_tampered_ledger_refuses_to_boot`](../../services/http-gateway/tests/state_dir_boot.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:24`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:80`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:106`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:471`](../../docs/launch/pipeline/build-deck.js)

### `hardware-key-custody` (reference)

A hardware trust root (HSM or TPM) in which the root secret never enters process memory, with root attestation and key rotation.

**Until it is operable, say:** "Key custody is software: keystores are files. A hardware trust root with attestation is implemented and tested against a software stand-in (services/kms/tests/rc_28.rs); no HSM or TPM has been used."

**Milestone.** RC-40.3 passes with a release artefact signed by a key held in a real HSM (RF-SEC-8, RF-SC-8).

- Criteria: RC-40.3
- Source:
  - [`services/kms/src/hardware.rs:167`](../../services/kms/src/hardware.rs)
- Tests:
  - [`services/kms/tests/rc_28.rs::decapsulation_happens_on_the_device_not_in_the_process`](../../services/kms/tests/rc_28.rs)
  - [`services/kms/tests/rc_28.rs::root_attestation_verifies_and_fails_closed_on_tamper`](../../services/kms/tests/rc_28.rs)
- Stated at:
  - [`CHANGELOG.md:93`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:112`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:483`](../../docs/launch/pipeline/build-deck.js)

## Core: fabric, identity and governance

### `audit-ledger` (operable)

Edge admission and every object-fabric call are written to one append-only ledger signed with ML-DSA-65. Each response carries the id of the ledger entry that authorised it, and the ledger is verified again at every start.

**Limit.** The ledger covers what the running services do. Agent steps are not in it, because no service runs the agent runtime (see agent-runtime).

- Source:
  - [`crypto/audit-writer/src/ledger.rs:5`](../../crypto/audit-writer/src/ledger.rs)
- Tests:
  - [`crypto/audit-writer/tests/tamper.rs::compromised_signer_cannot_rewrite_history`](../../crypto/audit-writer/tests/tamper.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::a_query_returns_real_objects_and_its_audit_id_is_a_real_ledger_row`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/durable_enclosure.rs::an_action_and_its_lineage_survive_a_restart`](../../services/http-gateway/tests/durable_enclosure.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::real_socket_round_trip`](../../services/http-gateway/tests/http.rs)
  - [`deploy/compose/smoke.sh:24`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:54`](../../README.md)
  - [`CHANGELOG.md:49`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:334`](../../docs/launch/pipeline/build-deck.js)

### `object-fabric` (operable)

The object fabric serves typed objects, links and actions through the HTTP edge. Markings (a band and compartments) and purposes are enforced per object at query time, and an object a caller may not read is absent from the result.

- Source:
  - [`services/http-gateway/src/enclosure.rs:368`](../../services/http-gateway/src/enclosure.rs)
- Tests:
  - [`services/http-gateway/tests/fabric_edge.rs::mandatory_access_control_hides_objects_outside_the_callers_compartments`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::above_clearance_query_is_denied_403_and_audited`](../../services/http-gateway/tests/secured_edge.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::real_socket_round_trip`](../../services/http-gateway/tests/http.rs)
  - [`deploy/compose/smoke.sh:50`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:54`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:332`](../../docs/launch/pipeline/build-deck.js)

### `ontology-file` (operable)

An enclosure's object types, link types and actions come from an ontology file with a published, versioned JSON Schema. The gateway registers the file at every start and refuses to start if it is invalid.

- Source:
  - [`services/http-gateway/src/ontology_file.rs:144`](../../services/http-gateway/src/ontology_file.rs)
  - [`contracts/schema/ontology.v1.schema.json`](../../contracts/schema/ontology.v1.schema.json)
- Tests:
  - [`services/http-gateway/tests/ontology_boot.rs::an_invalid_ontology_refuses_to_boot`](../../services/http-gateway/tests/ontology_boot.rs)
  - [`services/http-gateway/tests/contract_schemas.rs::ontology_fixtures_get_the_same_verdict_from_the_schema_and_the_parser`](../../services/http-gateway/tests/contract_schemas.rs)
  - [`services/http-gateway/tests/contract_schemas.rs::the_ontology_schema_allows_exactly_the_keys_the_parser_allows`](../../services/http-gateway/tests/contract_schemas.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:50`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:309`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:335`](../../docs/launch/pipeline/build-deck.js)

### `checked-import` (operable)

mg-admin import adds objects and links from a JSON Lines file with a published record schema. Every record is checked against the ontology and the store before anything is written, and one bad line refuses the whole file.

- Source:
  - [`tools/mg-admin/src/main.rs:376`](../../tools/mg-admin/src/main.rs)
  - [`services/http-gateway/src/import_file.rs:91`](../../services/http-gateway/src/import_file.rs)
  - [`contracts/schema/import-record.v1.schema.json`](../../contracts/schema/import-record.v1.schema.json)
- Tests:
  - [`services/http-gateway/tests/import_file.rs::a_bad_line_refuses_the_whole_file_and_names_it`](../../services/http-gateway/tests/import_file.rs)
  - [`services/http-gateway/tests/contract_schemas.rs::import_fixtures_get_the_same_verdict_from_the_schema_and_the_importer`](../../services/http-gateway/tests/contract_schemas.rs)
- Service tests:
  - [`tools/mg-admin/tests/import.rs::an_import_fills_a_real_enclosure_that_a_gateway_then_serves`](../../tools/mg-admin/tests/import.rs)
- Stated at:
  - [`README.md:325`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:335`](../../docs/launch/pipeline/build-deck.js)

### `human-gated-actions` (operable)

An action the ontology marks as human-gated is refused at the edge unless the request carries a human-gate token, and actions are gated unless the ontology says otherwise.

**Limit.** The edge accepts any non-empty token: it does not yet check that the token is a named person's authorisation (RF-UID-3).

- Source:
  - [`services/http-gateway/src/enclosure.rs:375`](../../services/http-gateway/src/enclosure.rs)
- Tests:
  - [`services/http-gateway/tests/fabric_edge.rs::a_gated_action_without_a_human_gate_token_is_refused_and_changes_nothing`](../../services/http-gateway/tests/fabric_edge.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::real_socket_round_trip`](../../services/http-gateway/tests/http.rs)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:333`](../../docs/launch/pipeline/build-deck.js)

### `login-and-session-tokens` (operable)

A principal logs in at the edge with a password, and a time-based one-time code when enrolled with one, and receives an ML-DSA-signed session token. Principals are held in a durable directory.

**Limit.** The console's login form has no field for the one-time code.

- Source:
  - [`services/iam/src/auth.rs:1`](../../services/iam/src/auth.rs)
- Tests:
  - [`services/http-gateway/tests/rc_37.rs::login_issues_a_pq_token_that_authorises_data_requests`](../../services/http-gateway/tests/rc_37.rs)
  - [`services/http-gateway/tests/login_directory.rs::a_durable_principal_needs_both_factors_and_survives_a_restart`](../../services/http-gateway/tests/login_directory.rs)
- Service tests:
  - [`tools/mg-admin/tests/cli.rs::a_principal_enrolled_by_mg_admin_logs_in_through_the_gateway`](../../tools/mg-admin/tests/cli.rs)
- Stated at:
  - [`README.md:54`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:447`](../../docs/launch/pipeline/build-deck.js)

### `webauthn-oidc-federation` (reference)

WebAuthn assertion verification, a JWKS endpoint and federation with an enterprise identity provider.

**Until it is operable, say:** "Users sign in with a password and an optional one-time code. Administrators hold ML-DSA passkeys in keystore files. WebAuthn verification and federation are implemented and tested as libraries; no browser flow uses them."

**Milestone.** The second-factor decision is recorded and the console login uses it (RF-IAM-1, RF-UI-1).

- Source:
  - [`admin/identity/src/webauthn.rs:1`](../../admin/identity/src/webauthn.rs)
  - [`services/iam/src/directory.rs:1`](../../services/iam/src/directory.rs)
- Tests:
  - [`admin/identity/tests/passkeys.rs::a_registered_passkey_signs_in_after_a_restart`](../../admin/identity/tests/passkeys.rs)

### `policy-decisions` (operable)

Access is denied unless a written rule grants it: role, marking and purpose must all match, and mandatory access control is evaluated first. A decision can be dry-run at the edge, and the dry-run is signed and audited.

- Source:
  - [`services/runtime/src/policy_file.rs:1`](../../services/runtime/src/policy_file.rs)
- Tests:
  - [`services/http-gateway/tests/policy_file.rs::an_empty_rule_set_is_valid_and_grants_nothing`](../../services/http-gateway/tests/policy_file.rs)
  - [`crypto/pq-mtls/tests/authz.rs::deny_by_default_pdp_blocks_channel`](../../crypto/pq-mtls/tests/authz.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::fabric_and_governance_routes_are_not_reachable_without_a_caller`](../../services/http-gateway/tests/http.rs)
  - [`deploy/kind/smoke.sh:115`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:351`](../../docs/launch/pipeline/build-deck.js)

### `dual-control-enrolment` (operable)

Enrolling a principal online needs two different administrators, each proving possession of a post-quantum passkey, and the enrolment is audited.

- Criteria: AC-AD-3, AC-AD-4
- Source:
  - [`services/http-gateway/src/admin_api.rs:274`](../../services/http-gateway/src/admin_api.rs)
- Tests:
  - [`services/http-gateway/tests/admin_enrolment.rs::an_enrolment_needs_a_second_administrator_and_survives_a_restart`](../../services/http-gateway/tests/admin_enrolment.rs)
- Service tests:
  - [`tools/mg-admin/tests/enrol.rs::two_administrators_enrol_a_principal_through_a_running_gateway`](../../tools/mg-admin/tests/enrol.rs)
- Stated at:
  - [`CHANGELOG.md:56`](../../CHANGELOG.md)

## Admin

### `admin-guarded-actions` (reference)

A guarded-action orchestrator that makes a mutating admin action one transaction: authenticate, simulate, require a second approver for high-risk changes, commit with a single-use token, and audit.

**Until it is operable, say:** "Dual control for high-risk admin changes is implemented and tested as a library (admin/orchestrator/tests/rc_31.rs). In the running services it applies to principal enrolment only; the admin console runs on fixture data."

**Milestone.** The admin surface is served from a running service through the orchestrator (RF-AD-1, RF-AD-4).

- Criteria: AC-AD-9
- Source:
  - [`admin/orchestrator/src/lib.rs:43`](../../admin/orchestrator/src/lib.rs)
- Tests:
  - [`admin/orchestrator/tests/rc_31.rs::single_author_high_risk_commit_is_denied`](../../admin/orchestrator/tests/rc_31.rs)
  - [`admin/orchestrator/tests/rc_31.rs::dual_approved_commit_is_audited_as_one_linked_transaction`](../../admin/orchestrator/tests/rc_31.rs)

### `break-glass` (reference)

Break-glass access: a scoped, time-boxed elevation that alerts a security officer, expires by itself and is audited throughout.

**Until it is operable, say:** "Break-glass alerting with durable, at-least-once delivery is implemented and tested as a library (AC-AD-8). The admin console is a design-complete interface running on fixture data; it is not connected to a backend."

**Milestone.** RF-CA-11: the alert path runs in a service and the console reads it.

- Criteria: AC-AD-8
- Source:
  - [`admin/breakglass/src/alert.rs:84`](../../admin/breakglass/src/alert.rs)
- Tests:
  - [`admin/breakglass/tests/rc_32.rs::grant_delivers_a_real_alert_and_elevates_access_which_auto_expires`](../../admin/breakglass/tests/rc_32.rs)
  - [`admin/breakglass/tests/rc_32.rs::alert_delivery_is_durable_across_a_transient_bus_outage`](../../admin/breakglass/tests/rc_32.rs)

### `config-reconciliation` (reference)

Signed, versioned configuration with diff and rollback, and a reconciler that detects and corrects drift between declared and observed configuration.

**Until it is operable, say:** "A signed, hash-chained configuration store with diff, rollback and a drift reconciler is implemented and tested as a library (AC-AD-7). No running service is reconciled by it, and it is not a Git workflow."

**Milestone.** The configuration-store decision is recorded and a subsystem adapter runs in a service (RF-AD-3).

- Criteria: AC-AD-7
- Source:
  - [`admin/config/src/reconcile.rs:171`](../../admin/config/src/reconcile.rs)
- Tests:
  - [`admin/config/tests/ac_ad_7.rs::ac_ad_7_versioned_with_diff_and_rollback`](../../admin/config/tests/ac_ad_7.rs)

### `consoles` (reference)

Seven console interfaces: Midgard core, Forge, Mimir, Bifrost, Heimdall, Admin and Clients.

**Until it is operable, say:** "The Midgard core console can be pointed at the HTTP edge and then shows live enclosure data; no automated test drives it. The other six consoles run on built-in sample data, because their services expose no console routes."

**Milestone.** Each console has a backend route and a test that drives it (RF-UID-1, RF-TST-2).

- Source:
  - [`products/admin/ui/src/main.tsx:17`](../../products/admin/ui/src/main.tsx)
- Tests:
  - [`services/http-gateway/tests/rc_37.rs::login_issues_a_pq_token_that_authorises_data_requests`](../../services/http-gateway/tests/rc_37.rs)
- Stated at:
  - [`README.md:812`](../../README.md)
  - [`CHANGELOG.md:97`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:118`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:544`](../../docs/launch/pipeline/build-deck.js)

## Mimir

### `mimir-inference-cpu` (operable)

mimir-server answers inference requests from a real model inside the enclosure: an OpenAI-compatible engine on loopback (llama.cpp on CPU), reached only over PQ-mTLS by a certified caller, with every call audited.

**Limit.** One model has been served: IBM Granite 3.3 2B Instruct on CPU. The caller receives one whole completion, not a token stream.

- Source:
  - [`mimir/model-server/src/backends/openai_compat.rs:60`](../../mimir/model-server/src/backends/openai_compat.rs)
- Tests:
  - [`mimir/mimir-server/tests/serve.rs::inference_is_served_over_pqmtls_audited_and_survives_a_restart`](../../mimir/mimir-server/tests/serve.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
  - [`deploy/compose/granite-smoke.sh:40`](../../deploy/compose/granite-smoke.sh)
  - [`deploy/kind/smoke.sh:73`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:72`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:22`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:381`](../../docs/launch/pipeline/build-deck.js)

### `mimir-inference-gpu` (reference)

GPU serving through vLLM, with a first token in under 800 ms.

**Until it is operable, say:** "vLLM on GPU is wired and has not been run. The 800 ms first-token objective is unvalidated; the measured figure is 325 ms for a 2B model on a 12-core CPU."

**Milestone.** RC-40.4 passes on a GPU node (RF-MI-1).

- Criteria: RC-40.4
- Source:
  - [`mimir/model-server/src/backends/gpu_docker.rs:90`](../../mimir/model-server/src/backends/gpu_docker.rs)
- Tests:
  - [`mimir/mimir-server/tests/serve.rs::a_signed_model_is_served_through_its_sidecar_and_a_tampered_one_never_starts`](../../mimir/mimir-server/tests/serve.rs)
- Stated at:
  - [`CHANGELOG.md:94`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:114`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:541`](../../docs/launch/pipeline/build-deck.js)

### `signed-model-manifest` (operable)

A model loads only from a manifest signed with ML-DSA, and its weights are checked against the signed digest before anything is served. One changed byte stops the service from starting.

**Limit.** No vendor signature is checked cryptographically; an upstream signature is recorded as unverifiable. The served model was signed with a development key and did not pass through the staging intake.

- Criteria: AC-MS-2
- Source:
  - [`tools/mg-model/src/main.rs:1`](../../tools/mg-model/src/main.rs)
- Tests:
  - [`mimir/model-server/tests/model_dir.rs::changed_truncated_or_swapped_weights_are_refused`](../../mimir/model-server/tests/model_dir.rs)
- Service tests:
  - [`mimir/mimir-server/tests/serve.rs::a_signed_model_is_served_through_its_sidecar_and_a_tampered_one_never_starts`](../../mimir/mimir-server/tests/serve.rs)
  - [`deploy/compose/granite-smoke.sh:31`](../../deploy/compose/granite-smoke.sh)
- Stated at:
  - [`CHANGELOG.md:74`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:380`](../../docs/launch/pipeline/build-deck.js)

### `model-family-routing` (reference)

Routing a workload to one of several model families (Mistral, Nemotron, Gemma, Granite) and hot-swapping between them without a change to the consumer.

**Until it is operable, say:** "Routing across model families and hot-swap between them are implemented and tested as a library against a deterministic engine (AC-MS-9). The running inference service serves one model; one real model, IBM Granite 3.3 2B on CPU, has been served end to end."

**Milestone.** RF-CA-8: mimir-server uses the router with more than one real model.

- Criteria: AC-MS-4, AC-MS-9
- Source:
  - [`mimir/model-server/src/router.rs:28`](../../mimir/model-server/src/router.rs)
- Tests:
  - [`mimir/model-server/tests/router.rs::ac_ms_4_workload_routes_to_the_right_family_no_consumer_change`](../../mimir/model-server/tests/router.rs)
- Stated at:
  - [`GETTING_STARTED.md:283`](../../GETTING_STARTED.md)

### `grounded-retrieval` (reference)

Grounding an answer in the object fabric: a retrieval plan over declared types, with every claim bound to a citation, and a refusal when a task cannot be grounded.

**Until it is operable, say:** "Grounded, cited retrieval over the object fabric is implemented and tested as a library (AC-MI-2). Nothing routes a request to it: the running inference service answers a prompt without retrieval."

**Milestone.** A service hosts the retriever and the agent runtime (RF-MI-4).

- Criteria: AC-MI-2
- Source:
  - [`mimir/oag-retriever/src/ground.rs:35`](../../mimir/oag-retriever/src/ground.rs)
- Tests:
  - [`mimir/oag-retriever/tests/grounding.rs::grounding_binds_citations_and_ranks_by_relevance`](../../mimir/oag-retriever/tests/grounding.rs)
  - [`mimir/agent-runtime/tests/ac_mi_2.rs::ungroundable_task_refuses_without_fabricating`](../../mimir/agent-runtime/tests/ac_mi_2.rs)

### `agent-runtime` (reference)

A governed agent loop: plan, ground, assemble, guard, evaluate, check reversibility, pass a human gate for consequential or irreversible actions, audit, then execute.

**Until it is operable, say:** "The governed agent loop is implemented and tested as a library (AC-MI-3): a consequential action runs only after a recorded human decision. No service runs the loop, so no agent acts on a deployed enclosure."

**Milestone.** A service hosts the loop, with executors beyond the object fabric (RF-MI-4).

- Criteria: AC-MI-3
- Source:
  - [`mimir/agent-runtime/src/govloop.rs:37`](../../mimir/agent-runtime/src/govloop.rs)
- Tests:
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::mutation_is_denied_when_human_denies`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::irreversible_action_forces_human_even_if_preauthorised`](../../mimir/agent-runtime/tests/ac_mi_3.rs)

### `guardrails` (reference)

Input guardrails against prompt injection: a deterministic filter with de-obfuscation, a classifier seam, and rewriting of a prompt that can be made safe.

**Until it is operable, say:** "Input guardrails are implemented and tested as a library against a small hand-written corpus. The classifier behind the seam is a test double, and no running service applies the guardrails."

**Milestone.** A classifier served by the model plane, a corpus with a stated block rate, and a service that applies them (RF-MI-7).

- Source:
  - [`mimir/agent-runtime/src/defend.rs:317`](../../mimir/agent-runtime/src/defend.rs)
- Tests:
  - [`mimir/agent-runtime/tests/guardrail_paraphrase.rs::obfuscated_paraphrases_are_blocked_even_though_not_literal_entries`](../../mimir/agent-runtime/tests/guardrail_paraphrase.rs)
  - [`mimir/agent-runtime/tests/guardrail_paraphrase.rs::sanitise_rewrites_the_prompt_instead_of_passing_it_through`](../../mimir/agent-runtime/tests/guardrail_paraphrase.rs)

### `evals` (reference)

An evaluation suite for the AI touchpoints, recorded per evaluation and per model family.

**Until it is operable, say:** "The evaluation suites are implemented and run as tests against deterministic stand-ins for four model families. No evaluation has been run against a real model."

**Milestone.** An evaluation runner drives the suites through mimir-server against a real model (RF-MI-6).

- Source:
  - [`mimir/agent-runtime/src/evals.rs:511`](../../mimir/agent-runtime/src/evals.rs)
- Tests:
  - [`mimir/agent-runtime/tests/eval_matrix.rs::eval_matrix_scores_each_family_distinctly_and_reports_per_eval_family`](../../mimir/agent-runtime/tests/eval_matrix.rs)

## Forge

### `forge-connectors` (operable)

forge-worker syncs delimited files, PostgreSQL and S3-compatible stores into sealed, versioned datasets, resuming from a checkpoint, then projects the rows onto the ontology and into the object fabric over PQ-mTLS. Each connector is held to an egress policy naming the exact host and port.

**Limit.** The Kafka connector has been tested only against a fake broker. The PostgreSQL connector has no TLS and cannot use the egress broker before PostgreSQL 17.

- Source:
  - [`forge/worker/src/lib.rs:10`](../../forge/worker/src/lib.rs)
  - [`forge/connector-sdk/src/connector.rs:84`](../../forge/connector-sdk/src/connector.rs)
- Tests:
  - [`forge/engine/tests/rc_27.rs::rc_27_egress_violating_connector_is_blocked`](../../forge/engine/tests/rc_27.rs)
- Service tests:
  - [`forge/worker/tests/sync.rs::the_worker_syncs_versioned_datasets_into_the_enclosure_over_the_mesh`](../../forge/worker/tests/sync.rs)
  - [`deploy/compose/smoke.sh:50`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:105`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:67`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:20`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:367`](../../docs/launch/pipeline/build-deck.js)

### `forge-compute-engine` (reference)

A partitioned compute engine in Rust: filter, derive, join, aggregate, sort and window, with a hash shuffle and spill to disk.

**Until it is operable, say:** "A partitioned compute engine with join, aggregate, sort, window and spill to disk is implemented and tested as a library. The running Forge worker syncs and projects rows; it does not run pipelines on the engine."

**Milestone.** forge-worker executes a pipeline on the engine (RF-F-1, RF-F-3).

- Source:
  - [`forge/engine/src/partitioned.rs:56`](../../forge/engine/src/partitioned.rs)
- Tests:
  - [`forge/engine/tests/partitioned.rs::partitioned_matches_reference_with_shuffle_and_spill`](../../forge/engine/tests/partitioned.rs)
  - [`forge/engine/tests/relational.rs::inner_join_then_aggregate`](../../forge/engine/tests/relational.rs)
- Stated at:
  - [`CHANGELOG.md:113`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:371`](../../docs/launch/pipeline/build-deck.js)

### `forge-data-expectations` (reference)

Data-quality expectations that run on a build and block promotion when they fail.

**Until it is operable, say:** "Expectation checks with a promotion gate are implemented and tested as a library (forge/health). The running Forge worker does not evaluate them: a schema change fails the sync, and there is no quarantine."

**Milestone.** forge-worker runs expectations and quarantines failing records (RF-F-4).

- Source:
  - [`forge/health/src/gate.rs:1`](../../forge/health/src/gate.rs)
- Tests:
  - [`forge/health/tests/ac_f_3.rs::failing_expectation_blocks_promotion_and_reports`](../../forge/health/tests/ac_f_3.rs)

### `multimodal-ingest` (spec)

Ingest of imagery, audio and video, with georeferencing, captioning and transcription.

**Until it is operable, say:** "Forge ingests tabular data from files, PostgreSQL, S3-compatible stores and Kafka. Ingest of imagery, audio and video, with georeferencing, captioning and transcription, is not implemented and is not yet specified in a SAD."

**Milestone.** RF-CA-12: a SAD change first, then the implementation.


## Bifrost

### `bifrost-fleet` (operable)

bifrost-fleet records each installation's version, liveness and drift from its channel. An agent checks in over PQ-mTLS, every request is authorised and audited, and fleet state survives a restart.

**Limit.** The agent reports a version and a health flag. It does not install, verify or roll back anything.

- Source:
  - [`bifrost/fleet-server/src/lib.rs:104`](../../bifrost/fleet-server/src/lib.rs)
- Tests:
  - [`bifrost/fleet/tests/durable_restart.rs::fleet_state_survives_restart`](../../bifrost/fleet/tests/durable_restart.rs)
- Service tests:
  - [`bifrost/fleet-server/tests/serve.rs::agents_check_in_operators_read_the_fleet_and_it_survives_a_restart`](../../bifrost/fleet-server/tests/serve.rs)
  - [`deploy/compose/smoke.sh:31`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:87`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:405`](../../docs/launch/pipeline/build-deck.js)

### `bifrost-rollout` (reference)

Ring-by-ring rollout with health gates between rings and automatic rollback, without downtime.

**Until it is operable, say:** "Rolling and blue-green upgrade logic with health-gated rollback is implemented and tested as a library (AC-M-7, AC-B-2). Deployed services run one replica each and restart on their recovered state (RC-40.1); upgrading one interrupts it."

**Milestone.** RF-CA-9, after the replica-model decision: the fleet manager drives a rollout against real agents (RF-B-2, RF-B-3).

- Criteria: AC-M-7, AC-B-2
- Source:
  - [`bifrost/rollout/src/orchestrator.rs:101`](../../bifrost/rollout/src/orchestrator.rs)
- Tests:
  - [`bifrost/rollout/tests/rollout.rs::ac_b_2_broad_ring_gate_failure_rolls_back_automatically_no_downtime`](../../bifrost/rollout/tests/rollout.rs)
  - [`services/scheduler/tests/rollout_real.rs::rolling_upgrade_serves_every_request_with_real_replicas`](../../services/scheduler/tests/rollout_real.rs)
- Stated at:
  - [`docs/releases/v0.1.0-rc.1.md:119`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:545`](../../docs/launch/pipeline/build-deck.js)

### `air-gap-bundle` (operable)

mg-bundle packs gate-verified artefacts into one signed bundle and imports it offline, verifying every artefact, a chunk at a time so that model weights are never held in memory. A refused import leaves nothing behind.

**Limit.** The bundle carries binaries. The chart, base image, advisory snapshot and trust anchors come from a repository checkout, and an import returns no signed receipt.

- Criteria: AC-B-3, AC-MS-3
- Source:
  - [`bifrost/bundle/src/stream.rs:151`](../../bifrost/bundle/src/stream.rs)
  - [`deploy/air-gap-bundle/install.sh:75`](../../deploy/air-gap-bundle/install.sh)
- Tests:
  - [`bifrost/bundle/tests/streaming.rs::damage_is_located_and_a_forged_chunk_is_still_refused`](../../bifrost/bundle/tests/streaming.rs)
- Service tests:
  - [`tools/mg-bundle/tests/cli.rs::a_bundle_from_a_sealed_keystore_imports_offline_and_refuses_everything_else`](../../tools/mg-bundle/tests/cli.rs)
- Stated at:
  - [`CHANGELOG.md:13`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:25`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/compliance/GA_READINESS.md:77`](../../docs/compliance/GA_READINESS.md)

## Heimdall

### `heimdall-feed-and-picture` (operable)

heimdall-feed accepts an observation only over PQ-mTLS, only from the feed whose certified identity it carries, and only with a valid signature. Observations that share an entity key fuse into one tracked entity, and the picture is sealed at rest and recovered at restart.

**Limit.** A resubmitted observation is counted again, observations carry no marking, and there is no feed-staleness indication.

- Source:
  - [`heimdall/feed-server/src/lib.rs:166`](../../heimdall/feed-server/src/lib.rs)
- Tests:
  - [`heimdall/picture-sync/tests/rc_26.rs::rc_26_unsigned_feed_is_rejected_at_the_boundary`](../../heimdall/picture-sync/tests/rc_26.rs)
- Service tests:
  - [`heimdall/feed-server/tests/serve.rs::sources_feed_the_picture_analysts_read_it_and_it_survives_a_restart`](../../heimdall/feed-server/tests/serve.rs)
  - [`deploy/compose/smoke.sh:42`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:95`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:391`](../../docs/launch/pipeline/build-deck.js)
  - [`docs/launch/pipeline/build-deck.js:393`](../../docs/launch/pipeline/build-deck.js)

### `heimdall-picture-replication` (reference)

Replication of the common picture between nodes over PQ-mTLS.

**Until it is operable, say:** "Replication of the picture between two nodes over PQ-mTLS is implemented and tested as a library. The running feed service does not replicate, and the merge does not converge when two nodes hold different state with the same timestamp."

**Milestone.** The merge is made convergent and the feed service replicates (RF-H-3).

- Criteria: AC-H-5
- Source:
  - [`heimdall/picture-sync/src/replicator.rs:14`](../../heimdall/picture-sync/src/replicator.rs)
- Tests:
  - [`heimdall/picture-sync/tests/rc_26.rs::rc_26_feed_over_pqmtls_converges_across_two_nodes`](../../heimdall/picture-sync/tests/rc_26.rs)

### `heimdall-decision-support` (reference)

Decision support: ranked options, each cited and grounded against the picture, and workflows in which a person authorises a consequential action and the decision is audited.

**Until it is operable, say:** "Advisory option generation (ranked options, each cited and grounded against the operating picture) is implemented and tested as a library (AC-H-3, EV-A8). The running feed gateway fuses and serves the picture; it does not yet generate options."

**Milestone.** RF-CA-10, after two fixes in RF-H-4: ungrounded options are rejected, and no action runs without a recorded human decision.

- Criteria: AC-H-3, AC-H-4
- Source:
  - [`heimdall/decision-support/src/adapter.rs:62`](../../heimdall/decision-support/src/adapter.rs)
  - [`heimdall/workflow/src/engine.rs:131`](../../heimdall/workflow/src/engine.rs)
- Tests:
  - [`heimdall/decision-support/tests/ac_h_3.rs::ac_h_3_options_are_ranked_and_cite_evidence`](../../heimdall/decision-support/tests/ac_h_3.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::ac_h_4_consequential_action_needs_human_authorisation_and_is_audited`](../../heimdall/workflow/tests/ac_h_4.rs)

## Clients

### `client-core` (reference)

One shared client core in Rust: sealed local edits, clearance checks, and conflict-free sync that converges after a disconnection.

**Until it is operable, say:** "A shared client core with sealed edits, conflict-free sync and tiered inference is implemented and tested as a library. It keeps its state in memory, and no application is built on it."

**Milestone.** Client-side persistence and language bindings (RF-PER-5, RF-CL-1).

- Criteria: AC-C-6, AC-C-10
- Source:
  - [`clients/core/src/lib.rs:48`](../../clients/core/src/lib.rs)
- Tests:
  - [`clients/core/tests/one_core.rs::two_cores_converge_on_reconnect`](../../clients/core/tests/one_core.rs)
  - [`clients/sync/tests/convergence.rs::all_permutations_converge_and_are_idempotent`](../../clients/sync/tests/convergence.rs)
  - [`clients/inference/tests/rc_57.rs::tier2_request_delegates_over_real_pqmtls`](../../clients/inference/tests/rc_57.rs)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:416`](../../docs/launch/pipeline/build-deck.js)

### `client-shells` (spec)

Native applications for iOS, Android and Linux, a Tauri desktop application, a web application and a browser extension, with keys held in device hardware.

**Until it is operable, say:** "Six client shells are specified in SAD_06 and scaffolded as manifests and accessibility declarations. None is built: there is no application code and there are no language bindings, so no key is held in device hardware."

**Milestone.** RF-CL-1 to RF-CL-7.

- Criteria: AC-C-11
- Source:
  - [`clients/shells/SHELL_BUILD.md:1`](../../clients/shells/SHELL_BUILD.md)

## Supply chain and release

### `supply-chain-gate` (operable)

Each release artefact passes one gate, in order, and each stage fails closed: built twice and proven bit-identical, described by a CycloneDX SBOM with component hashes, scanned offline for advisories and licences, signed with ML-DSA with a provenance attestation, then verified.

**Limit.** The gate has run with throwaway keys only (a rehearsal on 2026-09-15). No key ceremony has been held, release keys have no revocation, and the release workflow has not run on a signing runner.

- Criteria: AC-M-6, AC-B-1
- Source:
  - [`justfile:373`](../../justfile)
  - [`tools/ci/release.sh:170`](../../tools/ci/release.sh)
- Tests:
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::an_artefact_that_was_never_signed_is_refused`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-scan/tests/cli.rs::ac_rf_sc_4a_a_critical_or_high_advisory_blocks_and_names_the_component`](../../tools/mg-scan/tests/cli.rs)
- Service tests:
  - [`tools/ci/run.sh:225`](../../tools/ci/run.sh)
- Stated at:
  - [`README.md:742`](../../README.md)
  - [`docs/compliance/GA_READINESS.md:66`](../../docs/compliance/GA_READINESS.md)
  - [`docs/launch/pipeline/build-deck.js:402`](../../docs/launch/pipeline/build-deck.js)

### `offline-build` (operable)

The workspace builds and its tests run with no network: every dependency is vendored, Cargo is pinned to the vendored directory, and the CI jobs run in a container started with no network.

**Limit.** The recorded gate run covers the one commit named in docs/compliance/gate-runs/CURRENT. Later commits are not covered until the gate is recorded again.

- Criteria: AC-M-5
- Source:
  - [`.cargo/config.toml:18`](../../.cargo/config.toml)
  - [`tools/ci/in-toolchain.sh:12`](../../tools/ci/in-toolchain.sh)
  - [`tools/ci/vendor-check.sh`](../../tools/ci/vendor-check.sh)
- Service tests:
  - [`tools/ci/run.sh:208`](../../tools/ci/run.sh)
- Stated at:
  - [`README.md:130`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:508`](../../docs/launch/pipeline/build-deck.js)

### `verify-before-run` (operable)

Each service image re-verifies the ML-DSA signature, SBOM and provenance of its binary against the pinned release trust key before starting it, and refuses to start when verification fails.

**Limit.** Exercised in the release rehearsal of 2026-09-15 with throwaway drill keys only. No release image has been built, and the compose and kind images are built from source and start their binaries directly.

- Criteria: AC-B-1
- Source:
  - [`deploy/images/entrypoint.sh:16`](../../deploy/images/entrypoint.sh)
  - [`deploy/images/service.Dockerfile`](../../deploy/images/service.Dockerfile)
- Service tests:
  - [`deploy/images/build.sh:103`](../../deploy/images/build.sh)
- Stated at:
  - [`README.md:756`](../../README.md)
  - [`docs/releases/v0.1.0-rc.1.md:24`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/compliance/GA_READINESS.md:114`](../../docs/compliance/GA_READINESS.md)
  - [`CHANGELOG.md:83`](../../CHANGELOG.md)

### `acceptance-gate` (operable)

Each of the 72 acceptance criteria maps to a test, and one script runs them all in a container with no network.

**Limit.** Recorded at the commit named in docs/compliance/gate-runs/CURRENT: every criterion passed, in a CI run under --network none, not on release infrastructure. Later commits are not covered until the gate is recorded again.

- Source:
  - [`deploy/acceptance/run-airgapped.sh:10`](../../deploy/acceptance/run-airgapped.sh)
- Service tests:
  - [`tools/ci/run.sh:208`](../../tools/ci/run.sh)
- Stated at:
  - [`GETTING_STARTED.md:5`](../../GETTING_STARTED.md)

## Deployment and operations

### `deployment` (operable)

Five services (http-gateway, mimir-server, bifrost-fleet, heimdall-feed, forge-worker) deploy with Docker compose, or with a Helm chart that runs one replica of each as a hardened StatefulSet behind default-deny network policies.

**Limit.** The chart has run on a three-node kind cluster, not on a production cluster.

- Criteria: RC-40.1
- Source:
  - [`deploy/compose/compose.yaml`](../../deploy/compose/compose.yaml)
  - [`deploy/kind/values.yaml:4`](../../deploy/kind/values.yaml)
- Service tests:
  - [`deploy/compose/smoke.sh:13`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:73`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:63`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:18`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:323`](../../docs/launch/pipeline/build-deck.js)

### `high-availability` (spec)

Highly available clusters, replication between sites, zero-downtime operation and a 99.9% control-plane availability objective.

**Until it is operable, say:** "Each service runs as one replica holding a single-writer state directory. Availability comes from restart and recovery, not from replicas. High-availability and multi-site topologies are specified, not implemented, and no availability figure has been measured."

**Milestone.** The replica-model decision, then RF-DEP-2 and RF-RES-3.

- Source:
  - [`docs/ops/DR_PLAN.md:16`](../../docs/ops/DR_PLAN.md)
- Stated at:
  - [`docs/releases/v0.1.0-rc.1.md:119`](../../docs/releases/v0.1.0-rc.1.md)

### `telemetry` (operable)

Each service exposes liveness, readiness and Prometheus metrics, and writes structured logs, with no dependency that could send telemetry out of the enclosure.

**Limit.** The tracing module is unused, the observability console shows fixed figures, and no service-level objective has been measured in a deployment.

- Source:
  - [`services/telemetry/src/lib.rs:1`](../../services/telemetry/src/lib.rs)
- Tests:
  - [`services/telemetry/tests/telemetry.rs::metrics_render_prometheus_text`](../../services/telemetry/tests/telemetry.rs)
  - [`services/http-gateway/tests/rc_33.rs::service_exposes_liveness_readiness_metrics_and_an_observable_slo_breach`](../../services/http-gateway/tests/rc_33.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:15`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:125`](../../CHANGELOG.md)

### `compliance-mapping` (spec)

Alignment with NIST SP 800-53, OWASP ASVS and ISO 27001.

**Until it is operable, say:** "A mapping of platform controls to NIST SP 800-53, OWASP ASVS and ISO 27001 is written (docs/compliance/COMPLIANCE_MATRIX.md). No control has automated evidence and no assessment has been carried out."

**Milestone.** Control mappings generate evidence from CI (RF-CMP-1).

- Source:
  - [`docs/compliance/COMPLIANCE_MATRIX.md:1`](../../docs/compliance/COMPLIANCE_MATRIX.md)

## Outside this repository

### `likan` (external)

Likan is a separate, proprietary tool for data-structure ontologies. It is not part of the open-source Midgard repository and is not covered by its licence, its acceptance gate, its SBOMs or its published figures. Midgard loads ontologies from a documented file format (see ontology-file) and needs no other tool to do so. Likan is in active development and is intended to export that format. No file produced by Likan has been validated against it in this repository yet, and no pipeline between the two has been run here.

- Stated at:
  - [`README.md:833`](../../README.md)

