# Contributing to Midgard

Thanks for helping build a sovereign, post-quantum platform. This guide covers how to
propose changes so they pass the gates on the first try.

## Ground rules

- **Licence & DCO.** Contributions are accepted under [Apache-2.0](LICENSE). Sign off
  every commit (`git commit -s`) to certify the [Developer Certificate of
  Origin](https://developercertificate.org/). By contributing you agree your work is
  licensed under Apache-2.0.
- **Security issues do NOT go here.** Never file a vulnerability as a public issue or
  PR — follow [SECURITY.md](SECURITY.md).
- **Sovereign posture.** No change may add outbound network dependencies to the OSS
  core, weaken the deny-by-default / fail-closed posture, or introduce a non-OSI
  dependency into the enclosure core. The build is air-gapped and vendored.
- **Signed commits.** Every commit is signed with a key listed in
  `deploy/trust/allowed_signers`, and `main` accepts signed commits only (a
  required-signatures rule set by the repository owner). CI's
  `bash tools/ci/run.sh commits` (`tools/ci/verify-commits.sh`) fails a change
  holding a commit that does not verify against that file as the change's base has
  it, and names it. To sign with an SSH key:

  ```bash
  git config --global gpg.format ssh
  git config --global user.signingkey ~/.ssh/id_ed25519.pub
  git config --global commit.gpgsign true
  # to check your own commits locally, as CI does:
  git config --global gpg.ssh.allowedSignersFile deploy/trust/allowed_signers
  ```

  Then ask for your public key to be added to `deploy/trust/allowed_signers` as
  `you@example.org namespaces="git" ssh-ed25519 AAAA...`, in a signed commit by a
  maintainer (docs/ops/PKI.md, "Git signing"). A branch with unsigned commits is
  re-signed with `git rebase --exec 'git commit --amend --no-edit -S' <base>`.
  Sigstore (gitsign) signatures are not verified by this check yet.
  [VLD-ANB-MIDGARD-0088]

## Provenance tags (required)

Every source change must cite the requirement/decision it implements with an inline
tag — `[FR-...]`, `[NFR-...]`, `[DD-...]`, `[AC-...]`, `[RC-...]`, or `[STD:...]`. CI
(`just check-provenance-tags`) fails a PR whose changed sources carry none. This is
the traceability guarantee the platform relies on (DD-6). Match the density and style
of the surrounding code.

## Building & testing (offline, reproducible)

The canonical toolchain is the pinned, digest-locked Docker image — the build is
fully vendored and runs with `--network none`:

```bash
docker build -t midgard-acceptance:1.82.0 deploy/acceptance
bash tools/ci/in-toolchain.sh just build-rust
bash tools/ci/in-toolchain.sh just test
```

CI runs its Rust steps the same way. `docs/dev/BUILD_ENV.md` covers faster local
iteration and Windows.

Install the pre-commit hooks once per clone, so a committed secret or unformatted Rust
is caught before it leaves your machine rather than in CI:

```bash
pip install pre-commit      # or your package manager's pre-commit
pre-commit install          # then every `git commit` runs the hooks
pre-commit run --all-files  # the whole tree, as CI scans it
```

`.pre-commit-config.yaml` runs `gitleaks protect --staged` with the repository's
`.gitleaks.toml` and `cargo fmt --all -- --check`, both as local hooks using the tools
on your PATH: install gitleaks at the version `tools/ci/run.sh` pins
(`GITLEAKS_VERSION`, and check the download against the checksum beside it) and the
Rust toolchain `rust-toolchain.toml` names. On a machine with no host Rust toolchain,
run the format check in the toolchain image instead
(`bash tools/ci/in-toolchain.sh cargo fmt --all -- --check`) and skip that hook with
`SKIP=cargo-fmt git commit ...`. The hooks are a convenience; CI's `secrets` and `fmt`
jobs still decide. [VLD-ANB-MIDGARD-0091]

Before opening a PR, ensure:

- `just test` (or the crate you touched) is green in the pinned image.
- `just check-licences` passes (no non-OSI vendored crate).
- `bash contracts/check-conformance.sh` passes if you touched the SDK/contract surface.
- New behaviour has a test. Acceptance-criteria code (`RC-nn` / `AC-...`) needs an
  acceptance test that fails-closed on the negative path.
- No secrets, keys, or credentials in source (CI runs a secret scan).

Dependency and advisory updates follow the **weekly advisory refresh** and Renovate
([CI.md](docs/dev/CI.md), "Dependency updates"): a scheduled job refreshes the
vendored advisory snapshots and opens a pull request, which a maintainer reviews
before it merges. To refresh by hand, `just advisory-db-refresh` and
`just advisory-db-npm-refresh` (online), then commit the snapshots.
[VLD-ANB-MIDGARD-0090] [VLD-ANB-MIDGARD-0076]

## Style

- Rust: match the surrounding idiom, comment density, and naming. `cargo fmt`;
  no warnings. Prefer new seams over rewriting working code; keep existing tests
  passing.
- Fail closed. Public-facing operations that mutate or authorise must have an
  explicit error path and audit.
- Honesty over aspiration: don't claim hardware/GPU/network behaviour you can't test
  offline — implement the seam, test with a documented stand-in, and state the
  boundary.

## Pull requests

1. Branch from `main` (or `dev`); keep PRs focused.
2. Fill in what changed, why, and the requirement tag(s).
3. CI must be green: script self-tests, provenance-tag lint, signed commits,
   vendored-dependency check, secret scan, rendered-manifest check, format and lint, build and test,
   reproducible build, advisory and licence scan, and the air-gapped acceptance
   suite (`just ci list`; see [docs/dev/CI.md](docs/dev/CI.md)). There is no contract
   breaking-change gate yet.
4. A code owner ([CODEOWNERS](.github/CODEOWNERS)) reviews. Being a code owner means
   being asked to review; anyone may contribute.

## Branches and the public tree

Work lands on `dev` and reaches `main` by an ordinary merge through a pull request.
There is no export step and no private upstream: what is on `main` is exactly the
tracked tree that was merged, dot-directories included, so nothing can be dropped
between the working tree and the published one. `dev` always descends from `main`;
both branches refuse force pushes and deletion, and take a change only through a pull
request a code owner has approved.

Every commit that reaches `main` or `dev` is signed. The forge refuses an unsigned
commit (the `signed history` ruleset on GitHub, the push rules on CodeLab; see
[docs/dev/CI.md](docs/dev/CI.md), "Repository rules"), and the `commits` job fails a
pull request that carries one, naming it. Sign with an SSH key:

```bash
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519.pub   # the public half; a hardware key is better
git config --global commit.gpgsign true
git config --global tag.gpgsign true
```

Register the same key on GitHub as a *signing* key. A maintainer's key is also listed
in `deploy/trust/allowed_signers`, which is how the `commits` job verifies commits
without asking the forge ([docs/ops/PKI.md](docs/ops/PKI.md), "Git signing"). To sign
commits already made on your branch, `git rebase --exec 'git commit --amend --no-edit
-S' <base>` and push the branch with `--force-with-lease` (never `main` or `dev`). A
maintainer merges a pull request with a merge commit, which the forge signs; the
commits in the pull request must still be signed.

## Releases and versions

Midgard is pre-1.0 and follows [Semantic Versioning](https://semver.org/) in its 0.x
form:

- **`0.MINOR.PATCH`.** A minor version may change the public API, wire formats,
  on-disk state or chart values; a patch version does not. The CHANGELOG entry says
  what an operator must do to upgrade.
- **Release candidates are `0.MINOR.PATCH-rc.N`.** A candidate runs the whole release
  pipeline and is published, if at all, as a GitHub **pre-release**. It is not a
  supported release. The final version is promoted only when the deployment tier
  (RC-40) passes on real infrastructure and the GA gates in
  `docs/compliance/GA_READINESS.md` are met.
- **One version for everything.** The workspace (`[workspace.package]` in
  `Cargo.toml`), the Helm chart (`version` and `appVersion`) and the image tag
  (`image.tag` in the chart's values) move together. The release workflow refuses a
  tag that is not `v` followed by the workspace version.

Releases are tag-driven. Every binary in `supply-chain/release-artefacts.txt` passes
the full per-artefact gate: a reproducible build, an SBOM, a CVE and licence scan,
and a PQ signature with attestation. The list must name every binary the workspace
builds (`bash tools/ci/release-artefacts.sh --check`), so a new binary cannot skip
the gate. See `.github/workflows/release.yml` and `docs/ops/PKI.md`.
