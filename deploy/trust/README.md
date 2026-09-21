# Pinned trust anchors

Every release verification is anchored to a **pinned public key** committed here. A
signature by a key that is not pinned is rejected as untrusted, and a missing anchor is
a refusal, never a pass. [NFR-PQ-3] [AC-B-1]

## What lives here

| File | Algorithm | Pins | Checked by |
|---|---|---|---|
| `release-trust.pub` | ML-DSA-87 | the release signing key | `mg-verify`: the release gate's verify stage, and every image's entrypoint before it starts its binary |
| `bifrost-trust.pub` | ML-DSA-65 | the Bifrost admission key | `mg-bundle import`: the air-gap bundle and each artefact in it |

Both are **public** keys, safe to commit. Each file holds one or more lines in the format
`mg-sign keygen --export-verifying-key` writes:

```text
ML-DSA-87 midgard-release-2026 <hex public key>
```

Blank lines and `#` comments are ignored. The release workflow's preflight refuses to
run unless both files exist, hold a key of the right algorithm, and hold nothing private.

## Provisioning

The **key ceremony** (RT-35, `docs/ops/PKI.md`) populates both files: when a keypair is
generated, its public half is committed here and the key id recorded. Until then the
anchors are absent, and verification **fails closed**. That is the correct posture, not
a bug. Do not commit a placeholder or development key: an unpinned or fake anchor would
defeat the purpose.

To rotate, append the new key's line and keep the old one, so artefacts signed under
the previous key keep verifying until they are re-signed; then remove the old line.
