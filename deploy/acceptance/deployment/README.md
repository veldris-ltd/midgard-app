# Deployment-tier acceptance (RT-40 / RC-40)

Midgard has **three** acceptance tiers. The first two run against source on a
single host and prove correctness offline; this third tier proves the platform
works **deployed on real infrastructure**, and it is the gate a release must clear
before it is promoted.

| Tier | Command | Runs where | Proves |
|------|---------|-----------|--------|
| Unit / integration | `just test` | one host, no infra | logic, crypto KATs, fail-closed paths |
| Air-gapped suite | `bash deploy/acceptance/run-airgapped.sh` | one host, `--network none` | whole platform builds+verifies with zero egress |
| **Deployment tier** | `just deploy-accept` | **real staging cluster** | clustering, cross-host PQ-mTLS, HSM signing, live GPU inference |

The deployment tier is deliberately **separate from `just test`**: it cannot run
in the offline toolchain container because it needs a multi-node cluster, a
hardware security module, and a GPU. Those are staging-environment resources.

## The four checks (RC-40)

| Check | ID | What actually runs |
|-------|----|--------------------|
| Clustering | RC-40.1 | Every Midgard StatefulSet must be rolled out and ready; then every service is restarted and each must come back ready — re-verifying its ledger and recovering its state from its own volume. Each service holds a single-writer, locked state directory, so it runs as one replica: its availability rests on that recovery, not on replicas. |
| Cross-host PQ-mTLS | RC-40.2 | Applies [`pqmtls-probe.yaml`](pqmtls-probe.yaml): a responder Pod + an anti-affine initiator Job running the real `pqmtls-probe` binary. A genuine hybrid X25519+ML-KEM-768 handshake + record round-trip **between two nodes**. The runner also asserts the two pods landed on different hosts. |
| HSM signing | RC-40.3 | Drives the configured PKCS#11 token (`MIDGARD_PKCS11_MODULE`), confirms the release key is present, and produces + checks a real hardware signature. |
| Live inference | RC-40.4 | Applies [`inference-probe.yaml`](inference-probe.yaml): one certified call to the deployed `mimir-server` with `mimir-probe`'s mesh identity, over PQ-mTLS through the enclosure's NetworkPolicies. Inference has no HTTP endpoint to call (FR-MI-8). The engine `mimir-server` logged at start must be a real model, not the reference engine, and it must answer with a non-empty completion. The check passes only if `mimir-server` has a GPU (`nvidia.com/gpu` limit) and the first token came within 800 ms (NFR-PERF-1). Without a GPU, the live CPU completion and its latency are reported and the GPU is the missing dependency. |

## Honesty contract — why this suite cannot lie

This tier exists precisely because everything else is offline. It therefore holds
itself to a strict rule: **no PASS without the real dependency.**

- A **strict preflight** probes for a reachable cluster (and reports HSM presence).
  With no cluster it **fails closed** (exit 2) — it does not "pass vacuously".
- Each check that finds its dependency missing calls `missing`, which **fails the
  suite** (fail closed) unless you opt into `ALLOW_SKIP=1`.
- `ALLOW_SKIP=1` turns a *missing* dependency into a visible **SKIP** so a partial
  staging environment can exercise the checks it can. **SKIP is never counted as
  PASS.** Any check that actually *runs* and fails still fails the suite.

Exit codes make the honesty contract concrete:

| Exit | Meaning | Promotable? |
|------|---------|-------------|
| `0` | all four checks genuinely ran and passed | **yes** |
| `1` | a check ran and failed, or nothing could run (all skipped) | no |
| `2` | preflight fail-closed — no reachable cluster | no |
| `3` | partial — some checks passed, others skipped (missing infra) | no |

So the only way to get a green (exit 0) is a real cluster where all four checks
genuinely ran and passed. This is by design and must not be relaxed.

## Prerequisites (operator-provided, staging)

```
KUBECONFIG               admin/deployer context for the staging cluster
MIDGARD_NAMESPACE        default: midgard-enclosure
MIDGARD_PKCS11_MODULE    path to the HSM PKCS#11 .so
MIDGARD_HSM_KEY_LABEL    token label of the release signing key (default: midgard-release)
MIDGARD_PROBE_IMAGE      optional: an image carrying pqmtls-probe and mimir-probe
```
Tools on PATH: `kubectl`, `pkcs11-tool` (opensc).

In the namespace, the live inference check needs the Secret `mimir-probe-identity` (the
mesh identity `mg-admin mesh issue --service mimir-probe` writes), and `mimir-server`
serving a real model through the chart's `modelServing`.

The `pqmtls-probe` image (`registry.enclosure.local/midgard/pqmtls-probe:0.0.0`)
is built from the workspace with the probe binary baked in:

```bash
cargo build --release -p pq-mtls --features backend-fips --example pqmtls_probe
# COPY target/release/examples/pqmtls_probe -> /usr/local/bin/pqmtls-probe in the image
```

`MIDGARD_PROBE_IMAGE` substitutes another image carrying the probe at that path. The
development image `deploy/compose/Dockerfile` builds (`midgard/dev:local`) does.

## Running

```bash
# Full gate (fails closed if the cluster is unreachable):
KUBECONFIG=~/.kube/staging just deploy-accept

# Partial run on incomplete staging (missing infra → visible SKIP, exits 3):
ALLOW_SKIP=1 KUBECONFIG=~/.kube/staging just deploy-accept
```

A local kind cluster (`just kind-up`) exercises clustering and cross-host PQ-mTLS.
Once `just kind-granite` serves a real model, it also makes RC-40.4's certified
inference call and reports the CPU completion and latency. It has no HSM or GPU, so
RC-40.3 and RC-40.4 SKIP and the best result is exit 3 — never a promotion:

```bash
ALLOW_SKIP=1 MIDGARD_PROBE_IMAGE=midgard/dev:local just deploy-accept
```

## Release promotion

Promotion is not automated. `.github/workflows/release.yml` gates every binary,
builds the images and the air-gap bundle, and publishes at most a draft
**pre-release**; it deploys nothing. Before a release candidate becomes a final
version:

1. Install that release's bundle on a staging cluster with HSM and GPU nodes, using
   `deploy/air-gap-bundle/install.sh`.
2. Run this suite against it, and record the result in
   `docs/compliance/GA_READINESS.md`.

Only exit 0 permits promotion: all four checks genuinely ran and passed. This is the
first point where the four real-infrastructure properties are proven end to end.

## What this tier does NOT claim

- It is **not** a certification (FIPS 140-3 module cert, CC, ISO). Those are lab
  activities on top of these controls — see `docs/compliance/COMPLIANCE_MATRIX.md`.
- The cross-host mTLS check uses **ephemeral, deterministic** identities to prove
  the transport+crypto path; production PKI/custody is a separate concern proven
  by the HSM check and the CA lifecycle (RT-35).
