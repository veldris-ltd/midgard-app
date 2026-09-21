# deploy/air-gap-bundle/ — signed offline release bundles

The transit medium across the air gap. A bundle is one self-describing, ML-DSA-signed
`.mgbundle` file (magic `MGBUNDLE`) that packages signed artefacts under one signed
table of contents. It is produced on the connected side and **imported fully offline**
on a disconnected site. The bundle signature and every contained artefact are verified
before anything is built or deployed, and any tampered byte is rejected. This closes
FR-B-5 and the AC-B-3 / AC-MS-3 (Mimir model bundles) / AC-C-7 (client artefacts)
forward links. [FR-B-5] [AC-B-3]

## Keys

A bundle is signed with the **Bifrost admission key** (ML-DSA-65), the only algorithm
`mg-bundle import` accepts. The binaries inside it carry their own **release**
signatures (ML-DSA-87, `.sig` + `.att`), which every container re-verifies before it
starts. Two roots, two custody chains; see `docs/ops/PKI.md`.

| Anchor | Algorithm | Checks |
|---|---|---|
| `deploy/trust/bifrost-trust.pub` | ML-DSA-65 | the bundle, and each artefact's admission signature |
| `deploy/trust/release-trust.pub` | ML-DSA-87 | each binary's release signature, inside its image |

## Produce (connected side)

The release workflow's `bundle` job packs every gate-verified binary from
`supply-chain/release-artefacts.txt`, with its `.sig` and `.att` as `attestation`
artefacts. By hand:

```bash
# produce.sh OUT.mgbundle ID LABEL KEYSTORE  [<component>=]<id>:<ver>:<path> ...
bash deploy/air-gap-bundle/produce.sh midgard-0.1.0-rc.1.mgbundle midgard-0.1.0-rc.1 v0.1.0-rc.1 \
    "$MIDGARD_BIFROST_KEYSTORE" \
    http-gateway:0.1.0-rc.1:target/verified/http-gateway \
    attestation=http-gateway.sig:0.1.0-rc.1:target/verified/http-gateway.sig \
    attestation=http-gateway.att:0.1.0-rc.1:target/verified/http-gateway.att
```

The keystore is sealed (`mg-sign keygen --alg ML-DSA-65`), and the signing key is
unsealed in memory only. A binary's adjacent `<path>.sbom.cdx.json` is packed as its
SBOM.

## Transport

Copy the single `.mgbundle` file to physical media and carry it to the enclosure.
Nothing else crosses the gap.

## Install (disconnected site, no network)

```bash
bash deploy/air-gap-bundle/install.sh midgard-0.1.0-rc.1.mgbundle \
    --bundle-trust deploy/trust/bifrost-trust.pub \
    --registry registry.enclosure.local/midgard --push
```

`install.sh` does three things, in order, and stops at the first failure:

1. **Verify, then extract.** `mg-bundle import --out-dir` verifies the bundle offline
   and only then writes the artefacts out.
2. **Build the images.** `deploy/images/build.sh` builds the verify-before-run images
   from those artefacts, and runs `mg-verify` inside each one against the release
   anchor.
3. **Deploy.** It pushes the images to the enclosure's registry, and deploys the
   bundle's version with the Helm chart. The bundle's version must be the chart's
   `appVersion`.

`--kind CLUSTER` loads the images into a local kind cluster instead of pushing them, as
a rehearsal. The enclosure's secrets (identities, seeds, forge-jobs) come from key
custody, never from a bundle.

## Size: bundles stream

`mg-bundle pack` writes **format version 2**: each payload is written in chunks of
16 MiB, and each chunk is recorded with its own SHA-256. `mg-bundle import` reads the
bundle back a chunk at a time, so an artefact larger than memory, such as a model's
weights, crosses the gap. Version 1 bundles still import.

- **Authenticity is unchanged.** The bundle signature covers every artefact's payload
  and SBOM digest, and each artefact's signature covers its own. The chunk digests are
  not signed; they locate damage. A damaged file is refused at its first bad chunk,
  with the artefact and chunk named.
- **Refusals leave nothing.** An import writes into a staging directory beside
  `--out-dir`, and moves the artefacts into place only after the last one verifies.
  A refused import leaves nothing behind, and no import overwrites a file.

## Model bundles

A model crosses the gap as its own signed bundle (AC-MS-3). `mg-model` builds it from a
model directory staged with `mg-model sign`:

```bash
# Connected side: only a model that passes the load gate is packed.
mg-model pack --dir models/granite --model-trust model-signing.pub \
    --keystore "$MIDGARD_BIFROST_KEYSTORE" --out granite-3.3-2b.mbundle

# Enclosure: verify the bundle offline, write the model out, run the load gate.
mg-model unpack --bundle granite-3.3-2b.mbundle --bundle-trust deploy/trust/bifrost-trust.pub \
    --model-trust model-signing.pub --out /models/granite
```

The bundle carries `model.manifest` and the weights as `model-bundle` artefacts. It is
signed with the Bifrost admission key, and proves how the files travelled. The model
manifest, signed with the model-signing key, proves what they are. `unpack` checks
both, and if the model does not verify it removes the directory it wrote, so nothing
unverified is left to serve.
