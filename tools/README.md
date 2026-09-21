# tools/ — the supply-chain toolchain

The reproducibility + SBOM + signing tools the CI gate calls. Shared with
Bifrost so build and delivery use one implementation of "reject unless proven".

| Tool | Role | Provenance |
|---|---|---|
| `mg-repro` | Build an artefact twice; assert bit-identical digests | `[AC-M-6]` `[NFR-OSS-1]` |
| `mg-sbom` | Emit a deterministic CycloneDX SBOM, with resolved licences | `[NFR-OSS-1]` `[AC-B-4]` |
| `mg-scan` | Vulnerability + licence gate over the SBOM; **fails closed** | `[FR-MS-6]` `[NFR-OSS-1]` |
| `mg-sign` | Detached ML-DSA signature + DSSE attestation, keys via KMS | `[NFR-PQ-3]` |
| `mg-verify` | Verify signature + SBOM + provenance; **fails closed** | `[AC-B-1]` `[AC-B-4]` |

`mg-sign` and `mg-verify` are thin CLIs over [`crypto/attest`](../crypto/attest),
which holds the bundle format and **the** acceptance decision — the same one
Bifrost runs pre-deploy and Mimir runs at model load. Adding a second
implementation of "is this acceptable" anywhere is the thing to avoid.

## Running the gate

Both key inputs are named explicitly; neither is ever searched for, and an unset
value fails the stage rather than skipping it.

```bash
just keygen mg.keystore mg.pub
```
```bash
MIDGARD_SIGNING_KEYSTORE=mg.keystore MIDGARD_TRUST_ANCHORS=mg.pub just gate mg-sbom
```

`gate` builds twice under a normalised environment, proves the output
bit-identical, **emits the proven artefact** to `target/verified/`, then SBOMs,
scans, signs and verifies that one. Signing a separately built copy would produce
a signature over a binary nobody could reproduce from source, so the chain
deliberately operates on the same bytes throughout. Every stage fails closed, so
the first refusal stops it — and the scan sits *before* the signature, so a
failing scan can never reach one.

## Advisory database

`mg-scan` reads the RustSec database vendored at `vendor/advisory-db`, pinned by
commit in `snapshot.toml`. It is stale-checked: a warning past 30 days, a refusal
past 180, because a database nobody refreshes still reports "clean". Refresh it —
the only online step in the gate — with:

```bash
just advisory-db-refresh
```

Commit the result and review the diff. A *shrinking* advisory count means
something went wrong.

Check reproducibility on its own:

```bash
just repro
```

That covers the whole workspace — two builds, every binary and library `.rlib`
compared — and writes `repro.json`, which `mg-sign` embeds into the provenance
attestation. `supply-chain/repro-exceptions.txt` quarantines a known-bad package
with an owner and a tracking item; it must be empty at GA.

> **Status.** All four tools are real. The release-time CVE and licence scan is
> still a placeholder — it belongs between `sbom` and `sign` so a failing scan
> cannot reach a signature (RF-SC-4), and the release matrix is still
> hand-maintained (RF-SC-5). See
> [`docs/dev/fixes/remedial-fixes.md`](../docs/dev/fixes/remedial-fixes.md).
