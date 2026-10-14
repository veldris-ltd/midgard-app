# Continuous integration and release pipelines

`[NFR-OSS-1]` `[NFR-PRV-1]` `[RC-07]` `[AC-M-6]`

One pipeline, described twice: for GitHub Actions in
[`.github/workflows/`](../../.github/workflows) and for GitLab CI in
[`.gitlab-ci.yml`](../../.gitlab-ci.yml). Both are committed, and each forge reads only
its own, so the repository builds the same way on GitHub, on a GitLab mirror, and on a
fork on either, with nothing to remodel when it moves.

## How it is put together

Neither pipeline file contains build logic. Every step in both is one line:

```bash
bash tools/ci/run.sh <job>
```

What a job does is written once, in [`tools/ci/run.sh`](../../tools/ci/run.sh). The few
things CI systems genuinely do differently — how an error is annotated, how a log
section folds, where the commit, the tag and the merge base are recorded, how a value
reaches a later step — live in [`tools/ci/lib.sh`](../../tools/ci/lib.sh), which works
out which system it is running under from the variables that system always sets
(`GITHUB_ACTIONS`, `GITLAB_CI`), and falls back to a plain shell otherwise.

| File | What it is |
|---|---|
| `tools/ci/run.sh` | every job, by name |
| `tools/ci/lib.sh` | platform detection and the per-system helpers |
| `tools/ci/pipelines.sh` | checks the two pipeline files call the same jobs |
| `tools/ci/in-toolchain.sh` | runs a command in the pinned toolchain image, no network |
| `tools/ci/release.sh` | the release, end to end, with a dry-run mode |
| `tools/ci/release-artefacts.sh` | the release manifest's Rust binaries, derived from the workspace |
| `tools/ci/js-artefacts.sh` | its JavaScript artefacts, and their reproducible build |
| `tools/ci/release-preflight.sh` | asserts a signing runner is provisioned |
| `tools/ci/vendor-check.sh` | checks `/vendor` is exactly what `Cargo.lock` names, and that Cargo is pinned to it |
| `tools/ci/claims.sh` | renders the claims ledger and checks every row against the files, lines and tests it cites |
| `tools/ci/tests/*.test.sh` | tests for the scripts above |

### Keeping the two in step

`bash tools/ci/pipelines.sh --check` fails if:

- a job is called by one pipeline and not the other;
- either calls a job `run.sh` does not define, or `run.sh` defines one neither calls;
- either pipeline runs something that is not a job (build logic in the YAML).

It runs as part of the `selftest` job on both forges, so adding a job on one and
forgetting the other turns both red.

## Running a job yourself

```bash
just ci list
```

```bash
just ci platform
```

```bash
just ci selftest
```

`platform` prints what the scripts detected: the CI system, the forge, the commit, the
tag and the runner. Outside CI the forge comes from the `origin` remote — `github.com`
is GitHub; `gitlab.com`, any host containing `gitlab`, and any host of the form
`git.<domain>` are taken to be GitLab. Set `MIDGARD_FORGE` to override the guess.

Jobs marked `[toolchain]` in `just ci list` run inside the digest-pinned image
(`midgard-acceptance:1.82.0`) with `--network none`, so they need Docker and a Linux
host; build the image first with `just ci toolchain`. See [BUILD_ENV.md](BUILD_ENV.md).

## The jobs

| Job | Runs | What it proves |
|---|---|---|
| `selftest` | host | the CI scripts' own tests pass and both pipelines call the same jobs |
| `provenance` | host | changed sources under governed trees cite a requirement |
| `vendor` | host | `/vendor` is exactly the crates `Cargo.lock` names, and Cargo is pinned to it |
| `claims` | host | every row of `docs/compliance/claims.toml` cites files, lines and tests that exist, and `CLAIMS.md` is what it renders |
| `gate-record` | host | the committed gate record is what its logs give, and the README and GA_READINESS status tables are what it gives |
| `numbers` | host | [`docs/compliance/NUMBERS.md`](../compliance/NUMBERS.md) is what the tree gives, and no swept document quotes a crate, line, test or criteria count that differs from it |
| `secrets` | host | no credential is committed (gitleaks, pinned by version and checksum) |
| `k8s-render` | host | `deploy/k8s/enclosure.yaml` is what the Helm chart renders |
| `toolchain` | host | builds or restores the toolchain image — the only online step |
| `fmt`, `lint` | toolchain | formatting; clippy at deny-warnings |
| `build`, `test` | toolchain | the release build; the test suite on both crypto backends, its log kept in `target/gate` |
| `repro` | toolchain | two normalised builds of the workspace are bit-identical |
| `scan` | toolchain | advisory and licence scan of the dependency graph, PR mode |
| `acceptance` | host | the full acceptance suite under a genuine air gap, its log kept in `target/gate` |
| `js-build` | host | every UI bundle and npm package built twice, bit-identical |
| `release-dry-run` | toolchain | the whole release gate, throwaway keys, nothing published |
| `release-preflight` | host | the signing runner is provisioned |
| `release` | toolchain | every artefact gated, admitted to Bifrost, and accounted for |
| `release-images` | host | the verify-before-run images, each built twice to the same digest |
| `release-bundle` | toolchain | the signed air-gap bundle |
| `release-publish` | host | a draft pre-release (GitHub) or package-registry upload (GitLab) |
| `shell-audit` | host | one client shell built and audited on a real runner |
| `shell-audit-gate` | host | every shell has a fresh, passing audit record |

## When each runs

| Trigger | GitHub Actions | GitLab CI |
|---|---|---|
| a pull or merge request | `ci.yml`: hygiene, lint, test, repro, scan | the same jobs |
| …that touches release machinery or a UI | `release.yml`: `js-build`, `release-dry-run` | `js-build`, `release-dry-run` |
| a push to `main` or `dev` | `ci.yml`, plus `acceptance` | the same, plus `acceptance` |
| a `v*` tag | `release.yml`: the release | the release |
| by hand | *Run workflow* → `mode`: `dry-run` or `release` | *Run pipeline* → `MIDGARD_RELEASE_MODE`: `dry-run`, `release` or `audit` |

## The gate record

The acceptance gate is two runs in the toolchain image with no network: the acceptance
suite (`deploy/acceptance/run-airgapped.sh`), which prints a verdict for each acceptance
criterion, and `just test`. The `acceptance` and `test` jobs make those runs through
[`tools/ci/gate-run.sh`](../../tools/ci/gate-run.sh), which leaves each run's complete
log and its metadata (commit, image, toolchain, times, exit status) in `target/gate`.
Both pipelines keep that directory as an artefact: `gate-acceptance` and `gate-test`.

[`tools/ci/gate-record.sh`](../../tools/ci/gate-record.sh) turns one such run into a
record under [`docs/compliance/gate-runs/`](../compliance/gate-runs/README.md), and
rewrites the status tables in the README and `GA_READINESS.md` from it. It refuses a run
that does not give every criterion of the acceptance spec a verdict. To record the run
CI made of a pushed commit:

```bash
gh run download <run-id> -n gate-acceptance -D target/gate
gh run download <run-id> -n gate-test -D target/gate
just gate-record
```

or make the run locally, on a clean checkout of the commit: `just gate-run`, then
`just gate-record`. The record names the commit that was gated, so it is committed
after it. The `gate-record` job checks the committed record on every change, and
`gate-record --release`, in the release's preflight, also requires that the commit
being released differs from the gated commit only in the record and the prose
generated from it.

Every headline figure — crates, lines of Rust, test functions, criteria, and the
gate run's results — is generated into
[`docs/compliance/NUMBERS.md`](../compliance/NUMBERS.md) by
[`tools/ci/numbers.sh`](../../tools/ci/numbers.sh). After a change that moves a
figure, `just numbers --write`; the `numbers` job fails until the sheet is the
tree's, and fails on any swept document that quotes a different figure.

## The release

[`tools/ci/release.sh`](../../tools/ci/release.sh) is the whole release as one script:

1. **Derive the manifest.** The set of artefacts comes from the workspace's own
   manifests, so one added to the workspace is gated with no edit to any pipeline:
   - every Rust binary (`just release-manifest`). `supply-chain/release-artefacts.txt`
     only annotates that set — component kind and image — and cannot hide a binary or
     invent one;
   - every JavaScript artefact (`bash tools/ci/js-artefacts.sh --rows`): a workspace
     package with a Vite config ships as a `ui-bundle`, one under `sdk/` as an
     `npm-package`. Internal libraries ship inside the bundles that import them.
2. **Gate each artefact** into one fresh directory. The five stages are the same for
   every kind — a reproducible build, an SBOM, the advisory and licence scan, an
   ML-DSA signature, verification — and each artefact keeps its own bundle beside it.
3. **Admit each to the Bifrost registry.** `mg-register` re-verifies the release bundle
   against the pinned release key before signing anything for admission, and records
   the artefact together with its signature, provenance and reproducibility report.
4. **Compare the published set with the gated set.** The registry is asked what it
   holds for this version. An artefact gated and not there, or there and not gated,
   fails the release.

### The five stages, by kind

| Stage | Rust binary | UI bundle, npm package |
|---|---|---|
| reproducible build | `mg-repro`: two normalised builds, compared | `tools/ci/js-artefacts.sh build`: two builds, compared — a deterministic tar of `dist/`, or the packed tarball |
| SBOM | `mg-sbom`, from `Cargo.lock` | `mg-sbom --pnpm-lock`, the importer's production closure from `pnpm-lock.yaml` |
| scan | `mg-scan` against RustSec (`vendor/advisory-db`) | `mg-scan` against the OSV npm snapshot (`vendor/advisory-db-npm`) |
| sign, verify | `mg-sign`, `mg-verify` | the same tools, unchanged |

A component is matched only against its own ecosystem's advisories, read from its
purl, so a crate and an npm package that share a name are never confused. An npm
package the snapshot was not filtered for is a blocking finding; refresh it with
`just advisory-db-npm-refresh` whenever `pnpm-lock.yaml` gains a package.

The JavaScript artefacts are built where node is — the Rust toolchain image has none —
by the `js-build` job, which leaves them in `target/js-out` with the source revision
they were built from. The release then gates them inside the toolchain image and
refuses any built from a different revision. `pnpm install --frozen-lockfile` is that
job's online step, as building the toolchain image is for Rust: it installs exactly
what the lockfile pins, by integrity hash, or fails.

The container images get the same reproducibility stage: `deploy/images/build.sh`
builds each one twice, uncached, with a fixed `SOURCE_DATE_EPOCH` and rewritten file
times, and requires the two image digests to be equal.

### Dry run

```bash
just release-dry-run
```

```bash
just release-dry-run mg-sbom,midgard-sdk
```

A dry run does every step above with throwaway keys and a throwaway registry created
outside the checkout and removed afterwards. Nothing it signs is trusted by anything
and nothing is published. It runs automatically on any pull or merge request that
touches the release machinery, and by hand on any branch.

### A real release

Needs a signing runner provisioned as [docs/ops/PKI.md](../ops/PKI.md) describes —
labelled `[self-hosted, kms]` on GitHub, tagged `kms` on GitLab — with three settings
naming files on that runner:

| Setting | What |
|---|---|
| `MIDGARD_SIGNING_KEYSTORE_PATH` | the release signing keystore (ML-DSA-87) |
| `MIDGARD_BIFROST_KEYSTORE_PATH` | the Bifrost admission keystore (ML-DSA-65) |
| `MIDGARD_BIFROST_REGISTRY_PATH` | the Bifrost registry directory |

On GitHub these are repository secrets; on GitLab, CI/CD variables. The public halves
are committed as `deploy/trust/release-trust.pub` and `deploy/trust/bifrost-trust.pub`.
The keystores are mounted read-only into the toolchain container; nothing else of the
runner's environment reaches it.

A release also waits on the client shell audits, which fail closed until every shell
has a buildable project (see [`clients/shells/AUDIT_SCHEMA.md`](../../clients/shells/AUDIT_SCHEMA.md)).

Publishing is deliberately limited. On GitHub, `release-publish` creates a **draft**
pre-release. GitLab has no draft state, so there the files are uploaded to the
project's package registry and creating the release entry is left to a person. Either
way the gate record, its raw logs and the numbers sheet go with the binaries.

### Cutting the tag

A release is built from a signed, annotated tag on the tip of `main`, and from nothing
else. On the commit to be released, with your signing key configured:

```bash
bash tools/ci/tag-release.sh --check     # what is not ready, all at once
bash tools/ci/tag-release.sh             # make the signed tag, verify it, print the push
```

It names the tag from the workspace version and makes it only when the tree is clean
and is `origin/main`, the gate record covers the commit (`gate-record --release`), the
numbers sheet and the claims ledger check, the release notes exist, and
`deploy/trust/allowed_signers` is published. It does not push. On the runner, the
`release-tag` job ([`tools/ci/verify-tag.sh`](../../tools/ci/verify-tag.sh)) verifies
the tag against `deploy/trust/allowed_signers` before anything is built or signed, so
an unsigned tag, a lightweight tag, or one signed by a key not listed there starts no
release.

Because the gate record names the commit it gated, the order is: merge to `main`; run
the gate on that commit and record it (see "The gate record"); write the numbers sheet;
merge that records-only change; tag its result. `gate-record --release` accepts the
tagged commit only when it differs from the gated one in the record and the prose
generated from it.

### Repository rules

The forge enforces what the scripts cannot: that history on the release path is
signed, and that a published tag stays put. These are the repository owner's to set.
On GitHub, as rulesets (Settings → Rules → Rulesets), or with `gh`:

```bash
gh api -X POST repos/veldris-ltd/midgard-app/rulesets --input - <<'EOF'
{ "name": "signed history", "target": "branch", "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/heads/main", "refs/heads/dev"], "exclude": [] } },
  "rules": [ { "type": "required_signatures" }, { "type": "non_fast_forward" }, { "type": "deletion" } ] }
EOF
gh api -X POST repos/veldris-ltd/midgard-app/rulesets --input - <<'EOF'
{ "name": "release tags are immutable", "target": "tag", "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/tags/v*"], "exclude": [] } },
  "rules": [ { "type": "deletion" }, { "type": "non_fast_forward" }, { "type": "update" } ] }
EOF
```

`non_fast_forward` on the branches also refuses force pushes, so a classic protection
rule that allows them on `dev` must be turned off as well. On GitLab: protect `v*` under
Settings → Repository → Protected tags with no one allowed to create them but
maintainers, and turn on *Reject unsigned commits* in the push rules (a Premium feature).

## What a GitLab instance needs

- A Docker executor with Docker-in-Docker allowed, for the jobs that build and use the
  toolchain image. The checkout is mounted into the toolchain container from the job's
  build directory, which GitLab shares with the `dind` service.
- A `kms`-tagged shell-executor runner with Docker, node and pnpm, for releases.
- A `macos`-tagged runner for the `apple` shell audit. Without one that job waits, and
  the release with it.

## What has and has not been exercised

The scripts are tested and were run locally: the `selftest`, `provenance` and
`js-build` jobs, and a release dry run covering Rust binaries, the SDK package and the
UI bundles. The pipeline files themselves parse as YAML and pass the parity check, but
**they have not run on GitHub or GitLab** — the first run on each forge is their real
test. Three things to expect on that first run:

- The hosted runners' disk. Two release builds of the workspace plus the debug build
  are large; if a job runs out of space, split it or use a larger runner.
- The container images named in `.gitlab-ci.yml` are pinned by tag, not by digest.
- `release-images`, `release-bundle` and `release-publish` have never run end to end:
  they need the signing runner, the service base image and real keys. The image
  build's reproducibility was checked here on a stand-in base image, not on the real
  one.
