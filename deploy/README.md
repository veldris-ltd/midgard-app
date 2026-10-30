# deploy/ — deployment & air-gap bundles

Deployment tooling for the enclosure: hardened, verify-before-run container images,
Kubernetes/Helm manifests, and (RT-19) signed offline bundles. Networking is
default-deny, egress-denied, PQ-mTLS on every internal hop. [NFR-AVA-1] [NFR-PRV-1]
[NFR-SEC-1] SAD_00 §10.

## Layout

- **`images/`** — the per-service runtime image.
  - `service.Dockerfile` — minimal, non-root, read-only-rootfs image built **from
    the signed release artefact** (binary + `.sig` + `.att` + `.sbom.cdx.json`),
    parameterised by `--build-arg SERVICE=<name>`. Base pinned by digest.
  - `entrypoint.sh` — **verify-before-run**: `mg-verify` re-checks the ML-DSA
    signature + SBOM + provenance against the pinned trust key before the service is
    exec'd; a tampered image fails closed and the container refuses to start.
    [AC-B-1] [NFR-PQ-3]
- **`compose/`** — Midgard on one machine with docker compose: the HTTP edge, the
  Mimir inference plane, the Bifrost fleet manager, the Heimdall feed gateway and the
  Forge worker in hardened containers (non-root, read-only rootfs, no
  capabilities), each with only its own volumes. A one-shot `init` creates the
  enclosure and issues each mesh service its identity (`mg-admin init`, `mg-admin mesh
  issue`); the mesh services sit on an internal network and are reachable only over
  PQ-mTLS. `smoke.sh` builds, starts, probes and restarts a throwaway copy. The image
  is built from source (`compose/Dockerfile`), **not** the signed release image.
  [RC-33] [NFR-SEC-1]
- **`installer/`**: `mg-install`, the guided installer for one server: a server check,
  use-case profiles (each with a starter schema, policy and sample Forge jobs), the choice
  of products, first users and administrators enrolled before anything starts, an
  optional web console image, and checks that each chosen service answers. It drives
  `compose/` and adds a site overlay; it also manages the install afterwards (status,
  users, backups, removal). See [installer/README.md](installer/README.md). [RC-33]
- **`kind/`** — Midgard on a local three-node Kubernetes cluster: `up.sh` creates
  the kind cluster, builds and loads the image, installs the chart and provisions
  the secrets (`bootstrap-secrets.sh` mints the enclosure's keys and every mesh
  identity with the same `init` compose uses); `smoke.sh` runs each mesh client as a
  Job through the NetworkPolicies. Built from source, not the signed release images.
  [RC-33]
- **`helm/midgard/`** — the Helm chart for the core services + product edges:
  a default-deny/egress-denied namespace, a `ResourceQuota`, the pinned trust-key
  `ConfigMap`, and one hardened `Deployment` + `Service` (+ ingress `NetworkPolicy`)
  per enabled service — non-root, read-only rootfs, dropped capabilities,
  readiness/liveness probes, resource requests/limits, and a zero-downtime rolling
  strategy (`maxUnavailable: 0`).
- **`k8s/enclosure.yaml`** — the chart rendered with its release values
  (`helm template`), for `kubectl apply` without Helm. Generated: regenerate it when
  the chart changes.
- **`acceptance/`** — the offline acceptance harness (`run-airgapped.sh`, the
  `verify-*.sh` suites, and the pinned toolchain `Dockerfile`).
- **`trust/`** — the pinned public release trust key `mg-verify` checks against.
- **`air-gap-bundle/`** — signed offline release bundles produced/consumed by
  Bifrost (RT-19), verified offline before deploy (AC-B-3). *Not yet present.*

## Applying

```bash
# Render + apply the concrete manifest (or `helm install midgard deploy/helm/midgard`):
kubectl apply -f deploy/k8s/enclosure.yaml
```

The release pipeline (`.github/workflows/release.yml`) builds and PQ-signs one
`service.Dockerfile` image per shippable artefact from the verified bundle.

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

The runtime image's entrypoint passes the container arguments to the service
(`mimir-server --identity-dir … --state-dir …`). With none, it refuses to start: no
service is ever started on a default address, and `http-gateway` in particular needs
`--state-dir`. The gateway's development mode (`--dev`) listens on loopback only, so it
cannot be served from an image. [MG-SEC-06]

A real multi-node `kubectl apply` + cross-host PQ-mTLS smoke belongs to the
**deployment-tier acceptance suite (RT-40)**, run on a staging cluster — it cannot
run inside the offline unit harness.
