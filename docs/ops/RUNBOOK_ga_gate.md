# Runbook — execute the GA gate (readiness §4 A–C) on release infra

The release-engineering procedure for the three in-repo GA gates that
[`GA_READINESS.md`](../compliance/GA_READINESS.md) §4 lists as *runnable now on
release infrastructure*: **A** acceptance suites, **B** shell audits +
accessibility, **C** per-artefact release gate. Running all three green is a
precondition — not the whole condition — for GA; the two external activities
(§4 D FIPS 140-3 cert, §4 E deployment-tier assurance / RT-40) remain.

Every gate here **fails closed**: a single failing criterion exits non-zero, and
that is a stop, not a warning.

- **Trigger:** cutting a release candidate; re-assessing GA readiness.
- **Run from:** the workspace root, at the **exact commit/tag** being released
  (record it — the whole point is a verifiable release, not "latest").
- **Operators:** release engineering, with security present for step C (key use).

---

## 0. Prerequisites

| Need | Detail |
|------|--------|
| Repo at the release ref | `git rev-parse HEAD` recorded; working tree clean (`git status --short` empty) |
| Docker | Able to build once online, then run `--network none` |
| Release ref for shells | A release string/tag (e.g. `1.4.0`) — step B keys on it |
| Signing key material | Step C. For a **real** release this is the **HSM-backed release key** (RT-28); a local `just keygen` keystore is **dry-run only** (see step C note) |

The offline posture is already in place: deps resolve from vendored `/vendor`
(`.cargo/config.toml`), and the acceptance image bakes `rustfmt`/`clippy`/`just`
so `rustup` needs no channel sync under `--network none` (see
`deploy/acceptance/Dockerfile`).

---

## A. Acceptance suites (air-gapped) — AC-M / MI / F / B / H / AD / C

One orchestrator builds the pinned image (the **only** online step) and runs
core + all six product suites + the static accessibility check under a genuine
air-gap.

**Linux / CI runner:**
```bash
bash deploy/acceptance/run-airgapped.sh
```

**Windows release box (Git Bash):**
```bash
MSYS_NO_PATHCONV=1 REPO='D:\repos\veldris\midgard-app' \
  bash deploy/acceptance/run-airgapped.sh
```

What it does, in order (all inside `docker run --network none`):
1. `docker build -t midgard-acceptance:1.82.0 - < deploy/acceptance/Dockerfile` — one-time, online.
2. `rustup show` — must resolve entirely offline (proves the air-gap).
3. `verify-core.sh` → then `verify-{mimir,forge,bifrost,heimdall,admin,clients}.sh`.
4. `verify-shells-accessibility.sh` — the **static** AC-C-11 declaration check.

**Evidence to capture:** the full stdout, ending in
`>>> FULL SUITE PASSED, AIR-GAPPED (--network none).`. Each `run_ac` line prints
`<AC-id>: PASS` with the underlying `test result:` output.

**Fail closed:** any suite non-zero → `>>> FULL SUITE FAILED` and a non-zero exit.
Do not proceed to a release. Diagnose the specific `<AC-id>: FAIL` block, fix, re-run.

> **Coverage note:** `run-airgapped.sh` runs the *static* accessibility check but
> **not** the *on-device* shell audit — that is step B2, which is device-bound and
> cannot run inside this container.

---

## B. Shell audits + accessibility — RT-49 / AC-C-11

Two halves; A already ran B1.

**B1 — static accessibility declaration** (already run in A; standalone form):
```bash
bash deploy/acceptance/verify-shells-accessibility.sh
```

**B2 — on-device audit records** (the half A cannot cover):
```bash
bash deploy/acceptance/verify-shell-audits.sh <release>   # e.g. 1.4.0
```
This gate does **not** perform an audit — it verifies that a **real, on-device
audit ran and was recorded** for this release, for every shell:
`apple, android, linux-gtk, desktop-tauri, web, extension`.

It **fails closed** if, for `<release>`, any shell: has no record; carries a
placeholder runner/device/tool; is not `overall = "pass"`; or declares an
affordance not `MEASURED pass`. Records come from the on-device jobs in
`.github/workflows/shell-audit.yml` (schema: `clients/shells/AUDIT_SCHEMA.md`).

**Therefore B2 has a real-world precondition:** the on-device audit workflow must
have **run on actual devices/runners for `<release>`** first. If it hasn't, this
gate will (correctly) refuse — that is not a bug to work around; it means the
device audits are still owed. This is the one part of §4 A–C that needs physical
devices, not just release infra.

**Evidence to capture:** per-shell `pass` lines and the final aggregate pass.

---

## C. Per-artefact release gate — `just gate <pkg>`

Runs the full fail-closed chain per deployable:
`repro → sbom → scan → sign → verify`
(reproducible double-build → deterministic CycloneDX SBOM → offline CVE+licence
scan → ML-DSA sign + provenance → verify-before-publish).

**The deployables** are every binary the workspace builds. The list is derived, not
maintained by hand: `just release-manifest` prints it.

**C0 — key material.** `sign` refuses if `MIDGARD_SIGNING_KEYSTORE` is unset and
`verify` refuses if `MIDGARD_TRUST_ANCHORS` is unset — "I forgot the key" and
"this artefact is fine" must never look the same.

> **Real release vs dry run.** For a genuine GA release the signing key is the
> **HSM-backed release key (RT-28)** and anchors are distributed per RF-SC-8 —
> custody of that key is a deployment-tier concern (§4 E). A local
> `just keygen` keystore is for a **mechanics dry-run only** and must never sign
> a shipped artefact. Do C with the real release key, with security present.

**C1 — dry run (mechanics only).** One command gates every deployable, admits each
to a throwaway Bifrost registry under throwaway keys, checks the admitted set equals
the gated set, and removes the keys and the registry:
```bash
just release-dry-run
```
`just release-dry-run mg-sbom,mg-verify` narrows it to some binaries.

**C2 — the release.** On a provisioned signing runner (`docs/ops/PKI.md`), with
`MIDGARD_SIGNING_KEYSTORE`, `MIDGARD_BIFROST_KEYSTORE` and `MIDGARD_BIFROST_REGISTRY`
set and the public anchors committed under `deploy/trust/`:
```bash
just ci release
```
This runs `tools/ci/release.sh` inside the pinned toolchain image, offline, with the
keystores mounted read-only. The release pipelines run exactly this; see
`docs/dev/CI.md`.

A single binary can still be gated on its own with `just gate <pkg> [<bin>]`, given
`MIDGARD_SIGNING_KEYSTORE` and `MIDGARD_TRUST_ANCHORS`.

**Artifacts produced per binary**, all beside it in `target/release-out/`
(`target/verified/` for a lone `just gate`): `<bin>.sbom.cdx.json`, `<bin>.sig`,
`<bin>.att`, `<bin>.repro.json`, `<bin>.scan-findings.json`. The release also writes
`release-manifest.txt`: each binary with its version, component and digest, as read
back from the registry.

**Fail closed:** the first refusal in the chain stops that package and nothing
downstream runs. A failing `scan` (a real CVE or a disallowed licence) can never
reach a signature — that is the design.

---

## Recording the result

1. Confirm A, B (both halves), and C are green, and capture the evidence bundles.
2. Update [`GA_READINESS.md`](../compliance/GA_READINESS.md): mark §4 A–C
   satisfied with the run date, the commit/tag, and links to the captured logs
   and gate artifacts.
3. §4 D–E (FIPS 140-3 cert, RT-40 deployment-tier) still gate GA — do **not**
   flip the §5 verdict to GO until *all* of §4 is satisfied and the §6 sign-off
   block is signed.

## If a gate fails

| Gate | Symptom | Action |
|------|---------|--------|
| A | `<AC-id>: FAIL` | Read that block's `test result:` output; fix the crate; re-run `run-airgapped.sh` |
| A | `rustup show` tries to fetch | The acceptance image wasn't used / was rebuilt wrong — rebuild from `deploy/acceptance/Dockerfile` |
| B2 | a shell has no record / placeholder | The on-device audit didn't run for `<release>` — run `shell-audit.yml` on real devices, then re-gate |
| C | `scan` blocks | A real advisory or licence violation — remediate the dependency (never bypass); re-`sbom`+`scan` |
| C | `sign`/`verify` refuse | The key/anchor env var is unset or wrong — set to the **release** key material, not a stale dev keystore |

> This runbook covers only what is executable in-repo on release infra. It does
> **not** and cannot substitute for §4 D (external CMVP lab) or §4 E (real
> HSM/GPU/multi-node acceptance, RT-40) — both are external by nature and are the
> remaining GA blockers after A–C pass.
