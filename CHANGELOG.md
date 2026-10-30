# Changelog

All notable changes to Midgard are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/) from 1.0.

Midgard is pre-1.0; the public API and wire formats may change until 1.0. See
CONTRIBUTING.md for the version policy.

## [Unreleased]

### Added
- **Shamir secret sharing in the crypto core.** `pqc_agility::shamir` splits a secret
  k of n over GF(2^8) in constant time and zeroises its intermediates. Dvalin
  uses it, when key fragmentation is chosen, to split the escrow key that
  seals the release keystores among named holders. It is new and marked for
  independent review before production reliance. [RT-35] [NFR-PQ-2]
- `docs/ops/KEY_CEREMONY.md`: where the key ceremony is held, and what it leaves in
  this repository.

### Changed
- **The key ceremony is held with Dvalin**, a separate application
  (veldris-ltd/dvalin-app): a guided Windows and Linux app, with a console
  form, whose key generation, signing and verification are this repository's
  `attest`, pinned by revision. `deploy/trust/ceremony.sh` is removed; its claims are
  that repository's tests, and its contract test runs this repository's release
  checks against what the app publishes. [RT-35] [AC-RF-CA-6a]

### Security
- **The audit ledger can no longer be quietly shortened or deleted, or checked with a
  key of the attacker's choosing.** [MG-SEC-05]
  - Every append refreshes a signed checkpoint of the ledger's length and last entry
    (`<ledger>.checkpoint`, replaced atomically). A ledger shorter than its checkpoint,
    or whose last entry is not the checkpointed one, fails closed.
  - `FileLedger::open` refuses a missing ledger or checkpoint; a new ledger is made by
    `FileLedger::create`, and `mg-admin init` now creates the governance ledger with a
    signed empty checkpoint. Stores that make their own ledger on first start use
    `FileLedger::open_or_create`.
  - The gateway checks the ledger under a pinned audit key, `--audit-pin FILE`
    (default: `trust/audit.pub` in the state directory, with a warning), and refuses
    an audit keystore that does not hold it. `audit_writer::verify_ledger` is the
    check.
  - Upgrading: a ledger written before this change has no checkpoint and does not
    open. Run `mg-admin audit checkpoint --state-dir DIR --audit-pin FILE` once: it
    verifies the whole ledger under the pinned key and records its first checkpoint.
- **Break-glass needs a real second person.** The approver was free text checked only
  for being non-empty. A grant now takes the requester's and the approver's
  `AdminSession`s, which exist only after each signed in with their own passkey: the
  requester must be the principal being elevated, the approver a different
  administrator who re-authenticated within the approval window, and the grant and the
  audit ledger record both identities (`breakglass-approve`, then `breakglass-grant`).
  `GrantRequest` no longer has an `approver` field. Signed in is not enough: a grant
  also takes the `IdentityDirectory`, and the requester must hold `breakglass.request`
  and the approver `breakglass.approve` (the security officer's) through role bindings
  in force at the time and covering the grant's scope, or it is refused
  (`RequesterNotAuthorised`, `ApproverNotAuthorised`) with nothing alerted, issued or
  sealed. [MG-SEC-07]
- **Services check who they connected to.** [MG-SEC-01] Every mesh client names the
  service it means to call, and refuses any other before sending its key exchange.
- **Mesh certificates expire.** [MG-SEC-02] Certificates hold a serial and a validity
  window, checked at every handshake (format `MGCERT2`; v1 certificates and anchors
  are refused). `mg-admin mesh issue` takes `--days` (default 30, at most 365) and
  `mg-admin mesh renew` replaces a certificate that is due.
- **Revocation works in practice.** [MG-SEC-03] Revocation is a list signed by the
  mesh CA (`mesh.crl`): every service requires it at start, re-reads it every 30
  seconds and refuses an older or forged one. `mg-admin mesh revoke` signs the next
  version and writes it to each service directory named. A resumed session is
  refused if either side's certificate was revoked or has expired since, and a
  ticket never outlives the certificate.
- **Clearance comes from the certificate.** [MG-SEC-04] A certificate carries the
  clearance it grants (`mg-admin mesh issue --clearance`); the server authorizes the
  lower of that and what the client asks for, and keeps it on the session.
- **Keys are wiped from memory.** [MG-SEC-08] The X25519 and AES libraries are built
  with their zeroize features, intermediate secrets are wiped, the ML-KEM secret-key
  buffer is allocated at its final size, and a failure to lock memory is logged
  (`pqc_agility::mlock_failures`).
- **Standard hybrid key combination.** [MG-SEC-09] The hybrid secret uses the X-Wing
  combiner (SHA3-256 over both secrets, the X25519 ciphertext and public key), and
  an all-zero X25519 result is refused. This is protocol version 2.
- **Hybrid required, rekeying and an end-of-stream marker.** [MG-SEC-12] Both sides
  require a hybrid suite by default; the record layer rekeys after 2^24 records or
  2^34 bytes; a close marker ends a stream, so a cut connection is reported as
  truncated rather than finished.
- **Server hardening.** [MG-SEC-13] The mesh gateway and services accept at most 512
  connections at once, time out a handshake after 10 seconds and an idle stream
  after 5 minutes, and cap a frame at 64 KiB during the handshake and 1 MiB after,
  before reserving memory.
- **The gateway is safe by default.** [MG-SEC-06] Started with no arguments it
  refuses to start; it needs `--state-dir`. Development mode, with its published demo
  logins, is behind an explicit `--dev` flag and listens on loopback only; a bare
  address no longer starts it.
- **Vetted crypto libraries.** [MG-SEC-10] The hand-written HMAC, HKDF, PBKDF2 and
  constant-time comparison are replaced by RustCrypto's `hmac`, `hkdf`, `pbkdf2` and
  `subtle`, behind the same functions, and checked against the RFC 4231, 5869, 6070
  and 7914 vectors.
- **Stronger password storage.** [MG-SEC-11] New passwords are Argon2id. A stored hash
  outside fixed cost bounds is refused like a wrong password, so an edited entry
  cannot weaken itself; legacy PBKDF2 entries (210,000 iterations) still sign in and
  are replaced with Argon2id at that sign-in, recorded in the audit ledger.
- **Tighter checks before loading signed software.** [MG-SEC-14] Model manifests and
  bundles are checked against their signature before they are parsed; an older
  release, or the same version with different content, is refused across restarts;
  model weights are hashed through the handle that is then used; and the image no
  longer reads `MIDGARD_RELEASE_TRUST_KEY`.
- **Tests that prove the claims.** [MG-SEC-15] CI fails a build whose dependency graph
  enables the insecure test backend (`tools/ci/no-test-backend.sh`). The NIST ACVP
  ML-KEM and ML-DSA vectors run offline; the ledger tamper and truncation tests and the
  handshake downgrade tests also run on the FIPS backend; the whole crypto suites run
  there, not chosen files; and seven crates no recipe tested now are.

### Fixed
- `.gitignore` ignored every `*.pub`, including the two trust anchors the ceremony
  publishes to be committed, so committing them failed. The anchors are now excepted;
  other `.pub` files stay ignored, and `tools/ci/tests/ceremony.test.sh` holds both.

## [0.1.0-rc.1] — release candidate, not yet tagged

The first release candidate: from verified libraries to a platform that can be
deployed and used. **A release candidate is not a supported release.** It exercises
the whole release pipeline. GA remains blocked on:

- the deployment tier passing on real infrastructure: HSM signing and GPU inference
  have not run;
- FIPS 140-3 module certification;
- shell audits on real devices.

See `docs/compliance/GA_READINESS.md`.

### Added
- **Mistral model family.** `MistralAdapter` serves Mistral AI's Apache-2.0 open-weight
  models. The adapter set, the router policy, the model loader and the eval matrix
  cover four families: Mistral, Nemotron, Gemma and Granite. [FR-MS-7]
- **Sample datasets.** `samples/` holds real data recorded from live public sources on
  22 October 2026: the configuration it was recorded with, Forge datasets, Heimdall feed
  snapshots and Heimdall observations. Only files whose sources state an open licence
  that allows redistribution are included; `catalogue.json` and `NOTICE.md` give every
  file's source, licence and required credit, and which ones are only in the full
  `midgard-sample-datasets.zip`. [FR-F-1] [FR-H-3]
  - `samples/convert.py` turns them into an import file for `mg-admin import`, CSV files
    and jobs for the Forge worker, and observation lines for `heimdall-source`, and
    writes a build-compatible ontology, naming what it leaves out (`retention`, and one
    action on a boolean property).
  - Ingested by both routes on this build: 418 objects and 28 links by `mg-admin
    import`, and every job through Forge in the installer runs.
- **Sample data and Heimdall feeds in the installer.** [RC-33]
  - A screen chooses the sample data, from the repository or the full zip, and the
    Heimdall feed families to replay. Each feed gets its own mesh identity; the replay
    keeps the picture under the 1 MiB frame Heimdall returns it in, and the picture is
    read back.
  - The install now fails, naming the job, if any Forge job does not sync; it needs
    Python 3.8+, and offers to install it.
  - The install report lists the data and feeds loaded and the credits their licences
    require.
- **A guided installer for one server.** `deploy/installer/mg-install` installs the
  suite, whole or in part, on a Linux server, and manages it afterwards. [RC-33]
  [NFR-SEC-1]
  - A server check: architecture, distribution, sizing tier, disk, Docker, ports, Plesk
    and network; it can install Docker from Docker's own repository.
  - Four use-case profiles: legal practice (EU), healthcare (NHS aligned), military
    (British and EU aligned) and utilities (water, electricity, transport,
    communications). Each sets the products, model, second factor and dual control, and
    brings a starter schema, a policy with `operator` and `viewer` roles, Forge jobs over
    fictional records, and sector guidance. Each was checked by installing it, syncing
    every Forge job and signing in.
  - The first user and, where the profile calls for it, two IAM administrators are
    enrolled before anything starts. Passwords are never stored or logged; the
    authenticator key is shown once.
  - An optional web console image (`console.Dockerfile`), served read-only on
    `127.0.0.1:5173` with a strict content security policy.
  - Checks after start: the gateway answers, Forge syncs, the first user signs in
    (with a computed authenticator code) and reads the synced data, Mimir answers over
    PQ-mTLS, Bifrost and Heimdall answer reads, the console is served.
  - `status`, `start`, `stop`, `logs`, `add-user`, `backup`, `handed-over` and
    `uninstall [--purge]`; unattended installs from an answers file; every step safe
    to repeat, so a failed install resumes.
- **The console signs in principals with a second factor.** `AuthGate` has an
  authenticator-code field, and the SDK's `login` sends the code when one is entered.
  Before this, a principal enrolled with `--totp` could sign in over the API but not
  in the console. [RC-37] [RC-29]
- **The edge on the real enclosure.** `http-gateway` serves the object-fabric routes
  from `fabric::Fabric` and its single governance spine, so edge admission and every
  fabric call go to one PQ-signed ledger.
- **Persistent keys, policy and users.**
  - `mg-admin` initialises a state directory: issuer, audit and storage keys, a
    durable store, a ledger and an identity directory.
  - The gateway loads ontology and policy files at startup.
  - Operators enrol users offline, or online with dual control and post-quantum
    passkeys, and import data.
- **Runnable services on a shared PQ-mTLS runtime.** Besides the edge, four services
  run: `mimir-server`, `bifrost-fleet` (with `bifrost-agent`), `heimdall-feed` (with
  `heimdall-source`) and `forge-worker`. The shared runtime provides mesh identity from
  files, durable audit, and health and metrics. `mg-admin mesh issue` gives each
  workload its identity.
- **Deployment.** Docker compose and a Helm chart deploy the services. The chart runs
  hardened one-replica StatefulSets with default-deny NetworkPolicies, and has run on a
  three-node kind cluster. The plain `deploy/k8s/enclosure.yaml` is checked in CI
  against the chart.
- **Forge connectors.** Hand-rolled, std-only clients for PostgreSQL (protocol v3,
  SCRAM-SHA-256), S3 (SigV4) and Kafka, syncing incrementally from checkpoints. An
  egress broker (HAProxy, TLS 1.3 with a pinned CA per route) reaches sources outside
  the enclosure.
- **Model serving.**
  - `mimir-server` serves a real model through an OpenAI-compatible engine on loopback:
    llama.cpp on CPU, vLLM on GPU.
  - A model loads only from a signed manifest whose weights stream to the signed digest.
  - `mg-model` generates the signing key, signs weights and verifies a model directory.
  - Granite 3.3 2B Instruct (Apache-2.0) is staged from an operator download. No
    weights are vendored.
- **Deployment tier.** RC-40.4 is a certified PQ-mTLS inference call to the deployed
  `mimir-server`, which must be answered by a real model.
- **Release pipeline.**
  - Every binary the workspace builds is listed in `supply-chain/release-artefacts.txt`
    and passes the per-artefact gate; a coverage check fails on a missing binary (RC-07).
  - Services become verify-before-run images.
  - The verified set travels as one signed air-gap bundle.
  - A GitHub pre-release is published only on a manual run that names the tag.

- **Model bundles that stream.**
  - `bifrost-bundle` format version 2 writes each payload in chunks, each with its own
    SHA-256.
  - `pack_streaming` and `import_streaming` pack and verify a bundle a chunk at a time,
    so a model's weights cross the air gap without being held in memory.
  - Authenticity is unchanged: the signed payload digests decide it, and the unsigned
    chunk digests locate damage.
  - An import stages its output and moves it into place only once everything has
    verified.
  - `mg-bundle` packs and imports this way; version 1 bundles still import.
  - `mg-model pack` bundles a model directory that passes the load gate.
  - `mg-model unpack` verifies the bundle offline and runs the load gate on what it
    wrote, removing the directory if the model does not verify. [AC-MS-3]
- **Release readiness: every published claim checks against the tree.**
  - A claims ledger (`docs/compliance/claims.toml`, rendered to `CLAIMS.md`) gives each
    capability a status and its evidence; `tools/ci/claims.sh` holds every row to the
    files, lines, tests and criteria it cites. [RF-CA-1]
  - `tools/ci/numbers.sh` generates `docs/compliance/NUMBERS.md` from the tree and the
    gate record, and fails when a swept document quotes a figure that differs. [RF-CA-2]
  - `tools/ci/gate-run.sh` and `tools/ci/gate-record.sh` keep the acceptance and test
    logs and turn a run into a record under `docs/compliance/gate-runs/`; the README
    and GA_READINESS status tables are generated from it. [RF-CA-5]
  - `tools/ci/tag-release.sh` cuts the signed tag only once the gate record, numbers
    sheet and claims ledger cover the commit, and `tools/ci/verify-tag.sh` verifies the
    tag against `deploy/trust/allowed_signers` before a release builds. [RF-CA-6]
  - `deploy/trust/ceremony.sh` does the key ceremony's mechanical part: it generates
    the release and Bifrost admission keys into sealed keystores outside the checkout
    and writes the public trust anchors. It commits nothing.
- **CI on GitHub and GitLab.** `.github/workflows/` and `.gitlab-ci.yml` hold no build
  logic: every step calls `tools/ci/run.sh`, and `tools/ci/pipelines.sh` fails if the
  two call different jobs. `tools/ci/release.sh` runs the whole release, or a dry run
  with throwaway keys. `tools/ci/gitlab-rules.sh` applies the repository rules to a
  GitLab mirror. [RF-CA-4]
- **Offline build from a fresh clone.** `.cargo/config.toml` resolves every crate from
  `vendor/`, and `tools/ci/vendor-check.sh` requires `vendor/` to be exactly the crates
  `Cargo.lock` names. [RF-CA-3]
- **npm artefacts are gated like binaries.** `mg-sbom` reads the production closure
  of a pnpm importer, and `mg-scan` matches each component against its own
  ecosystem's vendored advisories: RustSec for cargo, an OSV snapshot for npm.
- **Contract schemas** for the ontology and import files
  (`contracts/schema/*.v1.schema.json`), with fixtures and a test that holds the
  parsers to them. [RF-CA-13]
- `.github/CODEOWNERS`, and `docs/guides/IMPORT_YOUR_OWN_DATA.md`.
- **A recorded waiver for the client shell audits, for this pre-release only.** No
  shell has a buildable project yet, so no on-device audit can run. The gate honours a
  line in `clients/shells/audit-waivers.txt` naming the exact pre-release version, its
  approver and the reason, and still lists every missing record. A final release
  cannot be waived. [RC-49]

### Fixed
- **Heimdall keeps words that Rust reads as non-finite numbers as text.** `parse_attrs`
  turned `nan`, `inf` and `infinity`, in any case, into float attributes, which the
  picture then showed as `null`: the airport code `NAN` (Nadi, Fiji) in the sample
  feeds lost its value. Only finite numbers are numbers now. [FR-H-3]
- **The sample datasets are strict JSON.** The full set's `air_quality.json` held one
  `NaN`, which made the gateway refuse the dataset; it is `null` in the corrected
  archive, which records the change in `CORRECTIONS.md`.
- **Quieter install logs.** The compose image creates its service account
  without `useradd`'s system-range warning, and the container health check reads the
  whole response before closing, so the gateway no longer logs a reset connection on
  every check. [RC-33]
- Release signing and verification (`mg-sign`, `mg-verify`, `attest`) parse the SBOM
  strictly, require a purl on every component, and refuse to sign under an algorithm
  no verifier accepts. [RF-SC-1] [RF-SC-2]
- `mg-repro` compares exactly the artefacts Cargo reports for the build, so stale
  files no longer count. [RF-SC-3]
- `mg-scan` reads a compound version requirement as one range, so it no longer clears
  the vulnerable versions below it, and judges staleness by the system clock. [RF-SC-4]
- `mg-register` admits to the Bifrost registry only artefacts that verify against the
  pinned release anchor.
- The gate run keeps the test log when a build step left `target/` owned by root.
- RT-21: model provenance checks no longer report `Verified` without comparing bytes.
  A present vendor signature is recorded as unchecked, and a commit is verified only
  against a pinned file listing.

### Known limits in this candidate
- Key custody is software: keystores are files, and no HSM is wired in.
- Model serving on GPU (vLLM) has not run. The 800 ms first-token target is unproven.
- The Kafka connector has been tested only against a fake broker. PostgreSQL cannot
  use the egress broker before PostgreSQL 17.
- Consoles other than Midgard's run on mock data.

### Launch-readiness hardening (carried into this candidate)

Taking the platform from end-of-design + core-logic to a production-defensible
posture. Highlights:
- **Supply chain:** real PQ signing/verification (`mg-sign`/`mg-verify`), CycloneDX
  SBOM, reproducible-build and coverage gates, offline CVE + licence gates; signed
  air-gap bundle.
- **Durability:** every stateful store is a KMS-sealed, append-only journal with
  replay-on-boot and fail-closed-on-tamper (fabric, forge, bifrost, heimdall, admin,
  IAM directory, audit ledger).
- **Transport & mesh:** PQ-mTLS session resumption, connection reuse, a method router,
  and per-identity rate limiting on the gateway; Heimdall feed ingest over PQ-mTLS,
  as a running service. Picture replication between nodes is a tested library that
  no service runs yet.
- **Data platform (Forge):** real sandboxed connectors with egress enforcement, a
  pure-Rust partitioned compute engine (hash shuffle + spill-to-disk), and IR breadth
  (join / aggregate / sort / window).
- **Key custody:** a hardware (HSM/TPM) trust-root seam with attestation and key
  rotation, tested against a software stand-in. Custody is software today: keystores
  are files, and the residual risk is documented.
- **Identity & admin:** principal authentication at the edge with a password and an
  optional one-time code, and a durable directory. WebAuthn verification, JWKS and
  federation are tested libraries with no browser flow. The guarded-action
  orchestrator (authenticate, simulate, dual control, single-use token, commit, audit)
  and break-glass with durable alerts and time-boxed elevation are tested libraries
  that no service runs yet; dual control is live for principal enrolment.
- **Operability:** in-enclosure telemetry (structured logs, Prometheus `/metrics`,
  tracing, liveness/readiness, SLOs — zero-dependency, cannot egress); operational
  runbooks, a DR plan with a **tested restore drill**, backup/restore procedures, and
  an incident-response / on-call policy.
- **PKI:** a mesh certificate authority that issues, renews and revokes workload
  certificates, with revocation honoured at the next handshake, tested as a library.
  The certificates deployments use come from `mg-admin mesh issue`, which sets no
  expiry and does not renew. HSM key-ceremony and rotation documents; signing runner
  provisioning.
- **Consoles:** the Midgard console authenticates against the edge (login → PQ-signed
  token) and renders enclosure data; MockGateway retained for offline demos.
- **Governance:** LICENSE (Apache-2.0) + NOTICE, SECURITY.md, CONTRIBUTING, CODEOWNERS,
  a threat model, and a compliance/data-residency matrix.

### Changed
- **CI no longer uses Nix.** `ci.yml` builds the digest-pinned toolchain image and runs
  formatting, lints, the build, the tests, the reproducibility check and the advisory
  scan in it, with no network (`tools/ci/in-toolchain.sh`). These are the same
  toolchain and posture as developers and the release runners. The Nix steps ran on a
  flake with no lock file, which BUILD_ENV already said no automated path should depend
  on. [RT-47]
- The marketing site no longer states commercial tiers, prices, support SLAs or
  placeholder company details; the pricing page is a licence page. The README, guides
  and site pages are worded to the claims ledger.
- The GitHub actions are pinned by SHA to their first Node 24 releases.
- Contract surface reconciled with reality: protos govern the client SDK edge;
  internal service-to-service uses the lean `pq-mtls` binary codec (Rust-typed).

### Security
- Fail-closed everywhere on the critical paths (signature/verification, policy
  enforcement, durability replay, revocation). See SECURITY.md and
  `docs/security/THREAT_MODEL.md`.

---

Tagged releases will be listed here once the first tag ships.
