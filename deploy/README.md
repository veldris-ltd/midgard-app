# deploy/ — deployment & air-gap bundles

Deployment tooling for the enclosure: hardened, verify-before-run container images,
Kubernetes/Helm manifests, and (RT-19) signed offline bundles. Networking is
default-deny, egress-denied, PQ-mTLS on every internal hop. [NFR-AVA-1] [NFR-PRV-1]
[NFR-SEC-1] SAD_00 §10.

## Layout

- **`images/`** — the per-service runtime image.
  - `service.Dockerfile` — minimal, non-root, read-only-rootfs image built **from
    the signed release artefact** (binary + `.sig` + `.att` + `.sbom.cdx.json`),
    parameterised by `--build-arg SERVICE=<name>`. Two stages, each pinned by digest:
    a `prep` stage on the toolchain image lays out the files, the service account and
    the modes, and builds nothing; the runtime stage is distroless
    (`gcr.io/distroless/cc-debian12:nonroot`: glibc and libgcc, no shell, no package
    manager, no compiler), which must stay on the toolchain's Debian release
    (bookworm) because the gate-built binaries link glibc. [VLD-ANB-MIDGARD-0068]
  - **verify-before-run** is the image's entrypoint, `mg-verify --run-service`:
    it re-checks the ML-DSA signature + SBOM + provenance of
    `/app/$MIDGARD_SERVICE` against the pinned trust key and only then execs the
    service with the container's arguments; a tampered image fails closed and the
    container refuses to start. The trust key is always `/trust/release-trust.pub`,
    owned by root and named on the entrypoint's own command line: no environment
    variable or container argument can point verification at another key
    (`MIDGARD_RELEASE_TRUST_KEY` is gone). [AC-B-1] [NFR-PQ-3] [MG-SEC-14]
  - The chart's `seed` init container, which unpacks a service's first state, needs
    a shell and `tar`, which the distroless runtime does not have. It runs in its own
    image, `seedImage` in the chart's values (distroless `base-debian12:debug-nonroot`,
    whose BusyBox carries both), pinned by digest, with the same hardened
    securityContext. The `keystore-passphrase` init container, which copies each
    service's passphrase into memory as an owner-only file, needs a shell too and runs
    in the same image. Mirror it into the enclosure's registry with the service images.
- **`compose/`** — Midgard on one machine with docker compose: the HTTP edge, the
  Mimir inference plane, the Bifrost fleet manager, the Heimdall feed gateway and the
  Forge worker in hardened containers (non-root, read-only rootfs, no
  capabilities), each with only its own volumes. A one-shot `init` creates the
  enclosure and issues each mesh service its identity (`mg-admin init`, `mg-admin mesh
  issue`); the mesh services sit on an internal network and are reachable only over
  PQ-mTLS. `smoke.sh` builds, starts, probes and restarts a throwaway copy. The image
  is built from source (`compose/Dockerfile`), **not** the signed release image.
  [RC-33] [NFR-SEC-1]
  `compose.granite.yaml` serves a real model (Granite on llama.cpp) instead of the
  reference engine. The llama.cpp sidecar never mounts the models volume: mimir-server
  verifies the model, stages the verified weights read-only into the `mimir-serve`
  volume (`--serve-dir`), which only it writes, and writes `staged.ready` last;
  llama.cpp mounts that volume read-only and starts on the staged copy once the marker
  exists. The chart's `modelServing` (and `kind/model.sh`) does the same with an
  emptyDir at `/serve`. [MG-SEC-14]
- **`installer/`**: `mg-install`, the guided installer for one server: a server check,
  use-case profiles (each with a starter schema, policy and sample Forge jobs), the choice
  of products, first users and administrators enrolled before anything starts, an
  optional web console image, and checks that each chosen service answers. It drives
  `compose/` and adds a site overlay; it also manages the install afterwards (status,
  users, backups, removal). See [installer/README.md](installer/README.md). [RC-33]
- **`kind/`** — Midgard on a local three-node Kubernetes cluster: `up.sh` creates
  the kind cluster, builds and loads the image, installs the chart and provisions
  the secrets (`bootstrap-secrets.sh` mints the enclosure's keys, every mesh
  identity and each keystore passphrase with the same `init` compose uses, and refuses
  any kubectl context that is not `kind-*`); `smoke.sh` runs each mesh client as a
  Job through the NetworkPolicies. Built from source, not the signed release images.
  [RC-33] [VLD-ANB-MIDGARD-0018]
- **`helm/midgard/`** — the Helm chart for the core services + product edges:
  a default-deny/egress-denied namespace, a `ResourceQuota`, the pinned trust-key
  `ConfigMap`, and one hardened `Deployment` + `Service` (+ ingress `NetworkPolicy`)
  per enabled service — non-root, read-only rootfs, dropped capabilities,
  readiness/liveness probes, resource requests/limits, and a zero-downtime rolling
  strategy (`maxUnavailable: 0`). Every ingress rule names the workloads that may
  reach its port; a port open to any pod (`from: ["*"]`) refuses to render. The
  mesh services' health ports (`/healthz`, `/readyz`, `/metrics` on 9090) bind the
  pod's own address and admit only the scraper: label your Prometheus pod
  `app.kubernetes.io/name=metrics-scraper` (or set `scraper.podLabel`), and on a CNI
  that applies NetworkPolicy to the kubelet's probes list the node ranges in
  `scraper.nodeCidrs`. [VLD-ANB-MIDGARD-0027] `secrets.provider` chooses where
  identities, keystore passphrases and seeds come from: Kubernetes Secrets (only with
  etcd encrypted at rest), the Secrets Store CSI driver, or external-secrets; see
  [helm/midgard/README.md](helm/midgard/README.md).
- **`k8s/enclosure.yaml`** — the chart rendered for a development cluster
  (`helm template` with `image.allowTag=true`, until releases publish digests, and
  `secrets.provider=csi`), for `kubectl apply` without Helm.
  Generated: regenerate it when the chart changes.
- **`acceptance/`** — the offline acceptance harness (`run-airgapped.sh`, the
  `verify-*.sh` suites, and the pinned toolchain `Dockerfile`).
- **`trust/`** — the pinned public release trust key `mg-verify` checks against.
- **`air-gap-bundle/`** — signed offline release bundles produced/consumed by
  Bifrost (RT-19), verified offline before deploy (AC-B-3). *Not yet present.*

## Applying

```bash
# Apply the development rendering (images by tag), on a development cluster:
kubectl apply -f deploy/k8s/enclosure.yaml
# Or a release: its signed, digest-pinned values (target/release-out/chart/values.yaml):
helm install midgard deploy/helm/midgard -f chart/values.yaml
```

The release pipeline (`.github/workflows/release.yml`) builds and PQ-signs one
`service.Dockerfile` image per shippable artefact from the verified bundle.

### Images by digest

Each image re-verifies its binary before it runs, but the verifier, its key and the
entrypoint travel inside the image, so only the image's digest binds a pod to the
signed build: a tag can be moved in the registry to an image that skips the check.
[VLD-ANB-MIDGARD-0012]

- **`image.digests`** in `helm/midgard/values.yaml` holds one `sha256:` digest per
  service, empty by default. The chart renders `<registry>/<service>@<digest>`, pulled
  `IfNotPresent`, and annotates each pod `midgard.veldris.io/image-pinned: "true"`.
  It refuses to render while a digest is empty or not `sha256:` and 64 hex digits.
- **`helm/set-digests.sh IMAGES.LOCK [VALUES] [--out FILE]`** sets them from the
  `images.lock` that `images/build.sh` writes and the release signs, refusing a lock
  that lacks a service or carries a malformed digest. The `release-images` job runs it
  and renders the chart with the result; `chart/values.yaml` and `chart/enclosure.yaml`
  are signed and published with the release. `air-gap-bundle/install.sh` pins the
  images it builds the same way.
- **`image.allowTag: true`** is for development clusters only: a service with no digest
  is rendered by tag, pulled `Always`, and annotated `image-pinned: "false"`. The
  committed `k8s/enclosure.yaml` is rendered this way until a release produces
  digests; `kind/values.yaml` sets it too, beside `image.single`.
- The acceptance probes (`acceptance/deployment/*.yaml`) name an all-zero digest that
  `run-deployment-tier.sh` replaces with `MIDGARD_PROBE_IMAGE`, pinned by digest, and
  never applies.

## Status & scope

Five services have deployable binaries today: `http-gateway`, the enclosure's edge
(built → **verified before run** → started → reachable on `/api/v1/health`), which
can also open a PQ-mTLS door for syncing records into the fabric; `mimir-server`,
`bifrost-fleet` and `heimdall-feed`, which serve PQ-mTLS only and answer `/healthz`,
`/readyz` and `/metrics` on a separate port; and `forge-worker`, a mesh client that
syncs its sources through the gateway's door. All five run under compose and, through
the Helm chart, on a local kind cluster. The signed release images for the mesh
services are not built yet. The remaining services (admin) join with the identical posture as their `main.rs` entrypoints land, built on
`services/runtime`; the release **coverage gate** (RT-07) forces each new binary
through the release gate first.

The runtime image's entrypoint (`mg-verify --run-service`) passes the container arguments to the service
(`mimir-server --identity-dir … --state-dir …`). With none, it refuses to start: no
service is ever started on a default address, and `http-gateway` in particular needs
`--state-dir`. The gateway's development mode (`--dev`) listens on loopback only, so it
cannot be served from an image. [MG-SEC-06]

The gateway edge speaks plaintext HTTP, so TLS termination off the host (PQ-mTLS or
TLS 1.3) is mandatory for any route to it other than an SSH tunnel to loopback. The
gateway refuses a bind address that is not loopback unless it is given
`--allow-non-loopback`; compose, the Helm chart, the rendered `deploy/k8s/enclosure.yaml`
and `mg-install` pass it after `--bind 0.0.0.0:7710`, because a container must listen
on its own interface. Compose publishes that port on the host's loopback only. The
console image (`deploy/installer/console.Dockerfile`) refuses to build against a
`GATEWAY_URL` that is plaintext `http://` off loopback, and its nginx serves the same
security headers on every location from `console.security-headers.conf`.
[VLD-ANB-MIDGARD-0014] [VLD-ANB-MIDGARD-0032] [VLD-ANB-MIDGARD-0085]

The gateway limits login, activation and administration attempts per source address
(`--login-burst`, default 20, 0 is off; `--login-per-sec`, default 0.5). Behind the
terminating proxy every client arrives from the proxy's address and so shares one
allowance for the whole site. Pass `--trusted-proxy IP`, once for each proxy address
the gateway sees connections from, and a request from that address is charged to the
client its `X-Forwarded-For` names: the right-most entry that is not itself a trusted
proxy. The proxy must append the address it was reached from (nginx:
`proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`). The header is
ignored from every other peer, and an entry that is not an address makes the gateway
fall back to the proxy's own allowance. Under compose, connections arrive through
docker-proxy, which adds no header, so do not name it. [VLD-ANB-MIDGARD-0005]

A real multi-node `kubectl apply` + cross-host PQ-mTLS smoke belongs to the
**deployment-tier acceptance suite (RT-40)**, run on a staging cluster — it cannot
run inside the offline unit harness.
