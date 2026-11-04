# CA / PKI & Certificate Lifecycle

How the PQ-mTLS mesh's certificate authority is bootstrapped and operated: issuance,
renewal, revocation, the HSM key ceremony, rotation cadence, optional threshold
signing, and the release **signing-runner** provisioning. All in-enclosure, all
post-quantum (ML-DSA / FIPS 204). Implemented in `services/iam` (`MeshCa`) over
`crypto/pq-mtls`.

## Model (why it looks like this)

Midgard's mesh CA is the IdP's ML-DSA key. It issues **short-lived, rekeyed workload
certificates that carry their own terms**, and a **signed revocation list** that every
service holds beside its identity:

- **A certificate carries its terms.** Each one holds a serial, a validity window
  (`not_before`, `not_after`) and the clearance the service may be authorised at, all
  inside the CA's signature (format `MGCERT2`). `verify_cert` checks the window on
  every handshake, so an expired certificate fails the handshake on either side
  (`PeerCertExpired`), and a resumption ticket never outlives the certificate it was
  issued under. [MG-SEC-02]
- **The clearance is the certificate's, not the caller's.** A server authorises the
  lower of what the certificate grants and what the client asks for (the lower level,
  the shared compartments), and keeps it on the session. [MG-SEC-04]
- **A client names the peer it means to reach.** Every mesh call carries the expected
  service; any other certificate, however valid, is refused before the client sends
  its key exchange (`PeerIdentityMismatch`). [MG-SEC-01]
- **Revocation is a list the CA signs.** `mesh.crl` (format `MGCRL1`) holds the
  SHA-256 fingerprints of revoked certificates and a version number. Every service
  needs it to start, re-reads it every 30 seconds, and refuses a list the CA did not
  sign or one older than the list in force (`CrlUntrusted`, `CrlRollback`). Since
  renewal **rekeys** (a new certificate is a new key), a fingerprint names exactly one
  certificate, so revoking one never revokes its renewals. [MG-SEC-03]
- The **CA registry** (`MeshCa`) records what was issued, its window and its status;
  `status(serial, now)` and `expiring_by(deadline)` drive a renewer.

## Bootstrap

1. **Root/CA key ceremony** (below) provisions the CA (IdP) ML-DSA signing key. In a
   deployment today that is `mg-admin init`, which writes the issuer keystore to the
   state directory's `keys/`.
2. Publish the CA **trust anchor** (`mesh.anchor`, format `MGANCHOR2`: the algorithm
   and the public key) to every workload — it is the mesh's single root of trust —
   together with the current `mesh.crl`.
3. Each workload's own keypair is generated for it and never leaves its identity
   directory.

## Issuance

```bash
mg-admin mesh issue --state-dir STATE --service mimir-server --roles inference-server     --out /srv/mimir-identity [--days 30] [--clearance open] [--compartments A,B]
```

writes the service's identity keystore, a separate audit keystore, its certificate
(valid for `--days`, default 30, at most 365, granting `--clearance`, default `open`),
`mesh.anchor`, and the current `mesh.crl`. It never overwrites an identity, and the
issue is recorded in the enclosure's audit ledger before anything is written. In the
library: `MeshCa::issue(identity, not_before, not_after, clearance)`.

## Renewal (downtime-free)

```bash
mg-admin mesh renew --state-dir STATE --service mimir-server --out /srv/mimir-identity     --within 10
```

gives the service a **new** identity key and a fresh certificate, keeping its roles and
its audit keystore. With `--within N` it does nothing until the certificate expires
within N days, so it can run daily from a timer. Restart the service to present the new
certificate. The old certificate keeps working until it expires or is revoked, so no
handshake fails during the swap (verified in `services/iam/tests/rc_35.rs`).

- **Cadence:** renew at about two-thirds of the window — for 30-day certificates, a
  daily `mesh renew --within 10`. Services do not yet renew themselves (RF-IAM-6).

## Revocation (honoured within seconds)

```bash
mg-admin mesh revoke --state-dir STATE --cert /srv/fleet-clients/fleet-agent-1.cert     --out /srv/mimir-identity,/srv/fleet-identity,/srv/heimdall-identity,/srv/gateway-identity
```

adds the certificate's fingerprint to the list, signs the next version, keeps it as the
master copy in the state directory's `trust/mesh.crl`, and writes it into every `--out`
directory. Name **every** service directory: a service only refuses what its own copy
lists. Each running service picks the newer list up within 30 seconds, without a
restart; the next handshake with that certificate fails with `PeerCertRevoked`, and a
resumption ticket issued to it no longer resumes. Running it again changes nothing.
Sessions already open are not cut; to end them, restart the peer.

On Kubernetes the list is part of each `<service>-identity` Secret; update the Secret's
`mesh.crl` key and the kubelet updates the mounted file.

## Upgrading identities from an earlier release

Certificates and anchors written before certificates carried terms are refused, and
there is no `mesh.crl` beside them. Bring each service directory up to date in place:

```bash
mg-admin mesh upgrade --state-dir STATE --service mimir-server --roles inference-server     --out /srv/mimir-identity
```

It reissues the certificate for the key the service already holds and rewrites
`mesh.anchor` and `mesh.crl`. It keeps **both keystores**: the audit keystore signs the
service's ledger and seals its state, so a new one would lock the service out of both
(which is why `mesh issue` is not the way to upgrade). On a current service it changes
nothing. The whole procedure is in [UPGRADING.md](UPGRADING.md).

## Signing roots (two, deliberately)

Release signing and Bifrost registry admission use **separate roots at different
strengths**. This is a settled decision, not an oversight — the mismatch is load-bearing,
so do not "fix" it by aligning the algorithms.

| Root | Algorithm | Constant | Signs | Anchor |
|------|-----------|----------|-------|--------|
| Release signing | ML-DSA-87 | `kms::RELEASE_SIG_ALG` | release artefacts, SBOMs, in-toto provenance (`mg-sign`) | `deploy/trust/release-trust.pub` |
| Bifrost admission | ML-DSA-65 | `bifrost_artefact::BIFROST_SIG_ALG` | admission of an artefact to the artefact registry (`mg-register`) | the registry's own trust key |

Why separate:

- **Compartmentalisation.** Compromising the registry's admission key does not yield a
  valid release signature, and compromising the release key does not admit anything to a
  fleet. Two independent gates, two independent custody chains.
- **Different jobs.** The release signature attests *this binary was reproducibly built
  from this source*, and is verified by anyone, forever — hence the level-5 parameter set.
  Admission attests *this artefact is eligible for rollout in this enclosure*, verified
  by the registry against its own anchor; level 3 is the deliberate floor there.

Operationally this means **two keystores**, and `mg-register` refuses a keystore whose
algorithm does not match `BIFROST_SIG_ALG` rather than producing a signature the registry
would reject. Mint the admission key with:

```bash
mg-sign keygen --alg ML-DSA-65 --keystore <registry-admission.keystore>
```

The release runner needs both: `MIDGARD_SIGNING_KEYSTORE_PATH` (ML-DSA-87) for the gate,
and `MIDGARD_BIFROST_KEYSTORE_PATH` (ML-DSA-65) plus `MIDGARD_BIFROST_REGISTRY_PATH` for
the handoff. Neither has a default; a runner missing either fails the release.
`[FR-B-1]` `[FR-B-4]` `[AC-B-1]` `[NFR-PQ-3]`

## HSM key ceremony (CA + release signing key)

**To hold the ceremony**, follow [KEY_CEREMONY.md](KEY_CEREMONY.md): with this
repository's own tools ("Your own ceremony"), or with Dvalin where you have it (Veldris's
ceremony application, available internally and to enterprise customers, with its own
procedure). Neither needs a forge or a network. Dvalin is a guided Windows (and Linux) app
that generates the release signing key (ML-DSA-87) and the Bifrost admission key
(ML-DSA-65) into sealed keystores outside the checkout, proves each signs and verifies
through this repository's `attest`, writes `deploy/trust/release-trust.pub`,
`bifrost-trust.pub` and `allowed_signers`, checks them as the release will, and writes
the ceremony record under `docs/ops/ceremonies/`. It can rehearse with throwaway keys
first, can optionally split an escrow key among named holders (k of n, with
`pqc_agility::shamir`) for recovery, and commits nothing. Until custody is hardware,
it stands in for steps 2 to 5 below with software keystores; step 6 remains the
officers'.

The hardware target, for when custody moves to an HSM. Run with the duty Security
Officer and a second witness (dual control), recorded in the audit chain:

1. **Prepare** a FIPS 140-3 HSM (or TPM 2.0 with a PQ command set); confirm firmware +
   attestation (RT-28).
2. **Generate** the CA ML-DSA key **on-device**, `CKA_EXTRACTABLE=false` / `fixedTPM` —
   the private key never exists in host memory.
3. **Attest** the key is device-resident and non-exportable; record the attestation.
4. **Export the public half** only; commit it as the trust anchor
   (`deploy/trust/release-trust.pub` for release signing; the mesh anchor for PQ-mTLS).
5. **Escrow** the recovery/quorum material per DR (see DR_PLAN.md step 4) — without it,
   DR cannot unseal or re-issue.
6. **Witness sign-off**: both officers confirm; the ceremony is sealed into the audit
   chain.

## Git signing (tags and commits)

Artefacts are signed with ML-DSA; git cannot verify that, so tags and commits are
signed with SSH keys, and the ceremony publishes which ones count.

1. Each person who may cut a release tag, and each maintainer, holds an SSH signing
   key (ed25519) in a custodian of their own — a hardware key where available — and
   configures git with it (`gpg.format = ssh`, `user.signingkey`, `commit.gpgsign` and
   `tag.gpgsign` true; CONTRIBUTING.md, "Branches and the public tree").
2. The ceremony collects the public halves and commits them as
   `deploy/trust/allowed_signers`, in git's allowed-signers format, each for the
   namespace `git`: the release tag keys and the maintainers' commit keys, one line
   each, under the email the person commits as:

   ```text
   release@example.org namespaces="git" ssh-ed25519 AAAA...
   maintainer@example.org namespaces="git" ssh-ed25519 AAAA...
   ```

3. The fingerprint of each key (`ssh-keygen -lf key.pub`) is published through a
   second channel, so a reader can tell the committed file was not swapped.
4. The same keys are registered on the forge as signing keys, so the branch rule that
   requires signed commits accepts them, and each maintainer turns on *vigilant mode*
   on GitHub, so a commit that claims to be theirs without their signature shows as
   unverified.

`tools/ci/verify-tag.sh` verifies a release tag against that file, and the release's
`release-tag` job runs it before anything is built: with no file, no tag verifies.
The `commits` CI job (`tools/ci/verify-commits.sh`) verifies every commit a pull
request or push brings against the same file, as the base of the change has it, and
fails closed while it lists no key (VLD-ANB-MIDGARD-0069).

The file is changed only by a pull request whose commits are signed by a key already
listed there, reviewed by the other maintainer (CODEOWNERS names both for
`/deploy/trust/`, and the ruleset requires a code owner's approval). Because the job
reads the file as the base has it, a key takes effect for the commits after the change
that adds it is merged, never for that change itself. Rotating a key is such a change:
add the new line, and remove the old one once nothing still to be merged is signed by
it. A removal is recorded in `CHANGELOG.md` with the key's fingerprint and the reason.
The first keys go in the same way, verified by GitHub instead (the job accepts GitHub's
verification while the file lists no key); until they are in, the job fails on CodeLab.

On the signing runners the committed file is not the authority: a commit that added a
key would otherwise verify a tag on itself. The runner holds its own copy, and the tag
verifies only when the committed file is byte-for-byte that copy (see "Signing runner",
item 6). So a key change takes effect for releases only once the release manager has
put it on the runners too.

## Rotation cadence

| Key | Cadence | Mechanism |
|-----|---------|-----------|
| Workload certs | 30 days by default (`--days`), renew at ~⅔ | `mg-admin mesh renew --within` on a timer (rekey) |
| Mesh CA key | annually, or on suspicion | new ceremony → dual trust-anchor overlap → re-issue workloads → retire old CA |
| KMS trust root | per RT-28 policy | `RotatingKms::rotate_to` (old blobs unchanged) |
| Release signing key | annually | ceremony → update `deploy/trust/release-trust.pub` |
| Bifrost admission key | annually | ceremony → re-open the registry under the new anchor (every resident artefact is re-verified on open, so rotation re-proves the whole registry) |

CA rotation is downtime-free the same way workload renewal is: publish the new CA
anchor alongside the old (verifiers trust both during overlap), re-issue workloads
under the new CA, then drop the old anchor.

## Optional: threshold / quorum signing

For the highest-value keys (CA root, release key) the ceremony **may** split signing
authority so no single HSM/officer can sign alone:

- **n-of-m HSM quorum** — the signing operation requires quorum approval across m HSMs;
  no single device holds a usable key. Maps onto the dual-control the platform already
  enforces (RT-31): a release/CA signature becomes a guarded action needing distinct
  approvers.
- Status: **documented, optional.** The current implementation is single-HSM with
  dual-control *ceremony*; threshold signing is a hardware/HSM-config upgrade behind the
  same `MeshCa` / signer seam (no code change above it).

## Signing runner (release)

The release jobs (`.github/workflows/release.yml`) run on self-hosted `[self-hosted,
kms]` runners. **Provision them** as follows. The workflow's *Signing-runner preflight*
job (`tools/ci/release-preflight.sh`) asserts each item and fails closed with a pointer
here if any is missing:

0. **A current runner.** The workflows' actions run on Node.js 24 and need GitHub's
   runner 2.327.1 or later (`./config.sh --version` on the runner).
1. **Pinned toolchain image.** Build `midgard-acceptance:1.82.0` from
   `deploy/acceptance/Dockerfile` (Rust `1.82.0` and `just`, on a digest-pinned base),
   and pull both bases `deploy/images/service.Dockerfile` names: `BASE`, the toolchain
   image its prep stage lays files out on, and `RUNTIME`, the distroless
   `cc-debian12:nonroot` image it runs on. Pull the chart's seed image too
   (`seedImage` in `deploy/helm/midgard/values.yaml`, distroless
   `base-debian12:debug-nonroot`, by digest), which the enclosure's registry must
   carry for first start. Do all of this while connected. Every build and signing step runs in that image with
   `--network none`. The runner itself only reaches GitHub.
2. **Offline advisory database.** The RustSec database is vendored at
   `vendor/advisory-db`, with the licence policy and exception tables in
   `supply-chain/`. The scan stage (`mg-scan`) reads nothing else.
   Helm, on the runner itself, renders the release chart with its images pinned by
   the digests `deploy/helm/set-digests.sh` lifts from the signed `images.lock`.
3. **Key material, as runner secrets naming files outside the checkout:**
   - `MIDGARD_SIGNING_KEYSTORE_PATH`: the release keystore (ML-DSA-87);
   - `MIDGARD_BIFROST_KEYSTORE_PATH`: the Bifrost admission keystore (ML-DSA-65), for
     registry admission and the air-gap bundle;
   - `MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE` and
     `MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE`: the passphrase each keystore's custody
     root is wrapped under, a separate secret from the keystore;
   - `MIDGARD_BIFROST_REGISTRY_PATH`: the registry directory.

   Each keystore file and its passphrase file is mounted read-only into the container
   that uses it. A passphrase file is a separate secret from its keystore: on GitHub a
   repository secret, on GitLab a protected CI/CD variable, each of exactly the name
   above, holding only the file's path on the runner. The workflows map the path and
   nothing else; no workflow, script or log prints a passphrase, and the preflight
   checks only that each file is readable and outside the job's workspace. A Dvalin
   ceremony hands the two passphrases over as `release.passphrase` and
   `bifrost.passphrase`; put them on the runner owner-only, outside every checkout
   (for example `/srv/midgard-passphrases/`), and point the two secrets at them.
4. **Pinned trust anchors.** The public halves from the ceremony, committed as
   `deploy/trust/release-trust.pub` and `deploy/trust/bifrost-trust.pub` (see
   `deploy/trust/README.md`).
5. **Nothing outlives a job.** No checkout, token or build output may be left beside
   the key material for the next job. Install a job-completed hook that removes the
   work directory (`ACTIONS_RUNNER_HOOK_JOB_COMPLETED` in the runner's `.env`, naming
   the script), and better still run the runner ephemeral (`./config.sh --ephemeral`,
   re-registered for each job) as well. The preflight asserts the hook: on a self-hosted
   runner it fails closed unless that variable names a readable script. The workflows'
   checkouts set `persist-credentials: false`, so the job token is not written into
   `.git/config` either (VLD-ANB-MIDGARD-0081). Run it **ephemeral**: the preflight
   cannot see that a runner was registered with `--ephemeral`, so it is the owner's to
   confirm.
6. **The runner's own `allowed_signers`, outside the checkout** (VLD-ANB-MIDGARD-0035).
   A release tag is verified against signers the tagged commit cannot choose. The
   release manager copies the ceremony's `allowed_signers` onto each runner, outside the
   work directory and beside the keystores, checks it against the committed file
   (`sha256sum` of both must agree), and names it with the runner secret
   `MIDGARD_ALLOWED_SIGNERS_PATH` (on GitLab, the CI/CD variable of that name).
   `tools/ci/verify-tag.sh` then refuses a tag unless the committed
   `deploy/trust/allowed_signers` is byte-for-byte that copy: "the committed
   allowed_signers differs from the runner-pinned copy; a key change must be
   provisioned on the runner first". On a self-hosted runner with the variable unset it
   refuses every tag. Rotating a key is therefore two steps: the signed, reviewed
   commit, then the same file on every runner. The preflight asserts the variable names
   a non-empty file of public keys outside the job's workspace, and that every keystore
   lies outside the workspace too.
7. **A gate that is not the tag's.** The release starts from a pushed `v*` tag, but the
   checks that decide whether anything else runs do not come from it: the preflight job
   checks out `main` a second time into `.ci-trusted` and runs `release-tag` and
   `release-preflight` from there, judging the tagged checkout. Only then do the tagged
   commit's own scripts run, by then proven signed and on `main` (`verify-tag.sh` refuses
   a tag whose commit is not an ancestor of `origin/main`). The preflight also asserts
   that the docker daemon answers and that `tools/ci/in-toolchain.sh` starts every build
   with `--network none`.
8. **Only release managers create release tags.** The tag ruleset that admits a `v*`
   tag is the forge's to enforce, not the repository's: on GitHub, the "release tags are
   created by release managers" ruleset (`tools/ci/github-rules.sh`) with the
   `release-managers` team as its only bypass actor; on GitLab, `v*` protected so that
   only maintainers create one (`tools/ci/gitlab-rules.sh`). Nothing on the runner can
   check this; the exported ruleset evidence in `docs/compliance/evidence/` is the
   record. Without a forge there is no ruleset, and the control is `allowed_signers`
   itself: list only release managers' keys in it, and `verify-tag.sh` refuses a tag
   anyone else signed ([Releasing without a forge](#releasing-without-a-forge)).

If any is absent, the preflight **fails closed**, so a release never ships unsigned,
unscanned, or from a half-provisioned runner.

**Custody today is software.** The keystores are files: the signing key sealed under
an ML-KEM-1024 root, and the root wrapped under an operator passphrase with
PBKDF2-HMAC-SHA256 (600,000 iterations, a fresh 32-byte salt) and AES-256-GCM, bound to
the whole header: the key id, the verifying key and the root public key (keystore format
version 3, `crypto/attest/src/keystore.rs`). Whoever can write the file but lacks the
passphrase cannot swap the verifying key or the root public key: the root no longer
unwraps, and the unsealed signing key must sign under the stored verifying key. The file
alone, or a backup of it, does not yield the root; the file and its passphrase
together are the key, so keep them in separate secrets. The format also takes a root
sealed to an external `kms::TrustRoot` (`RootWrap::External`), the seam an HSM or TPM
slots into, tested against the SoftHSM stand-in. The HSM path above (keys generated
on-device, never exportable) is the GA target and is not wired in: signing from a
PKCS#11 token needs a signer behind the same custody seam. Keystores written before
format version 2 hold the root in the clear, and version 2 keystores do not authenticate
their header; every reader refuses both, `mg-admin keystore migrate` converts each once
to version 3, and every other copy of the old file (backups, volumes, Secrets) must then
be destroyed. Migration keeps the same root, so where a version 1 copy may have been
exposed, give the keystore a new root with `mg-admin keystore rotate-root --keystore
FILE [--state-dir DIR]`: the signing key and verifying key are kept, so trust anchors
stay valid, and the new file must unseal the same key under a different root before it
replaces the old one. That is for keystores whose root seals nothing but their own key:
release, admission and model-signing keystores, a mesh service's identity keystore
(`<service>.keystore`) and an administrator's passkey. A state directory's
`keys/*.keystore` and a mesh service's `<service>.audit.keystore` seal the enclosure's
data (identity directory, ledgers, object store) under their root, so `rotate-root`
refuses them; re-initialise the enclosure instead. [VLD-ANB-MIDGARD-0007]

## Releasing without a forge

No step of signing or releasing Midgard needs a forge: not a connection to one, not
its API, not a signature or a "verified" mark it makes, and not its pipeline. GitHub
and GitLab are conveniences that run the same scripts; with neither, or with both
offline, a release is made on one machine. [VLD-ANB-MIDGARD-0035]

**The machine** is provisioned as the signing runner above, without the forge's runner:
the toolchain image built locally (`bash tools/ci/run.sh toolchain`), the vendored
advisory database, pnpm with its store filled (or the `js-build` output carried in),
each keystore and passphrase file outside every checkout, and its own copy of
`allowed_signers` outside the checkout. Set the same variables a pipeline maps from its
secrets, and name `main` locally, since there is no remote to fetch it from:

```bash
export MIDGARD_SIGNING_KEYSTORE=/srv/midgard-keys/release.keystore
export MIDGARD_SIGNING_KEYSTORE_PASSPHRASE_FILE=/srv/midgard-passphrases/release.passphrase
export MIDGARD_BIFROST_KEYSTORE=/srv/midgard-keys/bifrost.keystore
export MIDGARD_BIFROST_KEYSTORE_PASSPHRASE_FILE=/srv/midgard-passphrases/bifrost.passphrase
export MIDGARD_BIFROST_REGISTRY=/srv/midgard-registry
export MIDGARD_ALLOWED_SIGNERS_PATH=/srv/midgard-trust/allowed_signers
export MIDGARD_MAINLINE_REF=refs/heads/main
```

**Then**, on the commit to release, checked out:

```bash
bash tools/ci/tag-release.sh                    # the signed tag, verified
bash tools/ci/run.sh release-offline v1.2.3     # every release step, in order
```

`release-offline` runs what the release workflow's `preflight`, `release` and `publish`
jobs run, in their order: the tag gate, the preflight, the gate record, the numbers, the
JavaScript build, the gate and Bifrost admission, the images, the air-gap bundle, and a
local publish. It stops at the first step that fails. The publish writes every file a
forge would attach, with a `SHA256SUMS`, to `target/release-publish/<tag>/` (or
`MIDGARD_RELEASE_DIR`), and never writes over a release already there. Carry that
directory to the enclosure; `mg-verify` checks each binary against
`deploy/trust/release-trust.pub` there, again with no forge.

**What changes without one.** `MIDGARD_MAINLINE_REF` names `main` locally, so "on
`main`" means the `main` of that machine: keep it the reviewed history. Who may release
is decided by `allowed_signers` alone (item 8 above). There is no runner to clean up
after a job, so the preflight's job-completed hook check does not apply: use a machine
for nothing else, and remove the checkout's `target/` when the release has been copied
off. Publishing the same release on a forge later is optional: push the tag and let its
pipeline run, or attach the published directory's files by hand.

## Tested

`services/iam/tests/rc_35.rs` exercises the lifecycle over a **real PQ-mTLS mutual
handshake**: issue → handshake ✓ → renew (rekey) → both new and old certificates
handshake ✓ (downtime-free) → revoke old → the next handshake with it is rejected
`PeerCertRevoked` while the renewed certificate still connects; the signed list carries
a revocation to verifiers elsewhere; an expired certificate no longer handshakes.
`crypto/pq-mtls/tests/identity_files.rs` covers the window, edited terms, forged and
older lists and live reload; `tools/mg-admin/tests/mesh.rs` drives `mesh issue`,
`renew`, `revoke` and `upgrade` through the real binary.
