# CA / PKI & Certificate Lifecycle

How the PQ-mTLS mesh's certificate authority is bootstrapped and operated: issuance,
renewal, revocation, the HSM key ceremony, rotation cadence, optional threshold
signing, and the release **signing-runner** provisioning. All in-enclosure, all
post-quantum (ML-DSA / FIPS 204). Implemented in `services/iam` (`MeshCa`) over
`crypto/pq-mtls`.

## Model (why it looks like this)

Midgard's mesh CA is the IdP's ML-DSA key. It issues **short-lived, rekeyed workload
certs** and maintains an **authoritative issuance registry + a CRL distributed inside
the trust anchor**, rather than long-lived certs carrying embedded validity:

- The **CA registry** (`MeshCa`) is the source of truth for what was issued, its
  validity window, and its status.
- **Revocation** is a set of revoked **verifying-key fingerprints** (SHA-256) carried
  in `pq_mtls::TrustAnchor.revoked`. Since renewal **rekeys** (a new cert = a new key),
  a fingerprint names exactly one issued cert, so revoking one never revokes its
  renewals. `verify_cert` checks the CRL on **every handshake** — so revocation is
  honoured on the *next* handshake with no cert re-parsing needed.
- **Expiry** is enforced by a short renewal cadence: certs are renewed well before
  `not_after`, so live handshakes always use fresh certs. The registry's
  `status(serial, now)` and `expiring_by(deadline)` drive the renewer.

This is the OCSP-/short-lived-cert posture, chosen because the mesh is fully
in-enclosure and the CA is always reachable — no need to push long-lived certs and
chase them with distributed CRLs on a WAN.

## Bootstrap

1. **Root/CA key ceremony** (below) provisions the CA (IdP) ML-DSA signing key on the
   HSM/TPM.
2. Publish the CA **trust anchor** (`ca_alg` + public `ca_key`) to every workload — it
   is the mesh's single root of trust (`Idp::trust_anchor` / `MeshCa::trust_anchor`).
3. Each workload generates its **own** keypair locally (the private key never leaves
   the workload) and submits its `WorkloadIdentity` (id + roles + public key) for
   certification.

## Issuance

`MeshCa::issue(identity, not_before, not_after) -> (IdentityCert, serial)`:
- the CA signs the workload's canonical identity with its ML-DSA key,
- records `{serial, subject, fingerprint, not_before, not_after}` in the registry,
- returns the cert (presented in the PQ-mTLS handshake) + its serial.

## Renewal (downtime-free)

`MeshCa::renew(new_identity, not_before, not_after)`:
- the workload generates a **new** keypair (rekey) and the CA issues a fresh cert with
  a new window and serial;
- the **old cert keeps working** until it expires or is revoked, so there is an overlap
  window during which both certs validate — **no session is dropped and no handshake
  fails** while the workload swaps in the new cert. (Verified in
  `services/iam/tests/rc_35.rs`: the renewed cert AND the still-valid old cert both
  complete a mutual handshake.)
- **Cadence:** renew at ~⅔ of the validity window (e.g. issue for 24 h, renew at 16 h).
  An automated renewer walks `MeshCa::expiring_by(now + renewal_horizon)` and renews
  each, then retires the old cert after the overlap.

## Revocation (honoured on the next handshake)

`MeshCa::revoke(serial)`:
- marks the registry record revoked and adds its fingerprint to the CRL;
- every subsequently built `MeshCa::trust_anchor()` carries that fingerprint, so the
  verifier rejects the cert with `MtlsError::PeerCertRevoked` on the **next handshake**
  (verified in `rc_35.rs`). Existing sessions are unaffected (revocation gates *new*
  handshakes); to also kill live sessions, tear down the peer's connections.
- Distribute the updated anchor/CRL to verifiers on change (in-enclosure push); the
  gateway/Heimdall pick it up for subsequent handshakes.

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

**The mechanical part, as one run.** `deploy/trust/ceremony.sh` generates the release
signing key (ML-DSA-87) and the Bifrost admission key (ML-DSA-65) into sealed keystores
outside the checkout, writes `deploy/trust/release-trust.pub`, `bifrost-trust.pub` and
`allowed_signers`, checks them as the release will, and writes a ceremony record under
`docs/ops/ceremonies/` with every digest and fingerprint and blank sign-off lines. It
commits nothing. Until custody is hardware, it stands in for steps 2 to 4 below with
software keystores; steps 5 and 6 remain the officers'.

```bash
bash deploy/trust/ceremony.sh --custody /secure/midgard-keys-2026 --signer you@veldris.dev=$HOME/.ssh/id_ed25519_codelab.pub --witness "A. Witness"
```

Run with the duty Security Officer + a second witness (dual-control), recorded in the
audit chain:

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
   release@veldris.dev namespaces="git" ssh-ed25519 AAAA...
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
| Workload certs | ≤ 24 h validity, renew at ~⅔ | `MeshCa::renew` (rekey) |
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
handshake**: issue → handshake ✓ → renew (rekey) → both new and old certs handshake ✓
(downtime-free) → revoke old → next handshake with it is rejected `PeerCertRevoked`
while the renewed cert still connects (targeted revocation). Plus expiry/status
tracking and the renewal worklist.
