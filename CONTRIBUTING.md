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

Before opening a PR, ensure:

- `just test` (or the crate you touched) is green in the pinned image.
- `just check-licences` passes (no non-OSI vendored crate).
- `bash contracts/check-conformance.sh` passes if you touched the SDK/contract surface.
- New behaviour has a test. Acceptance-criteria code (`RC-nn` / `AC-...`) needs an
  acceptance test that fails-closed on the negative path.
- No secrets, keys, or credentials in source (CI runs a secret scan).

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
3. CI must be green: script self-tests, provenance-tag lint, vendored-dependency
   check, secret scan, rendered-manifest check, format and lint, build and test,
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
both branches refuse force pushes and deletion.

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
