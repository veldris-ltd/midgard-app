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
- **Search-around is split into seed resolution, link collection and a clearance
  filter.** `Enclosure::search_around` is now orchestration over `seed_roots`,
  `adjacent_links` and a `Walk` whose `admit` applies the per-node read check through
  an injected predicate, so the filter is unit-tested on its own. Responses are
  unchanged, node for node and edge for edge, which a golden test pins. One policy
  outcome changes: a single-seed walk is audited on the seed's own id
  (`object:<type>/<key>`) when a rule permits that and otherwise on its type's id
  (`object-type:<type>`), under the seed's marking, so a policy that permits only
  `object-type:<type>` walks from one object of the type as it reads it, instead of
  refusing the walk. A deny on either id still hides the seed as unknown.
  [VLD-ANB-MIDGARD-0094] [VLD-ANB-MIDGARD-0023]

### Security
- **The Heimdall, Forge, Bifrost and admin bridges no longer default to
  UNCLASSIFIED.** `MimirOptionProvider`, `MimirSuggester`, `MimirAdvisor` and
  `MimirCopilot` take the caller's posture in `new(inference, marking)`;
  `with_marking` is removed, so a bridge cannot be built without declaring what its
  prompt may hold. [VLD-ANB-MIDGARD-0034]
- **The agent runtime marks its request for memory and history, not only
  grounding.** `Layers.memory` and `Layers.history` are now `Vec<MarkedItem>` (text and
  `marking_level`), and `AssembledPrompt::max_marking_level` is the highest level among
  the included facts, memory items and history window; an item trimmed by the budget
  does not count. [VLD-ANB-MIDGARD-0034]
- **The model server's prompt bound counts the system text.** `max_prompt_bytes` is
  checked against `system_prompt` and `prompt` together, so a large system turn is
  refused `PROMPT_TOO_LARGE` like a large body. [VLD-ANB-MIDGARD-0016]
- **Mistral's tool-calling tokens are neutralised.** `[AVAILABLE_TOOLS]`,
  `[/AVAILABLE_TOOLS]`, `[TOOL_CALLS]`, `[TOOL_RESULTS]`, `[/TOOL_RESULTS]` and
  `[TOOL_CONTENT]` in the system or user text are escaped like the family's other
  control tokens, so data cannot forge a tool list, call or result.
  [VLD-ANB-MIDGARD-0060]
- **Deleting `revoked.list` no longer reverses every revocation.** Once the revocation
  list has existed (at start-up or at a re-check since), mimir-server treats its
  removal as unreadable: serving stops `MODEL_REVOKED` with "the revocation list …
  has been removed" until it is restored. A deployment that never had a list is
  unchanged. [VLD-ANB-MIDGARD-0057]
- **An oversized feed observation no longer uses up its sequence, and its refusal is
  audited.** heimdall-feed checks the size and attribute bounds before the replay
  guard records the sequence, so the feed can resend at the same sequence once the
  observation fits. The refusal still replies `MALFORMED`, and is now in the ledger as
  `Ingest refused: oversized observation` with reason `oversized`
  (`REASON_OVERSIZED`). [VLD-ANB-MIDGARD-0017] [VLD-ANB-MIDGARD-0041]
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
- **A mesh sync can no longer declassify an object or undo a human-gated change.**
  `SyncRecords` replaced each object it named in full, marking and purposes included,
  after one batch decision against a fixed resource, so a compromised Forge worker
  could re-sync a confidential object as open or revert a change a named human
  approved. A sync is now refused whole, with nothing written, if any record would
  lower the stored object's marking (`marking-downgrade`), widen its purposes
  (`purpose-widening`), carry a marking above the gateway's sync clearance
  (`marking-above-sync-clearance`) or touch an object with a pending, approved or
  redeemed human-gate approval (`human-gated-target`). Each refusal is audited as
  `SYNC refused: <reason>` against the object, and the worker is told only the id it
  sent. The sync clearance is set with `--mesh-sync-clearance BAND[:COMPARTMENT,...]`
  (`SecuredGateway::with_sync_clearance`); the default, the highest band with no
  compartments, keeps existing syncs working, and a deployment that syncs
  compartmented objects must now grant them. [VLD-ANB-MIDGARD-0001]
- **The mesh door decides before it reads.** A sync is authorised on `SyncRecords`
  before its records are parsed, so a workload the policy does not permit gets the
  plain denial and no ontology detail. A malformed sync from a permitted workload is
  audited as `SYNC records: MALFORMED`; the ledger action `DENY SyncRecords:MALFORMED`
  is gone, and an unpermitted malformed sync is recorded as `DENY SyncRecords`.
  [VLD-ANB-MIDGARD-0073]
- **A session can be ended at the server, and a signed-out token is refused.**
  [VLD-ANB-MIDGARD-0021]
  - Every session token carries a random 128-bit id, and the signed format is now
    `midgard-token-v2`: a token minted before this change no longer verifies, so
    everyone signs in once more after upgrading.
  - `POST /api/v1/logout`, authenticated by the token it ends, revokes it until its
    expiry (204) and records `LOGOUT` in the ledger with the token id. The gateway
    refuses a revoked token (401 `token revoked`) on every route, and on a gateway
    that issues its own tokens it also refuses one whose subject is no longer
    enrolled (401 `subject not enrolled`). Neither refusal writes a ledger row; both
    are counted in `/metrics` as `http_session_refusals_total`.
  - A gateway serving a state directory journals revocations in
    `revocations.journal`, sealed under the issuer key's custody root, so a
    signed-out token stays refused after a restart. Revocations are purged once
    their token has expired.
  - The console holds the token in memory only, never in `sessionStorage`, so a
    reload returns to sign-in. **Account → Sign out** calls `logout` before clearing
    the session; the SDK gains `logout(baseUrl, token)`.
- **The plaintext gateway edge stays off the network unless told otherwise, and the
  console will not send credentials over plaintext to another host.** A
  state-directory gateway now refuses a bind address that is not loopback (a usage
  error, exit 2) unless it is given `--allow-non-loopback`; the old warning is gone.
  Development mode has no such override. Compose, the Helm chart, the rendered
  `deploy/k8s/enclosure.yaml` and `mg-install` pass the flag after
  `--bind 0.0.0.0:7710`; a deployment with its own start command must add it. The SDK
  (`assertSafeBaseUrl`, called by `login`, `whoami` and `createHttpGateway`), the
  console at start-up, its Vite build and `console.Dockerfile` all refuse a gateway
  URL that is plaintext `http://` to anything but `127.0.0.1`, `localhost` or `[::1]`.
  The console's nginx sends its security headers from one shared file on every
  location, so `/assets/` no longer drops Referrer-Policy, X-Frame-Options and COOP;
  the CSP gains `form-action 'self'`, a Permissions-Policy denies camera, microphone,
  geolocation, payment and USB, and `connect-src` admits the origin of the gateway URL
  the console was built against. `style-src 'unsafe-inline'` stays, recorded in the
  residual risk register. The explicit `--dev` flag and the fail-closed image
  entrypoint landed earlier (MG-SEC-06). [RC-24] [VLD-ANB-MIDGARD-0014]
  [VLD-ANB-MIDGARD-0032] [VLD-ANB-MIDGARD-0085]
- **A type-scoped policy rule holds on every route.** Resource ids for fabric data were
  built differently per route: `/query` named `object-type:<type>`, search-around
  named `object-type:` (its body has no `objectType`), the enclosure and the fabric
  used the bare type, and per-object checks the raw object id. A deny on
  `object-type:aircraft` therefore held on `/query` and was bypassed through
  search-around. The scheme now lives once in `fabric::resource`:
  `object-type:<type>` for a type and `object:<type>/<key>` for one object. The edge
  takes a search-around's type from its seed, and the per-object check behind query
  and search results, search-around nodes and action targets consults both the
  object's id and its type's, so a type deny hides every object of the type and an
  `object:` deny hides that one object everywhere. Route resources are unchanged.
  Hand-written rules on bare type ids or raw object ids stop matching, and audit
  consumers keyed on the old search-around resource need updating; the shipped
  policies use `*`. [VLD-ANB-MIDGARD-0023]
- **The audit view is its own permission and is scoped to the caller.** Any principal
  allowed to Read could list the whole ledger through `GET /api/v1/audit`, including
  the ids of compartmented objects a search-around had reached and every login
  attempt by name. The route is now admitted by `ReadAudit` on `audit-ledger`, not by
  a generic Read. Rows about an object the caller cannot read now are omitted, as are
  login rows about other principals unless the caller holds `ReadAudit` on
  `audit-ledger:sessions`. Rows are omitted, not redacted, and filtered before the
  limit; the response is still a bare array, and the `X-Audit-Filtered` header says
  whether rows were omitted. **Deployments must add `ReadAudit` rules or the audit
  view returns 403**: the dev policy, `deploy/compose/core-policy.json`, the installer
  profiles, the sample configuration and `mg-admin init`'s starter policy grant it on
  `audit-ledger` to `operator` and `approver`, and on `audit-ledger:sessions` to
  `operator`. [VLD-ANB-MIDGARD-0031]
- **A poisoned lock is recovered observably, everywhere.** A panic while a lock was
  held left security state to be used silently by later requests in some places
  (the gateway's enclosure, directory, login throttle and administration locks, the
  feed and fleet services' state and governance locks) and failed every later
  request in others (the gateway's SLO lock, the metrics registry, the inference
  handler's governance lock). Every one now recovers through one policy,
  `service_runtime::sync::recover`: the recovery is counted in the new
  `lock_poison_recoveries_total{lock}` metric, logged at error level, and, for
  security state, recorded in the ledger as `RECOVERED poisoned lock` on
  `lock:<name>`. The metrics registry, which cannot count itself, renders
  `metrics_lock_poisoned 1` once its lock has been recovered. [VLD-ANB-MIDGARD-0030]
- **A crash part-way through an audit append no longer stops the service starting.**
  An append now writes its whole frame or none of it (a failed write is cut back
  off). A partial final frame that a crash leaves is cut off when the ledger opens,
  and recorded in it as `LEDGER repaired: torn tail`, but only when it follows
  exactly the entries the signed checkpoint vouches for, the one frame a crash can
  tear; any other partial frame still fails closed, so the repair cannot drop a
  complete entry. The rest of the finding, an undetectably shortened ledger, was
  closed by the signed checkpoint and its mirror under MG-SEC-05; no other head file
  or option is added. [VLD-ANB-MIDGARD-0040]
- **Federation assertions are scoped, short-lived and single-use.** An upstream
  assertion carried only a subject and attributes, so `PqFederation` would have
  accepted a captured one for ever. The payload now starts with a version byte (0x02)
  and carries an issuer, an audience, `issued_at`, `expires_at` and a 16-byte nonce.
  After the signature verifies, `verify` refuses a wrong issuer or audience, a
  validity window that is empty or longer than 300 s, an `issued_at` more than 60 s
  ahead, an expired assertion and a nonce it has already accepted; the earlier
  unversioned payload is `Malformed`. The replay cache is purged of expired nonces on
  every call and holds at most 4096; when full it refuses rather than evicting.
  `PqFederation::new` takes the expected issuer and audience, `FederatedIdp::verify`
  takes `&mut self` and the time, and `federated_login` takes `&mut dyn FederatedIdp`.
  A deployment must share one `PqFederation` behind a `Mutex` so the cache is
  global. The library is still not wired into any service. [VLD-ANB-MIDGARD-0049]
  [RC-29]
- **Library controls: duplicate workloads, break-glass and the client keystore
  floor.** [VLD-ANB-MIDGARD-0051] [VLD-ANB-MIDGARD-0055] [VLD-ANB-MIDGARD-0056]
  - `QuotaScheduler::submit` refuses a workload id that is already running or queued
    (`Admission::Rejected`), so a resubmission no longer reserves its request twice
    and leaks quota. [RC-43]
  - A break-glass grant's time-box is capped at `MAX_TTL_SECS` (four hours);
    a longer one fails with `BreakGlassError::TtlTooLong`. The requester and
    approver were already required to be different authenticated administrators
    (MG-SEC-07). `BreakGlassController::review` now revokes the grant's capability
    at once, and the new `BreakGlassController::close` ends a grant early: it
    revokes the capability, cuts the time-box so the review falls due, and seals
    a `breakglass-closed` entry. [FR-AD-8]
  - `SoftwareKeystore` always reports the `NonExtractable` floor: it holds its key
    in process memory and can no longer declare `HardwareBacked`. A higher floor
    comes from a platform keystore; `ClientCore` is now generic over the keystore
    so one can be plugged in. [FR-C-2]
  - API changes: `SoftwareKeystore::generate(provider, key_id, rng)` drops its floor
    argument (tests that exercise the floor gate use `generate_at_floor` behind the
    keystore's `test-floors` feature); `ClientCore` is `ClientCore<K: Keystore +
    TrustRoot>`, and `ClientCore::open` keeps its argument order.
    `midgard_client_keystore` re-exports `TrustRoot`.
- **Release service images run on a distroless runtime, with no shell.** The service
  image is now two stages, each pinned by digest: the toolchain stage only lays out
  the gate-built files, and the image runs on `gcr.io/distroless/cc-debian12:nonroot`
  (glibc and libgcc; no shell, package manager, compiler or curl), which stays on the
  toolchain's Debian release because the binaries link glibc. Verify-before-run moves
  from `deploy/images/entrypoint.sh`, now removed, into `mg-verify --run-service`, the
  image's exec-form entrypoint: it verifies `/app/$MIDGARD_SERVICE` against the pinned
  `/trust/release-trust.pub` and only then execs the service with the container's
  arguments. It still refuses a container given no options (exit 2), and neither the
  environment nor the arguments can name another trust key. The chart's `seed` init
  container, which needs a shell and `tar`, runs in its own digest-pinned image
  (`seedImage`, distroless `base-debian12:debug-nonroot`) with the same hardened
  securityContext. The signing-runner preflight requires the runtime and seed images,
  and `tools/ci/tests/service-image.test.sh` replaces the entrypoint script's test.
  [VLD-ANB-MIDGARD-0068] [AC-B-1] [NFR-PQ-3]
- **The mesh services' health and metrics ports are no longer open to every pod in
  the cluster.** [VLD-ANB-MIDGARD-0027] In the Helm chart the four 9090 health ports
  admit only the scraper — pods labelled
  `app.kubernetes.io/name=metrics-scraper` (`scraper.podLabel`), plus any node
  ranges in `scraper.nodeCidrs` for CNIs that police the kubelet's probes — instead
  of `from: ["*"]`, and a port whose `from` is `"*"` now fails `helm template` naming
  the service and port. Each StatefulSet gets `POD_IP` from the downward API and the
  health port binds `$(POD_IP):9090`, not every interface. `deploy/k8s/enclosure.yaml`
  is re-rendered, and `tools/ci/tests/k8s-ingress.test.sh` (run by `selftest`) fails
  on any ingress rule without a `from:`. The services warn at start when the health
  port binds `0.0.0.0`. Compose keeps `0.0.0.0:9090` on its internal, unpublished
  mesh network; that residual is in the residual risk register. Upgrading: label
  your Prometheus pod `app.kubernetes.io/name=metrics-scraper` (or set
  `scraper.podLabel`) before upgrading the chart, or scraping stops.
- **Compose services run under resource limits, and the gateway has no route out of
  the host.** [VLD-ANB-MIDGARD-0029] [VLD-ANB-MIDGARD-0061]
  - Every service in `deploy/compose/compose.yaml` has a memory limit with no swap
    above it, a CPU share, a PID limit and a descriptor limit: 512 MB, one core, 256
    PIDs and 4096 descriptors by default; the gateway 1 GB, 512 PIDs and 8192
    descriptors; mimir-server 4 GB and four cores; the Granite llama.cpp sidecar 6 GB
    and four cores. A flood of idle connections or inference load now exhausts one
    container, not the host. These are evaluation sizes: `mg-install` writes limits
    sized from the server's cores and memory into `compose.site.yaml` (the gateway
    min(1 GB, RAM/8); Mimir half the memory with Granite, a quarter without, and
    every core but two; the rest 512 MB and one core). Sites that set interim limits
    in their own overlay can remove them.
  - The `edge` network the gateway is published from no longer masquerades and
    carries no traffic between its containers, so the gateway, which was on an
    ordinary NAT bridge, cannot open connections out of the host; the loopback
    publish on 127.0.0.1:7710 is unchanged, and the console reaches the gateway
    through it. The header of `compose.yaml` said nothing could reach outside the
    host, which was true only of the mesh; it now describes both networks.
  - The bridge setting alone is not enough where the host forwards or NATs the
    bridge's subnet itself (Docker Desktop's VM does), so `edge`'s bridge now has a
    fixed interface name, `mg-edge0`, and `mg-install` adds a host firewall rule: in
    Docker's `DOCKER-USER` chain (and ip6tables' where Docker manages one), replies
    to existing connections return, then anything forwarded from `mg-edge0` to
    another interface is dropped. The rules are tagged `midgard-edge-egress`; a
    systemd unit, `midgard-edge-egress.service`, puts them back after every boot and
    Docker restart; `mg-install uninstall` removes both. A server with no iptables
    or no `DOCKER-USER` chain (Docker's nftables backend, or `"iptables": false`)
    fails the step rather than installing unprotected. Plain compose, without the
    installer, still relies on the bridge setting.
  - `tools/ci/tests/compose-limits.test.sh` (in `selftest`) fails if a long-running
    service loses a limit or `edge` regains a route out, and checks the installer's
    sizing; `tools/ci/tests/edge-egress.test.sh` checks the bridge name and, with
    fake iptables and systemctl, the installer's rules, their idempotence, their
    removal and the failure without iptables; `deploy/compose/smoke.sh` checks each
    running container's limits and that the gateway cannot connect out (skipped on
    Docker Desktop, which is not a server target).
- **`mg-install` checks the install folder before it fills or deletes it, and the
  Docker key before it trusts it.** [VLD-ANB-MIDGARD-0048]
  - `MG_INSTALL_DIR` (or `INSTALL_DIR` in an answers file) must be an absolute path at
    least two levels deep, with no `..`, not a symbolic link, not the installer's
    repository or inside it, and whose parent is owned by root and not writable by
    everyone. Every command refuses anything else, so `/`, `/srv` or a typo cannot
    become the folder written, or deleted, as root. `uninstall --purge` also requires
    the folder to hold `install.env` and be owned by root before it deletes anything.
  - Upgrading: an install in a one-level folder such as `/midgard` is refused until it
    is moved to, say, `/opt/midgard`.
  - Docker's apt signing key is installed only if its fingerprint is the one Docker
    publishes (`9DC8 5822 9FC7 DD38 854A E2D8 8D81 803C 0EBF CD88`); `gnupg` is
    installed to check it. On Red Hat family servers a Docker repository file with
    `gpgcheck` off is refused.
  - The unattended install's `OPERATOR_PASSWORD_FILE` must be a regular file owned by
    root with mode 0600 or 0400, as `answers.example` always said.
  - The installer can be sourced without running, and
    `tools/ci/tests/mg-install.test.sh` tests these checks unprivileged.
- **The published samples have a data-protection screening (WP-25 A).** Every
  published entry in `samples/catalogue.json` carries a `data_protection` record
  (`personal_data` none, possible or yes; basis; date; screening role; notes), which
  `convert.py check` enforces and `samples/NOTICE.md` shows. `convert.py screen` lists
  the fields a reviewer must assess and counts the small craft (no IMO number, hull
  under 24 m) in the AIS vessel snapshot; `--apply` pseudonymised the 212 it found
  (name and call sign removed, MMSI replaced by `small-craft-<n>`).
  `docs/compliance/DATA_PROTECTION_SCREENING.md` is the screening record, and leaves
  the full-DPIA decision, and the AIS observations sample, to the Data Protection
  Officer. [VLD-ANB-MIDGARD-0074]
- **The mock gateway is out of the production console bundle (WP-25 B).** The mocks
  and their fixtures (`MockGateway`, the six product mocks) move to the subpath entry
  `@midgard/sdk/mock`; `@midgard/sdk` itself no longer reaches them, and the gateway
  errors move to `sdk/ts/src/errors.ts`. The midgard console imports the mock
  dynamically, only when built with `VITE_GATEWAY=mock`, so an http build holds none
  of it (`tools/ci/tests/js-mock-bundle.test.sh`, run by the js-build job).
  `AuthGate`'s `mockClient` is optional, and mock mode without one fails loudly. The
  six mock-only consoles (admin, Bifrost, clients, Forge, Heimdall, Mimir) run under
  `AuthGate mode="mock"` with the new `DemonstrationBanner` ("Demonstration data. This
  console runs on built-in sample data and is not connected to an enclosure.") and
  load their mock the same way, so moving one to http is its `mode` line.
  [VLD-ANB-MIDGARD-0086]
- **The compliance documents match the code (WP-25 C).** `human-gated-actions` cites
  `redeemable()` and states what it does not cover (any other principal the policy lets
  approve can decide, with no fresh second factor; approvals are in memory);
  `bifrost-rollout` cites `roll_out_live` and no longer counts the `roll_out` model's
  nominal `min_available` as zero-downtime evidence; `policy-decisions` scopes marking
  and purpose to the HTTP edge. The risk register no longer says the edge accepts any
  non-empty token. DATA_RESIDENCY lists the controls that exist and says retention,
  erasure and export are not yet available; BACKUP_RESTORE says custody is the version 2
  software keystore and no HSM or TPM holds the root; RETENTION lists the parked
  enrolments journal and what WP-15 changes about it. `claims.sh --check` accepts
  `path:line:token` anchors and fails when the line stops containing the token.
  [VLD-ANB-MIDGARD-0087]
- **Every commit in a change must be signed (WP-25 D).** The new `signed-commits` job,
  after `secrets` in the hygiene job on both forges, verifies each commit in the
  change's range (the request's base, or the commit a push moved from, to HEAD) against
  `deploy/trust/allowed_signers` and fails naming each one that is unsigned or signed
  by an unlisted key. CONTRIBUTING.md says how to sign. Until the key ceremony
  publishes `deploy/trust/allowed_signers` and authors sign, the job fails every
  change; the owner enables the required-signatures rule on `main`.
  [VLD-ANB-MIDGARD-0088]
- **Gitleaks suppressions are scoped to a file (WP-25 E).** `.gitleaks.toml` has no
  global stopword list: each of the four prose literals is allowed only in the one file
  it occurs in (`condition = "AND"`), and the two fixture tokens of a branch outside
  this tree are gone; that branch adds its own scoped entry when it merges.
  [VLD-ANB-MIDGARD-0089]
- **A monthly dependency review (WP-25 F).** `.github/workflows/dependency-review.yml`
  (the 1st of each month, and by hand) and a GitLab schedule run the new
  `dependency-review` job: both advisory sources are fetched on the runner,
  `just advisory-db-refresh` and `just advisory-db-npm-refresh` (which now take an
  already fetched input) rewrite the snapshots offline in the toolchain, the scan runs,
  and when a snapshot moved a pull request is opened with `gh` from
  `chore/advisory-refresh-<date>`. BUILD_ENV.md and CONTRIBUTING.md describe the
  review. [VLD-ANB-MIDGARD-0090]
- **A pre-commit gate (WP-25 G).** `.pre-commit-config.yaml` runs the CI secret scan on
  what is staged (`gitleaks protect --staged` with `.gitleaks.toml`) and
  `cargo fmt --all -- --check` as local hooks; CONTRIBUTING.md explains
  `pre-commit install` and the pinned gitleaks version, and
  `tools/ci/tests/precommit.test.sh` keeps both hooks in the file.
  [VLD-ANB-MIDGARD-0091]
- **Keystores no longer hold the custody root in the clear (WP-50).** Keystore format
  version 2 wraps each keystore's ML-KEM-1024 root, the root that also seals the
  identity directory, the audit ledger and the object store: under an operator
  passphrase (PBKDF2-HMAC-SHA256 at 600,000 iterations with a fresh 32-byte salt,
  AES-256-GCM bound to the key id), or sealed to an external `kms::TrustRoot`, the HSM
  or TPM seam (tested against the SoftHSM stand-in). A wrong passphrase is reported as
  locked, never as corruption. Every program that opens a keystore reads the passphrase
  from `--keystore-passphrase-file` or the file `MIDGARD_KEYSTORE_PASSPHRASE_FILE`
  names, and refuses to start with neither. Version 1 keystores are refused by every
  reader; `mg-admin keystore migrate` converts one in place after proving the new file
  unseals the same key, and `mg-admin keystore verify` checks one unlocks. Key files are
  created 0600 with `create_new` and renamed into place, never overwritten (so `mg-sign
  keygen` refuses an existing keystore), and `keys/` and mesh `--out` directories are
  created 0700. Compose `init` gives each keystore holder a random passphrase in a
  volume of its own and migrates existing keystores, so re-running it is the upgrade;
  the release jobs need `MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE` and
  `MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE`. The README, BACKUP_RESTORE, DR_PLAN, PKI,
  KEY_CEREMONY, UPGRADING and DATA_RESIDENCY documents now describe the custody the
  software delivers. [VLD-ANB-MIDGARD-0007] [NFR-PQ-2]
- **The model trust anchor no longer lives beside the weights it vouches for (WP-66).**
  The model-signing public key is a pinned anchor, `deploy/trust/model-signing.pub`,
  published by the key ceremony with the release and Bifrost anchors and carried
  read-only in the images at `/etc/midgard/trust`; compose passes `--model-trust
  /etc/midgard/trust/model-signing.pub`, and `deploy/kind/model.sh` builds the
  `mimir-model-trust` ConfigMap from the committed file, never from the models volume.
  Until the ceremony commits it, a deployment serving a real model fails closed, and the
  release preflight refuses a release without it. `mimir-server` and `mg-model
  verify`/`pack`/`unpack` refuse a trust key inside the model directory. `mg-model sign`
  requires `--sha256`, so a download whose digest was never checked is never signed.
  `stage-model.sh` no longer writes a key into `midgard-models`: by default it signs
  with the keystore provisioned from the staging enclave and checks the result against
  the committed anchor; `--development` generates a development key (keystore version
  2 under a passphrase) whose public half goes into a volume of its own,
  `midgard-model-trust`, read by `compose.dev-model.yaml`, with a warning. Installs
  staged by an earlier release must stage again (`mg-install` does so itself).
  [VLD-ANB-MIDGARD-0028]
- **Kubernetes custody path (WP-29).** `deploy/kind/bootstrap-secrets.sh` refuses any
  kubectl context that is not `kind-*`, gives each keystore holder a
  `<name>-keystore-passphrase` Secret of its own, and seeds only keys, trust anchors,
  configuration and the gateway's governance ledger (which the gateway cannot start
  without since MG-SEC-05), never the identity directory, object store, admin state or
  other journals. The Helm chart gains `secrets.provider`: `kubernetes` (the default,
  which fails to render outside the kind profile unless
  `secrets.acknowledgeEtcdEncryption` records that etcd encrypts Secrets at rest),
  `csi` (Secrets Store CSI driver volumes from operator-supplied SecretProviderClass
  objects) or `externalSecrets` (ExternalSecret objects populating the Secret names the
  chart mounts). Every workload now gets its keystore passphrase: an init container
  copies it into a memory-backed volume as a file the service's user owns, mode 0400,
  because a secret volume reaches a non-root pod only group-readable and the services
  refuse a passphrase file wider than 0600; that rule is unchanged.
  `deploy/k8s/enclosure.yaml` is rendered with the acknowledgement and says so; the
  kind smoke and the deployment tier's inference probe use the passphrase Secrets.
  `deploy/helm/midgard/README.md` documents the providers, mandatory etcd encryption,
  RBAC and the kind-only script. The compose header no longer claims that no service
  sees the enclosure's keys: the gateway mounts core-state, which holds `keys/`.
  [VLD-ANB-MIDGARD-0018] [NFR-SEC-1]
- **The compliance documents no longer claim deletion (WP-29).** DATA_RESIDENCY says
  subject erasure and export are not implemented; `docs/compliance/RETENTION.md` lists
  each durable store, the personal data it may hold, its retention target, the
  crypto-shred mechanism planned for it and the controller's obligations;
  BACKUP_RESTORE points at it; and the claims ledger records `personal-data-erasure`
  as spec. Erasure itself is a follow-on work package. [VLD-ANB-MIDGARD-0070]
- **Heimdall: an action's class is the deployment's, every execution has a human
  decision, and only grounded options reach the audit lineage.**
  [VLD-ANB-MIDGARD-0045] [VLD-ANB-MIDGARD-0079]
  - `heimdall_workflow::ActionTypeCatalog` maps each action type to
    `ActionClass::Consequential` or `Informational`, and parses a
    `<type> consequential|informational` line file
    (`deploy/compose/heimdall-actions.catalog` is the sample for the demo types).
    `WorkflowEngine::propose` classifies from it and refuses an unlisted type with
    `WorkflowError::UnknownActionType` before anything is audited; an engine with
    no catalogue refuses every proposal.
  - **Breaking:** `ProposedAction::consequential` is private and
    `set_consequential` is removed; read the class with `class()` or
    `is_consequential()`. Callers give the engine a catalogue with `with_catalog`
    (`DurableWorkflow::with_catalog` for the durable store, which re-derives the
    class of recovered records from it).
  - `WorkflowEngine::execute` requires `Authorised` for every action, so an
    informational action, which used to run straight from `Proposed`, now needs a
    recorded human decision too.
  - `DispatchActuator::with_catalog` re-checks each action: an unlisted type fails
    with `ActuatorError::Unsupported` and a class that disagrees with the
    catalogue with the new `ActuatorError::ClassMismatch`; with no catalogue it
    refuses everything.
  - Decision support offers only grounded options: `AdvisoryOptions` now carries
    `option_set` (grounded options, re-ranked 1..n), `grounded:
    Vec<GroundedOption>` and `rejected: Vec<RejectedOption>` with each rejected
    option's unresolved citations; `OptionGrounding` and the `grounding` field are
    removed. `OptionSet::parse` refuses duplicate or gapped ranks.
    `ProposedAction::from_course_of_action` takes a `&GroundedOption`, which only
    the adapter can make.
- **The insecure test backend is kept out of release builds by a guard a profile
  setting cannot switch off, and by the CI gate its documentation promised.**
  pqc-agility's release guard on `test-backend` was keyed on `debug_assertions`, which
  `debug-assertions = true` in a release profile defeats; it is now keyed on a
  `midgard_release` cfg that the crate's new build.rs sets from Cargo's `PROFILE` for
  the release profile and every profile inheriting from it. `just check-backends`, the
  `check-backends` CI job on both pipelines, fails if the workspace build or any
  package in the release matrix resolves `test-backend`, proves a release build with
  it fails on the guard, and builds the FIPS backend in release. It reads resolved
  features, so it also catches a binary whose own default features enable the test
  backend, which the feature-graph walk of `tools/ci/no-test-backend.sh` does not
  show. [VLD-ANB-MIDGARD-0067] [RT-08]
- **The seven consoles build with Vite 7.** `vite` moves from 5.4.21 (end of life) to
  7.3.7 and `@vitejs/plugin-react` from 4.7.0 to 5.2.0 in every `products/*/ui`, which
  lifts the dev-server and launch-editor advisories on Vite 5 and the build-time ones
  under it: the lockfile now resolves esbuild 0.28.2, postcss 8.5.30, nanoid 3.3.20,
  source-map-js 1.2.2, browserslist 4.29.3 and baseline-browser-mapping 2.11.29. No
  `pnpm.overrides` entry was needed. `tools/ci/tests/js-toolchain.test.sh` fails the
  selftest job if any of these resolves below its floor again. The shipped bundles
  are unchanged in content (react, react-dom and react-router-dom are not touched);
  every bundle hash changes, and `just js-build` still proves them bit-identical. The
  `vite.config.ts` files are unchanged: `build.target` stays unset, so the consoles
  take Vite 7's default target (baseline widely available: Chrome and Edge 107,
  Firefox 104, Safari 16). Node 22.12 or later is now required (`engines`), and the
  release dry run sets it up on the hosted runner. [VLD-ANB-MIDGARD-0047]
  [VLD-ANB-MIDGARD-0095] [VLD-ANB-MIDGARD-0096] [VLD-ANB-MIDGARD-0097]
  [VLD-ANB-MIDGARD-0098] [VLD-ANB-MIDGARD-0099] [VLD-ANB-MIDGARD-0100]
- **The consoles route with react-router 7.** Every `products/*/ui` replaces
  `react-router-dom` ^6.28.0 (resolved 6.30.4, with `react-router` 6.30.4 and
  `@remix-run/router` 1.23.3 under it) with `react-router` ^7.18.4, resolved 7.18.4,
  which brings `cookie` 1.1.1 and `set-cookie-parser` 2.7.2; each was published at
  least two weeks ago, and no `pnpm.overrides` entry is needed. This closes the
  open-redirect XSS advisory on react-router-dom 6.30.4 and the SSR-disclosure and
  backslash open-redirect advisories on react-router 6.30.4, none of whose
  preconditions the consoles met. The 26 imports change from `react-router-dom` to
  `react-router` and nothing else: the consoles use `createHashRouter`,
  `RouterProvider`, `useNavigate`, `useSearchParams`, `useLocation` and `Link`, with
  flat routes, no splat routes, no loaders, actions, forms or fetchers, so none of
  the v7 behaviour changes applies. `tools/ci/tests/js-toolchain.test.sh` fails if
  react-router resolves below 7.18.0 or react-router-dom is resolved at all. Every
  bundle hash changes; `just js-build` still proves them bit-identical.
  [VLD-ANB-MIDGARD-0082] [VLD-ANB-MIDGARD-0083]
- **The launch-media tooling is off its vulnerable image libraries.**
  `docs/launch/pipeline` moves sharp from 0.34.5 to 0.35.5 (libvips 1.3.4, lifting the
  libvips, libheif and librsvg advisories) and, through an `overrides` entry, image-size
  from 1.2.1 to 2.0.4 (pptxgenjs 4.0.1, the latest, still asks for 1.x but never calls
  it). `npm audit` reports no advisories. The pipeline stays a maintainer-workstation
  tool outside the workspace and the release gate; its README says so and installs it
  with `npm ci --ignore-scripts`, and `js-toolchain.test.sh` holds its lockfile to both
  floors. [VLD-ANB-MIDGARD-0050] [VLD-ANB-MIDGARD-0062]
- **Advisory data can no longer quietly age for six months.** Both vendored snapshots
  now fail the scan past 45 days (was 180), and `mg-scan import-osv` writes that limit;
  `snapshot_older_than_fail_after_days_fails` proves a 46-day-old snapshot is refused.
  The RustSec snapshot is refreshed to upstream `2295466` of 2026-10-30 (1,375
  advisories, was 1,279 of 2026-10-12). A new `advisory-refresh` job, run weekly on both
  forges (`.github/workflows/advisory-refresh.yml`, a GitLab pipeline schedule),
  refreshes both snapshots, re-scans every SBOM, and opens a pull or merge request with
  the scan diff when either moved; the two refresh recipes take an already fetched
  input so the offline toolchain can run them. `renovate.json` proposes weekly,
  reviewable updates for the pnpm workspace, `docs/launch/pipeline`, the Cargo
  manifests (re-vendored by a self-hosted Renovate, by a maintainer otherwise), the
  GitHub Actions SHAs and the GitLab image digests. docs/dev/CI.md, "Dependency
  updates", has the workflow. [VLD-ANB-MIDGARD-0076]
- **A release tag no longer vouches for itself.** The release's tag gate verified a tag
  against `deploy/trust/allowed_signers` from the tagged commit, with that commit's own
  scripts, and never checked that the commit was on `main`; so a commit that added a key
  could verify a tag on itself, and its scripts would run beside the signing material.
  Now `tools/ci/verify-tag.sh` verifies against the signing runner's own copy of
  `allowed_signers` (`MIDGARD_ALLOWED_SIGNERS_PATH`, outside the workspace), refuses the
  tag unless the committed file is byte-for-byte that copy, and refuses a tagged commit
  that is not an ancestor of `origin/main`, fetched afresh. On a self-hosted runner with
  no pinned copy it refuses every tag. Both pipelines' preflight jobs check out `main`
  a second time into `.ci-trusted` and run `release-tag` and `release-preflight` from
  there before anything from the tagged commit runs (`tools/ci/pipelines.sh` counts
  those calls as the same jobs). The preflight now also refuses an allowed-signers copy
  or keystore inside the job's workspace, an unreachable docker daemon, and an
  `in-toolchain.sh` that does not build with `--network none`. New
  `tools/ci/github-rules.sh` applies the repository rulesets as code, among them a
  `v*` tag-creation rule whose only bypass actor is the `release-managers` team, kept
  apart from the immutability ruleset so that no one can move or delete a published tag.
  Owner actions: create the team and apply the rulesets, provision the runners' copy of
  `allowed_signers`, run the runners ephemeral (`docs/ops/PKI.md`, "Signing runner",
  items 5 to 8). [VLD-ANB-MIDGARD-0035]
- **Release-time CI tooling is pinned, and no checkout keeps the workflow token.**
  The shell-audit job, whose record gates the release, no longer runs `http-server` and
  `wait-on` by name with `npx --yes`: they are pinned to exact versions (14.1.1 and
  8.0.5) in `tools/ci/shell-audit/package-lock.json`, with the registry's integrity
  hashes, installed with `npm ci --ignore-scripts` and run from its `node_modules/.bin`.
  A selftest lint (`tools/ci/tests/npx-pin.test.sh`) refuses an unpinned `npx` under
  `tools/`, `.github/` or in `.gitlab-ci.yml`. Every `actions/checkout` step sets
  `persist-credentials: false` (checked by `tools/ci/tests/workflows.test.sh`), and the
  release preflight fails closed on a self-hosted runner with no job-completed cleanup
  hook (`ACTIONS_RUNNER_HOOK_JOB_COMPLETED`); `docs/ops/PKI.md` "Signing runner" item 5
  is the owner action. [VLD-ANB-MIDGARD-0058] [VLD-ANB-MIDGARD-0081]
- **Unsigned commits are refused by CI, and the branch rules are code with exported
  evidence.** The documented `required_signatures` rule was not in force when unsigned
  commits reached `main`, and nothing in the pipeline checked. A new `commits` job, in
  both pipelines, runs `tools/ci/verify-commits.sh` on every commit between the change's
  base and its head: each must verify against `deploy/trust/allowed_signers` as the base
  has it (so a change cannot add the key that vouches for it), or be verified by GitHub,
  or be a forge merge whose combined diff is empty; it names every failure, and with no
  listed key and no forge to ask it fails closed. `tools/ci/github-rules.sh` adds a
  `pull_request` rule to the `signed history` ruleset on `main` and `dev` (one approving
  code-owner review after the last push, merge commits only, no bypass), and its
  `--export` is tested for the normalised `docs/compliance/evidence/rulesets-<date>.json`.
  CODEOWNERS names both maintainers for the trust set, the pipelines, `supply-chain/` and
  `docs/compliance/`; CONTRIBUTING.md and `docs/ops/PKI.md` give the signing setup and
  how `allowed_signers` is maintained. Owner actions: register the maintainers' SSH
  signing keys on GitHub with vigilant mode, commit them to `allowed_signers` by a signed
  and reviewed change, apply the rulesets and commit the export, and turn on CodeLab's
  push rules (`docs/dev/CI.md`, "Repository rules"). Until the keys are committed the
  job passes on GitHub only. [VLD-ANB-MIDGARD-0069]
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
- **Permitted purposes are certificate attributes too, and mesh services authorise
  each request at the certified clearance.** [VLD-ANB-MIDGARD-0063]
  - `mg-admin mesh issue`, `renew` and `upgrade` take `--purposes P[,P...]`. A
    certificate that binds purposes is version 3 (`MGCERT3`, signed under its own
    domain tag), and a channel declared for any other purpose is refused
    (`AuthzError::PurposeNotCertified`). Without `--purposes` the certificate binds
    none and is written as version 2, byte for byte as before; `renew` keeps the
    purposes the old certificate bound. Existing `MGCERT2` certificates load, verify
    and connect unchanged and stay unconstrained in purpose: the control takes
    effect for a service once its clients are reissued with `--purposes`. A release
    before this one refuses `MGCERT3` files and handshakes carrying them; roll back by
    reissuing with that release's `mg-admin`.
  - The gateway hands every request handler the channel's `Authorization` (the
    certified clearance capped by the requested posture, the declared purpose and the
    certificate's purposes). `service_runtime::mesh_context` builds the per-request
    policy context from it; heimdall-feed, bifrost-fleet, mimir inference and the
    gateway's mesh sync use it instead of a hand-built UNCLASSIFIED clearance.
  - A mesh sync from a workload whose certificate binds purposes is refused whole if
    any record is marked above the clearance it is certified at
    (`marking-above-sync-clearance`, audited). Older certificates are not held to it.
  - `heimdall-feed`, `bifrost-fleet` and `mimir-server` take
    `--resource-marking BAND[:COMPARTMENT,...]` (default `open`, as before). A
    deployment that marks a resource above open must reissue its clients with a
    matching `--clearance`, or their channels are refused.
- **Keys are wiped from memory.** [MG-SEC-08] The X25519 and AES libraries are built
  with their zeroize features, intermediate secrets are wiped, the ML-KEM secret-key
  buffer is allocated at its final size, and a failure to lock memory is logged
  (`pqc_agility::mlock_failures`).
- **Derived keys are wiped too, and freeing one secret no longer unlocks another.**
  [VLD-ANB-MIDGARD-0036] The KMS data-encryption key is now `Zeroizing`, as the
  pq-mtls traffic, resumption and ticket keys already were (`SecretArray`);
  `SecretArray::new` wipes the array it was given; and the hybrid ML-KEM secret key
  is written in place into a buffer of its final size instead of being appended to.
  Dropping a secret no longer calls `munlock`, which on Linux unlocks whole pages and
  so could unlock a long-lived key sharing the page. Instead every mesh service
  (http-gateway, mimir-server, bifrost-fleet, heimdall-feed, forge-worker) locks its
  whole process with `mlockall` at start through `service_runtime::harden_process`,
  when `RLIMIT_MEMLOCK` is unlimited, and logs one line naming `RLIMIT_MEMLOCK` and
  `CAP_IPC_LOCK` if it cannot; `MIDGARD_NO_MLOCKALL=1` turns it off (logged).
  `pqc_agility::Zeroizing` is re-exported for downstream crates. RB-11 in
  `docs/ops/RUNBOOKS.md` covers the limit.
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
- **A hand-off for the independent cryptographic review.** [VLD-ANB-MIDGARD-0024]
  [VLD-ANB-MIDGARD-0059] `docs/security/CRYPTO_REVIEW_HANDOFF.md` lists each
  primitive and construction, the test files that hold its vectors and the open
  questions for the reviewer. The hybrid combiner is pinned by a vector computed
  outside the crate, and SHA3-256 by FIPS 202 answers
  (`crypto/pqc-agility/tests/hybrid_kat.rs`).
- **Stronger password storage.** [MG-SEC-11] New passwords are Argon2id. A stored hash
  outside fixed cost bounds is refused like a wrong password, so an edited entry
  cannot weaken itself; legacy PBKDF2 entries (210,000 iterations) still sign in and
  are replaced with Argon2id at that sign-in, recorded in the audit ledger.
- **Password storage documented as delivered.** [VLD-ANB-MIDGARD-0025] The compliance
  matrix now states the PBKDF2 floor the code enforces (210,000 iterations, legacy
  entries only) and the ceilings, SAD_00 gives the Argon2id parameters, the
  login-and-session-tokens claim names the KDF and its migration, and the residual
  risk register gains a "Password KDF" row. New tests pin the PBKDF2 floor and
  ceiling (a tampered count is refused in under 50 ms) and the Argon2id format.
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
- **A fleet check-in is bounded.** `bifrost-fleet` took any channel name and strings
  as long as the 1 MiB frame, let an agent's report overwrite its installation's
  channel, and appended a sealed, fsynced journal frame for every check-in with no
  compaction, so one agent identity could grow `fleet.journal` without bound, slow
  every restart and misreport its channel. [VLD-ANB-MIDGARD-0042]
  - `AgentReport::decode` refuses an install id over 128 bytes, a channel or version
    over 64, an empty field and a control character, each with a fixed message.
  - A check-in must name a channel in `channels.json`, and an installation's channel
    is the one it first registered on: anything else is refused and audited as
    `CheckIn refused: unknown channel` or `CheckIn refused: channel mismatch`.
    Moving an installation is an operator action: the new `SetChannel` mesh method
    (policy action `SetChannel`, granted to `fleet-operator` in the compose policy),
    sent by `bifrost-agent --set-channel ID --channel C`. `Fleet::check_in` now
    returns `Result<bool, FleetError>`.
  - The journal records changes, not calls: an unchanged check-in journals a
    `Heartbeat` at most once per `--heartbeat-journal-interval` seconds per
    installation (default `--stale-after`), so after a restart last-seen is known to
    within that interval. Past `4 x installations + 1,000` frames, on open or after a
    write, the journal is rewritten as one `Snapshot` frame (`fleet.journal.tmp`,
    fsynced, renamed). A journal from an earlier build still loads, and is compacted
    when it is past the threshold; a journal that starts with a snapshot is not
    readable by an earlier build.
  - `bifrost-fleet` defaults the shared mesh limits to a burst of 10 and 0.1
    requests a second per identity (one check-in every 10 s) and 1,000 requests per
    connection; `--rate-burst`, `--rate-per-sec` and `--requests-per-conn` still tune
    them, through the same limiter.
- **Inference is admitted against a budget.** `max_tokens` came from the wire and
  reached the sidecar uncapped, the prompt was bounded only by the 1 MiB frame, and
  nothing bounded calls in flight, so two callers with large budgets could hold the
  llama.cpp sidecar's two slots for the whole 120 s read timeout. [VLD-ANB-MIDGARD-0016]
  [AC-62.1] [AC-62.3] [FR-MI-8]
  - `InferenceHandler::with_limits(InferenceLimits)`: after policy permits a call,
    a prompt over `max_prompt_bytes` (default 256 KiB) is refused `PROMPT_TOO_LARGE`;
    a call over `max_concurrent` in flight (default 4) or
    `max_concurrent_per_identity` for its caller (default 2) is refused `BUSY`; and
    `max_tokens` is clamped, not refused, to `max_tokens` (default 1024). Each refusal
    is a `Denied` with its own audit entry (`Infer:PROMPT_TOO_LARGE`, `Infer:BUSY`)
    after the call's decision. The slot is held until the engine returns.
  - `mimir-server` takes `--max-tokens`, `--max-prompt-bytes`, `--max-concurrent`,
    `--max-concurrent-per-identity` and `--infer-timeout` (seconds, default 60), logs
    them as `inference_limits` on the serving line, and counts refusals as
    `inference_refused_total{reason}` with `busy`, `prompt_too_large` and
    `throttled` (a request over WP-31's per-identity rate limit, through the new
    `Gateway::on_throttle`). A value that is not a whole number of at least 1 is a
    usage error.
  - The sidecar's read timeout defaults to 60 s instead of 120 s, and is
    `--infer-timeout` in mimir-server: a stuck completion fails and frees its slot.
    A completion is timed out, not cancelled.
  - A client's end-of-stream marker no longer counts as a request against the rate
    limit or the per-connection quota, so a one-call client spends one token.
  - `compose.granite.yaml` passes `--max-concurrent 4` beside llama.cpp's
    `--parallel 2`, and the Helm values give mimir-server every limit at its default.
    Consumers that need more than 1024 completion tokens (Heimdall deep reasoning)
    now need `--max-tokens` raised.
- **Model revocation and operator disengagement act on a running mimir-server.**
  Revocation was consulted once, when the model loaded; a later revocation, a
  withdrawn model trust key or an operator's need to stop inference had no effect short
  of stopping the container. [VLD-ANB-MIDGARD-0057] [AC-67.1] [AC-67.2] [AC-67.3]
  [AC-67.4] [AC-MI-7]
  - Every `--reload-interval` seconds (default 60, at least 1) mimir-server looks for
    `STOP` in its locked state directory and, for a model engine, re-reads the
    revocation list (`revoked.list` there, or `--revocation-file`; one artefact id per
    line, `#` comments, absent means nothing revoked) and `--model-trust`, and
    re-checks the manifest it verified at start-up against them, signature and gates,
    without hashing the weights again (`ModelLoader::with_revoked`,
    `ModelLoader::reverify_manifest`, `parse_revocation_list`).
  - While `STOP` exists a permitted call is refused `SERVING_STOPPED`; while the
    model is revoked or its manifest no longer verifies (or either file cannot be
    read) it is refused `MODEL_REVOKED`. The refusal is made on WP-62's admission path
    (`InferenceHandler::with_serving_state(ServingState)`), audited as
    `Infer:MODEL_REVOKED` or `Infer:SERVING_STOPPED` and counted as
    `inference_refused_total{reason}`; `/readyz` reports `model-verified` not ready;
    the log says `serving stopped`, then `drained` once the completions admitted
    before the stop have returned (waiting at most `--infer-timeout`). Serving resumes
    (`serving resumed`) once every check passes again. The reference engine honours
    `STOP` only. A service started with `STOP` present is never ready.
  - There is no network control surface and no signal handling: the files in the
    state directory are the switch. `docs/ops/RUNBOOKS.md` gains RB-8, disengage
    Mimir inference, and RB-9, rotate or withdraw the model trust anchor; the compose
    file and Helm values pass `--reload-interval 60`.
- **The installer checks the console's security headers, and the deployment tier's
  PQ-mTLS probe no longer runs with group 0 or a published CA seed.** `mg-install`
  gains the step "Checking the web console sends its security headers", which fails
  unless the page and a static asset both carry every header in
  `console.security-headers.conf`, and the asset its Cache-Control. Both
  `pqmtls-probe.yaml` pods set `runAsGroup` and `fsGroup` 10001; the manifest holds
  `__CA_SEED__` in place of the constant seed, and `run-deployment-tier.sh` draws a
  seed for each run, logs it and refuses a manifest that still holds the placeholder.
  `tools/ci/tests/console-headers.test.sh` covers the step, and the running console
  image when docker has it; `tools/ci/tests/probe-manifests.test.sh` covers the probe.
  [VLD-ANB-MIDGARD-0064] [VLD-ANB-MIDGARD-0075]
- **The audit record says where a request came from, how it ended and why, and
  refusals are recorded.** [VLD-ANB-MIDGARD-0052] [VLD-ANB-MIDGARD-0053]
  [VLD-ANB-MIDGARD-0054] [VLD-ANB-MIDGARD-0072]
  - `AuditRecord` carries an `AuditContext`: source address, session, outcome
    (`Permit`, `Deny`, `Refused`, `Failed`, `Info`), reason code, and for a human gate
    the approval, approver and target. Ledger entries are written as entry schema 2,
    and a record with a context is hashed under its own canonical layout
    (`midgard-audit-record-v2`). Schema 1 ledgers still open and verify: their entries
    decode with an empty context, and an empty context hashes exactly as before, so
    every stored entry hash and checkpoint still holds. A binary from before this
    change cannot open a ledger holding schema 2 entries.
  - Governance row ids are the ledger position, `AUD-<seq>`, unique even within a
    second; `authorize` records the decision as the outcome, and `authorize_with` and
    `record_event_with` take a context.
  - The gateway takes each request's source from the accepted socket, gives it a
    16-hex reference, and writes one JSON log line per request (method, path, status,
    latency, source, reference, subject). Edge admission rows carry the source and
    session.
  - A missing or rejected bearer token is answered `{"error":"token rejected"}`
    whatever failed, counted in `http_auth_rejections_total{class}`, logged with its
    class and source, and written to the ledger as `AUTH rejected (<class>)` at most
    once a minute per source address.
  - Server errors read `{"error":"internal error","reference":"..."}`; the store,
    journal or crypto detail is logged under that reference. An administration
    privilege refusal reads `refused` and a failed administrator sign-in
    `unauthorised`.
  - A missing or invalid human gate, a self-approval and a decision on an approval
    that is not pending are written as `REFUSED Apply` or `REFUSED Approve`, with the
    reason code and target, after the edge's PERMIT row.
  - The Forge worker no longer ignores a failed run's audit write: the row carries
    outcome Failed and a sanitised error class, and a row the ledger refuses is
    logged as not audited.
- **A human gate's request, decision and redemption are bound in the signed ledger,
  and approvals are bounded and survive a restart.** [VLD-ANB-MIDGARD-0038]
  [VLD-ANB-MIDGARD-0039]
  - Each approval state change writes an `APPROVAL requested`, `approved`,
    `rejected`, `redeemed` or `expired` row on `approval:<id>`, carrying the approval
    id, approver, target and gated action; the fabric's row for a gated action
    (`Fabric::apply_action_with`) names the approval and its approver. `GET /audit`
    rows show `outcome`, `reason`, `approvalId`, `approver` and `target` where the
    row states them.
  - New state file `approvals.journal`: approvals are sealed under the audit root, one
    frame per state change, and replayed on boot, so a pending or approved gate
    survives a restart and a used one stays used. Back it up with the other journals
    (`docs/ops/BACKUP_RESTORE.md`).
  - The store holds at most 256 approvals and no longer drops the oldest whatever its
    state: a pending approval expires after 24 hours, expired entries are pruned and
    audited, rejected and used ones make room, and a request that finds no room, or
    a requester who already has 16 open requests, is refused with 429 and
    `Retry-After: 60`.
- **Logs, metrics, ledger rows and connector errors can no longer be broken or
  stuffed by what a caller or a source sends.** [VLD-ANB-MIDGARD-0033]
  [VLD-ANB-MIDGARD-0037] [VLD-ANB-MIDGARD-0065] [VLD-ANB-MIDGARD-0071]
  - JSON log lines escape every control character (`\r`, `\t`, `\u00XX` for the
    rest of U+0000 to U+001F and U+007F) and U+2028/U+2029 (`telemetry::json_escape`);
    Prometheus label values keep their own escapes and replace every other control
    character with U+FFFD (`telemetry::label_value`).
  - Ledger resources built from a request (the query body's `objectType`, the
    request path, the administration path) are stripped of control characters and
    capped at 256 characters.
  - A login with a username that matches no enrolled principal is recorded as
    `unknown:<16 hex>`, an HMAC-SHA256 under a new key file, `keys/actor-hash.key`
    (32 random bytes, 0600, created on first boot), so a password typed into the
    username box never reaches the ledger. Enrolled names and registered
    administrators are recorded by name as before. Back the key up with the
    keystores: after a restore without it, new hashes no longer match old ones.
  - `ConnectorError` has a stable `class()` (sandbox-denied, credential, parse, data,
    store, io); its display shows hosts and secret names only as `h:`/`s:` plus 8 hex
    of their SHA-256, absolute paths as file names, and source text control-free,
    capped at 120 characters. `diagnostic()` keeps the full text, which the Forge
    worker logs at debug level only with the new `--diagnostics` flag. The worker's
    failure log line gains `class`, and a connector failure's ledger row records
    that class. `Credential` and `Io` are now struct variants (`secret`/`target`
    plus `detail`); log consumers that matched hosts or secret names in error text
    must use the hashed references.
- **A certified feed can no longer freeze or replay an entity in the Heimdall
  picture.** [VLD-ANB-MIDGARD-0015]
  - `heimdall-feed` takes the receipt time from its own clock and fuses an
    observation only if its `observed_at` is at most `--max-observation-skew`
    seconds ahead (default 300) and at most `--max-observation-age` seconds behind
    (default 604800, seven days). Anything else is refused and written to the ledger
    as `Ingest refused: observed_at outside the accepted window`, outcome Refused,
    reason `observed-at-out-of-window`, target the entity key.
  - The last 4096 observation ids from each feed are remembered (in memory); a
    repeated id is refused as `Ingest refused: replayed observation`, reason `replay`,
    and is not counted again. The other-feed and signature refusals carry reason
    codes too (`another-feed`, `signature`).
  - Each track records `last_received`, the server's receipt time, and `ReadPicture`
    gives every entity `lastReceived` and `stale` (nothing received for longer than
    `--stale-after` seconds, default 900).
  - The picture delta and sealed picture layout is version 2, which carries
    `last_received`. Version 1 pictures still open (their `last_received` reads as
    `last_seen`), but a binary from before this change cannot open a version 2
    picture or merge a version 2 delta: replicating peers must be upgraded together.
    The observation wire format and its signature are unchanged.
  - A picture merge with equal `last_seen` keeps the larger `last_received`, then the
    greater canonical bytes, so two nodes converge whatever order they merge in.
  - `FusionEngine::ingest` and `ingest_all` take the receipt time; `Track::opened_at`
    and `Track::observe` take both times; `FeedService::new` takes `IngestLimits`.
  - Feeds that replay history older than seven days, such as a recorded backfill, are
    refused until `--max-observation-age` is widened. The compose and kind smoke tests
    now date their observations by the host clock. `mg-install` replays the bundled
    sample feeds, which keep the times they were recorded at, by restarting
    `heimdall-feed` with `--max-observation-age` wide enough for the oldest sample for
    the replay only, then restarting it with the default, whether or not the replay
    succeeded.
- **A captured Heimdall feed observation can no longer be replayed, and one feed can
  no longer submit another's.** [VLD-ANB-MIDGARD-0041]
  - Observations carry their feed's monotonic `sequence` (`Observation::with_sequence`)
    inside the signed bytes: the observation format is version 2, and a version 1
    observation is refused at decode ("unsupported observation format version 1 (no
    sequence)"). Every feed source must be rebuilt; Heimdall workloads and feed clients
    are deployed together. Pictures and deltas are not affected.
  - `heimdall-feed-gateway` gains `ReplayGuard` and `FreshnessPolicy`. After the
    signature verifies, and only then, `FeedGateway::authenticate` refuses sequence 0
    (`SequenceUnset`), a sequence not above the feed's last admitted one
    (`SequenceNotMonotonic`), an `observed_at` outside the window (`Stale`) and an id
    among the feed's last 4,096 (`Replayed`); a failed signature consumes no state.
    The window is WP-23's (`--max-observation-skew` 300 s, `--max-observation-age`
    seven days), now enforced through the gateway; `FeedGateway::with_replay_guard`,
    `with_clock` and `with_freshness` configure it.
  - `FeedGateway::authenticate_from` binds the observation's feed to the PQ-mTLS peer
    (its service id, or a peer `FeedRegistry::delegate` names, refused otherwise as
    `NotFromPeer`); `serve_feed_connection` uses it for every record.
  - `heimdall-feed` holds one guard across requests (it replaces WP-23's in-memory id
    ring) and seals each feed's last sequence in `feed-sequences.sealed` in its state
    directory before fusing, so a replay is refused after a restart too. A sequence
    refusal is audited as `Ingest refused: replayed observation`, reason `replay`; an
    unnumbered observation as `Ingest refused: observation has no sequence`, reason
    `sequence-unset`. `PictureState::with_sequences` and `SequenceFile` are new.
  - Feed sources number observations with `FeedSequencer` (from 1, after a saved
    value, or from the clock). `heimdall-source` takes `--sequence N` and otherwise
    numbers by the clock in nanoseconds, so `mg-install`'s sample replay and the
    compose and kind smoke tests, which run it once per observation, need no change.
    A feed that restarts its sequence below its last is refused until it continues
    above it or is re-registered; `docs/ops/RUNBOOKS.md` RB-10 says what each refusal
    means.

- **A hostile data source cannot crash or stall the Forge worker.**
  [VLD-ANB-MIDGARD-0009] [VLD-ANB-MIDGARD-0011] The connector HTTP client added a
  server-supplied chunk size to the body length with plain arithmetic, so a size of
  `ffffffffffffffff` wrapped past the 256 MiB body cap and panicked the worker; each
  chunk is now checked against what is left of the cap, and a size that does not fit
  is refused as a parse error. The SCRAM-SHA-256 client used by the PostgreSQL and
  Kafka connectors accepted any iteration count a server asked for, so one answering
  `i=4294967295` kept the scheduler hashing for hours; counts above 1,000,000 are now
  refused before any PBKDF2 work. A panic in one job's run is logged as a failed run
  and retried, rather than ending the worker. The release profile now sets
  `overflow-checks = true` across the workspace, so arithmetic on an untrusted length
  that does wrap panics instead.
- **A hostile count can no longer size an allocation in the handshake decoder or the
  bundle reader.** A ClientHello advertising more than 16 KEM suites or signature
  algorithms is refused as malformed before anything is reserved for its lists. The
  bundle reader bounds its entry and artefact counts by the bytes left in the file, so
  an unsigned header claiming four billion of either is refused as a truncated bundle
  by `read_bundle` and `from_bytes` instead of aborting the reader. A libFuzzer target
  for `decode_message` lives in `crypto/pq-mtls/fuzz`, outside the workspace and the CI
  gate, run on the release runner. [VLD-ANB-MIDGARD-0010] [VLD-ANB-MIDGARD-0044]
  [NFR-SEC-1]

### Tests
- **The edge's token refusals and the handshake decoder's negative paths are tested.**
  The secured edge answers 401, naming the reason and carrying no claims, for an
  expired token (and still serves it one second before expiry), a not-yet-valid token,
  a token signed by another IdP, a non-hex bearer, a token without the `Bearer`
  scheme or under `Basic`, and a token with trailing bytes; none of these writes an
  audit row. In `crypto/pq-mtls`, the four frames of a live handshake are recorded
  through a recording channel: every truncation and every trailing byte of each is
  refused, the record and handshake frame readers refuse a length one over their
  limit and a frame shorter than its prefix, a recorded ClientKeyExchange replayed to
  a fresh responder fails its signature, and an unknown tag or certificate-presence
  byte is malformed, on the test and FIPS backends alike. Ticket single use at the
  gateway was already proven by `a_second_connection_resumes_and_the_ticket_is_single_use`.
  [VLD-ANB-MIDGARD-0092] [VLD-ANB-MIDGARD-0093]
- **Images are deployed by digest, not by a tag that can be moved.**
  [VLD-ANB-MIDGARD-0012] The verifier, its key and the entrypoint travel inside each
  service image, so only the digest binds a pod to the signed build. The Helm chart
  gains `image.digests`, one per service, renders `<registry>/<service>@<digest>` and
  annotates each pod `midgard.veldris.io/image-pinned`; it refuses to render while a
  digest is empty or malformed unless `image.allowTag=true` (development only, pulled
  `Always`). `deploy/helm/set-digests.sh` sets the digests from the signed
  `images.lock`; the `release-images` job renders the chart with them and signs and
  publishes `chart/values.yaml` and `chart/enclosure.yaml`, and the signing-runner
  preflight now requires helm. `air-gap-bundle/install.sh` deploys the images it built
  by digest. The committed `deploy/k8s/enclosure.yaml` is now the development
  rendering (tags, `imagePullPolicy: Always`). The acceptance probes name an all-zero
  sentinel digest that `run-deployment-tier.sh` replaces with `MIDGARD_PROBE_IMAGE`,
  pinned by digest, or refuses to run.
- **PQ-mTLS session resumption is hardened before any service enables it.**
  [VLD-ANB-MIDGARD-0046]
  - The gateway sends the session ticket as the first sealed record of the session
    (`pq_mtls::NEW_TICKET_AAD`), never as a plaintext frame; a client opens it with
    its `RecordOpener` before its first request.
  - The responder checks the binder before the replay marker, so a ResumeHello from
    someone without the resumption secret cannot use up a client's ticket.
  - Every resumption runs a fresh ML-KEM exchange in the ticket's suite (the TLS 1.3
    `psk_dhe_ke` pattern): the binder covers the client's ephemeral key, and the
    traffic secret is HKDF with the transcript hash as salt and `RMS || shared
    secret` as input keying material. A resumed leg now has forward secrecy.
  - The ticket carries the client's whole certificate, and resumption re-runs
    `verify_cert` (CA signature, validity window, the revocation list in force).
  - Gateway ticket keys rotate once per ticket TTL with a key id in the ticket's
    first byte, keeping the previous key; the seen-ticket set holds SHA-256 digests
    until each ticket expires, at most 65 536, and refuses (and counts) resumptions
    beyond that, so the client falls back to a full handshake.
  - Wire change: tickets, ResumeHello and ResumeAccept are version 3, and
    `run_resume_initiator` / `run_resume_responder` take the crypto provider (and the
    responder a slice of ticket keys). Earlier tickets are refused and the client
    falls back to a full handshake; a client built before this change cannot use a
    resumption-enabled gateway. No deployment enables resumption.
- **Dual-control enrolment covers the credentials, not only the name.**
  [VLD-ANB-MIDGARD-0002] [VLD-ANB-MIDGARD-0066]
  - A request no longer carries a password hash or TOTP secret; one that does is
    refused. The gateway creates the credentials at approval and returns a single-use,
    128-bit activation code, valid for 72 hours, to the approver, who hands it to the
    principal out of band. The principal sends it with the password they choose to the
    new `POST /api/v1/activate` (throttled and audited like login) and receives their
    TOTP secret there; neither administrator ever holds the second factor, and an
    approved account cannot log in until it is activated.
  - Roles and purposes must be ones the served policy names, and compartments are
    letters, digits and hyphens; the listing shows the whole record, and the approver
    names the subject they approve (`mg-admin enrol approve --subject`).
  - A directory write that fails at approval leaves the enrolment parked for its
    approver to complete; `POST /api/v1/admin/enrolments/reject` (`mg-admin enrol
    reject --reason`) discards a waiting request. A gateway without a durable identity
    directory refuses requests with 501 instead of parking them.
  - `mg-admin enrol request` no longer reads a password or takes `--totp`. Identity
    directory records are now tagged and versioned; directories written earlier still
    open, but one written by this version does not open on an older build, so back up
    the state directory before upgrading. Waiting enrolments from an earlier build are
    kept without their credentials and are rewritten on first boot.
- **A one-time code logs in once.** [VLD-ANB-MIDGARD-0026] A TOTP code was accepted
  for any of three adjacent 30-second steps and nothing recorded its use, so an
  observed code could be replayed for up to 90 seconds to mint further one-hour
  tokens (RFC 6238 section 5.2). The identity directory now keeps, per principal, the
  step of the last code accepted, and a login refuses a code for that step or an
  earlier one with the same 401 as any other refusal. The step is written, as a small
  sealed record, before the directory lock is released and before the token leaves,
  so two concurrent logins with one code cannot both succeed; if it cannot be written
  the login fails with 500 and no token. A wrong password never consumes a code. The
  30-second step, six digits and one step of clock skew are unchanged. Someone who
  logs in twice within one step (two consoles, say) now waits for the next code.
  Directory records are now tagged version 3; version 1 and 2 records still open, but
  a directory written by this version does not open on an older build, so back up the
  state directory before upgrading. `iam::TotpSecret::verify_step` and
  `iam::Authenticator::authenticate_full` return the step; `verify` is unchanged.
- **Administrator MFA is two factors the gateway verifies.** [VLD-ANB-MIDGARD-0008]
  An administrator session counted as multi-factor whenever the passkey assertion's
  `userVerified` flag was true, a flag the client chooses and `mg-admin` always set,
  so possession of one passkey keystore file was enough to request, list and (with a
  second file) approve enrolments. The flag is now recorded for audit and trusted for
  nothing: a session satisfies MFA only with two distinct factors, one of them
  phishing-resistant, and every administration request (`enrolments`, `approve`,
  `reject`, `list`) must also carry `totp`, a one-time code the gateway checks against
  the administrator's secret in the new sealed `admin/totp.journal` and accepts once
  (the single-use step tracking above). A missing, wrong or reused code is 401
  "second factor required", audited as `ADMIN refused (401)`; enrolment events name
  the factors verified, as in `principal:olivia factors=totp+webauthn
  uv-claimed=false`. `mg-admin admin add` enrols the administrator's TOTP secret and
  prints its `otpauth://` URI once; `mg-admin admin totp-reset` replaces a lost one,
  audited as `ROTATE administrator totp`; the enrol commands take `--totp <code>` or
  read the code from standard input, and sign `userVerified` truthfully (false until
  the passkey keystore is wrapped under a passphrase). **Upgrading:** every existing
  administrator needs a secret before the upgraded gateway accepts their requests:
  stop the gateway and run `mg-admin admin totp-reset --state-dir <dir> --admin
  <name>` for each; the gateway's start-up line says how many have none.
  `AdminSession::record_factor`, `factor_names` and `uv_claimed` are new, and
  `AdminControl::open` takes the TOTP store.
- **The principal passkey verifier checks origin, user verification and the
  signature counter.** [VLD-ANB-MIDGARD-0084] `iam`'s WebAuthn assertion verifier read
  the client-data type and challenge by substring and did not check the origin, the
  user-verified flag or the signature counter (WebAuthn Level 2 section 7.2), so, once
  wired to a browser, it would have accepted a phished assertion from another origin
  or one from a cloned authenticator. Client data is now parsed at the edge
  (`http_gateway::webauthn_client_data::parse_client_data`, over the shared JSON
  parser) into `iam::ClientData`; `verify_assertion` takes a `WebAuthnPolicy` (exact
  origins, whether user verification is required) and returns the signature counter,
  which must increase unless the authenticator keeps none. The counter is persisted
  per passkey like the TOTP step, before the token leaves. The default policy names no
  origin and requires user verification, so no assertion verifies until a relying
  party configures one; user verification is never counted as a factor of its own. No
  browser flow uses the verifier yet, so nothing user-facing changes. Directory
  records are now tagged version 4; version 1 to 3 records still open, but a directory
  written by this version does not open on an older build, so back up the state
  directory before upgrading. `AuthAssertion` carries `client_data`,
  `WebAuthnCredential` carries `sign_count`, and `AuthOutcome` and `Login` carry
  `passkey_count`.
- **Untrusted text in a prompt is data, never an instruction or a header.** Every
  prompt assembler concatenated grounding facts, memory, history, user messages and
  feed, ledger, schema and page fields into the model input verbatim, so anyone who
  could write an object property, submit an observation or share a page selection
  could forge a `=== SYSTEM ===` block. [VLD-ANB-MIDGARD-0080] [FR-MI-7]
  - `mimir_prompts::delimit` (no dependencies) renders untrusted text as a DATA block:
    `DATA label= source= boundary=`, every line indented two spaces, C0 controls
    (bar newline and tab), DEL, U+200B to U+200F, U+2028, U+2029 and U+FEFF removed,
    the boundary neutralised inside the data (`[boundary-removed]`), each field
    bounded at 16 KiB with a `[truncated N bytes]` marker, then `END boundary=`.
    Headers count only as whole lines at column zero. `delimit::Boundary` is the
    per-request token, drawn from the OS CSPRNG for every prompt.
  - The agent runtime puts grounding (`fabric`), memory (`agent-memory`), history
    (`task-history`) and the user message (`user`) in DATA blocks, appends the
    verbatim `mimir_prompts::DATA_HANDLING_RULE` to the SYSTEM block, and returns
    `AssembledPrompt::system_text` and `body_text` beside `text` for role-separated
    delivery. `Pipeline::with_boundary` fixes the boundary for tests. Grounding fact
    lines pass every field through `delimit::inline`. The marker lines are counted,
    so the token accounting stays exact; each untrusted section costs two more lines
    and the rule about 70 words, so a budget near its limit may trim one more fact.
  - Heimdall decision support (`picture`, `operator`), the admin copilot
    (`admin-state`, `admin`), simulation explainer (`policy-simulation`, `admin`) and
    audit assistant (`ledger`, `admin`), Forge AI assist (`dataset-schema`, `user`),
    Bifrost release risk (`release`), the on-device assistant (`local-store`, `user`)
    and the extension broker (`web-page`, `enclosure`) use the same renderer and
    expose `system_text` and `body_text`. Trusted sections (the transform library; the
    copilot's role, policy and contract; the on-device and broker policy, tier and
    contract) now sit with the system prompt, ahead of the data. Entity fields,
    column names, release facts and context ids stay on one line. The tier-selection
    prompt holds only values the client computes and is unchanged.
  - The rule given to the model is advisory; the indentation, the boundary and the
    character cleaning are the controls. The broker (no crypto dependencies, KDD-2)
    and the client inference crate (no CSPRNG under its test backend) draw their
    boundary from the standard library's randomly keyed hasher
    (`Boundary::from_std_entropy`).
- **The agent guardrails screen what could carry an attack, and nothing else.**
  [VLD-ANB-MIDGARD-0077] [VLD-ANB-MIDGARD-0078] [FR-MI-7]
  - The layer-8 input guardrail ran over the whole assembled prompt, so a data word
    in a grounding fact or memory item ("rotate the credentials") blocked the turn
    for everyone who retrieved it, and any 40-character alphanumeric run (an object
    id, a hash) produced a sanitise rewrite that deleted long tokens and collapsed
    newlines across the SYSTEM, OUTPUT_CONTRACT, POLICY and TOOLS blocks. It now
    screens each grounding fact line, memory item, history turn and the user
    message before composition; a block names the layer and a sanitise verdict
    replaces only that item. The trusted layers are never inspected or rewritten,
    so `AssembledPrompt::system_text` is always the SYSTEM block.
  - The `SelfDefending` pre-filter exempts hexadecimal runs and `obj:`, `entity:`,
    `feed:`, `track:` and `sha256:` identifiers, and treats a run as encoded only at
    64 or more base64 characters including a `+`, `/` or `=`; its rewrite works line
    by line and keeps newlines. A named secret (`api key`, `secret key`,
    `credentials`) blocks only with a request to disclose it ("print the api key"),
    and "ignore prior" and "disregard prior" join the override phrases.
    `SelfDefending::verdict_rates` reports the sanitise rate beside the block rate.
  - The governed loop's output guardrail inspected only the answer and inferences;
    it now inspects `AgentOutput::to_contract_json`, so every proposed action's type,
    parameters, rationale and citations are screened, and a sanitise verdict on
    output is a block. Before the gate, `ActionCatalog::validate` refuses an
    undeclared action type, a parameter key outside the action's declared keys, a
    value over its bound (4096 bytes by default) and a key or value with control
    characters, halting with `HaltReason::ActionRejected` and nothing gated or
    executed. `ActionMeta` gains `params` and `max_value_len`;
    `ActionCatalog::with_schema` declares them and `with` declares no parameters;
    `LogicBuilder::params` declares a tool's keys. `GovernedLoop::new` now wires
    `SelfDefending` as both input and output guardrail. An action catalogue that
    declared an action with `with` and proposals that carry parameters must move to
    `with_schema`.
  - The `Classifier` seam has no implementation in the library; the only ones are
    test doubles.
- **The system prompt reaches the model in its own role, and model control tokens in
  any text are neutralised.** The family adapters rendered the whole prompt, system
  prompt and policy included, as one user turn, and llama-server parses special
  tokens in a raw prompt, so text holding a family's control tokens (`[/INST]`,
  `<|end_of_text|>`, `<|start_of_role|>system<|end_of_role|>`) could close the user
  turn and open a system or assistant turn. [VLD-ANB-MIDGARD-0060] [FR-MS-7]
  - `InferenceRequest` carries `system_prompt` beside `prompt` (on the wire straight
    after `purpose`; a frame from an older consumer is refused as malformed). The
    agent runtime sends `AssembledPrompt::system_text` and `body_text`; the Heimdall
    decision-support, admin copilot, Forge and Bifrost bridges send their assembler's `system_text` and
    `body_text`. `mimir-probe --system TEXT` sets it.
  - `FamilyAdapter::render(system, user)` puts the system text in the family's system
    turn (Granite, Nemotron) or first in the single user turn, a blank line before
    the user text (Mistral, Gemma). An empty system text renders the user text alone,
    as before.
  - Each adapter declares its family's control vocabulary (`special_tokens`), and
    `neutralise` puts a backslash before the final character of every occurrence in
    system and user text (`[/INST]` becomes `[/INST\]`), so the only control tokens
    sent to the engine are the template's own. Text that legitimately holds a token
    shows the backslash. The reference engine, and a sidecar with no adapter, read
    the system text and the prompt as one, as before.
- **The marking of a prompt reaches the policy decision, and every served completion
  leaves its prompt and completion digests in the ledger.** `mimir-server` decided
  `Infer` with a fixed UNCLASSIFIED clearance against an UNCLASSIFIED resource, so a
  prompt holding marked or compartmented grounding could not be refused, and neither
  prompt nor completion left any trace. [VLD-ANB-MIDGARD-0034] [FR-MI-8]
  - `InferenceRequest` carries `marking`, the marking of the data in its prompt: on
    the wire after `max_tokens`, a level byte, a `u32` count and each compartment as
    a length-prefixed string in ascending order. An unknown level, a repeated or
    unsorted compartment, or a compartment name outside `[A-Za-z0-9._-]{1,64}` is
    malformed, and a frame from an older consumer, without the marking, is refused.
  - The handler authorises each call against the inference resource with its
    marking raised to the request's, and the model's accreditation as the clearance:
    `mimir-server --model-marking LEVEL[:COMPARTMENT,...]` (levels `UNCLASSIFIED`,
    `CONFIDENTIAL`, `SECRET`, `TOP_SECRET`), default `UNCLASSIFIED`, which refuses
    every marked prompt. The refusal is the call's `DENY Infer` entry. The serving
    log line names `model_marking`; compose and the Helm chart pass
    `--model-marking UNCLASSIFIED` (`modelServing.modelMarking`).
  - After a served completion the handler writes a second entry, `Infer:COMPLETE
    decision_seq=… marking=… model=… prompt_sha256=… completion_sha256=…
    prompt_tokens=… completion_tokens=…`, where the prompt digest is over the system
    text, a blank line and the body. If that entry cannot be written the caller gets
    `AUDIT_FAILURE`, not the completion. A served call is now two ledger entries.
  - The agent runtime marks its request with the highest level among the included
    grounding facts (`AssembledPrompt::max_marking_level`) and the caller's
    compartments; the Heimdall, Forge, Bifrost and admin bridges take the caller's
    posture through `with_marking` (default UNCLASSIFIED). `mimir-probe --marking`
    sets it.
  - The prompt still crosses the loopback to llama.cpp in clear inside the network
    namespace mimir-server shares with it; that is now a recorded trust assumption.

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
