# Verifying Midgard

How to build and test the workspace, run the air-gapped acceptance gate, and take a binary through the supply-chain release gate. The [README](../../README.md) lists the everyday commands.

## Build and test

```bash
just build      # cargo build --workspace --release; Go/TS/Python members build themselves
just test       # runs BOTH crypto backends explicitly (deterministic test-backend + FIPS 203/204/205)
```

> `just test` deliberately does **not** use `cargo test --workspace --all-features`: the crypto crates gate their backend behind mutually-exclusive features, and a naive workspace test would compile those tests out. Use `just test`.

Without a local toolchain:

```bash
docker run --rm -v "$PWD":/work -w /work midgard-acceptance:1.82.0 just build
```

## The air-gapped acceptance gate

Each of the 72 acceptance criteria maps to a test, and this builds and runs them all with the network cut. Most criteria are exercised against libraries rather than the running services; the [claims ledger](../compliance/CLAIMS.md) says which. The latest recorded run is in [docs/compliance/gate-runs/](../compliance/gate-runs/README.md). One command bakes the pinned image (the only online step) and runs everything under `docker --network none`:

```bash
bash deploy/acceptance/run-airgapped.sh
#   Windows Git Bash:
#   MSYS_NO_PATHCONV=1 REPO='D:\path\to\midgard' bash deploy/acceptance/run-airgapped.sh
```

Expect a tail of `>>> FULL SUITE PASSED, AIR-GAPPED (--network none).` and exit `0`. Allow roughly two hours from cold: it compiles the workspace several times over. Any single failing criterion fails the whole run.

Individual phase gates, each mapping its criteria to concrete tests and reporting PASS/FAIL per id:

```bash
# inside the pinned container, from the workspace root
bash deploy/acceptance/verify-core.sh        # AC-M-1..9  (Midgard core)
bash deploy/acceptance/verify-mimir.sh       # AC-MI-1..8 + AC-MS-1..9 except AC-MS-3
bash deploy/acceptance/verify-forge.sh       # AC-F-1..7
bash deploy/acceptance/verify-bifrost.sh     # AC-B-1..6 + AC-MS-3
bash deploy/acceptance/verify-heimdall.sh    # AC-H-1..7
bash deploy/acceptance/verify-admin.sh       # AC-AD-1..13
bash deploy/acceptance/verify-clients.sh     # AC-C-1..13
bash deploy/acceptance/verify-shells-accessibility.sh   # AC-C-11 (static HIG conformance)
```

`run-airgapped.sh` covers everything above except the **on-device shell audit**, which is device-bound and cannot run in a container:

```bash
bash deploy/acceptance/verify-shell-audits.sh <release>   # e.g. 1.4.0
```

It does not perform an audit: it verifies that a real one ran and was recorded for every shell, and fails closed otherwise. See [BUILD_PLAYBOOK.md, Appendix B](../../BUILD_PLAYBOOK.md#appendix-b--consolidated-acceptance-criteria-matrix-72) for the full matrix.

## The supply-chain release gate

Every binary the workspace builds passes the same fail-closed chain: reproduce → SBOM → scan → sign → verify.

```bash
just keygen                                   # once: ML-DSA keypair + exported trust anchor
export MIDGARD_SIGNING_KEYSTORE=mg.keystore   # never searched for; unset means refuse to sign
export MIDGARD_TRUST_ANCHORS=mg.pub           # never searched for; unset means refuse to verify
just gate http-gateway                        # the full chain for one package's binary
just gate forge-worker forge-egress           # a package that builds several binaries, one at a time
```

`just gate` builds the package **twice** under a normalised environment, proves the outputs bit-identical, and then SBOMs, signs and verifies *that proven artefact*: signing anything else would produce a signature over a binary nobody can reproduce from source. `supply-chain/repro-exceptions.txt` must be empty at GA; it is.

The set of artefacts to release is derived from the workspace (`just release-manifest` for the binaries, `tools/ci/js-artefacts.sh --rows` for the UI bundles and the SDK package), so a new one cannot skip the gate; `supply-chain/release-artefacts.txt` only records how each one ships. `just release-dry-run` runs the whole release with throwaway keys and publishes nothing. On a provisioned signing runner, the release pipeline (`.github/workflows/release.yml`, or `.gitlab-ci.yml` on a GitLab mirror) does the following:

1. Gates each artefact (every Rust binary, UI bundle and npm package) through the same five stages.
2. Admits each to the Bifrost registry, and fails unless the registry then holds exactly the gated set.
3. Builds the verify-before-run images.
4. Packs the signed air-gap bundle.
5. Publishes at most a draft pre-release.

Both pipelines call the same jobs in `tools/ci/run.sh`; see [docs/dev/CI.md](../dev/CI.md).

See [the release candidate notes](../releases/v0.1.0-rc.1.md) and `docs/ops/PKI.md`.

At run time, each release image's entrypoint, `mg-verify --run-service` on a distroless runtime with no shell, re-verifies the signature, SBOM and provenance against the pinned release trust key before it runs any service, so a tampered or unsigned binary refuses to start.
