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
| `bifrost-trust.pub` | ML-DSA-65 | the bundle, and each artefact's admission signature |
| `release-trust.pub` | ML-DSA-87 | each binary's release signature, inside its image |

**Both anchors come from the operator, never from the bundle.** An anchor carried inside
the bundle it verifies proves nothing. The operator brings `bifrost-trust.pub` and
either `release-trust.pub` or its SHA-256 (`sha256sum release-trust.pub`), each taken
from the key ceremony's record or `deploy/trust/` of a reviewed checkout, by a route
other than the bundle. The bundle does carry `deploy/trust/` in its deploy kit, because
the images embed it; `install.sh` refuses unless that copy of `release-trust.pub` is the
operator's.

## What the bundle carries

`run.sh release-bundle` packs everything an install needs, so nothing else crosses the
gap but the operator's two anchors and the tools listed under
[Install](#install-disconnected-site-no-network). [VLD-ANB-MIDGARD-0035]

| Member | Component | What it is |
|---|---|---|
| every gated binary, with `<id>.sig` and `<id>.att` | `service`, `tool`, `client`, `attestation` | what `release` gated, from `artefacts.txt` |
| `<image>.oci.tar`, one per line of `images.lock` | `image` | the release's own verify-before-run service images, as `release-images` built them |
| `images.lock`, `.sig`, `.att` | `deploy`, `attestation` | each service image's digest, signed with the release key |
| `chart-values.yaml`, `.sig`, `.att` | `deploy`, `attestation` | the chart's values with every image pinned by digest, signed |
| `enclosure.yaml`, `.sig`, `.att` | `deploy`, `attestation` | the chart rendered from those values, signed, for review |
| `helm-chart.tgz` | `deploy` | the release's own chart, from `helm package`; its `appVersion` is the release |
| `deploy-kit.tar` | `deploy` | `install.sh`, `set-digests.sh`, `externals.sh` and `externals.lock`, and for `--rebuild` `build.sh`, `service.Dockerfile`, `release-artefacts.sh` with the annotation list and the release's rows; and `deploy/trust/` |
| one `.oci.tar` per line of `deploy/images/externals.lock` | `image` | the images Midgard runs but does not build: the seed image (always), the Granite llama.cpp server, the HAProxy egress broker, and the kind node image for rehearsals |

Every image travels as an OCI archive that keeps its digest: the service images as
`docker buildx` exported them, and the external images as `skopeo copy --all` or
`crane` wrote them, every platform of the pinned index. `docker save` is never used,
because it writes a new manifest with a new digest.

**Size.** Expect roughly 2 to 3 GB, most of it the external images: the kind node
image and the multi-platform llama.cpp and HAProxy indexes. It has not been measured
on a real release. The bundle streams (below), so its size is limited by the media, not
by memory.

## Provisioning the external images (connected, once)

The external images are fetched once, while the release machine is connected, as the
toolchain bases are; this is the only step of a release that needs a network. With
skopeo or crane on the release machine:

```bash
bash deploy/images/externals.sh fetch /srv/midgard-images     # by digest, every platform
bash deploy/images/externals.sh check /srv/midgard-images     # offline: there, and as pinned
export MIDGARD_EXTERNAL_IMAGES=/srv/midgard-images
```

`fetch` copies each image by its pinned digest, never a tag, and keeps an archive only
when its `index.json` names that digest over a manifest blob the archive carries.
`MIDGARD_EXTERNAL_IMAGES` has no default, and the preflight refuses a directory inside
the job's workspace. When a pin changes in `externals.lock` (and in the chart, compose
and kind beside it: `tools/ci/tests/externals-lock.test.sh` requires that they agree),
run `fetch` again.

## Produce (connected side)

`bash tools/ci/run.sh release-bundle`, after `release` and `release-images`, packs it all
with the admission keystore. Nothing is fetched: it refuses, packing nothing, when
`MIDGARD_EXTERNAL_IMAGES` is unset, an external archive is missing or not as pinned, a
service image archive, `images.lock`, the chart values or the rendering (or a signature
of one) is missing, or the chart's `appVersion` is not the release. `produce.sh` packs
any set of artefacts by hand:

```bash
# produce.sh OUT.mgbundle ID LABEL KEYSTORE  [<component>=]<id>:<ver>:<path> ...
bash deploy/air-gap-bundle/produce.sh midgard-0.1.0-rc.1.mgbundle midgard-0.1.0-rc.1 v0.1.0-rc.1 \
    "$MIDGARD_BIFROST_KEYSTORE" \
    http-gateway:0.1.0-rc.1:target/verified/http-gateway \
    attestation=http-gateway.sig:0.1.0-rc.1:target/verified/http-gateway.sig \
    image=http-gateway.oci.tar:0.1.0-rc.1:target/release-out/images/http-gateway.oci.tar
```

The keystore is sealed (`mg-sign keygen --alg ML-DSA-65`), and the signing key is
unsealed in memory only. An artefact's adjacent `<path>.sbom.cdx.json` is packed as its
SBOM; one is generated for an artefact without.

## Transport

Copy the single `.mgbundle` file to physical media and carry it to the enclosure. The
operator brings the two anchors separately (see [Keys](#keys)), and the tools.

## Install (disconnected site, no network)

**Prerequisites** on the install host, all brought in with the anchors, none fetched:
`mg-bundle` (the release's own), `helm` and `kubectl`; to push, `skopeo` or `crane`; for a
kind rehearsal, `kind`; for `--rebuild`, `docker` with buildx and the build bases. Plus
`bash`, `tar`, `sha256sum` and `cmp`.

`install.sh` is in the bundle's deploy kit, and refuses to run as any other copy. Take
it from the verified bundle first, then run it in a directory holding only the bundle and
the anchors:

```bash
mg-bundle import --trust-anchor bifrost-trust.pub --out-dir kit midgard-0.1.0-rc.1.mgbundle
tar -xOf kit/deploy-kit.tar deploy/air-gap-bundle/install.sh > install.sh && rm -rf kit
bash install.sh midgard-0.1.0-rc.1.mgbundle \
    --bundle-trust bifrost-trust.pub --release-trust release-trust.pub \
    --registry registry.enclosure.local/midgard --push
```

It works in this order, and refuses, **having changed nothing**, at the first of steps 1
to 4 that fails:

1. **Verify, then extract.** `mg-bundle import --out-dir` verifies the bundle offline
   against `--bundle-trust`, and only then writes the artefacts out. Refused: a bundle
   that does not verify, or carries several versions.
2. **Every member.** The deploy kit, the chart, `images.lock` and `chart-values.yaml`
   with their signatures and provenance, an archive for every image `images.lock`
   names, and one for every image the kit's `externals.lock` names. Refused: any of them
   missing; a kit without `install.sh`, `externals.sh`, `externals.lock`,
   `set-digests.sh` or `release-trust.pub`; a script that is not the kit's
   `install.sh`; a chart whose `appVersion` is not the bundle's version; a kit
   `release-trust.pub` that is not the operator's file, or does not have the operator's
   SHA-256.
3. **Every digest.** Each archive's `index.json` must name exactly one manifest, carried
   in the archive with that digest, and the digest must be the image's pin in
   `images.lock` or `externals.lock`. Refused: any archive that is not as pinned.
4. **The tools.** `helm` and `kubectl`, and `skopeo` or `crane` to push (both are
   named when neither is there; `docker push` is not used, since it can change a
   digest), or `kind` for a rehearsal.
5. **Make the images available.** With skopeo, `copy --all --preserve-digests
   --digestfile` from the archive; with crane, `push` of the archive's layout, which
   pushes its one manifest as it is. Each service image goes to
   `<registry>/<image>:<version>`, and each external image to
   `<registry>/<path>:sha256-<hex>` (its path without the host it was fetched from,
   tagged with its own digest so no later push replaces it). The digest the registry
   reports must be the pin, or the install stops before anything is deployed. The kind
   node image is not pushed.
6. **Deploy.** `helm upgrade --install` of the bundled `helm-chart.tgz` with the bundled
   `chart-values.yaml`, `image.registry` set to the registry, and `seedImage`,
   `modelServing.image` and `egressBroker.image` set to the registry's copies by their
   pinned digests. Operator `-f` values come after the chart values; the external
   images are `--set`, which no values file overrides. The kit's `release-trust.pub`, checked in step 2, becomes the chart's trust
   ConfigMap.

No step pulls an image, reads a chart repository or uses a checkout. The other options:
`--context` names the kube context, and `-f` adds values files.

**`--rebuild`** builds the service images again from the bundle's binaries with the
kit's `build.sh`, as a check of the archives the release shipped, and uses them instead.
It needs `docker` with buildx and service.Dockerfile's `BASE` and `RUNTIME` images loaded
from elsewhere: the bundle does not carry them. The images are reproducible, so the
digests set into the chart's values must be those of the signed `chart-values.yaml`;
the comparison always runs, and a difference stops the install before anything is
pushed. The external images are never rebuilt.

**`--kind CLUSTER`** loads the images into a local kind cluster instead of a registry, as
a rehearsal. Each archive's `index.json` is rewritten to carry the name the chart's
reference resolves to (`io.containerd.image.name`), its digest checked unchanged, and
loaded with `kind load image-archive`, which imports every platform of an OCI archive
into the nodes' containerd. The cluster comes from `deploy/kind/cluster.yaml`, whose
nodes run `kindest/node` pinned by digest. To create it with no network, load
`kindest-node.oci.tar` into docker first; only Docker's containerd image store keeps an
OCI archive's index digest on `docker load`, and without it kind looks for the pinned
digest, finds none, and tries to pull.

The enclosure's secrets (identities, seeds, forge-jobs) come from key custody, never
from a bundle.

**Not yet proven.** `tools/ci/tests/air-gap-bundle.test.sh` runs `install.sh` from a
directory with no checkout against stub tools, and `tools/ci/tests/release-bundle.test.sh`
checks what `release-bundle` packs; `tools/mg-bundle/tests/cli.rs` round-trips image
archives through a real bundle. No install from a real bundle into a real registry or
cluster, with real skopeo, crane or kind, has been run yet.

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

`pack` packs exactly what its load gate verified: the manifest bytes it checked, and the
weights read through the handle they were hashed through, each refused unless it still
has the verified digest. An import checks the bundle signature over the manifest's bytes
as they lie in the file before decoding any of them. Older releases do not get back in:
Bifrost's artefact registry refuses a release older than one it has accepted for the
same id (or the same version with a different manifest), and `mimir-server` does the
same for models. [MG-SEC-14]
