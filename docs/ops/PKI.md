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

**To hold the ceremony, use Dvalin** (veldris-ltd/dvalin-app), and
follow its `docs/KEY_CEREMONY.md`, step by step; [KEY_CEREMONY.md](KEY_CEREMONY.md)
here says what it leaves in this repository. It is a guided Windows (and Linux) app
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

1. Each person who may cut a release tag holds an SSH signing key (ed25519) in a
   custodian of their own — a hardware key where available — and configures git with
   it (`gpg.format = ssh`, `user.signingkey`).
2. The ceremony collects the public halves and commits them as
   `deploy/trust/allowed_signers`, in git's allowed-signers format, each for the
   namespace `git`:

   ```text
   release@example.org namespaces="git" ssh-ed25519 AAAA...
   ```

3. The fingerprint of each key (`ssh-keygen -lf key.pub`) is published through a
   second channel, so a reader can tell the committed file was not swapped.
4. The same keys are registered on the forge as signing keys, so the branch rule that
   requires signed commits accepts them.

`tools/ci/verify-tag.sh` verifies a release tag against that file, and the release's
`release-tag` job runs it before anything is built: with no file, no tag verifies.
Rotating a key is a commit to `deploy/trust/allowed_signers`, itself signed by a key
already listed there.

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
   and pull the service image base `deploy/images/service.Dockerfile` names. Do both
   while connected. Every build and signing step runs in that image with
   `--network none`. The runner itself only reaches GitHub.
2. **Offline advisory database.** The RustSec database is vendored at
   `vendor/advisory-db`, with the licence policy and exception tables in
   `supply-chain/`. The scan stage (`mg-scan`) reads nothing else.
3. **Key material, as runner secrets naming files outside the checkout:**
   - `MIDGARD_SIGNING_KEYSTORE_PATH`: the release keystore (ML-DSA-87);
   - `MIDGARD_BIFROST_KEYSTORE_PATH`: the Bifrost admission keystore (ML-DSA-65), for
     registry admission and the air-gap bundle;
   - `MIDGARD_BIFROST_REGISTRY_PATH`: the registry directory.

   Each keystore file is mounted read-only into the container that uses it.
4. **Pinned trust anchors.** The public halves from the ceremony, committed as
   `deploy/trust/release-trust.pub` and `deploy/trust/bifrost-trust.pub` (see
   `deploy/trust/README.md`).

If any is absent, the preflight **fails closed**, so a release never ships unsigned,
unscanned, or from a half-provisioned runner.

**Custody today is software.** The keystores are files, sealed under an ML-KEM root in
the same file (see `crypto/attest/src/keystore.rs`). The HSM path above (keys generated
on-device, never exportable) is the target, and is not wired in: signing from a
PKCS#11 token needs a signer behind the same custody seam. Until then, protect the
keystore files as the keys themselves.

## Tested

`services/iam/tests/rc_35.rs` exercises the lifecycle over a **real PQ-mTLS mutual
handshake**: issue → handshake ✓ → renew (rekey) → both new and old certificates
handshake ✓ (downtime-free) → revoke old → the next handshake with it is rejected
`PeerCertRevoked` while the renewed certificate still connects; the signed list carries
a revocation to verifiers elsewhere; an expired certificate no longer handshakes.
`crypto/pq-mtls/tests/identity_files.rs` covers the window, edited terms, forged and
older lists and live reload; `tools/mg-admin/tests/mesh.rs` drives `mesh issue`,
`renew`, `revoke` and `upgrade` through the real binary.
