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
- **An upgrade path for enclosures from an earlier build.** `mg-admin mesh upgrade`
  reissues a service's certificate for the key it already holds and rewrites its trust
  anchor and `mesh.crl`, keeping both keystores (a new audit keystore would lock the
  service out of its ledger and sealed state). `mg-admin audit checkpoint` now adopts
  the gateway's dual-control ledger as well as its governance ledger, is safe to run
  again, and with `--identity-dir` adopts a mesh service's own ledger. Compose `init`
  and `mg-install`'s repair run both, so re-running them is the upgrade; the installer
  now stops every service, not only the gateway, before `init`. The kind identity
  Secrets carry `mesh.crl`. `docs/ops/UPGRADING.md` has every step. [MG-SEC-02]
  [MG-SEC-03] [MG-SEC-05]

### Changed
- **The key ceremony is held with Dvalin**, a separate application
  (veldris-ltd/dvalin-app): a guided Windows and Linux app, with a console
  form, whose key generation, signing and verification are this repository's
  `attest`, pinned by revision. `deploy/trust/ceremony.sh` is removed; its claims are
  that repository's tests, and its contract test runs this repository's release
  checks against what the app publishes. [RT-35] [AC-RF-CA-6a]

### Security
- **GitLab jobs run only digest-pinned images, and merge requests no longer run
  privileged.** [VLD-ANB-MIDGARD-0022] [VLD-ANB-MIDGARD-0043]
  - Every `image:` and service in `.gitlab-ci.yml` is pinned as
    `name:tag@sha256:<digest>`, resolved on 2026-10-31 and dated beside it.
    `tools/ci/pipelines.sh --check` (the `selftest` job) now fails on a GitLab image or
    service that is not pinned by digest, or a GitHub action not pinned to a commit,
    and prints each offending line.
  - Merge-request jobs (`lint`, `test`, `repro`, `scan`, `release-dry-run`) run
    directly inside the published toolchain image,
    `$CI_REGISTRY_IMAGE/toolchain@$MIDGARD_TOOLCHAIN_DIGEST`, with no Docker-in-Docker
    service. Their first step fails closed unless the variable is the digest committed
    in `tools/ci/toolchain.digest`. `tools/ci/in-toolchain.sh` runs commands in place
    when `MIDGARD_IN_TOOLCHAIN=1`, with `CARGO_NET_OFFLINE=true`; elsewhere it still
    uses `docker run --network none`.
  - A new job, `toolchain-publish`, run by hand on `main` or `dev`, builds and pushes
    the toolchain image and fails until its digest is committed. It and `acceptance`
    are the only jobs left on the privileged runner, which is tagged `protected`. On
    GitHub, `toolchain-publish` is a no-op.
  - Upgrading a GitLab mirror: run `toolchain-publish` once, commit the digest, set
    `MIDGARD_TOOLCHAIN_DIGEST`, and apply the runner settings in docs/dev/CI.md ("What
    a GitLab instance needs"); until then merge-request pipelines fail. GitLab's `test`
    job now records `network=offline`, which the gate record refuses: record the gate
    from a GitHub run or locally.
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
  - Checkpoints can be sent off the server. With `--checkpoint-mirror DIR` (the
    gateway, `mg-admin init`) every checkpoint is also written, atomically, to a
    directory on another mount, which only ever moves forward. A ledger rolled back
    together with its checkpoint, or restored from an older backup, is behind the
    mirror and refuses to boot; one deleted with its checkpoint is not re-created; and
    a mirror that holds nothing for a ledger with entries fails closed. An append
    whose checkpoint the mirror does not take fails, and the ledger takes no further
    entry until it does. `mg-admin audit checkpoint --checkpoint-mirror DIR` verifies
    the ledger under the pinned key and enables or re-seeds the mirror; it never moves
    one back. `audit_writer::CheckpointMirror` lets a deployment push checkpoints to a
    sink of its own; `FileCheckpointMirror` is the directory one. Without the option
    nothing changes.
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
  longer reads `MIDGARD_RELEASE_TRUST_KEY`. A llama.cpp or vLLM sidecar, which loads by
  path, is no longer given the model directory: `mimir-server --serve-dir` (required
  with a sidecar engine) copies the weights from the handle they were verified through,
  hashing them again, into a directory only it writes, read-only and renamed into place,
  and writes `staged.ready` last; the sidecar mounts only that directory, read-only, and
  starts once the marker exists, so a file swapped into the model directory after the
  check is never loaded. Readiness now watches the staged copy.
- **Tests that prove the claims.** [MG-SEC-15] CI fails a build whose dependency graph
  enables the insecure test backend (`tools/ci/no-test-backend.sh`). The NIST ACVP
  ML-KEM and ML-DSA vectors run offline; the ledger tamper and truncation tests and the
  handshake downgrade tests also run on the FIPS backend; the whole crypto suites run
  there, not chosen files; and seven crates no recipe tested now are.
- **Every listener is bounded, and the mesh rate limit is live.** The HTTP edge
  spawned a thread for every connection with no cap and no read timeout, so anyone
  who could open TCP connections could hold every thread and descriptor; and the
  gateway's per-identity rate limit and per-connection quota existed but no service
  binary turned them on. [VLD-ANB-MIDGARD-0004] [NFR-SEC-1]
  - `service_runtime::BoundedListener` (`serve_bounded`) is the one accept loop and
    the one connection cap: a slot before a thread, read and write timeouts set
    before the first read, a named thread with a stated stack, a spawn failure logged
    rather than fatal, and `{name}_connections_total{outcome}` counted. The mesh
    services serve through it, counting against the gateway's own cap (MG-SEC-13), so
    there is still one cap; `gateway::ConnectionGate` is that cap, shared.
  - The edge serves at most 256 connections; the next is answered 503 with
    `Retry-After: 1` without a thread. A request must arrive whole within a 30 s head
    deadline however its bytes are spaced, and a source address opening more than
    200 connections in a burst, 50 a second, is answered 429 with `Retry-After: 1`.
    `--max-connections`, `--idle-timeout` and `--edge-rate-per-sec` tune it.
  - `mimir-server`, `bifrost-fleet`, `heimdall-feed` and the gateway's mesh door
    apply `MeshLimits`: 128 connections, a 30 s read timeout (an idle authenticated
    connection is closed; every client opens one connection per call), a burst of
    100 and 20 requests a second per identity, and 10,000 requests per connection.
    `--max-connections`, `--idle-timeout`, `--rate-burst`, `--rate-per-sec` and
    `--requests-per-conn` tune them; a value that is not a number is a usage error.
  - The claims ledger's PQ-mTLS row and the residual-risk register's high-rate feed
    load row now cite the limits in force and the tests that prove them.
- **A login can no longer exhaust the gateway's memory or buy a ledger write per
  probe.** A username of up to 1 MiB was kept verbatim as a throttle key, so ten
  thousand made-up names outgrew the container and the restart discarded every
  token; and every attempt, throttled ones included, wrote a signed, fsynced ledger
  row under the enclosure lock. [VLD-ANB-MIDGARD-0005] [NFR-SEC-1]
  - Login fields are bounded before anything is kept or recorded: a username of at
    most 128 bytes with no control character, a password of at most 1,024 bytes and
    a one-time code of at most 16 characters, or a 400. Ten thousand tracked names
    now hold at most about 1.3 MB of keys.
  - Each source address may make 20 login and administration attempts in a burst,
    refilling at 0.5 a second; past that the edge answers 429 with `Retry-After: 2`
    before checking any password. `--login-burst` (0 is off) and `--login-per-sec`
    tune it. Under compose every client shares the proxy's address, so there it is
    one allowance for the whole site (recorded in the residual-risk register).
  - Throttled attempts are coalesced on the ledger: a locked name's first throttled
    attempt is recorded and later ones are counted onto its next recorded event
    (`LOGIN succeeded, 5 throttled attempts suppressed`); a throttled source is
    recorded at most once a minute with the count since. Ledger consumers that
    counted one row per throttled attempt see the count instead. Per-entry fsync is
    unchanged.
  - `login_attempts_total{outcome}` counts logins by outcome (`succeeded`,
    `refused`, `locked`, `throttled`, `source_throttled`, `failed`).
  - `gateway::RateLimiter` drops buckets that have refilled to capacity once it
    holds 1,024 identities, so a limiter keyed by address holds the addresses seen
    within one refill period rather than every address ever seen.
- **The login lockout can no longer be raced or flooded away.** The gateway read a
  name's lock, released it, checked the password and only then counted the failure,
  so 32 wrong passwords sent at once were all checked before the name locked; a
  flood of made-up names evicted the oldest entry, which was the victim's lock; and
  the failure count lapsed the moment the longest lock did.
  [VLD-ANB-MIDGARD-0019] [NFR-SEC-1]
  - Each attempt is reserved under the throttle's lock before its password is
    checked, and attempts in flight count towards the five free failures, so no more
    than six passwords are checked for a name before it locks however many arrive
    together. No more than four are checked at once for one name; a fifth gets 429
    `a login for this name is already being checked` with `Retry-After: 1`, counted
    as `login_attempts_total{outcome="busy"}` and not as a failure. A console that
    retries a login while the last is still being checked sees that 429 for a second.
  - When the table of tracked names is full, lapsed entries go first, then the
    unlocked entry idle longest; locked entries go last, soonest-expiring first, so a
    flood of names cannot clear a chosen name's lock.
  - The failure count now lasts 30 minutes without a failure (it was 15), longer
    than the longest lock (asserted when the crate compiles), so a name whose lock
    lapses locks again at its next failure. A user who fails five times and waits 16
    minutes no longer has free failures back.
  - Anyone who can reach the edge can still lock any name, an operator's included;
    the per-source allowance is the compensating control, recorded in the
    residual-risk register.
- **A flood of administrator challenges can no longer lock every administrator out.**
  `POST /api/v1/admin/challenge` is unauthenticated, and the gateway kept at most 256
  challenges, dropping the earliest whoever had asked for it: 256 cheap requests
  evicted every outstanding challenge, so no enrolment request or approval could
  complete while they continued. [VLD-ANB-MIDGARD-0020]
  - The request body is now `{"admin": "<id>"}`, refused with 400 if it is missing,
    not a string or joined by any other key. `mg-admin` sends it; an older
    `mg-admin` gets 400 until it is upgraded. The response is unchanged.
  - A challenge is bound to the administrator who asked: presented by anyone else it
    is refused with the same 401 as an unknown one, and consumed.
  - Each registered administrator holds at most four challenges
    (`MAX_CHALLENGES_PER_ADMIN`); a fifth replaces their own oldest, never another
    administrator's. A name with no passkey gets a nonce in the same shape, but
    nothing is kept for it.
  - The table holds at most 1,024 challenges; past that a new administrator is
    refused with 429 and no one's challenge is dropped. The refusal is recorded in
    the ledger at most once a minute (`ADMIN challenge refused, N refusals
    suppressed`).
  - The per-source allowance the administration routes share with login (above)
    remains the rate control.
- **Request paths no longer grow the gateway's memory or show up in `/metrics`.**
  `http_requests_total` and `http_request_latency_ms` were labelled with the raw
  request path, including on 401 and 404 answers, and the metrics registry made a
  new series for every label set for the life of the process, so unauthenticated
  GETs with unique paths (up to 16 KiB each) could exhaust the container's memory,
  and every path sent was listed on the unauthenticated `/metrics`.
  [VLD-ANB-MIDGARD-0003]
  - The `path` label is now the template of the route the request matched
    (`/api/v1/approvals/{id}/approve`, not the id), `OPTIONS` for a preflight and
    `other` for anything the edge does not serve. The label keeps its name, so
    dashboards and the RC-33 assertions are unchanged; grouping by approval id is
    gone by design.
  - `MetricsRegistry` holds at most 1,000 series per family (`MAX_SERIES_PER_FAMILY`)
    for every counter, gauge and histogram; a new label set past that is not
    recorded and is counted in `metrics_series_dropped_total{family}`, which is
    exempt from the cap. A rendered label value longer than 256 characters is cut
    and ends `...`. `describe()` now also takes help text before a family records
    anything, and the gateway describes its request families and the dropped counter.
  - `/metrics` on the edge stays unauthenticated, now bounded; the residual-risk
    register records it, with network restriction as the compensating control.
- **The Heimdall picture is bounded, journalled and read in pages.** Every admitted
  observation resealed and `fsync`ed the whole picture under the state lock, nothing
  capped or evicted entities, and `ReadPicture` answered with every entity in one
  frame, so past a few thousand entities every analyst read failed at the 1 MiB frame
  cap until the sealed picture was deleted. [VLD-ANB-MIDGARD-0017] [FR-H-1] [RC-13]
  - `heimdall_picture::PictureLimits`: at most 50,000 entities, 10,000 that one feed
    has contributed to, 64 attributes an observation, 4,096 bytes an attribute (and
    an entity key or kind). An observation past the attribute caps is `MALFORMED`. A
    new entity past an entity cap first evicts tracks not seen for 7 days; if none is
    that old it is refused `DENIED` with `picture-full` and audited. `heimdall-feed`
    takes `--max-entities`, `--max-entities-per-feed`, `--max-attrs`,
    `--max-attr-bytes` and `--evict-after`; metrics `picture_entities` and
    `picture_evictions_total`.
  - New state file `picture.journal` beside `picture.sealed`: each change appends one
    sealed frame (`audit_writer::SealedJournal`, domain `heimdall-picture-v1`), and
    the journal is compacted into `picture.sealed` once it holds more than twice the
    entity count plus 1,000 frames. A picture written by an earlier build opens as the
    snapshot; back up both files. To roll back, stop the service and run
    `heimdall-feed --compact` first, so `picture.sealed` holds the whole picture.
  - `ReadPicture` is paged: an empty body is the first page; a JSON body
    `{"offset", "limit", "includeHidden"}` asks for another (limit 500 by default,
    2,000 at most). The response adds `total`, `offset`, `limit` and `truncated`, the
    limit is halved until the body is under 900 KiB, and entities on hidden layers are
    left out unless asked for. `heimdall-source --picture` follows the pages
    (`--page-size`, `--include-hidden`). A consumer that read the whole picture in one
    call now gets the first page.
  - Ingest is rate limited per identity by the mesh limits `heimdall-feed` already
    applies (`--rate-burst`, `--rate-per-sec`, `--requests-per-conn`); no second
    limiter is added.

### Fixed
- **A deeply nested JSON body no longer takes the gateway down.** The request parser
  in `services/runtime` recursed once per nesting level with no bound, and the edge
  parses bodies of up to 1 MiB with it before authentication, so one unauthenticated
  login body of brackets overflowed a connection thread's stack and aborted the
  whole process, mesh door included. Nesting is now bounded at 64 levels
  (`json::MAX_DEPTH`); a deeper body is a 400, and an import line deeper than that
  refuses the file. The four other hand-rolled parsers (admin and forge ai-assist,
  bifrost risk, heimdall decision-support) carry the same bound. Connection threads
  are spawned with an explicit stack, and one that cannot be created is logged and
  its connection dropped instead of panicking the accept loop.
  `services/http-gateway/tests/security_edge.rs` is the shared socket harness for
  edge security regressions. [VLD-ANB-MIDGARD-0006] [NFR-SEC-1]
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
