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
| `tools/ci/in-toolchain.sh` | runs a command in the pinned toolchain image, no network; in place when already inside it |
| `tools/ci/toolchain.digest` | the digest of the published toolchain image GitLab's merge-request jobs run in |
| `tools/ci/release.sh` | the release, end to end, with a dry-run mode |
| `tools/ci/release-artefacts.sh` | the release manifest's Rust binaries, derived from the workspace |
| `tools/ci/js-artefacts.sh` | its JavaScript artefacts, and their reproducible build |
| `tools/ci/release-preflight.sh` | asserts a signing runner is provisioned |
| `tools/ci/vendor-check.sh` | checks `/vendor` is exactly what `Cargo.lock` names, and that Cargo is pinned to it |
| `tools/ci/claims.sh` | renders the claims ledger and checks every row against the files, lines and tests it cites |
| `renovate.json` | the weekly dependency updates for both ecosystems (see [Dependency updates](#dependency-updates)) |
| `tools/ci/shell-audit/` | the exact versions, in a lockfile, of the tools the shell-audit job runs |
| `tools/ci/tests/*.test.sh` | tests for the scripts above |

### Keeping the two in step

`bash tools/ci/pipelines.sh --check` fails if:

- a job is called by one pipeline and not the other;
- either calls a job `run.sh` does not define, or `run.sh` defines one neither calls;
- either pipeline runs something that is not a job (build logic in the YAML);
- a GitLab `image:` or service is not pinned as `name:tag@sha256:<64 hex>` (after
  expanding `$CI_REGISTRY_IMAGE`), or a GitHub `uses:` is not pinned to a 40-hex commit.
  The one reference by variable allowed is the published toolchain image,
  `$CI_REGISTRY_IMAGE/toolchain@$MIDGARD_TOOLCHAIN_DIGEST`, whose jobs check the
  variable first. Each offending line is printed. `[VLD-ANB-MIDGARD-0022]`

It runs as part of the `selftest` job on both forges, so adding a job on one and
forgetting the other turns both red.

### Checkouts and the tools jobs fetch

Every `actions/checkout` step sets `persist-credentials: false`. No job pushes, and
`release-publish` passes its token to `gh` in the environment, so nothing needs the
job token in `.git/config`, where it would otherwise stay for the rest of the job: on
the self-hosted signing runner, beside the key material. The checkout fetches first,
so `fetch-depth: 0` and the history checks are unaffected. `tools/ci/tests/workflows.test.sh`
fails if a checkout step leaves the option out, and the signing runner's own cleanup
is item 5 of [docs/ops/PKI.md](../ops/PKI.md) "Signing runner", asserted by the release
preflight. (VLD-ANB-MIDGARD-0081)

A job never runs a package fetched by name. The shell-audit job's `http-server` and
`wait-on` are pinned to exact versions in `tools/ci/shell-audit/package-lock.json`,
installed with `npm ci --ignore-scripts` and run from its `node_modules/.bin`; see that
directory's README to bump them. `tools/ci/tests/npx-pin.test.sh` fails on `npx --yes`,
`npx -y`, or an `npx` of a package with no exact version, anywhere under `tools/`,
`.github/` or in `.gitlab-ci.yml`. (VLD-ANB-MIDGARD-0058)

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
| `signed-commits` | host | every commit in the change (base..HEAD) is signed by a key in `deploy/trust/allowed_signers`; fails naming each commit that is not |
| `k8s-render` | host | `deploy/k8s/enclosure.yaml` is what the Helm chart renders |
| `toolchain` | host | builds or restores the toolchain image — the only online step; inside the published image, checks it is the one `tools/ci/toolchain.digest` pins |
| `toolchain-publish` | host | GitLab only: pushes the toolchain image to the project registry; fails until its digest is committed. On GitHub, a no-op |
| `fmt`, `lint` | toolchain | formatting; clippy at deny-warnings |
| `build`, `test` | toolchain | the release build; the test suite on both crypto backends, its log kept in `target/gate` |
| `check-backends` | toolchain | `just check-backends`: neither the workspace build nor any package in the release matrix resolves the insecure `test-backend`, a release build with it fails on the compile guard, and the FIPS release build succeeds |
| `repro` | toolchain | two normalised builds of the workspace are bit-identical |
| `scan` | toolchain | advisory and licence scan of the dependency graph, PR mode |
| `advisory-refresh` | host | weekly: both advisory snapshots refreshed, every SBOM re-scanned, and a pull or merge request opened with the scan diff when a snapshot moved |
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
| weekly, Mondays 03:17 UTC | `advisory-refresh.yml`: `advisory-refresh` | a pipeline schedule: `advisory-refresh`, and nothing else |
| by hand | *Run workflow* → `mode`: `dry-run` or `release` | *Run pipeline* → `MIDGARD_RELEASE_MODE`: `dry-run`, `release` or `audit` |
| by hand on `main` or `dev`, no mode | — (the toolchain image is built per run) | `toolchain-publish` |

## Dependency updates

`[AC-P4.4]` `[AC-P4.5]` (VLD-ANB-MIDGARD-0076)

Dependencies and the two advisory snapshots move every week, each as a pull or merge
request a person reviews. Nothing floats: every update lands in a pinned file (the
lockfiles, `Cargo.lock` with `/vendor`, an action's commit SHA, an image digest), and
nothing merges itself.

**Renovate** ([`renovate.json`](../../renovate.json)) runs on Monday mornings, at most
three open at a time, labelled `dependencies`, and only for releases at least 14 days
old (the same rule the lockfiles were built to). Its managers:

| Manager | Covers |
|---|---|
| `npm` | the pnpm workspace (`pnpm-lock.yaml`), and `docs/launch/pipeline` with its own `package-lock.json`, grouped as "launch pipeline" |
| `cargo` | every `Cargo.toml` in the workspace, re-vendored as described below |
| `github-actions` | the actions in `.github/workflows/`, kept pinned to a full commit SHA (`pinDigests`) |
| `gitlabci` | the images and services in `.gitlab-ci.yml`, kept pinned by digest (`pinDigests`) |
| `custom.regex` | the digest-pinned base images of the Dockerfiles under `deploy/`: digest refreshes of the tag they name; a new Rust or Node line stays a deliberate change |

Commits are made through the forge API (`platformCommit`), so they carry the forge's
signature. `tools/ci/tests/renovate.test.sh` (part of `selftest`) checks this shape.
The forge setup is the owner's: install the Renovate app on the GitHub repository, or
run a self-hosted Renovate against both forges.

**A Cargo update** cannot be finished by Renovate in place: crates are vendored, and
`.cargo/config.toml` points Cargo at `/vendor`, so `Cargo.lock` cannot be resolved
against the registry and `/vendor` would not match it anyway. On a **self-hosted**
Renovate whose global `allowedCommands` admits them, the cargo rule's
`postUpgradeTasks` do it: set the source replacement aside, `cargo update --package
<crate> --precise <version>`, `cargo vendor --versioned-dirs vendor`, restore the
replacement, run `tools/ci/vendor-check.sh`, and commit `Cargo.lock` and `/vendor`
to the pull request. **Hosted** Renovate cannot run commands, so its pull request
carries the manifest change only and the `vendor` job fails it, which is correct: it
is not mergeable yet. A maintainer finishes it on the branch, online:

```bash
git switch <renovate-branch>
# comment out the [source] block in .cargo/config.toml
cargo update --package <crate> --precise <version>
cargo vendor --versioned-dirs vendor > /dev/null
# restore the [source] block
bash tools/ci/vendor-check.sh
git add Cargo.lock vendor && git commit -S -m "chore(vendor): vendor <crate> <version>"
git push
```

Then let CI build and test it offline as usual. `zeroize` and `base64ct` are held below
1.9 and 1.7 for Rust 1.82 ([BUILD_ENV.md](BUILD_ENV.md)); the toolchain build catches
any other crate that needs a newer compiler.

**An npm update that adds a package** passes CI but is refused by the release gate
(`advisory-uncovered`): the npm advisory snapshot is filtered to the packages the
lockfile named when it was made. Run `just advisory-db-npm-refresh` (online) on the
branch and commit `vendor/advisory-db-npm` with it.

**The advisory snapshots** are refreshed by the `advisory-refresh` job, every Monday at
03:17 UTC: `.github/workflows/advisory-refresh.yml` on GitHub, and on GitLab a pipeline
schedule on the default branch, which runs that job and nothing else. The job clones the
RustSec database and downloads the OSV npm export on the runner, runs
`just advisory-db-refresh` and `just advisory-db-npm-refresh` from those inside the
offline toolchain image, scans the Rust workspace and every JavaScript artefact before
and after, and, when either snapshot moved, commits them to
`deps/advisory-refresh-<date>` as "chore(advisories): refresh snapshots <date>" and opens
a pull or merge request whose body is the scan diff. When nothing moved it says so and
opens nothing (`tools/ci/tests/advisory-refresh.test.sh`).

Both snapshots warn at 30 days and the scan refuses them past 45
(`fail_after_days` in each `snapshot.toml`), so a release attempted after six weeks
without a refresh fails closed until someone runs one.

The write access is the scheduled job's alone. On GitHub the workflow grants
`contents: write` and `pull-requests: write` to that job, and passes the workflow
token to it through the environment, never persisted in the checkout; a pull request
opened with that token does not start other workflows, so close and reopen it (or push
to it) to run CI. On GitLab `CI_JOB_TOKEN` can neither push nor open a merge request:
the owner creates a project access token with `api` scope, stores it as the protected,
masked CI/CD variable `MIDGARD_MR_TOKEN`, and creates the weekly pipeline schedule. The
token reaches `git` through a credential helper and `curl` through a header file, never
a command line. The bot's commit is unsigned; squash-merge it through the forge (which
signs the result) or re-sign it before merging into a branch that requires signatures.

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
are committed as `deploy/trust/release-trust.pub` and `deploy/trust/bifrost-trust.pub`,
beside `deploy/trust/model-signing.pub`, the model-signing key's public half. The
preflight (`release-preflight`) refuses a release unless all three are committed, each
with a key of the right algorithm and nothing private, so a release cannot ship without
the anchor mimir-server's load gate trusts. [VLD-ANB-MIDGARD-0028]
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
[`tools/ci/gitlab-rules.sh`](../../tools/ci/gitlab-rules.sh) applies all of it through
the API with a maintainer's token, and `--dry-run` shows the calls first:

```bash
GITLAB_TOKEN=... bash tools/ci/gitlab-rules.sh --url https://gitlab.example.org --project <group>/midgard-app
```

## What a GitLab instance needs

`[VLD-ANB-MIDGARD-0022]` `[VLD-ANB-MIDGARD-0043]`

- **An unprivileged Docker executor for merge requests.** The Rust jobs (`lint`,
  `test`, `repro`, `scan`, `release-dry-run`: everything that `extends: .toolchain`)
  run directly inside the published toolchain image,
  `$CI_REGISTRY_IMAGE/toolchain@$MIDGARD_TOOLCHAIN_DIGEST`, with no Docker service and
  `MIDGARD_IN_TOOLCHAIN=1`, so `tools/ci/in-toolchain.sh` runs each command in place.
  Their first step, `run.sh toolchain`, fails unless `MIDGARD_TOOLCHAIN_DIGEST` is the
  digest committed in [`tools/ci/toolchain.digest`](../../tools/ci/toolchain.digest).
- **A privileged Docker executor with Docker-in-Docker, for protected refs only.** Tag
  it `protected` and make it available only to protected branches and tags. Two jobs
  use it (`extends: .docker`): `toolchain-publish`, by hand, and `acceptance`, on
  pushes to `main` and `dev`, which starts its own containers under `--network none`.
  No merge-request job runs on it.
- **The CI/CD variable `MIDGARD_TOOLCHAIN_DIGEST`** (protected; masking not needed),
  set to the digest line of `tools/ci/toolchain.digest`.
- **Pipelines for fork merge requests require a maintainer's approval** (Settings →
  CI/CD → General pipelines), so code from a fork runs only once a maintainer has read
  it.
- A `kms`-tagged shell-executor runner with Docker, node 22.12 or later and pnpm, for releases. The
  release jobs (`release`, `release-images`, `release-bundle`) use the Docker on that
  host, not a `dind` service.
- A `macos`-tagged runner for the `apple` shell audit. Without one that job waits, and
  the release with it.
- For the weekly `advisory-refresh`: a pipeline schedule on the default branch and the
  `MIDGARD_MR_TOKEN` variable ([Dependency updates](#dependency-updates)). The job uses
  the Docker-in-Docker executor and needs to reach github.com and
  osv-vulnerabilities.storage.googleapis.com.

### Publishing the toolchain image

Merge-request jobs no longer build the toolchain image; they run the one published
from a protected branch. To publish it, the first time and whenever
`deploy/acceptance/Dockerfile` changes:

1. On `main` (or `dev`), *Run pipeline* with `MIDGARD_RELEASE_MODE` empty. The
   `toolchain-publish` job builds the Dockerfile, pushes it as
   `$CI_REGISTRY_IMAGE/toolchain:<dockerfile-sha16>` and prints the digest it was
   given. It fails while that digest is not the one committed.
2. Commit the two lines it prints to `tools/ci/toolchain.digest` through a merge
   request, and set `MIDGARD_TOOLCHAIN_DIGEST` to the same digest. Until the variable
   and the file agree, every `.toolchain` job fails, closed.
3. Running it again is safe: it finds the committed image published from this
   Dockerfile and pushes nothing. Keep that digest in the registry (exclude
   `toolchain` from cleanup policies), or merge-request jobs cannot start.

Until the first publish, merge-request pipelines on GitLab fail at that first step:
the file holds no digest yet, by design.

### Offline, without a network sandbox

A job inside the published image has no Docker daemon to start a `--network none`
container with, so on GitLab merge requests the offline property rests on Cargo: every
crate resolves from `/vendor` (`.cargo/config.toml`, held to `Cargo.lock` by the
`vendor` job in the hygiene stage), and `CARGO_NET_OFFLINE=true` (set by `.toolchain`
and by `in-toolchain.sh` in place) makes every cargo invocation `--offline`, so a step
that reached for a crate fails rather than fetching it. A script that opened a socket
itself would not be stopped; that is the residual recorded as "CI runner compromise"
in the [residual-risk register](../compliance/RESIDUAL_RISK_REGISTER.md).
`--network none` stays wherever a daemon is: on GitHub, on developers' machines, on
the `kms` release runners and in GitLab's `acceptance` job.

The gate record still requires `--network none` for both its parts. GitLab's `test`
job records `network=offline`, which `gate-record.sh` refuses, so make the record from
a GitHub run or a local `just gate-run`.

### Pinned images

Every image and service in `.gitlab-ci.yml` is pinned by the multi-arch index digest
the registry served for its tag on the date recorded beside it. Refresh a pin
deliberately:

```bash
docker buildx imagetools inspect node:22-bookworm --format '{{json .Manifest.Digest}}'
```

and record the new date. `node:22-bookworm` must stay at Node 22.12 or later.

The images are still pulled from Docker Hub. The owner should mirror them into the
project registry and switch each reference to
`$CI_REGISTRY_IMAGE/mirror/<name>:<tag>@sha256:<digest>`, keeping the same digest.

### Owner actions, and the evidence they leave

These are settings on the GitLab instance, not files in this repository. A CodeLab
administrator:

1. mirrors the pinned base images into the project registry, and switches the
   references in `.gitlab-ci.yml` to the mirror;
2. provides an unprivileged Docker executor for merge-request jobs;
3. assigns the privileged Docker-in-Docker runner to protected branches and tags only,
   and tags it `protected`;
4. runs `toolchain-publish` on `main`, commits the digest, and sets the protected CI/CD
   variable `MIDGARD_TOOLCHAIN_DIGEST` from `tools/ci/toolchain.digest`;
5. turns on approval for pipelines from fork merge requests;
6. exports the runner and project CI/CD settings to
   `docs/compliance/evidence/gitlab-ci-settings-<YYYY-MM-DD>.md`, recording: each
   runner, its executor, whether it is privileged, its tags and whether it runs only
   on protected refs; the fork merge-request pipeline setting; and the
   `MIDGARD_TOOLCHAIN_DIGEST` variable's value and protection.

## What has and has not been exercised

The scripts are tested and were run locally: the `selftest`, `provenance` and
`js-build` jobs, and a release dry run covering Rust binaries, the SDK package and the
UI bundles. The pipeline files themselves parse as YAML and pass the parity check, but
**they have not run on GitHub or GitLab** — the first run on each forge is their real
test. Three things to expect on that first run:

- The hosted runners' disk. Two release builds of the workspace plus the debug build
  are large; if a job runs out of space, split it or use a larger runner.
- The container images named in `.gitlab-ci.yml` are pinned by digest, but still
  pulled from Docker Hub until they are mirrored, and the toolchain image has not yet
  been published to a project registry.
- `release-images`, `release-bundle` and `release-publish` have never run end to end:
  they need the signing runner, the service base image and real keys. The image
  build's reproducibility was checked here on a stand-in base image, not on the real
  one.
