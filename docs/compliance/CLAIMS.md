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
| `operable` | 24 |
| `reference` | 19 |
| `spec` | 5 |
| `external` | 1 |
| **Total** | **49** |

No document is listed as wording a capability above its status.

## Core: cryptography and transport

### `pq-crypto-core` (operable)

ML-KEM, ML-DSA and SLH-DSA (FIPS 203, 204, 205) with X25519, behind one algorithm-agility layer. The FIPS backend passes the ACVP known-answer tests held in the repository, and the test backend cannot be compiled together with it or into a release build.

**Limit.** The module has no FIPS 140-3 certificate; that is an external laboratory activity and a GA blocker.

- Source:
  - [`crypto/pqc-agility/src/backend/mod.rs:14`](../../crypto/pqc-agility/src/backend/mod.rs)
  - [`crypto/pqc-agility/src/hybrid.rs:1`](../../crypto/pqc-agility/src/hybrid.rs)
  - [`docs/security/CRYPTO_REVIEW_HANDOFF.md`](../../docs/security/CRYPTO_REVIEW_HANDOFF.md)
  - [`crypto/pqc-agility/src/backend/mod.rs:16`](../../crypto/pqc-agility/src/backend/mod.rs)
  - [`crypto/pqc-agility/src/backend/mod.rs:28`](../../crypto/pqc-agility/src/backend/mod.rs)
  - [`crypto/pqc-agility/build.rs:19`](../../crypto/pqc-agility/build.rs)
  - [`justfile:150`](../../justfile)
- Tests:
  - [`crypto/pqc-agility/tests/fips_backend.rs::kem_roundtrip_every_suite`](../../crypto/pqc-agility/tests/fips_backend.rs)
  - [`crypto/pqc-agility/tests/hash_kat.rs::kat_hmac_sha256_rfc4231`](../../crypto/pqc-agility/tests/hash_kat.rs)
  - [`crypto/pqc-agility/tests/hash_kat.rs::kat_hkdf_sha256_rfc5869`](../../crypto/pqc-agility/tests/hash_kat.rs)
  - [`crypto/pqc-agility/tests/hybrid_kat.rs::combiner_pinned_vector`](../../crypto/pqc-agility/tests/hybrid_kat.rs)
  - [`crypto/pqc-agility/tests/fips_backend.rs::hybrid_refuses_an_all_zero_x25519_result`](../../crypto/pqc-agility/tests/fips_backend.rs)
  - [`crypto/pqc-agility/tests/backend_guard.rs::release_guard_is_keyed_on_the_build_script_cfg`](../../crypto/pqc-agility/tests/backend_guard.rs)
  - [`crypto/pqc-agility/tests/backend_guard.rs::build_script_registers_and_emits_the_release_cfg`](../../crypto/pqc-agility/tests/backend_guard.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:63`](../../README.md)
  - [`docs/compliance/GA_READINESS.md:64`](../../docs/compliance/GA_READINESS.md)
  - [`docs/launch/pipeline/build-deck.js:474`](../../docs/launch/pipeline/build-deck.js)

### `pq-mtls` (operable)

Services talk to each other over PQ-mTLS: a mutually authenticated handshake with hybrid X25519 and ML-KEM-768 key agreement, certified workload identities whose clearance and permitted purposes are attributes the issuer signs, session resumption that runs a fresh ML-KEM exchange and re-verifies the client certificate against the live revocation list on every resumption, and per-identity rate limiting and per-connection quotas, enabled by default in every mesh service.

- Criteria: RC-40.2
- Source:
  - [`crypto/pq-mtls/src/lib.rs:18`](../../crypto/pq-mtls/src/lib.rs)
  - [`services/runtime/src/serve.rs:345`](../../services/runtime/src/serve.rs)
  - [`crypto/pq-mtls/src/wire.rs:95`](../../crypto/pq-mtls/src/wire.rs)
- Tests:
  - [`crypto/pq-mtls/tests/real_fips_handshake.rs::real_handshake_negotiates_hybrid_and_agrees`](../../crypto/pq-mtls/tests/real_fips_handshake.rs)
  - [`crypto/pq-mtls/tests/tcp_transport.rs::tcp_handshake_denies_unauthorized`](../../crypto/pq-mtls/tests/tcp_transport.rs)
  - [`services/gateway/tests/session.rs::a_second_connection_resumes_and_the_ticket_is_single_use`](../../services/gateway/tests/session.rs)
  - [`services/runtime/tests/security_listener.rs::apply_mesh_limits_enforces_the_rate_limit`](../../services/runtime/tests/security_listener.rs)
  - [`services/runtime/tests/security_listener.rs::the_mesh_cap_refuses_the_extra_connection_and_counts_it`](../../services/runtime/tests/security_listener.rs)
  - [`services/runtime/tests/security_listener.rs::an_unauthenticated_idle_mesh_connection_is_closed_at_the_timeout`](../../services/runtime/tests/security_listener.rs)
  - [`crypto/pq-mtls/tests/security_decode.rs::a_client_hello_with_a_hostile_suite_count_is_refused`](../../crypto/pq-mtls/tests/security_decode.rs)
  - [`crypto/pq-mtls/tests/security_decode.rs::a_client_hello_with_seventeen_suites_is_refused`](../../crypto/pq-mtls/tests/security_decode.rs)
  - [`crypto/pq-mtls/tests/security_decode.rs::every_truncation_of_every_message_is_refused`](../../crypto/pq-mtls/tests/security_decode.rs)
  - [`crypto/pq-mtls/tests/security_decode.rs::a_replayed_client_key_exchange_is_refused`](../../crypto/pq-mtls/tests/security_decode.rs)
  - [`crypto/pq-mtls/tests/security_resumption.rs::resumed_traffic_differs_when_the_kem_share_changes`](../../crypto/pq-mtls/tests/security_resumption.rs)
  - [`services/gateway/tests/session.rs::a_revoked_or_expired_client_cert_cannot_resume`](../../services/gateway/tests/session.rs)
  - [`crypto/pq-mtls/tests/security_attributes.rs::certificate_attributes_are_signed`](../../crypto/pq-mtls/tests/security_attributes.rs)
  - [`crypto/pq-mtls/tests/security_attributes.rs::a_v1_certificate_still_verifies_and_is_unconstrained`](../../crypto/pq-mtls/tests/security_attributes.rs)
  - [`tools/mg-admin/tests/mesh.rs::mesh_issue_binds_clearance_and_purposes_into_the_certificate`](../../tools/mg-admin/tests/mesh.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:115`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:140`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:253`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:17`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:470`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:257`](../../CHANGELOG.md)
  - [`CHANGELOG.md:344`](../../CHANGELOG.md)

### `mesh-certificate-lifecycle` (reference)

A mesh certificate authority that issues, renews and revokes workload certificates, with a revocation honoured at the next handshake.

**Until it is operable, say:** "Workload certificates carry an expiry and a clearance. mg-admin mesh issue, renew and revoke manage them, and services re-read a signed revocation list every 30 seconds. Renewal is an operator command; services do not renew themselves yet."

**Milestone.** Services renew their own certificates before expiry (RF-IAM-6).

- Source:
  - [`tools/mg-admin/src/mesh.rs:217`](../../tools/mg-admin/src/mesh.rs)
  - [`tools/mg-admin/src/mesh.rs:382`](../../tools/mg-admin/src/mesh.rs)
  - [`crypto/pq-mtls/src/revocation.rs:1`](../../crypto/pq-mtls/src/revocation.rs)
  - [`services/runtime/src/identity.rs:74`](../../services/runtime/src/identity.rs)
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
  - [`deploy/kind/smoke.sh:105`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:301`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:471`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:305`](../../CHANGELOG.md)
  - [`CHANGELOG.md:392`](../../CHANGELOG.md)

### `hardware-key-custody` (reference)

A hardware trust root (HSM or TPM) in which the root secret never enters process memory, with root attestation and key rotation.

**Until it is operable, say:** "Key custody is software: keystores are files whose custody root is wrapped under an operator passphrase (keystore format version 3: PBKDF2-HMAC-SHA256 at 600000 iterations, AES-256-GCM), held apart from the keystore. A hardware trust root with attestation is implemented and tested against a software stand-in (services/kms/tests/rc_28.rs), and a keystore can seal its root to it (crypto/attest/tests/keystore_v2.rs); no HSM or TPM has been used."

**Milestone.** RC-40.3 passes with a release artefact signed by a key held in a real HSM (RF-SEC-8, RF-SC-8).

- Criteria: RC-40.3
- Source:
  - [`services/kms/src/hardware.rs:167`](../../services/kms/src/hardware.rs)
- Tests:
  - [`services/kms/tests/rc_28.rs::decapsulation_happens_on_the_device_not_in_the_process`](../../services/kms/tests/rc_28.rs)
  - [`services/kms/tests/rc_28.rs::root_attestation_verifies_and_fails_closed_on_tamper`](../../services/kms/tests/rc_28.rs)
  - [`crypto/attest/tests/keystore_v2.rs::external_root_wrap_round_trips`](../../crypto/attest/tests/keystore_v2.rs)
- Stated at:
  - [`CHANGELOG.md:288`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:112`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:483`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:292`](../../CHANGELOG.md)
  - [`CHANGELOG.md:379`](../../CHANGELOG.md)

## Core: fabric, identity and governance

### `audit-ledger` (operable)

Edge admission and every object-fabric call are written to one append-only ledger signed with ML-DSA-65. Each response carries the id of the ledger entry that authorised it, and the ledger is verified again at every start, under a pinned audit key, against a signed checkpoint of its length, so a shortened or deleted ledger does not start. Each row records outcome, reason, source address and session where the service knows them, and refused requests (rejected tokens, human-gate refusals) are recorded. The ledger view needs ReadAudit and is scoped to the caller: rows about objects the caller cannot read are omitted, and so are other principals' login rows unless policy grants ReadAudit on audit-ledger:sessions. Caller-derived resource fields are sanitised and usernames of unenrolled principals are recorded as a keyed hash. A partial final frame left by a crash during an append is cut off at start and the repair recorded in the ledger; a partial frame anywhere a crash cannot leave one still stops the start. A served inference call adds a second entry to mimir-server's ledger, linked to its decision, carrying the SHA-256 of the prompt and of the completion; a completion that cannot be recorded is not served.

**Limit.** The ledger covers what the running services do. Agent steps are not in it, because no service runs the agent runtime (see agent-runtime).

- Source:
  - [`crypto/audit-writer/src/ledger.rs:5`](../../crypto/audit-writer/src/ledger.rs)
  - [`crypto/audit-writer/src/checkpoint.rs:1`](../../crypto/audit-writer/src/checkpoint.rs)
  - [`services/http-gateway/src/enclosure.rs:856`](../../services/http-gateway/src/enclosure.rs)
  - [`crypto/audit-writer/src/record.rs:114`](../../crypto/audit-writer/src/record.rs)
  - [`services/http-gateway/src/lib.rs:566`](../../services/http-gateway/src/lib.rs)
  - [`services/http-gateway/src/lib.rs:1370`](../../services/http-gateway/src/lib.rs)
  - [`crypto/audit-writer/src/file.rs:23`](../../crypto/audit-writer/src/file.rs)
  - [`mimir/model-server/src/handler.rs:91`](../../mimir/model-server/src/handler.rs)
- Tests:
  - [`crypto/audit-writer/tests/tamper.rs::compromised_signer_cannot_rewrite_history`](../../crypto/audit-writer/tests/tamper.rs)
  - [`crypto/audit-writer/tests/truncation.rs::cutting_entries_off_the_end_is_detected`](../../crypto/audit-writer/tests/truncation.rs)
  - [`crypto/audit-writer/tests/truncation.rs::a_ledger_checked_with_the_wrong_key_is_refused`](../../crypto/audit-writer/tests/truncation.rs)
  - [`services/http-gateway/tests/state_dir_boot.rs::a_shortened_or_deleted_ledger_refuses_to_boot`](../../services/http-gateway/tests/state_dir_boot.rs)
  - [`services/http-gateway/tests/state_dir_boot.rs::an_audit_keystore_that_is_not_the_pinned_key_refuses_to_boot`](../../services/http-gateway/tests/state_dir_boot.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::a_query_returns_real_objects_and_its_audit_id_is_a_real_ledger_row`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/durable_enclosure.rs::an_action_and_its_lineage_survive_a_restart`](../../services/http-gateway/tests/durable_enclosure.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::the_audit_view_hides_rows_about_objects_the_caller_cannot_read`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::the_audit_view_hides_approval_rows_whose_target_the_caller_cannot_read`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::reading_the_audit_needs_the_read_audit_permission`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::other_principals_login_rows_need_the_sessions_grant`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`crypto/audit-writer/tests/append_verify.rs::a_ledger_written_before_the_context_existed_still_verifies`](../../crypto/audit-writer/tests/append_verify.rs)
  - [`services/governance/tests/authorize.rs::record_ids_are_unique_and_sequential`](../../services/governance/tests/authorize.rs)
  - [`services/governance/tests/authorize.rs::authorize_sets_the_outcome_field`](../../services/governance/tests/authorize.rs)
  - [`services/http-gateway/tests/security_record.rs::a_rejected_token_is_uniform_counted_logged_and_audited_once_per_minute`](../../services/http-gateway/tests/security_record.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::a_refused_human_gate_and_a_self_approval_are_in_the_ledger`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/security_record.rs::ledger_resource_fields_are_sanitised`](../../services/http-gateway/tests/security_record.rs)
  - [`services/http-gateway/tests/security_record.rs::an_unenrolled_username_is_recorded_as_a_keyed_hash`](../../services/http-gateway/tests/security_record.rs)
  - [`services/http-gateway/tests/security_record.rs::the_actor_hash_key_is_created_once_and_kept`](../../services/http-gateway/tests/security_record.rs)
  - [`crypto/audit-writer/tests/torn_tail.rs::a_torn_final_frame_is_truncated_and_reported`](../../crypto/audit-writer/tests/torn_tail.rs)
  - [`crypto/audit-writer/tests/torn_tail.rs::a_torn_frame_before_the_head_count_fails_closed`](../../crypto/audit-writer/tests/torn_tail.rs)
  - [`crypto/audit-writer/tests/torn_tail.rs::a_torn_frame_after_an_entry_the_checkpoint_does_not_cover_fails_closed`](../../crypto/audit-writer/tests/torn_tail.rs)
  - [`mimir/model-server/tests/security_marking.rs::completion_digests_are_recorded`](../../mimir/model-server/tests/security_marking.rs)
  - [`mimir/model-server/tests/security_marking.rs::audit_failure_after_completion_is_a_refusal`](../../mimir/model-server/tests/security_marking.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::real_socket_round_trip`](../../services/http-gateway/tests/http.rs)
  - [`deploy/compose/smoke.sh:24`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`README.md:54`](../../README.md)
  - [`CHANGELOG.md:244`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:334`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:248`](../../CHANGELOG.md)
  - [`CHANGELOG.md:335`](../../CHANGELOG.md)

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
  - [`README.md:313`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:335`](../../docs/launch/pipeline/build-deck.js)

### `checked-import` (operable)

mg-admin import adds objects and links from a JSON Lines file with a published record schema. Every record is checked against the ontology and the store before anything is written, and one bad line refuses the whole file. A mesh sync may raise but never lower an object's marking, never widen its purposes, and never touch an object under a human gate.

- Source:
  - [`tools/mg-admin/src/main.rs:392`](../../tools/mg-admin/src/main.rs)
  - [`services/http-gateway/src/import_file.rs:91`](../../services/http-gateway/src/import_file.rs)
  - [`contracts/schema/import-record.v1.schema.json`](../../contracts/schema/import-record.v1.schema.json)
  - [`services/http-gateway/src/mesh_sync.rs:258`](../../services/http-gateway/src/mesh_sync.rs)
  - [`tools/mg-admin/src/main.rs:440`](../../tools/mg-admin/src/main.rs)
- Tests:
  - [`services/http-gateway/tests/import_file.rs::a_bad_line_refuses_the_whole_file_and_names_it`](../../services/http-gateway/tests/import_file.rs)
  - [`services/http-gateway/tests/contract_schemas.rs::import_fixtures_get_the_same_verdict_from_the_schema_and_the_importer`](../../services/http-gateway/tests/contract_schemas.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_sync_that_lowers_a_marking_is_refused_and_changes_nothing`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_sync_may_raise_a_marking_and_narrow_purposes`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_sync_above_the_sync_clearance_is_refused`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_sync_never_touches_a_human_gated_target`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::an_unpermitted_peer_learns_nothing_from_a_malformed_sync`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_malformed_sync_from_a_permitted_peer_is_still_audited`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`services/http-gateway/src/enclosure.rs::gated_targets_cover_live_and_redeemed_approvals`](../../services/http-gateway/src/enclosure.rs)
- Service tests:
  - [`tools/mg-admin/tests/import.rs::an_import_fills_a_real_enclosure_that_a_gateway_then_serves`](../../tools/mg-admin/tests/import.rs)
- Stated at:
  - [`README.md:329`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:335`](../../docs/launch/pipeline/build-deck.js)

### `human-gated-actions` (operable)

An action the ontology marks as human-gated runs only with an approval that a second person granted to this requester for this action on this object, unexpired and unused; request, decision and redemption are bound in the signed ledger by approval id, requester, approver and target, and approvals survive a restart.

**Limit.** Approvals are bounded (256 total, 16 open per requester, pending entries expire after 24 hours); a full queue refuses new requests with 429.

- Source:
  - [`services/http-gateway/src/enclosure.rs:987`](../../services/http-gateway/src/enclosure.rs)
  - [`services/http-gateway/src/enclosure.rs:750`](../../services/http-gateway/src/enclosure.rs)
  - [`services/http-gateway/src/enclosure.rs:1008:fn redeemable`](../../services/http-gateway/src/enclosure.rs)
- Tests:
  - [`services/http-gateway/tests/fabric_edge.rs::a_gated_action_without_a_human_gate_token_is_refused_and_changes_nothing`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::an_approval_is_bound_to_its_requester_action_and_object_and_redeems_once`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::the_ledger_binds_request_decision_and_redemption_to_the_approval`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::pending_and_approved_gates_are_not_evicted_by_a_flood_of_requests`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/fabric_edge.rs::a_pending_approval_expires_and_is_audited`](../../services/http-gateway/tests/fabric_edge.rs)
  - [`services/http-gateway/tests/durable_enclosure.rs::approvals_survive_a_restart_and_redeem_once`](../../services/http-gateway/tests/durable_enclosure.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::real_socket_round_trip`](../../services/http-gateway/tests/http.rs)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:333`](../../docs/launch/pipeline/build-deck.js)

### `login-and-session-tokens` (operable)

A principal logs in at the edge with a password, and a time-based one-time code when enrolled with one, and receives an ML-DSA-signed session token. Principals are held in a durable directory. A session can be ended at the server by the holder of its token, and a revoked or de-enrolled token is refused. Passwords are stored as Argon2id (m=19 MiB, t=2, p=1); a legacy PBKDF2-HMAC-SHA256 entry (210,000 iterations) still verifies and is re-hashed to Argon2id at the next successful sign-in, and a stored cost outside fixed bounds is refused. A one-time code is accepted once (RFC 6238 section 5.2): the step of the last code accepted is kept per principal in the directory, across restarts, and that code or an earlier one is refused.

**Limit.** The console's login form has no field for the one-time code. Login fields are bounded and attempts are limited per name and per source; under compose every client shares the proxy's address, so the per-source limit is one for the whole site. Anyone who can reach the edge can lock any name for up to 15 minutes.

- Source:
  - [`services/iam/src/auth.rs:1`](../../services/iam/src/auth.rs)
  - [`services/http-gateway/src/revocations.rs:1`](../../services/http-gateway/src/revocations.rs)
  - [`services/iam/src/password.rs:1`](../../services/iam/src/password.rs)
  - [`services/iam/src/mfa.rs:1`](../../services/iam/src/mfa.rs)
  - [`services/iam/src/directory.rs:1`](../../services/iam/src/directory.rs)
- Tests:
  - [`services/http-gateway/tests/rc_37.rs::login_issues_a_pq_token_that_authorises_data_requests`](../../services/http-gateway/tests/rc_37.rs)
  - [`services/http-gateway/tests/login_directory.rs::a_durable_principal_needs_both_factors_and_survives_a_restart`](../../services/http-gateway/tests/login_directory.rs)
  - [`services/http-gateway/tests/security_edge.rs::a_one_mib_username_is_refused_before_any_state_is_touched`](../../services/http-gateway/tests/security_edge.rs)
  - [`services/http-gateway/tests/security_edge.rs::a_flooding_source_is_refused_before_verification`](../../services/http-gateway/tests/security_edge.rs)
  - [`services/http-gateway/tests/security_edge.rs::a_source_throttle_is_recorded_once_per_minute`](../../services/http-gateway/tests/security_edge.rs)
  - [`services/http-gateway/tests/edge_limits.rs::the_first_throttled_attempt_is_on_the_ledger_and_later_ones_are_coalesced`](../../services/http-gateway/tests/edge_limits.rs)
  - [`services/http-gateway/tests/security_edge.rs::parallel_wrong_passwords_cannot_exceed_the_free_failures`](../../services/http-gateway/tests/security_edge.rs)
  - [`services/http-gateway/tests/security_edge.rs::a_busy_name_is_429_quickly`](../../services/http-gateway/tests/security_edge.rs)
  - [`services/http-gateway/tests/session_revocation.rs::a_token_is_refused_after_sign_out`](../../services/http-gateway/tests/session_revocation.rs)
  - [`services/http-gateway/tests/session_revocation.rs::revocation_survives_a_restart_and_expired_entries_are_purged`](../../services/http-gateway/tests/session_revocation.rs)
  - [`services/http-gateway/tests/session_revocation.rs::a_token_for_a_removed_subject_is_refused`](../../services/http-gateway/tests/session_revocation.rs)
  - [`services/http-gateway/tests/secured_edge.rs::an_expired_token_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::a_not_yet_valid_token_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::a_token_from_another_issuer_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::a_non_hex_bearer_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::a_missing_scheme_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/http-gateway/tests/secured_edge.rs::a_token_with_trailing_bytes_is_rejected_401`](../../services/http-gateway/tests/secured_edge.rs)
  - [`services/iam/tests/primitives.rs::argon2id_hashes_verify_and_fail_closed`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/primitives.rs::pbkdf2_iteration_floor_and_ceiling_are_enforced`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/password_storage.rs::a_legacy_entry_is_upgraded_to_argon2id_on_sign_in_and_survives_a_restart`](../../services/iam/tests/password_storage.rs)
  - [`services/http-gateway/tests/login_directory.rs::a_legacy_password_hash_is_upgraded_at_the_next_sign_in`](../../services/http-gateway/tests/login_directory.rs)
  - [`services/http-gateway/tests/login_directory.rs::a_totp_code_cannot_be_reused_within_the_window`](../../services/http-gateway/tests/login_directory.rs)
  - [`services/http-gateway/tests/login_directory.rs::the_last_accepted_totp_step_survives_a_restart`](../../services/http-gateway/tests/login_directory.rs)
  - [`services/iam/tests/primitives.rs::totp_verify_step_returns_the_step_and_refuses_used_or_older_steps`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/rc_29.rs::a_wrong_password_does_not_consume_the_totp_step`](../../services/iam/tests/rc_29.rs)
  - [`services/iam/tests/rc_29.rs::a_reused_code_is_refused_by_the_authenticator`](../../services/iam/tests/rc_29.rs)
  - [`services/iam/tests/rc_29.rs::the_directory_reads_v2_records_and_replays_totp_steps`](../../services/iam/tests/rc_29.rs)
- Service tests:
  - [`tools/mg-admin/tests/cli.rs::a_principal_enrolled_by_mg_admin_logs_in_through_the_gateway`](../../tools/mg-admin/tests/cli.rs)
- Stated at:
  - [`README.md:54`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:447`](../../docs/launch/pipeline/build-deck.js)

### `plaintext-edge-guard` (operable)

The gateway edge speaks plaintext HTTP and is kept off the network by default. It binds loopback only unless --allow-non-loopback is given with a state directory; development mode with its demo logins needs --dev and is loopback only with no override. The console refuses a plaintext gateway URL off loopback when it is built and when it starts, and its static server sends the same security headers on every location.

**Limit.** TLS termination in front of the edge is a deployment responsibility. The code enforces it only by refusing non-loopback binds and URLs; style-src keeps unsafe-inline for the design system.

- Source:
  - [`services/http-gateway/src/cli.rs:1`](../../services/http-gateway/src/cli.rs)
  - [`sdk/ts/src/http/baseUrl.ts:1`](../../sdk/ts/src/http/baseUrl.ts)
  - [`deploy/installer/console.security-headers.conf:1`](../../deploy/installer/console.security-headers.conf)
- Tests:
  - [`services/http-gateway/tests/cli_modes.rs::a_bare_address_or_no_arguments_is_a_usage_error`](../../services/http-gateway/tests/cli_modes.rs)
  - [`services/http-gateway/tests/cli_modes.rs::non_loopback_binds_are_refused_unless_allowed`](../../services/http-gateway/tests/cli_modes.rs)
  - [`services/http-gateway/src/lib.rs::check_bind_refuses_non_loopback_hosts_unless_allowed`](../../services/http-gateway/src/lib.rs)
- Service tests:
  - [`services/http-gateway/tests/cli_modes.rs::dev_mode_serves_the_seeded_login_on_loopback`](../../services/http-gateway/tests/cli_modes.rs)
- Stated at:
  - [`README.md:162`](../../README.md)
  - [`deploy/README.md:95`](../../deploy/README.md)

### `webauthn-oidc-federation` (reference)

WebAuthn assertion verification, a JWKS endpoint and federation with an enterprise identity provider. The principal assertion verifier checks the type, challenge and origin of client data parsed at the edge, the RP id, user presence, user verification by policy and the signature counter, which the directory persists per passkey. Federation assertions carry an issuer, an audience, a validity window of at most five minutes and a single-use nonce, and the bridge refuses one that is mis-scoped, stale, not yet valid or replayed.

**Until it is operable, say:** "Users sign in with a password and an optional one-time code. Administrators hold ML-DSA passkeys in keystore files and prove a second factor, a one-time code the gateway checks and accepts once, on every administration request; the passkey alone is not MFA. WebAuthn verification and federation are implemented and tested as libraries; no browser flow uses them. Federation assertions carry an issuer, an audience, a validity window and a single-use nonce, and the replay cache is per process. The principal verifier checks type, challenge, origin, RP id, user presence, user verification by policy and the signature counter, and accepts no origin until one is configured; passkey registration (COSE/CBOR attestation) is out of scope."

**Milestone.** The second-factor decision is recorded and the console login uses it (RF-IAM-1, RF-UI-1).

- Source:
  - [`admin/identity/src/webauthn.rs:1`](../../admin/identity/src/webauthn.rs)
  - [`services/iam/src/directory.rs:1`](../../services/iam/src/directory.rs)
  - [`services/iam/src/federation.rs:1`](../../services/iam/src/federation.rs)
  - [`services/iam/src/mfa.rs:1`](../../services/iam/src/mfa.rs)
  - [`services/http-gateway/src/webauthn_client_data.rs:1`](../../services/http-gateway/src/webauthn_client_data.rs)
- Tests:
  - [`admin/identity/tests/passkeys.rs::a_registered_passkey_signs_in_after_a_restart`](../../admin/identity/tests/passkeys.rs)
  - [`admin/identity/tests/passkeys.rs::an_assertion_for_one_purpose_does_not_verify_for_another`](../../admin/identity/tests/passkeys.rs)
  - [`services/iam/tests/federation.rs::a_fresh_assertion_mints_a_local_token`](../../services/iam/tests/federation.rs)
  - [`services/iam/tests/federation.rs::expired_and_future_assertions_are_refused`](../../services/iam/tests/federation.rs)
  - [`services/iam/tests/federation.rs::a_replayed_assertion_is_refused_and_the_cache_is_bounded`](../../services/iam/tests/federation.rs)
  - [`services/iam/tests/federation.rs::audience_and_issuer_mismatches_are_refused`](../../services/iam/tests/federation.rs)
  - [`services/iam/tests/federation.rs::overlong_lifetime_and_v1_payloads_are_refused`](../../services/iam/tests/federation.rs)
  - [`services/iam/tests/primitives.rs::webauthn_assertion_verifies_and_fails_closed`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/primitives.rs::webauthn_origin_mismatch_is_refused`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/primitives.rs::webauthn_client_data_is_read_from_the_signed_bytes`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/primitives.rs::webauthn_uv_is_required_by_policy`](../../services/iam/tests/primitives.rs)
  - [`services/iam/tests/primitives.rs::webauthn_sign_count_must_increase`](../../services/iam/tests/primitives.rs)
  - [`services/http-gateway/tests/webauthn_client_data.rs::client_data_type_and_challenge_are_read_from_parsed_json`](../../services/http-gateway/tests/webauthn_client_data.rs)
  - [`services/iam/tests/rc_29.rs::a_passkey_sign_count_survives_a_restart`](../../services/iam/tests/rc_29.rs)
  - [`services/iam/tests/rc_29.rs::a_passkey_login_through_the_authenticator_records_the_counter`](../../services/iam/tests/rc_29.rs)

### `policy-decisions` (operable)

Access is denied unless a written rule grants it: role, marking and purpose must all match, and mandatory access control is evaluated first. On the mesh, clearance and permitted purposes are certificate attributes signed by the IdP; a declared posture can only narrow them, and every mesh service authorises a request at the clearance the certificate grants. A decision can be dry-run at the edge, and the dry-run is signed and audited. An inference call is decided the same way, with the marking of the data in its prompt as the resource's marking, at the lower of the caller's certified clearance and the model's accreditation.

**Limit.** On the mesh, purposes are bound only by certificates issued with mg-admin mesh issue --purposes; a certificate issued without them, or before them, is unconstrained in purpose. A mesh resource is open unless its service is given --resource-marking.

- Source:
  - [`services/runtime/src/policy_file.rs:1`](../../services/runtime/src/policy_file.rs)
  - [`services/fabric/src/resource.rs:1`](../../services/fabric/src/resource.rs)
  - [`crypto/pq-mtls/src/handshake.rs:594`](../../crypto/pq-mtls/src/handshake.rs)
  - [`services/runtime/src/mesh.rs:18`](../../services/runtime/src/mesh.rs)
  - [`mimir/model-server/src/handler.rs:159`](../../mimir/model-server/src/handler.rs)
- Tests:
  - [`services/http-gateway/tests/policy_file.rs::an_empty_rule_set_is_valid_and_grants_nothing`](../../services/http-gateway/tests/policy_file.rs)
  - [`crypto/pq-mtls/tests/authz.rs::deny_by_default_pdp_blocks_channel`](../../crypto/pq-mtls/tests/authz.rs)
  - [`services/http-gateway/tests/policy_resources.rs::a_type_scoped_deny_holds_on_query_search_around_and_per_object_reads`](../../services/http-gateway/tests/policy_resources.rs)
  - [`crypto/pq-mtls/tests/security_attributes.rs::a_posture_above_the_certificate_is_clipped_to_the_certificate`](../../crypto/pq-mtls/tests/security_attributes.rs)
  - [`crypto/pq-mtls/tests/security_attributes.rs::a_purpose_outside_the_certificate_is_refused`](../../crypto/pq-mtls/tests/security_attributes.rs)
  - [`services/runtime/src/mesh.rs::mesh_context_takes_clearance_from_the_certificate`](../../services/runtime/src/mesh.rs)
  - [`services/http-gateway/tests/mesh_sync.rs::a_worker_cannot_sync_above_its_certified_clearance`](../../services/http-gateway/tests/mesh_sync.rs)
  - [`mimir/model-server/tests/security_marking.rs::marked_prompt_against_unaccredited_model_is_denied_and_audited`](../../mimir/model-server/tests/security_marking.rs)
  - [`mimir/model-server/tests/security_marking.rs::compartmented_prompt_needs_the_compartment`](../../mimir/model-server/tests/security_marking.rs)
- Service tests:
  - [`services/http-gateway/tests/http.rs::fabric_and_governance_routes_are_not_reachable_without_a_caller`](../../services/http-gateway/tests/http.rs)
  - [`deploy/kind/smoke.sh:115`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:140`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:351`](../../docs/launch/pipeline/build-deck.js)

### `dual-control-enrolment` (operable)

Enrolling a principal online needs two different administrators, each proving possession of a post-quantum passkey, opened under its holder's passphrase, and a one-time code the gateway checks and accepts once, and the enrolment is audited with the factors verified. Neither administrator holds the principal's credentials: they are generated at approval and activated by the principal with a single-use code that sets their password and gives them their TOTP factor. Roles and purposes are limited to those the served policy names, the approver sees the full record, and a failed approval can be retried or the enrolment rejected.

- Criteria: AC-AD-3, AC-AD-4
- Source:
  - [`services/http-gateway/src/admin_api.rs:311`](../../services/http-gateway/src/admin_api.rs)
  - [`services/http-gateway/src/admin_api.rs:402`](../../services/http-gateway/src/admin_api.rs)
  - [`services/http-gateway/src/admin_totp.rs:1`](../../services/http-gateway/src/admin_totp.rs)
  - [`admin/identity/src/session.rs:1`](../../admin/identity/src/session.rs)
  - [`services/http-gateway/src/activation.rs:43`](../../services/http-gateway/src/activation.rs)
  - [`services/iam/src/auth.rs:123`](../../services/iam/src/auth.rs)
- Tests:
  - [`services/http-gateway/tests/admin_enrolment.rs::an_enrolment_needs_a_second_administrator_and_survives_a_restart`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::a_requester_cannot_supply_credential_material`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::an_unproven_caller_learns_nothing_about_roles_or_enrolments`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::a_challenge_for_one_operation_cannot_be_used_for_another`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::approval_returns_an_activation_code_and_no_second_factor_secret`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::activation_is_single_use_and_first_login_needs_both_factors`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::roles_and_purposes_outside_the_policy_are_refused_and_the_approver_sees_the_full_record`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::a_failed_approval_can_be_retried_and_a_parked_enrolment_can_be_rejected`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::a_passkey_without_a_second_factor_is_refused`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`services/http-gateway/tests/admin_enrolment.rs::an_administrator_totp_code_is_single_use`](../../services/http-gateway/tests/admin_enrolment.rs)
  - [`admin/identity/tests/ac_ad_1_2.rs::ac_ad_1_a_passkey_alone_does_not_satisfy_mfa`](../../admin/identity/tests/ac_ad_1_2.rs)
  - [`admin/identity/tests/ac_ad_1_2.rs::ac_ad_1_a_second_factor_not_the_uv_flag_grants_mfa`](../../admin/identity/tests/ac_ad_1_2.rs)
  - [`admin/privilege/tests/ac_ad_3_4_13.rs::ac_ad_1_a_passkey_alone_cannot_request_or_approve`](../../admin/privilege/tests/ac_ad_3_4_13.rs)
  - [`services/iam/tests/rc_29.rs::an_unactivated_record_refuses_login_with_the_same_401`](../../services/iam/tests/rc_29.rs)
  - [`services/iam/tests/rc_29.rs::the_directory_reads_v1_records_and_writes_v2`](../../services/iam/tests/rc_29.rs)
- Service tests:
  - [`tools/mg-admin/tests/enrol.rs::two_administrators_enrol_a_principal_through_a_running_gateway`](../../tools/mg-admin/tests/enrol.rs)
  - [`tools/mg-admin/tests/enrol.rs::enrol_reject_removes_a_parked_enrolment`](../../tools/mg-admin/tests/enrol.rs)
  - [`tools/mg-admin/tests/enrol.rs::administration_needs_the_passphrase_and_a_one_time_code`](../../tools/mg-admin/tests/enrol.rs)
  - [`tools/mg-admin/tests/enrol.rs::a_wrong_passkey_passphrase_stops_before_the_gateway`](../../tools/mg-admin/tests/enrol.rs)
- Stated at:
  - [`CHANGELOG.md:251`](../../CHANGELOG.md)
  - [`CHANGELOG.md:236`](../../CHANGELOG.md)
  - [`CHANGELOG.md:318`](../../CHANGELOG.md)
  - [`CHANGELOG.md:255`](../../CHANGELOG.md)
  - [`CHANGELOG.md:342`](../../CHANGELOG.md)

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

Break-glass access: a scoped, time-boxed elevation that alerts a security officer, expires by itself and is audited throughout. Grants are time-boxed to at most four hours, the approver must differ from the principal, and review or close revokes the capability at once. It takes two people: the requester and a different approver, each signed in with their own passkey and each holding the role that entitles them (the approver a security officer), and both identities are sealed into the audit ledger.

**Until it is operable, say:** "Break-glass alerting with durable, at-least-once delivery, and two-person approval between passkey-authenticated administrators, are implemented and tested as a library (AC-AD-8). The admin console is a design-complete interface running on fixture data; it is not connected to a backend."

**Milestone.** RF-CA-11: the alert path runs in a service and the console reads it.

- Criteria: AC-AD-8
- Source:
  - [`admin/breakglass/src/alert.rs:84`](../../admin/breakglass/src/alert.rs)
  - [`admin/breakglass/src/service.rs:1`](../../admin/breakglass/src/service.rs)
  - [`admin/breakglass/src/service.rs:20`](../../admin/breakglass/src/service.rs)
  - [`admin/breakglass/src/controller.rs:156`](../../admin/breakglass/src/controller.rs)
- Tests:
  - [`admin/breakglass/tests/rc_32.rs::grant_delivers_a_real_alert_and_elevates_access_which_auto_expires`](../../admin/breakglass/tests/rc_32.rs)
  - [`admin/breakglass/tests/rc_32.rs::alert_delivery_is_durable_across_a_transient_bus_outage`](../../admin/breakglass/tests/rc_32.rs)
  - [`admin/breakglass/tests/ac_ad_8.rs::a_ttl_above_the_ceiling_is_refused`](../../admin/breakglass/tests/ac_ad_8.rs)
  - [`admin/breakglass/tests/rc_32.rs::review_revokes_the_capability_immediately`](../../admin/breakglass/tests/rc_32.rs)
  - [`admin/breakglass/tests/rc_32.rs::close_revokes_and_audits`](../../admin/breakglass/tests/rc_32.rs)
  - [`admin/breakglass/tests/durable_restart.rs::the_controllers_close_survives_a_restart`](../../admin/breakglass/tests/durable_restart.rs)
  - [`admin/breakglass/tests/durable_restart.rs::a_journal_record_past_the_four_hour_cap_is_refused`](../../admin/breakglass/tests/durable_restart.rs)
  - [`admin/breakglass/tests/mg_sec_07.rs::the_requester_cannot_approve_their_own_break_glass`](../../admin/breakglass/tests/mg_sec_07.rs)
  - [`admin/breakglass/tests/mg_sec_07.rs::an_unauthenticated_approver_is_refused`](../../admin/breakglass/tests/mg_sec_07.rs)
  - [`admin/breakglass/tests/mg_sec_07.rs::a_valid_two_person_grant_records_both_identities_in_the_ledger`](../../admin/breakglass/tests/mg_sec_07.rs)
  - [`admin/breakglass/tests/mg_sec_07.rs::an_approver_without_breakglass_approve_is_refused`](../../admin/breakglass/tests/mg_sec_07.rs)
  - [`admin/breakglass/tests/mg_sec_07.rs::a_requester_without_breakglass_request_is_refused`](../../admin/breakglass/tests/mg_sec_07.rs)

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

**Until it is operable, say:** "The Midgard core console can be pointed at the HTTP edge and then shows live enclosure data; no automated test drives it. The other six consoles run on built-in sample data under a demonstration banner, because their services expose no console routes."

**Milestone.** Each console has a backend route and a test that drives it (RF-UID-1, RF-TST-2).

- Source:
  - [`products/admin/ui/src/main.tsx:33:new AdminMock`](../../products/admin/ui/src/main.tsx)
  - [`products/admin/ui/src/main.tsx:17`](../../products/admin/ui/src/main.tsx)
  - [`products/admin/ui/src/app/routes.tsx:1`](../../products/admin/ui/src/app/routes.tsx)
  - [`tools/ci/tests/js-toolchain.test.sh`](../../tools/ci/tests/js-toolchain.test.sh)
- Tests:
  - [`services/http-gateway/tests/rc_37.rs::login_issues_a_pq_token_that_authorises_data_requests`](../../services/http-gateway/tests/rc_37.rs)
- Stated at:
  - [`README.md:816`](../../README.md)
  - [`CHANGELOG.md:277`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:118`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:544`](../../docs/launch/pipeline/build-deck.js)
  - [`README.md:812`](../../README.md)
  - [`CHANGELOG.md:292`](../../CHANGELOG.md)
  - [`README.md:824`](../../README.md)
  - [`README.md:814`](../../README.md)
  - [`CHANGELOG.md:296`](../../CHANGELOG.md)
  - [`CHANGELOG.md:383`](../../CHANGELOG.md)

## Mimir

### `mimir-inference-cpu` (operable)

mimir-server answers inference requests from a real model inside the enclosure: an OpenAI-compatible engine on loopback (llama.cpp on CPU), reached only over PQ-mTLS by a certified caller, with every call audited and admitted against a budget: max_tokens clamped to a ceiling, a bounded prompt, and bounded calls in flight per identity and in all. The caller's system prompt reaches the model in the family's system turn, and the family's control tokens in any text are neutralised. Each request declares the marking of the data in its prompt, which the model's declared accreditation and the caller's certified clearance must both dominate, and a served completion leaves the SHA-256 of its prompt and of its text in the ledger. An operator can disengage it while it runs, and it drains the calls in flight before reporting the stop.

**Limit.** One model has been served: IBM Granite 3.3 2B Instruct on CPU. The caller receives one whole completion, not a token stream. A completion the sidecar is generating cannot be cancelled: it fails after the inference timeout passes with nothing read, and its slot is then free. The prompt and completion cross the loopback to the engine in clear, inside a network namespace only mimir-server and the engine share (a recorded trust assumption). The ledger keeps digests of the prompt and completion, not copies.

- Source:
  - [`mimir/model-server/src/backends/openai_compat.rs:60`](../../mimir/model-server/src/backends/openai_compat.rs)
  - [`mimir/model-server/src/handler.rs:447`](../../mimir/model-server/src/handler.rs)
  - [`mimir/mimir-server/src/main.rs:301`](../../mimir/mimir-server/src/main.rs)
  - [`mimir/model-server/src/backends/openai_compat.rs:203`](../../mimir/model-server/src/backends/openai_compat.rs)
  - [`mimir/model-server/src/marking.rs:1`](../../mimir/model-server/src/marking.rs)
- Tests:
  - [`mimir/mimir-server/tests/serve.rs::inference_is_served_over_pqmtls_audited_and_survives_a_restart`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/model-server/tests/security_admission.rs::max_tokens_is_clamped_to_the_ceiling`](../../mimir/model-server/tests/security_admission.rs)
  - [`mimir/model-server/tests/security_admission.rs::oversized_prompt_is_refused_and_audited`](../../mimir/model-server/tests/security_admission.rs)
  - [`mimir/model-server/tests/security_admission.rs::third_concurrent_call_for_one_identity_is_busy`](../../mimir/model-server/tests/security_admission.rs)
  - [`mimir/model-server/tests/security_admission.rs::global_limit_applies_across_identities`](../../mimir/model-server/tests/security_admission.rs)
  - [`mimir/mimir-server/tests/serve.rs::a_burst_over_the_rate_limit_is_throttled`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/model-server/tests/security_admission.rs::a_stop_lets_calls_in_flight_finish_and_counts_them`](../../mimir/model-server/tests/security_admission.rs)
  - [`mimir/mimir-server/tests/serve.rs::the_stop_file_disengages_and_removing_it_resumes`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/mimir-server/tests/serve.rs::in_flight_completion_drains_before_stop_is_logged`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/mimir-server/tests/serve.rs::a_signed_model_is_served_through_its_sidecar_and_a_tampered_one_never_starts`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/model-server/src/backends/openai_compat.rs::streams_a_completion_through_the_family_template`](../../mimir/model-server/src/backends/openai_compat.rs)
  - [`mimir/mimir-server/tests/serve.rs::a_prompt_marked_above_the_model_accreditation_is_refused_and_audited`](../../mimir/mimir-server/tests/serve.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:19`](../../deploy/compose/smoke.sh)
  - [`deploy/compose/granite-smoke.sh:40`](../../deploy/compose/granite-smoke.sh)
  - [`deploy/kind/smoke.sh:73`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:98`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:252`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:22`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:381`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:267`](../../CHANGELOG.md)
  - [`CHANGELOG.md:271`](../../CHANGELOG.md)
  - [`CHANGELOG.md:358`](../../CHANGELOG.md)

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
  - [`CHANGELOG.md:289`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:114`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:541`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:293`](../../CHANGELOG.md)
  - [`CHANGELOG.md:380`](../../CHANGELOG.md)

### `signed-model-manifest` (operable)

A model loads only from a manifest signed with ML-DSA, and its weights are checked against the signed digest before anything is served. One changed byte stops the service from starting. Revocation is enforced while the model is served, not only when it loads: a running service re-checks the manifest against the current trust key and revocation list on an interval, and stops serving when the id is revoked or the manifest no longer verifies.

**Limit.** No vendor signature is checked cryptographically; an upstream signature is recorded as unverifiable. The trust anchor is the pinned deploy/trust/model-signing.pub, never the models volume, but the key ceremony has not yet committed it, so a real model is served today only under an explicit, warned development key (stage-model.sh --development, in a trust volume of its own). The one real model served so far did not pass through the staging intake.

- Criteria: AC-MS-2
- Source:
  - [`tools/mg-model/src/main.rs:1`](../../tools/mg-model/src/main.rs)
  - [`mimir/model-server/src/artifact.rs:427`](../../mimir/model-server/src/artifact.rs)
  - [`tools/mg-model/src/main.rs:225`](../../tools/mg-model/src/main.rs)
  - [`mimir/mimir-server/src/main.rs:162`](../../mimir/mimir-server/src/main.rs)
  - [`deploy/trust/README.md`](../../deploy/trust/README.md)
  - [`deploy/compose/stage-model.sh`](../../deploy/compose/stage-model.sh)
- Tests:
  - [`mimir/model-server/tests/model_dir.rs::changed_truncated_or_swapped_weights_are_refused`](../../mimir/model-server/tests/model_dir.rs)
  - [`mimir/model-server/tests/model_dir.rs::a_verified_manifest_is_rechecked_against_the_current_key_and_revocations`](../../mimir/model-server/tests/model_dir.rs)
  - [`mimir/model-server/tests/model_dir.rs::a_revocation_list_parses_strictly`](../../mimir/model-server/tests/model_dir.rs)
  - [`tools/mg-model/tests/cli.rs::sign_without_sha256_is_a_usage_error`](../../tools/mg-model/tests/cli.rs)
  - [`tools/mg-model/tests/cli.rs::verify_refuses_a_trust_file_inside_the_model_dir`](../../tools/mg-model/tests/cli.rs)
  - [`tools/mg-model/tests/cli.rs::keygen_writes_keystore_v2_under_a_passphrase`](../../tools/mg-model/tests/cli.rs)
- Service tests:
  - [`mimir/mimir-server/tests/serve.rs::a_signed_model_is_served_through_its_sidecar_and_a_tampered_one_never_starts`](../../mimir/mimir-server/tests/serve.rs)
  - [`deploy/compose/granite-smoke.sh:31`](../../deploy/compose/granite-smoke.sh)
  - [`mimir/mimir-server/tests/serve.rs::a_revoked_model_stops_serving_within_the_interval`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/mimir-server/tests/serve.rs::a_replaced_trust_anchor_stops_serving`](../../mimir/mimir-server/tests/serve.rs)
  - [`mimir/mimir-server/tests/serve.rs::a_trust_anchor_inside_the_model_dir_is_refused_at_start`](../../mimir/mimir-server/tests/serve.rs)
  - [`deploy/compose/granite-smoke.sh:40`](../../deploy/compose/granite-smoke.sh)
- Stated at:
  - [`CHANGELOG.md:269`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:380`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:273`](../../CHANGELOG.md)
  - [`CHANGELOG.md:360`](../../CHANGELOG.md)

### `model-family-routing` (reference)

Routing a workload to one of several model families (Mistral, Nemotron, Gemma, Granite) and hot-swapping between them without a change to the consumer. Each family's adapter places the system prompt in the family's system role (ahead of the user text where the family has none) and neutralises the family's control tokens in all text, so the only role markers are the template's own.

**Until it is operable, say:** "Routing across model families and hot-swap between them are implemented and tested as a library against a deterministic engine (AC-MS-9). The running inference service serves one model; one real model, IBM Granite 3.3 2B on CPU, has been served end to end."

**Milestone.** RF-CA-8: mimir-server uses the router with more than one real model.

- Criteria: AC-MS-4, AC-MS-9
- Source:
  - [`mimir/model-server/src/router.rs:28`](../../mimir/model-server/src/router.rs)
  - [`mimir/model-server/src/adapter.rs:27`](../../mimir/model-server/src/adapter.rs)
  - [`mimir/model-server/src/adapter.rs:74`](../../mimir/model-server/src/adapter.rs)
- Tests:
  - [`mimir/model-server/tests/router.rs::ac_ms_4_workload_routes_to_the_right_family_no_consumer_change`](../../mimir/model-server/tests/router.rs)
  - [`mimir/model-server/tests/security_adapter_tokens.rs::injected_control_tokens_survive_only_as_literals`](../../mimir/model-server/tests/security_adapter_tokens.rs)
  - [`mimir/model-server/tests/security_adapter_tokens.rs::granite_and_nemotron_carry_the_system_text_in_their_system_turn`](../../mimir/model-server/tests/security_adapter_tokens.rs)
  - [`mimir/model-server/tests/security_adapter_tokens.rs::an_empty_system_text_renders_the_single_turn_shape`](../../mimir/model-server/tests/security_adapter_tokens.rs)
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

A governed agent loop: plan, ground, assemble (untrusted grounding, memory, history and user text as delimited, provenance-tagged data that cannot forge a layer header), guard the answer and every proposed action, refuse an action the catalogue does not declare or whose parameters fall outside its declared keys and bounds, evaluate, check reversibility, pass a human gate for consequential or irreversible actions, audit, then execute.

**Until it is operable, say:** "The governed agent loop is implemented and tested as a library (AC-MI-3): a consequential action runs only after a recorded human decision. No service runs the loop, so no agent acts on a deployed enclosure."

**Milestone.** A service hosts the loop, with executors beyond the object fabric (RF-MI-4).

- Criteria: AC-MI-3
- Source:
  - [`mimir/agent-runtime/src/govloop.rs:368`](../../mimir/agent-runtime/src/govloop.rs)
  - [`mimir/agent-runtime/src/govloop.rs:190`](../../mimir/agent-runtime/src/govloop.rs)
  - [`mimir/agent-runtime/src/assembly.rs:346`](../../mimir/agent-runtime/src/assembly.rs)
- Tests:
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::mutation_is_denied_when_human_denies`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::irreversible_action_forces_human_even_if_preauthorised`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::malicious_action_params_halt_the_turn`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::unknown_param_key_is_rejected_before_the_gate`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/ac_mi_3.rs::oversized_param_value_is_rejected`](../../mimir/agent-runtime/tests/ac_mi_3.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::forged_header_in_grounding_is_data`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::forged_header_in_user_memory_history_is_data`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::token_accounting_is_exact_with_blocks`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)

### `guardrails` (reference)

Input guardrails against prompt injection, applied to each untrusted item (grounding fact, memory item, history turn, user message) before composition, never to the trusted system, contract, policy or tool layers, which are never rewritten: a deterministic pre-filter with de-obfuscation that exempts identifiers and hashes, a classifier seam with no shipped implementation, and rewriting of only the item that can be made safe; the same pre-filter screens the agent's whole output, proposed actions included; and every prompt assembler places untrusted text only in indented, provenance-tagged DATA blocks with control and zero-width characters removed, each field bounded and the per-request boundary neutralised.

**Until it is operable, say:** "Input guardrails and prompt delimiting are implemented and tested as a library against a small hand-written corpus. The pre-filter is applied per untrusted item and trusted layers are never rewritten. The classifier seam has no shipped implementation (only test doubles implement it), the data-handling rule the model is given is advisory, and no running service applies the guardrails."

**Milestone.** A classifier served by the model plane, a corpus with a stated block rate, and a service that applies them (RF-MI-7).

- Source:
  - [`mimir/agent-runtime/src/assembly.rs:300`](../../mimir/agent-runtime/src/assembly.rs)
  - [`mimir/agent-runtime/src/defend.rs:607`](../../mimir/agent-runtime/src/defend.rs)
  - [`mimir/prompts/src/delimit.rs:245`](../../mimir/prompts/src/delimit.rs)
- Tests:
  - [`mimir/agent-runtime/tests/guardrail_scope.rs::sanitise_rewrites_only_the_offending_item`](../../mimir/agent-runtime/tests/guardrail_scope.rs)
  - [`mimir/agent-runtime/tests/guardrail_scope.rs::trusted_layers_are_never_inspected`](../../mimir/agent-runtime/tests/guardrail_scope.rs)
  - [`mimir/agent-runtime/tests/guardrail_scope.rs::data_word_in_grounding_no_longer_blocks_the_turn`](../../mimir/agent-runtime/tests/guardrail_scope.rs)
  - [`mimir/agent-runtime/tests/guardrail_scope.rs::object_id_and_hash_are_not_sanitised`](../../mimir/agent-runtime/tests/guardrail_scope.rs)
  - [`mimir/agent-runtime/tests/guardrail_scope.rs::injection_in_user_message_still_blocks`](../../mimir/agent-runtime/tests/guardrail_scope.rs)
  - [`mimir/agent-runtime/tests/guardrail_paraphrase.rs::obfuscated_paraphrases_are_blocked_even_though_not_literal_entries`](../../mimir/agent-runtime/tests/guardrail_paraphrase.rs)
  - [`mimir/agent-runtime/tests/guardrail_paraphrase.rs::sanitise_rewrites_the_prompt_instead_of_passing_it_through`](../../mimir/agent-runtime/tests/guardrail_paraphrase.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::control_and_zero_width_characters_are_removed`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::boundary_collision_is_neutralised`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::oversized_field_is_bounded`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`mimir/agent-runtime/tests/prompt_delimiting.rs::data_handling_rule_is_in_system_block`](../../mimir/agent-runtime/tests/prompt_delimiting.rs)
  - [`bifrost/risk/tests/prompt_delimiting.rs::forged_header_in_release_context_is_data`](../../bifrost/risk/tests/prompt_delimiting.rs)
  - [`clients/inference/tests/prompt_delimiting.rs::forged_header_in_user_message_or_context_id_is_data`](../../clients/inference/tests/prompt_delimiting.rs)

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

**Limit.** The Kafka connector has been tested only against a fake broker. The PostgreSQL connector has no TLS and cannot use the egress broker before PostgreSQL 17. Connector errors carry an error class and hashed references, not hosts or secret names.

- Source:
  - [`forge/worker/src/lib.rs:10`](../../forge/worker/src/lib.rs)
  - [`forge/connector-sdk/src/connector.rs:84`](../../forge/connector-sdk/src/connector.rs)
  - [`forge/connector-sdk/src/error.rs:1`](../../forge/connector-sdk/src/error.rs)
  - [`forge/connector-sdk/src/http.rs:127`](../../forge/connector-sdk/src/http.rs)
  - [`forge/connector-sdk/src/wire.rs:80`](../../forge/connector-sdk/src/wire.rs)
- Tests:
  - [`forge/engine/tests/rc_27.rs::rc_27_egress_violating_connector_is_blocked`](../../forge/engine/tests/rc_27.rs)
  - [`forge/connector-sdk/tests/security_wire.rs::connector_errors_redact_hosts_secret_names_and_server_text`](../../forge/connector-sdk/tests/security_wire.rs)
  - [`forge/connector-sdk/tests/http_bounds.rs::a_chunk_size_that_wraps_usize_is_refused_as_data`](../../forge/connector-sdk/tests/http_bounds.rs)
  - [`forge/connector-sdk/tests/http_bounds.rs::a_chunk_size_beyond_u64_is_malformed`](../../forge/connector-sdk/tests/http_bounds.rs)
  - [`forge/connector-sdk/tests/http_bounds.rs::a_chunk_larger_than_the_remaining_budget_is_refused`](../../forge/connector-sdk/tests/http_bounds.rs)
  - [`forge/connector-sdk/tests/http_bounds.rs::two_legitimate_chunks_reassemble_exactly`](../../forge/connector-sdk/tests/http_bounds.rs)
  - [`forge/connector-sdk/src/wire.rs::scram_refuses_an_iteration_count_above_the_ceiling`](../../forge/connector-sdk/src/wire.rs)
  - [`forge/connector-sdk/src/wire.rs::scram_boundary_counts`](../../forge/connector-sdk/src/wire.rs)
- Service tests:
  - [`forge/worker/tests/sync.rs::the_worker_syncs_versioned_datasets_into_the_enclosure_over_the_mesh`](../../forge/worker/tests/sync.rs)
  - [`deploy/compose/smoke.sh:50`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:105`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:130`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:247`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:20`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:367`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:262`](../../CHANGELOG.md)
  - [`CHANGELOG.md:266`](../../CHANGELOG.md)
  - [`CHANGELOG.md:353`](../../CHANGELOG.md)

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
  - [`CHANGELOG.md:308`](../../CHANGELOG.md)
  - [`docs/launch/pipeline/build-deck.js:371`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:312`](../../CHANGELOG.md)
  - [`CHANGELOG.md:399`](../../CHANGELOG.md)

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

bifrost-fleet records each installation's version, liveness and drift from its channel. An agent checks in over PQ-mTLS, every request is authorised and audited, and fleet state survives a restart. A check-in must name a configured channel and the installation's registered channel (only an operator moves it), report fields are bounded, check-ins are rate limited per identity, and the journal records changes and is compacted.

**Limit.** The agent reports a version and a health flag. It does not install, verify or roll back anything.

- Source:
  - [`bifrost/fleet-server/src/lib.rs:126`](../../bifrost/fleet-server/src/lib.rs)
  - [`bifrost/fleet/src/lib.rs:191`](../../bifrost/fleet/src/lib.rs)
  - [`bifrost/fleet/src/agent.rs:19`](../../bifrost/fleet/src/agent.rs)
  - [`bifrost/fleet/src/durable.rs:231`](../../bifrost/fleet/src/durable.rs)
  - [`bifrost/fleet-server/src/main.rs:75`](../../bifrost/fleet-server/src/main.rs)
  - [`bifrost/fleet-server/src/lib.rs:104`](../../bifrost/fleet-server/src/lib.rs)
- Tests:
  - [`bifrost/fleet/tests/durable_restart.rs::fleet_state_survives_restart`](../../bifrost/fleet/tests/durable_restart.rs)
  - [`bifrost/fleet/tests/security_checkin.rs::oversized_and_control_fields_are_refused_by_the_decoder`](../../bifrost/fleet/tests/security_checkin.rs)
  - [`bifrost/fleet/tests/security_checkin.rs::a_check_in_on_an_unconfigured_channel_is_refused`](../../bifrost/fleet/tests/security_checkin.rs)
  - [`bifrost/fleet/tests/security_checkin.rs::an_installation_keeps_its_registered_channel`](../../bifrost/fleet/tests/security_checkin.rs)
  - [`bifrost/fleet/tests/security_checkin.rs::unchanged_check_ins_do_not_grow_the_journal`](../../bifrost/fleet/tests/security_checkin.rs)
  - [`bifrost/fleet/tests/security_checkin.rs::compaction_bounds_the_journal_and_state_survives`](../../bifrost/fleet/tests/security_checkin.rs)
- Service tests:
  - [`bifrost/fleet-server/tests/serve.rs::agents_check_in_operators_read_the_fleet_and_it_survives_a_restart`](../../bifrost/fleet-server/tests/serve.rs)
  - [`deploy/compose/smoke.sh:31`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:87`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:112`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:405`](../../docs/launch/pipeline/build-deck.js)

### `bifrost-rollout` (reference)

Ring-by-ring rollout with health gates between rings and automatic rollback, without downtime.

**Until it is operable, say:** "Rolling and blue-green upgrade logic with health-gated rollback is implemented and tested as a library (AC-M-7, AC-B-2). Only roll_out_live observes availability, from live install counts; the roll_out model reports the fleet size by construction and is not evidence of zero downtime. Deployed services run one replica each and restart on their recovered state (RC-40.1); upgrading one interrupts it."

**Milestone.** RF-CA-9, after the replica-model decision: the fleet manager drives a rollout against real agents (RF-B-2, RF-B-3).

- Criteria: AC-M-7, AC-B-2
- Source:
  - [`bifrost/rollout/src/orchestrator.rs:160:fn roll_out_live`](../../bifrost/rollout/src/orchestrator.rs)
- Tests:
  - [`services/scheduler/tests/rollout_real.rs::rolling_upgrade_serves_every_request_with_real_replicas`](../../services/scheduler/tests/rollout_real.rs)
- Stated at:
  - [`docs/releases/v0.1.0-rc.1.md:119`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:545`](../../docs/launch/pipeline/build-deck.js)

### `air-gap-bundle` (operable)

mg-bundle packs gate-verified artefacts into one signed bundle and imports it offline, verifying every artefact, a chunk at a time so that model weights are never held in memory. A refused import leaves nothing behind.

**Limit.** A release's bundle carries its binaries, images, chart, signed values and installer, and the external images, and install.sh installs from it alone; that is tested with stub tools only. No install from a real bundle into a real registry or cluster has been run, and an import returns no signed receipt.

- Criteria: AC-B-3, AC-MS-3
- Source:
  - [`bifrost/bundle/src/stream.rs:151`](../../bifrost/bundle/src/stream.rs)
  - [`deploy/air-gap-bundle/install.sh:110`](../../deploy/air-gap-bundle/install.sh)
  - [`deploy/images/externals.lock`](../../deploy/images/externals.lock)
  - [`tools/ci/tests/air-gap-bundle.test.sh`](../../tools/ci/tests/air-gap-bundle.test.sh)
  - [`tools/ci/tests/release-bundle.test.sh`](../../tools/ci/tests/release-bundle.test.sh)
- Tests:
  - [`bifrost/bundle/tests/streaming.rs::damage_is_located_and_a_forged_chunk_is_still_refused`](../../bifrost/bundle/tests/streaming.rs)
  - [`bifrost/bundle/tests/security_counts.rs::a_hostile_entry_count_is_refused_not_allocated`](../../bifrost/bundle/tests/security_counts.rs)
  - [`bifrost/bundle/tests/security_counts.rs::a_hostile_artefact_count_is_refused`](../../bifrost/bundle/tests/security_counts.rs)
- Service tests:
  - [`tools/mg-bundle/tests/cli.rs::a_bundle_from_a_sealed_keystore_imports_offline_and_refuses_everything_else`](../../tools/mg-bundle/tests/cli.rs)
  - [`tools/mg-bundle/tests/cli.rs::image_archives_and_deploy_files_pack_and_import_byte_for_byte`](../../tools/mg-bundle/tests/cli.rs)
- Stated at:
  - [`CHANGELOG.md:13`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:25`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/compliance/GA_READINESS.md:77`](../../docs/compliance/GA_READINESS.md)

## Heimdall

### `heimdall-feed-and-picture` (operable)

heimdall-feed accepts an observation only over PQ-mTLS, only from the feed whose certified identity it carries (or one the feed registry delegates to it), and only with a valid signature over bytes that include the feed's monotonic sequence. Once the signature verifies, an observation is accepted only if its sequence is above the feed's last, its time is within a bounded window around the server's receipt time and its id is not among the feed's last 4,096; each feed's last sequence is sealed at rest, so a replay is refused across requests and restarts, and the picture exposes per-entity staleness. Observations that share an entity key fuse into one tracked entity, and the picture is sealed at rest and recovered at restart. The picture is bounded (entity, per-feed and attribute caps with stale-track eviction), persisted as a sealed per-entity journal with compaction, read in pages that never exceed the mesh frame cap, and Ingest is rate limited per identity.

**Limit.** Observations carry no marking, and a timestamp inside the accepted window is still the feed's own assertion. A feed that restarts its sequence below its last is refused until it continues above it or is re-registered.

- Source:
  - [`heimdall/feed-server/src/lib.rs:312`](../../heimdall/feed-server/src/lib.rs)
  - [`heimdall/store/src/picture_store.rs:115`](../../heimdall/store/src/picture_store.rs)
  - [`heimdall/feed-server/src/lib.rs:725`](../../heimdall/feed-server/src/lib.rs)
  - [`heimdall/feed-server/src/lib.rs:299`](../../heimdall/feed-server/src/lib.rs)
  - [`heimdall/feed-server/src/lib.rs:359`](../../heimdall/feed-server/src/lib.rs)
  - [`heimdall/feed-gateway/src/authenticator.rs:138`](../../heimdall/feed-gateway/src/authenticator.rs)
  - [`heimdall/feed-gateway/src/replay.rs:105`](../../heimdall/feed-gateway/src/replay.rs)
  - [`heimdall/feed-server/src/lib.rs:169`](../../heimdall/feed-server/src/lib.rs)
- Tests:
  - [`heimdall/picture-sync/tests/rc_26.rs::rc_26_unsigned_feed_is_rejected_at_the_boundary`](../../heimdall/picture-sync/tests/rc_26.rs)
  - [`heimdall/feed-server/tests/limits.rs::a_flood_of_distinct_keys_ten_times_the_cap_keeps_the_picture_bounded`](../../heimdall/feed-server/tests/limits.rs)
  - [`heimdall/feed-server/tests/limits.rs::read_picture_pages_never_exceed_the_frame_cap`](../../heimdall/feed-server/tests/limits.rs)
  - [`heimdall/feed-server/tests/limits.rs::oversized_attributes_are_malformed`](../../heimdall/feed-server/tests/limits.rs)
  - [`heimdall/feed-server/tests/limits.rs::hidden_layers_are_filtered_unless_asked`](../../heimdall/feed-server/tests/limits.rs)
  - [`heimdall/store/tests/durable_restart.rs::each_upsert_appends_one_frame_and_the_file_grows_by_one_entity_not_the_picture`](../../heimdall/store/tests/durable_restart.rs)
  - [`heimdall/store/tests/durable_restart.rs::compaction_shrinks_the_journal_and_the_picture_survives_restart`](../../heimdall/store/tests/durable_restart.rs)
  - [`heimdall/store/tests/durable_restart.rs::a_removed_entity_stays_removed_after_restart`](../../heimdall/store/tests/durable_restart.rs)
  - [`heimdall/store/tests/durable_restart.rs::a_picture_sealed_whole_by_an_earlier_build_opens_and_compacts_back_to_that_format`](../../heimdall/store/tests/durable_restart.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::an_observation_ten_years_ahead_is_refused_and_audited`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::an_observation_older_than_the_retention_window_is_refused`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::a_near_future_observation_does_not_freeze_the_track`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::a_replayed_observation_id_is_refused_once_per_feed_ring`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::read_picture_exposes_staleness`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-server/tests/ingest_window.rs::replay_protection_survives_a_restart`](../../heimdall/feed-server/tests/ingest_window.rs)
  - [`heimdall/feed-gateway/tests/security_replay.rs::an_observation_replayed_with_the_same_signature_is_refused`](../../heimdall/feed-gateway/tests/security_replay.rs)
  - [`heimdall/feed-gateway/tests/security_replay.rs::a_signature_failure_does_not_consume_state`](../../heimdall/feed-gateway/tests/security_replay.rs)
  - [`heimdall/feed-gateway/tests/security_replay.rs::a_feed_cannot_submit_another_feeds_observations`](../../heimdall/feed-gateway/tests/security_replay.rs)
- Service tests:
  - [`heimdall/feed-server/tests/serve.rs::sources_feed_the_picture_analysts_read_it_and_it_survives_a_restart`](../../heimdall/feed-server/tests/serve.rs)
  - [`heimdall/feed-server/tests/serve.rs::a_source_bursting_past_its_rate_limit_is_throttled_and_the_other_is_served`](../../heimdall/feed-server/tests/serve.rs)
  - [`deploy/compose/smoke.sh:42`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:95`](../../deploy/kind/smoke.sh)
  - [`heimdall/feed-server/tests/serve.rs::replay_state_persists_across_requests`](../../heimdall/feed-server/tests/serve.rs)
  - [`deploy/kind/smoke.sh:120`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`docs/launch/pipeline/build-deck.js:391`](../../docs/launch/pipeline/build-deck.js)
  - [`docs/launch/pipeline/build-deck.js:393`](../../docs/launch/pipeline/build-deck.js)

### `heimdall-picture-replication` (reference)

Replication of the common picture between nodes over PQ-mTLS.

**Until it is operable, say:** "Replication of the picture between two nodes over PQ-mTLS is implemented and tested as a library, and the merge converges when two nodes hold different state with the same timestamp. The running feed service does not replicate."

**Milestone.** The feed service replicates (RF-H-3).

- Criteria: AC-H-5
- Source:
  - [`heimdall/picture-sync/src/replicator.rs:14`](../../heimdall/picture-sync/src/replicator.rs)
- Tests:
  - [`heimdall/picture-sync/tests/rc_26.rs::rc_26_feed_over_pqmtls_converges_across_two_nodes`](../../heimdall/picture-sync/tests/rc_26.rs)

### `heimdall-decision-support` (reference)

Decision support: ranked options, each cited and grounded against the picture, and workflows in which a person authorises a consequential action and the decision is audited.

**Until it is operable, say:** "Advisory option generation (ranked options, each cited and grounded against the operating picture) is implemented and tested as a library (AC-H-3, EV-A8): ungrounded options are rejected, and only a grounded option can become a proposed action. In the workflow library an action's class comes from the deployment's catalogue, and no action runs without a recorded human decision. The running feed gateway fuses and serves the picture; it does not yet generate options or host the workflow."

**Milestone.** RF-CA-10: a running service generates options and hosts the workflow.

- Criteria: AC-H-3, AC-H-4
- Source:
  - [`heimdall/decision-support/src/adapter.rs:62`](../../heimdall/decision-support/src/adapter.rs)
  - [`heimdall/workflow/src/engine.rs:131`](../../heimdall/workflow/src/engine.rs)
  - [`heimdall/decision-support/src/prompt.rs:42`](../../heimdall/decision-support/src/prompt.rs)
  - [`heimdall/decision-support/src/adapter.rs:69`](../../heimdall/decision-support/src/adapter.rs)
  - [`heimdall/decision-support/src/options.rs:140`](../../heimdall/decision-support/src/options.rs)
  - [`heimdall/workflow/src/engine.rs:182`](../../heimdall/workflow/src/engine.rs)
  - [`heimdall/workflow/src/catalog.rs:89`](../../heimdall/workflow/src/catalog.rs)
- Tests:
  - [`heimdall/decision-support/tests/ac_h_3.rs::ac_h_3_options_are_ranked_and_cite_evidence`](../../heimdall/decision-support/tests/ac_h_3.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::ac_h_4_consequential_action_needs_human_authorisation_and_is_audited`](../../heimdall/workflow/tests/ac_h_4.rs)
  - [`heimdall/decision-support/tests/prompt_delimiting.rs::injected_entity_field_stays_inside_the_picture_block`](../../heimdall/decision-support/tests/prompt_delimiting.rs)
  - [`heimdall/decision-support/tests/ac_h_3.rs::ev_a8_grounded_catches_invented_evidence`](../../heimdall/decision-support/tests/ac_h_3.rs)
  - [`heimdall/decision-support/tests/ac_h_3.rs::misranked_options_fail_parse`](../../heimdall/decision-support/tests/ac_h_3.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::informational_action_still_needs_a_recorded_human_decision`](../../heimdall/workflow/tests/ac_h_4.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::unknown_action_type_is_refused_at_proposal`](../../heimdall/workflow/tests/ac_h_4.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::proposer_cannot_assert_the_class`](../../heimdall/workflow/tests/ac_h_4.rs)
  - [`heimdall/workflow/tests/ac_h_4.rs::ac_h_3_and_4_option_to_audited_action_lineage`](../../heimdall/workflow/tests/ac_h_4.rs)

## Clients

### `client-core` (reference)

One shared client core in Rust: sealed local edits, clearance checks, and conflict-free sync that converges after a disconnection.

**Limit.** The software keystore reports the NonExtractable floor only; higher floors require a platform keystore.

**Until it is operable, say:** "A shared client core with sealed edits, conflict-free sync and tiered inference is implemented and tested as a library. It keeps its state in memory, and no application is built on it."

**Milestone.** Client-side persistence and language bindings (RF-PER-5, RF-CL-1).

- Criteria: AC-C-6, AC-C-10
- Source:
  - [`clients/core/src/lib.rs:53`](../../clients/core/src/lib.rs)
- Tests:
  - [`clients/core/tests/one_core.rs::two_cores_converge_on_reconnect`](../../clients/core/tests/one_core.rs)
  - [`clients/core/tests/one_core.rs::the_core_is_generic_over_the_keystore`](../../clients/core/tests/one_core.rs)
  - [`clients/store/tests/at_rest_and_clearance.rs::the_software_keystore_cannot_claim_a_hardware_floor`](../../clients/store/tests/at_rest_and_clearance.rs)
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

Each release artefact passes one gate, in order, and each stage fails closed: built twice and proven bit-identical, described by a CycloneDX SBOM with component hashes, scanned offline for advisories and licences, signed with ML-DSA with a provenance attestation, then verified. The tooling the release jobs run is pinned too: the shell-audit server comes from a lockfile with registry integrity hashes, not fetched by name, and no checkout keeps the workflow token. A release tag is verified independently of the commit it tags: against the signing runner's own copy of allowed_signers, which the committed copy must equal, on a commit that must be on main, by scripts from a separate checkout of main; tag creation is restricted to a release-managers team by a ruleset applied as code. History is signed and reviewed: the commits job, in both pipelines, refuses a change set holding a commit that is neither signed by a key the base commit lists in allowed_signers nor verified by GitHub, nor a forge merge adding nothing of its own; the signed history ruleset on main and dev (required signatures, no force push or deletion, a pull request with code-owner review, no bypass) is applied as code and its live state exported for review; both maintainers own the trust set and the pipelines.

**Limit.** The gate has run with throwaway keys only (a rehearsal on 2026-09-15). No key ceremony has been held, release keys have no revocation, and the release workflow has not run on a signing runner. Every CI job image is pinned by digest on both forges (GitLab images by digest, GitHub actions by commit), but the GitLab images are still pulled from Docker Hub until the owner mirrors them. The advisory scan refuses a snapshot more than 45 days old; the weekly advisory-refresh job and Renovate are configured on both forges but have not yet run on either. The tag-creation ruleset, the runner's copy of allowed_signers and an ephemeral runner are owner actions not yet confirmed by exported evidence. No maintainer commit key is in allowed_signers yet, so the commits job passes only where GitHub verifies a commit and fails on CodeLab; that the signed history ruleset is live will be shown by docs/compliance/evidence/rulesets-<date>.json, which no maintainer has exported and committed yet.

- Criteria: AC-M-6, AC-B-1
- Source:
  - [`justfile:373`](../../justfile)
  - [`tools/ci/release.sh:178`](../../tools/ci/release.sh)
  - [`tools/ci/pipelines.sh:56`](../../tools/ci/pipelines.sh)
  - [`tools/ci/release.sh:190`](../../tools/ci/release.sh)
  - [`justfile:389`](../../justfile)
  - [`tools/ci/tests/js-toolchain.test.sh`](../../tools/ci/tests/js-toolchain.test.sh)
  - [`vendor/advisory-db/snapshot.toml:27`](../../vendor/advisory-db/snapshot.toml)
  - [`vendor/advisory-db-npm/snapshot.toml:15`](../../vendor/advisory-db-npm/snapshot.toml)
  - [`renovate.json`](../../renovate.json)
  - [`.github/workflows/advisory-refresh.yml`](../../.github/workflows/advisory-refresh.yml)
  - [`tools/ci/tests/advisory-refresh.test.sh`](../../tools/ci/tests/advisory-refresh.test.sh)
  - [`tools/ci/shell-audit/package-lock.json`](../../tools/ci/shell-audit/package-lock.json)
  - [`tools/ci/run.sh:374`](../../tools/ci/run.sh)
  - [`tools/ci/tests/npx-pin.test.sh`](../../tools/ci/tests/npx-pin.test.sh)
  - [`tools/ci/tests/workflows.test.sh`](../../tools/ci/tests/workflows.test.sh)
  - [`tools/ci/verify-tag.sh`](../../tools/ci/verify-tag.sh)
  - [`tools/ci/tests/verify-tag.test.sh`](../../tools/ci/tests/verify-tag.test.sh)
  - [`tools/ci/github-rules.sh`](../../tools/ci/github-rules.sh)
  - [`tools/ci/tests/github-rules.test.sh`](../../tools/ci/tests/github-rules.test.sh)
  - [`tools/ci/verify-commits.sh`](../../tools/ci/verify-commits.sh)
  - [`tools/ci/tests/verify-commits.test.sh`](../../tools/ci/tests/verify-commits.test.sh)
  - [`.github/CODEOWNERS`](../../.github/CODEOWNERS)
- Tests:
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::an_artefact_that_was_never_signed_is_refused`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-scan/tests/cli.rs::ac_rf_sc_4a_a_critical_or_high_advisory_blocks_and_names_the_component`](../../tools/mg-scan/tests/cli.rs)
  - [`tools/mg-scan/tests/cli.rs::snapshot_older_than_fail_after_days_fails`](../../tools/mg-scan/tests/cli.rs)
- Service tests:
  - [`tools/ci/run.sh:321`](../../tools/ci/run.sh)
  - [`tools/ci/tests/pipelines.test.sh:73`](../../tools/ci/tests/pipelines.test.sh)
  - [`tools/ci/run.sh:226`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:233`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:345`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:370`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:231`](../../tools/ci/run.sh)
- Stated at:
  - [`README.md:742`](../../README.md)
  - [`docs/compliance/GA_READINESS.md:66`](../../docs/compliance/GA_READINESS.md)
  - [`docs/launch/pipeline/build-deck.js:402`](../../docs/launch/pipeline/build-deck.js)
  - [`README.md:746`](../../README.md)
  - [`README.md:754`](../../README.md)
  - [`README.md:744`](../../README.md)

### `offline-build` (operable)

The workspace builds and its tests run with no network: every dependency is vendored, Cargo is pinned to the vendored directory, and the CI jobs run in a container started with no network, except GitLab merge-request jobs, which run unprivileged inside the digest-pinned toolchain image with Cargo forced offline.

**Limit.** The recorded gate run covers the one commit named in docs/compliance/gate-runs/CURRENT. Later commits are not covered until the gate is recorded again. On GitLab merge requests the offline property rests on Cargo (vendored sources, CARGO_NET_OFFLINE) rather than a network sandbox, and those runs cannot be recorded as the gate; the unprivileged and protected runner configuration is the owner's and is not yet evidenced.

- Criteria: AC-M-5
- Source:
  - [`.cargo/config.toml:18`](../../.cargo/config.toml)
  - [`tools/ci/in-toolchain.sh:12`](../../tools/ci/in-toolchain.sh)
  - [`tools/ci/in-toolchain.sh:60`](../../tools/ci/in-toolchain.sh)
  - [`tools/ci/vendor-check.sh`](../../tools/ci/vendor-check.sh)
  - [`.gitlab-ci.yml:117`](../../.gitlab-ci.yml)
- Service tests:
  - [`tools/ci/run.sh:307`](../../tools/ci/run.sh)
  - [`tools/ci/tests/in-toolchain.test.sh:43`](../../tools/ci/tests/in-toolchain.test.sh)
  - [`tools/ci/run.sh:216`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:222`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:210`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:214`](../../tools/ci/run.sh)
- Stated at:
  - [`README.md:130`](../../README.md)
  - [`docs/launch/pipeline/build-deck.js:508`](../../docs/launch/pipeline/build-deck.js)

### `verify-before-run` (operable)

Each service image re-verifies the ML-DSA signature, SBOM and provenance of its binary against the pinned release trust key before starting it, and refuses to start when verification fails.

**Limit.** Exercised in the release rehearsal of 2026-09-15 with throwaway drill keys only. No release image has been built, and the compose and kind images are built from source and start their binaries directly. The release chart binds each pod to the signed image by digest (deploy/helm/set-digests.sh), so a tag moved in the registry cannot substitute an image that skips the check. The images carry the pinned trust set read-only at /etc/midgard/trust, where mimir-server reads the model-signing anchor, and the release preflight refuses a release without that anchor; the key ceremony has not yet committed it.

- Criteria: AC-B-1
- Source:
  - [`tools/mg-verify/src/main.rs:292`](../../tools/mg-verify/src/main.rs)
  - [`deploy/images/service.Dockerfile`](../../deploy/images/service.Dockerfile)
  - [`deploy/images/service.Dockerfile:50`](../../deploy/images/service.Dockerfile)
  - [`tools/ci/release-preflight.sh:124`](../../tools/ci/release-preflight.sh)
- Tests:
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_refuses_an_unverified_binary_and_does_not_exec`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_execs_a_verified_binary_with_the_given_arguments`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_execs_the_bytes_it_verified_not_the_path`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_without_a_service_name_is_a_usage_error`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_with_an_artefact_path_is_a_usage_error`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_with_no_service_options_refuses_to_start`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
  - [`tools/mg-verify/tests/ac_rf_sc_2.rs::run_service_trust_anchor_comes_only_from_its_own_command_line`](../../tools/mg-verify/tests/ac_rf_sc_2.rs)
- Service tests:
  - [`deploy/images/build.sh:103`](../../deploy/images/build.sh)
- Stated at:
  - [`README.md:760`](../../README.md)
  - [`docs/releases/v0.1.0-rc.1.md:24`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/compliance/GA_READINESS.md:114`](../../docs/compliance/GA_READINESS.md)
  - [`CHANGELOG.md:263`](../../CHANGELOG.md)
  - [`README.md:756`](../../README.md)
  - [`CHANGELOG.md:278`](../../CHANGELOG.md)
  - [`README.md:768`](../../README.md)
  - [`README.md:758`](../../README.md)
  - [`CHANGELOG.md:282`](../../CHANGELOG.md)
  - [`CHANGELOG.md:369`](../../CHANGELOG.md)

### `acceptance-gate` (operable)

Each of the 72 acceptance criteria maps to a test, and one script runs them all in a container with no network. A release is gated only on a tag whose commit is on main, checked by the release pipeline itself, not only on the operator's side.

**Limit.** Recorded at the commit named in docs/compliance/gate-runs/CURRENT: every criterion passed, in a CI run under --network none, not on release infrastructure. Later commits are not covered until the gate is recorded again. On GitLab the suite runs only on protected branches, on the privileged runner tagged protected; merge-request jobs run unprivileged and never start it.

- Source:
  - [`deploy/acceptance/run-airgapped.sh:10`](../../deploy/acceptance/run-airgapped.sh)
  - [`.gitlab-ci.yml:245`](../../.gitlab-ci.yml)
  - [`tools/ci/verify-tag.sh`](../../tools/ci/verify-tag.sh)
  - [`tools/ci/tests/verify-tag.test.sh`](../../tools/ci/tests/verify-tag.test.sh)
- Service tests:
  - [`tools/ci/run.sh:314`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:216`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:222`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:210`](../../tools/ci/run.sh)
  - [`tools/ci/run.sh:214`](../../tools/ci/run.sh)
- Stated at:
  - [`GETTING_STARTED.md:5`](../../GETTING_STARTED.md)

## Deployment and operations

### `deployment` (operable)

Five services (http-gateway, mimir-server, bifrost-fleet, heimdall-feed, forge-worker) deploy with Docker compose, or with a Helm chart that runs one replica of each as a hardened StatefulSet behind default-deny network policies.

**Limit.** The chart has run on a three-node kind cluster, not on a production cluster. The committed development rendering uses tags; release renderings are digest pinned and signed.

- Criteria: RC-40.1
- Source:
  - [`deploy/compose/compose.yaml`](../../deploy/compose/compose.yaml)
  - [`deploy/kind/values.yaml:4`](../../deploy/kind/values.yaml)
  - [`deploy/helm/midgard/templates/workload.yaml:18`](../../deploy/helm/midgard/templates/workload.yaml)
- Service tests:
  - [`deploy/compose/smoke.sh:13`](../../deploy/compose/smoke.sh)
  - [`deploy/kind/smoke.sh:73`](../../deploy/kind/smoke.sh)
  - [`deploy/kind/smoke.sh:98`](../../deploy/kind/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:243`](../../CHANGELOG.md)
  - [`docs/releases/v0.1.0-rc.1.md:18`](../../docs/releases/v0.1.0-rc.1.md)
  - [`docs/launch/pipeline/build-deck.js:323`](../../docs/launch/pipeline/build-deck.js)
  - [`CHANGELOG.md:258`](../../CHANGELOG.md)
  - [`CHANGELOG.md:262`](../../CHANGELOG.md)
  - [`CHANGELOG.md:349`](../../CHANGELOG.md)

### `high-availability` (spec)

Highly available clusters, replication between sites, zero-downtime operation and a 99.9% control-plane availability objective.

**Until it is operable, say:** "Each service runs as one replica holding a single-writer state directory. Availability comes from restart and recovery, not from replicas. High-availability and multi-site topologies are specified, not implemented, and no availability figure has been measured."

**Milestone.** The replica-model decision, then RF-DEP-2 and RF-RES-3.

- Source:
  - [`docs/ops/DR_PLAN.md:16`](../../docs/ops/DR_PLAN.md)
- Stated at:
  - [`docs/releases/v0.1.0-rc.1.md:119`](../../docs/releases/v0.1.0-rc.1.md)

### `telemetry` (operable)

Each service exposes liveness, readiness and Prometheus metrics, and writes structured logs; a lock poisoned by a panic is recovered rather than failing later requests, and each recovery is counted and logged, with no dependency that could send telemetry out of the enclosure. Log and metric output escapes every control character.

**Limit.** The tracing module is unused, the observability console shows fixed figures, and no service-level objective has been measured in a deployment.

- Source:
  - [`services/telemetry/src/lib.rs:1`](../../services/telemetry/src/lib.rs)
  - [`services/telemetry/src/escape.rs:1`](../../services/telemetry/src/escape.rs)
  - [`services/runtime/src/sync.rs:1`](../../services/runtime/src/sync.rs)
- Tests:
  - [`services/telemetry/tests/telemetry.rs::metrics_render_prometheus_text`](../../services/telemetry/tests/telemetry.rs)
  - [`services/http-gateway/tests/rc_33.rs::service_exposes_liveness_readiness_metrics_and_an_observable_slo_breach`](../../services/http-gateway/tests/rc_33.rs)
  - [`services/http-gateway/tests/security_record.rs::every_request_writes_one_json_log_line_with_source_and_request_id`](../../services/http-gateway/tests/security_record.rs)
  - [`services/telemetry/tests/telemetry.rs::log_lines_escape_every_control_character`](../../services/telemetry/tests/telemetry.rs)
  - [`services/telemetry/tests/telemetry.rs::metric_labels_never_contain_raw_controls`](../../services/telemetry/tests/telemetry.rs)
  - [`services/telemetry/tests/telemetry.rs::a_poisoned_metrics_lock_recovers_and_reports`](../../services/telemetry/tests/telemetry.rs)
  - [`services/http-gateway/tests/rc_33.rs::a_poisoned_slo_lock_does_not_drop_responses`](../../services/http-gateway/tests/rc_33.rs)
  - [`services/http-gateway/tests/rc_33.rs::a_poisoned_enclosure_lock_is_recovered_logged_and_audited`](../../services/http-gateway/tests/rc_33.rs)
- Service tests:
  - [`deploy/compose/smoke.sh:15`](../../deploy/compose/smoke.sh)
- Stated at:
  - [`CHANGELOG.md:320`](../../CHANGELOG.md)
  - [`CHANGELOG.md:324`](../../CHANGELOG.md)
  - [`CHANGELOG.md:411`](../../CHANGELOG.md)

### `compliance-mapping` (spec)

Alignment with NIST SP 800-53, OWASP ASVS and ISO 27001.

**Until it is operable, say:** "A mapping of platform controls to NIST SP 800-53, OWASP ASVS and ISO 27001 is written (docs/compliance/COMPLIANCE_MATRIX.md). No control has automated evidence and no assessment has been carried out."

**Milestone.** Control mappings generate evidence from CI (RF-CMP-1).

- Source:
  - [`docs/compliance/COMPLIANCE_MATRIX.md:1`](../../docs/compliance/COMPLIANCE_MATRIX.md)

### `personal-data-erasure` (spec)

Personal data can be erased and exported per subject, and each durable store enforces a retention period: the directory, object and dataset journals seal each record under its own data key, erasure destroys the key and leaves a tombstone, and compaction drops shredded records.

**Limit.** The audit ledger is immutable by design and is not subject to erasure.

**Until it is operable, say:** "Every durable store is an append-only sealed journal: access control, encryption at rest and in transit and an immutable audit trail are provided, but no record is deleted, compacted or exported, and a replaced record (such as an earlier password hash) stays in the journal. Retention targets are documented in docs/compliance/RETENTION.md and are not enforced; subject erasure and export are not implemented."

**Milestone.** The follow-on to ANUBIS WP-29: per-record data keys with crypto-shred tombstones for the directory, object and dataset journals, then erasure, export and retention through mg-admin (VLD-ANB-MIDGARD-0070).

- Source:
  - [`docs/compliance/RETENTION.md:1`](../../docs/compliance/RETENTION.md)
  - [`services/iam/src/directory.rs:67`](../../services/iam/src/directory.rs)
  - [`docs/compliance/DATA_RESIDENCY.md:57:Act as data controller`](../../docs/compliance/DATA_RESIDENCY.md)

## Outside this repository

### `likan` (external)

Likan is a separate, proprietary tool for data-structure ontologies. It is not part of the open-source Midgard repository and is not covered by its licence, its acceptance gate, its SBOMs or its published figures. Midgard loads ontologies from a documented file format (see ontology-file) and needs no other tool to do so. Likan is in active development and is intended to export that format. No file produced by Likan has been validated against it in this repository yet, and no pipeline between the two has been run here.

- Stated at:
  - [`README.md:837`](../../README.md)
  - [`README.md:845`](../../README.md)
  - [`README.md:835`](../../README.md)

