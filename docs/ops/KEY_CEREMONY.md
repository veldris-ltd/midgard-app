# Release key ceremony

The ceremony makes the keys a release is signed with: the release signing key, the
Bifrost admission key, the model-signing key, and the list of SSH keys that may sign a
release tag. It needs no forge and no network, and is best held on a machine with every
link down. There are two ways to hold it, and they leave the same files here:

- **With this repository's tools**, as in [Your own ceremony](#your-own-ceremony)
  below. Anyone can do this, including whoever builds and signs their own Midgard.
- **With Dvalin**, Veldris's guided ceremony application, available internally and to
  enterprise customers. It ships with its own step-by-step procedure.

This page says what either leaves in this repository and how it is checked here.
[RT-35] [AC-RF-CA-6a] [NFR-PQ-3]

## Dvalin

Dvalin (DVALIN · MIDGARD KEY CEREMONY) is a Windows window app (`dvalin.exe`, which also runs on
Linux) with a console form (`mg-ceremony`). Its key generation, signing and
verification are this repository's `attest` crate, pinned by revision, so the keystores
it writes are read by the same code on the signing runner; before publishing, each key
signs a test artefact verified by that same code. It refuses to hold the ceremony
while Wi-Fi, Ethernet, Bluetooth or any other link is up, and records the readings. It
asks every person named for a point of contact, and can optionally split an escrow of
the keystores among named holders (`pqc_agility::shamir`). Its contract test runs this
repository's `tools/ci/release-preflight.sh`, `tools/ci/verify-tag.sh`, `mg-sign` and
`mg-verify` against what it produces. Its procedure comes with the application; nothing
in a ceremony, or in a release afterwards, contacts a forge.

## Your own ceremony

With no Dvalin, the same keys come from this repository's own tools. On a machine with
every network link down, from a checkout of the commit you will release (copied across
on media if need be), with the toolchain image built (`bash tools/ci/run.sh toolchain`)
or a Rust toolchain and the vendored crates:

1. **Build the tools**, offline: `cargo build --release --offline -p mg-sign -p mg-model`.
2. **Make a passphrase for each keystore**, each in a file of its own, outside every
   checkout:
   ```bash
   umask 077
   for name in release bifrost model-signing; do
       head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n' > "$name.passphrase"
   done
   ```
3. **Generate the release signing key** (ML-DSA-87, the default) and the **Bifrost
   admission key** (ML-DSA-65), each exporting its public half:
   ```bash
   mg-sign keygen --key-id release-key-1 --keystore release.keystore \
       --export-verifying-key release-trust.pub --keystore-passphrase-file release.passphrase
   mg-sign keygen --key-id bifrost-admission-1 --alg ML-DSA-65 --keystore bifrost.keystore \
       --export-verifying-key bifrost-trust.pub --keystore-passphrase-file bifrost.passphrase
   ```
4. **Generate the model-signing key**, as in [The model-signing key](#the-model-signing-key).
5. **List who may sign a release tag.** Each release manager makes an SSH signing key
   (`ssh-keygen -t ed25519 -f release-tag`) and contributes one line to
   `allowed_signers`: `<email> namespaces="git" <the public key>`. List release
   managers only: without a forge, this list is what decides who can release
   ([PKI.md, "Releasing without a forge"](PKI.md#releasing-without-a-forge)).
6. **Commit the public files** as the table below says, in one signed commit, with a
   ceremony record under `docs/ops/ceremonies/`.
7. **Keep each keystore and its passphrase file in separate secret storage**, and
   provision them on the release machine, outside every checkout, as
   [PKI.md](PKI.md#signing-runner-release) describes. Then run
   `bash tools/ci/release-preflight.sh` there, and a dry run
   (`bash tools/ci/run.sh release-dry-run`).

**Keystore format version 3.** Keystores wrap their custody root under a passphrase
(or an external trust root), the wrap bound to the whole header; a version 1 keystore,
which held the root in the clear, and a version 2 keystore, whose header was not
authenticated, are refused by the signing runner. Dvalin must pin an `attest` revision
that writes version 3 before the next ceremony, and each keystore holder's passphrase is
generated in the secret store and handed over separately from the keystore. Keystores
from an earlier ceremony are converted once with `mg-admin keystore migrate`, checked
with `mg-admin keystore verify`, and every earlier copy (including escrow fragments of
it) destroyed. Where a version 1 copy may have been exposed, `mg-admin keystore
rotate-root` gives a release, admission or model-signing keystore a new custody root,
keeping the signing key the published anchors pin. [VLD-ANB-MIDGARD-0007]

## What the ceremony writes here

| File | What it is | Checked by |
|---|---|---|
| `deploy/trust/release-trust.pub` | the release signing key's public half (ML-DSA-87) | `tools/ci/release-preflight.sh`, `mg-verify`, every image's entrypoint |
| `deploy/trust/bifrost-trust.pub` | the Bifrost admission key's public half (ML-DSA-65) | `tools/ci/release-preflight.sh`, `mg-bundle import` |
| `deploy/trust/model-signing.pub` | the model-signing key's public half (ML-DSA-65) | `tools/ci/release-preflight.sh`, `mimir-server`'s load gate, `mg-model verify` |
| `deploy/trust/allowed_signers` | the SSH keys that may sign a release tag | `tools/ci/verify-tag.sh` |
| `docs/ops/ceremonies/<date>-release-keys.md` | the ceremony record: digests, fingerprints, self-test, machine isolation, sign-off | the reviewer of the ceremony commit |

All five are public and committed in one signed commit. The private keystores, and
the confidential custody register (contact details, and fragment holders when key
fragmentation is chosen), never enter this repository.

## The model-signing key

The ceremony also generates the model-signing key (ML-DSA-65) that signs every model
manifest `mimir-server` loads. [VLD-ANB-MIDGARD-0028] With `mg-model` from this
repository, on the ceremony machine, with the holder's passphrase file:

```bash
MIDGARD_KEYSTORE_PASSPHRASE_FILE=PASSPHRASE_FILE mg-model keygen --out OUT_DIR
```

It writes `OUT_DIR/model-signing.keystore`, a keystore version 3 whose root is wrapped
under that passphrase, and `OUT_DIR/model-signing.pub`. The public file is committed as
`deploy/trust/model-signing.pub` with the others; the keystore and its passphrase go to
the staging enclave, where models are signed, and never into an enclosure or a volume
that holds weights. To stage a model on compose with it, provision the keystore into the
`midgard-model-signing` volume and its passphrase into `midgard-model-signing-passphrase`
(as `passphrase`), then run `bash deploy/compose/stage-model.sh`: the model must verify
against the committed file. After the ceremony, re-sign the Granite demo manifest under
the new key, and remove any development key from existing installs (`docker volume rm
midgard-model-signing midgard-model-trust` after re-staging).

## After the ceremony

Provision the signing runner as in [PKI.md, "Signing runner"](PKI.md#signing-runner-release),
then run `bash tools/ci/release-preflight.sh` on it. Rotation is described in
[`deploy/trust/README.md`](../../deploy/trust/README.md).
