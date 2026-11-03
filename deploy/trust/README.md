# Pinned trust anchors

Every release verification is anchored to a **pinned public key** committed here. A
signature by a key that is not pinned is rejected as untrusted, and a missing anchor is
a refusal, never a pass. [NFR-PQ-3] [AC-B-1]

## What lives here

| File | Algorithm | Pins | Checked by |
|---|---|---|---|
| `release-trust.pub` | ML-DSA-87 | the release signing key | `mg-verify`: the release gate's verify stage, and every image's entrypoint before it starts its binary |
| `bifrost-trust.pub` | ML-DSA-65 | the Bifrost admission key | `mg-bundle import`: the air-gap bundle and each artefact in it |
| `model-signing.pub` | ML-DSA-65 | the model-signing key | `mimir-server`'s load gate (`--model-trust /etc/midgard/trust/model-signing.pub`) and `mg-model verify`: the signed manifest of every model served |
| `allowed_signers` | SSH (ed25519) | the keys that may sign a release tag | `tools/ci/verify-tag.sh`: the release's `release-tag` job, before anything is built |

All four hold **public** keys, safe to commit. `allowed_signers` is in git's
allowed-signers format (see `docs/ops/PKI.md`, "Git signing"). On the signing runners
the committed copy is kept for review only: a tag verifies against the runner's own
copy (`MIDGARD_ALLOWED_SIGNERS_PATH`), and only when this file is byte-for-byte the
same, so a key added here takes effect once it is on the runners too (VLD-ANB-MIDGARD-0035;
`docs/ops/PKI.md`, "Signing runner"). The three `.pub` files each
hold one or more lines in the format `mg-sign keygen --export-verifying-key` (and
`mg-model keygen`) writes:

```text
ML-DSA-87 midgard-release-2026 <hex public key>
```

Blank lines and `#` comments are ignored. The release workflow's preflight refuses to
run unless all three files exist, hold a key of the right algorithm, and hold nothing
private.

The images carry this directory read-only at `/etc/midgard/trust` (the development image
from `deploy/compose/Dockerfile`, the release images from
`deploy/images/service.Dockerfile`), and the Helm chart's `mimir-model-trust` ConfigMap
is built from `model-signing.pub` here (`deploy/kind/model.sh`). The model-signing key's
public half is never taken from the volume that holds the weights it vouches for, and
`mimir-server` refuses a trust key inside its model directory. Until
`model-signing.pub` is committed, a deployment that serves a real model does not start;
`deploy/compose/stage-model.sh --development` is the explicit, warned exception for a
development install, with a key of its own in a separate trust volume.
[VLD-ANB-MIDGARD-0028]

## Provisioning

The **key ceremony** (RT-35) populates all four files. It is held with Dvalin
(veldris-ltd/dvalin-app; see
[`docs/ops/KEY_CEREMONY.md`](../../docs/ops/KEY_CEREMONY.md)), which generates each
keypair, writes its public half here and records the key id. Until then the
anchors are absent, and verification **fails closed**. That is the correct posture, not
a bug. Do not commit a placeholder or development key: an unpinned or fake anchor would
defeat the purpose.

To rotate, append the new key's line and keep the old one, so artefacts signed under
the previous key keep verifying until they are re-signed; then remove the old line.
