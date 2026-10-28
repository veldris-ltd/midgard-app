# Release key ceremony

The release key ceremony is held with **Midgard Signing**, a separate application:
[veldris-ltd/midgard-signing-app](https://github.com/veldris-ltd/midgard-signing-app)
(branch `dev`). Its step-by-step procedure is
[`docs/KEY_CEREMONY.md`](https://github.com/veldris-ltd/midgard-signing-app/blob/dev/docs/KEY_CEREMONY.md)
in that repository. Follow that document; this page says only what the ceremony leaves
in this repository and how it is checked here. [RT-35] [AC-RF-CA-6a] [NFR-PQ-3]

Midgard Signing is a Windows window app (`midgard-signing.exe`, which also runs on
Linux) with a console form (`mg-ceremony`). Its key generation, signing and
verification are this repository's `attest` crate, pinned by revision, so the keystores
it writes are read by the same code on the signing runner.

## What the ceremony writes here

| File | What it is | Checked by |
|---|---|---|
| `deploy/trust/release-trust.pub` | the release signing key's public half (ML-DSA-87) | `tools/ci/release-preflight.sh`, `mg-verify`, every image's entrypoint |
| `deploy/trust/bifrost-trust.pub` | the Bifrost admission key's public half (ML-DSA-65) | `tools/ci/release-preflight.sh`, `mg-bundle import` |
| `deploy/trust/allowed_signers` | the SSH keys that may sign a release tag | `tools/ci/verify-tag.sh` |
| `docs/ops/ceremonies/<date>-release-keys.md` | the ceremony record: digests, fingerprints, self-test, sign-off | the reviewer of the ceremony commit |

All four are public and committed in one signed commit. The private keystores, and
the confidential custody register (contact details, and fragment holders when key
fragmentation is chosen), never enter this repository.

## After the ceremony

Provision the signing runner as in [PKI.md, "Signing runner"](PKI.md#signing-runner-release),
then run `bash tools/ci/release-preflight.sh` on it. Rotation is described in
[`deploy/trust/README.md`](../../deploy/trust/README.md).
