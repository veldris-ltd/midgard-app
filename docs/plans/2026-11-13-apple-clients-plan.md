# Apple clients (macOS and iOS) Implementation Plan

**Goal:** Native Midgard apps for macOS and iOS with parity across the six release consoles (Midgard, Admin, Heimdall, Forge, Mimir, Bifrost), running on a bundled, signed demo dataset with no server, as agreed in `docs/plans/2026-11-13-apple-clients-design.md`.

**Architecture:** All security and data logic stays in the Rust client core (`clients/core`, crate `midgard-client-core`): console read models, clearance filtering, a demo enclosure that serves a signed dataset behind a data-source trait a real gateway connection can later implement, the sealed local cache and the CRDT queue. A new crate `clients/ffi` (`midgard-client-ffi`) exposes that core to Swift through UniFFI; one XCFramework carries it to macOS (arm64 + x86_64), iOS devices and the iOS simulator. The Swift package `MidgardKit` (rooted at `clients/shells/apple/`) holds the shared SwiftUI screens, the design tokens and a Secure Enclave key root that implements only the core's key-root callback. The macOS app (branch `clients_mac`) and the iOS app (branch `clients_iphone`) add platform navigation only.

**Tech Stack:** Rust 1.82.0 (pinned by `rust-toolchain.toml`), UniFFI 0.28.3, the workspace's own crates (`policy`, `pqc-agility`, `kms`, `attest`), Node 22 (fixture extraction and code generation, no npm packages), Swift 5 language mode with SwiftUI, CryptoKit and LocalAuthentication, Xcode 26.2 (Swift 6.2.3 toolchain), macOS 14+ and iOS 17+ deployment targets, XcodeGen for the app projects (Phase B), XCTest and XCUITest.

---

## How to read this plan

- **Phase A** (branch `clients_shared`) is written out step by step. Every path, type and function it names was read at `v0.1.0-rc.3` (`ea5899ab`); where the design assumes something the tag does not have, the task says so and says what it builds instead.
- **Phase B** (app targets) is at moderate detail. **Phases C to H** (one per console) are at task level: each one is expanded into full step detail, in the same style as Phase A, before it starts.
- The last section lists the risks and gaps found while reading the tag, and the open questions for the owner.

## Ground rules for every task

**Where to work.** Only in the worktree `~/Library/Caches/midgard-build/wt-clients-shared` (branch `clients_shared`, based on the signed tag `v0.1.0-rc.3`). Never edit `~/Desktop/Midgard/midgard-*` checkouts: the live suite runs from `midgard-app`. Do not push; publishing is local-only until the owner says otherwise.

**Shell environment for every command below.**

```bash
cd ~/Library/Caches/midgard-build/wt-clients-shared
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
export CARGO_TARGET_DIR=$HOME/Library/Caches/midgard-build/target-clients
```

`rust-toolchain.toml` pins `1.82.0`, so `cargo` inside the worktree is 1.82.0. Delete `$CARGO_TARGET_DIR` at the end of each working session (`rm -rf "$CARGO_TARGET_DIR"`): the storage budget for the whole suite is 10 GB.

**Offline and locked.** `.cargo/config.toml` replaces crates.io with `vendor/`, so every build is offline by construction. Use `--offline --locked` on every `cargo` command except the one dependency-update step in Task A11, which says exactly how to step outside it.

**Commits.** The repository signs commits automatically (`dev`/`main` require signed commits). Use the default author. Commit messages carry no mention of any assistant or AI, no `Co-Authored-By` line and no "Generated with" line. Check each commit with `git log --show-signature -1`.

**What CI checks on every change, and how to run each check on this Mac.** CI runs every job through `bash tools/ci/run.sh <job>`; most `[toolchain]` jobs run inside the pinned Linux image. The ones that matter for this work, and their local equivalents:

| CI job | What it enforces here | Local command (macOS) |
|---|---|---|
| `provenance` | every changed `*.rs`, `*.ts`, `*.tsx` under `services mimir products clients` cites a provenance id (`[FR-…]`, `[NFR-…]`, `[AC-…]`, `[RC-…]`, `[DD-…]`, `[STD:…]`) | needs bash 4 (`mapfile`): `/opt/homebrew/bin/bash tools/ci/provenance-lint.sh v0.1.0-rc.3` after `brew install bash`, or run it in the toolchain image |
| `vendor` | `vendor/` holds exactly the crates `Cargo.lock` names | `bash tools/ci/vendor-check.sh` (works on bash 3.2) |
| `numbers` | `docs/compliance/NUMBERS.md` is what the tree gives; README/GETTING_STARTED quote the same crate count | `bash tools/ci/numbers.sh --check`; fix with `bash tools/ci/numbers.sh --write` |
| `claims` | every claims row cites files/lines/tests that exist | needs bash 4: `/opt/homebrew/bin/bash tools/ci/claims.sh --check` |
| `fmt` | `cargo fmt --all --check` | same command |
| `lint` | the `just lint` recipe: clippy per package group at `-D warnings` | the matching `cargo clippy …` line from `justfile` (each task names its line) |
| `test` | the `just test` recipe, one `cargo test` invocation per line | the matching line from `justfile` |
| `build` / `check-backends` | release build of the workspace never resolves `test-backend` | `bash tools/ci/no-test-backend.sh` |
| `scan` / release licence gate | `mg-scan` over an SBOM made from the **whole** `Cargo.lock` | see Task A11, step "licence exceptions" |
| static shell check | `deploy/acceptance/verify-shells-accessibility.sh`: each `manifest.toml` has `links_core = "midgard-client-core"`, each `accessibility.toml` declares the seven affordances | `bash deploy/acceptance/verify-shells-accessibility.sh` |

Note: `numbers.sh --check` already fails at `c3f64f41` (the design commit), because the committed sheet describes the gated commit `5376656` and this tree is not that commit. That is expected; every commit in this plan that changes the tree ends with `bash tools/ci/numbers.sh --write` and commits `docs/compliance/NUMBERS.md` with it. Order matters: `numbers.sh` counts tracked files only, so stage the code first, then run `--write`, then stage `docs/compliance/NUMBERS.md`, then commit (`git add <code>` → `numbers.sh --write` → `git add docs/compliance/NUMBERS.md` → `git commit`). Run before the `git add`, it misses every new file.

**macOS bash 3.2.** `/bin/bash` here is 3.2. Scripts written in this plan must run on it, because the GitHub `macos-15` runner and this Mac both start them with `bash`. Rules: no `mapfile`/`readarray`, no associative arrays, no `${var,,}`; under `set -u`, never expand a possibly empty array as `"${arr[@]}"` (bash 3.2 reports it unbound) — use `${arr[@]+"${arr[@]}"}`; no `sed -i` without a suffix (BSD `sed` needs `sed -i ''` or `sed -i.bak`), and prefer writing to a temp file and `mv`.

**Test-first, every task.** Each task writes the failing test, runs it and checks the failure message, writes the smallest implementation, runs it to green, then commits.

## Phase A — shared base (branch `clients_shared`)

### What the tag has, and what Phase A adds

| Design layer | At `v0.1.0-rc.3` | Phase A adds |
|---|---|---|
| Rust core | `ClientCore<K: Keystore + TrustRoot>` (`clients/core/src/lib.rs`): `open`, `edit_field`, `read_field`, `load_cached`, `pending_ops`, `drain_pending`, `merge_remote`, `visible_objects`, `drain_pending_encoded`, `merge_remote_encoded`; one error `CoreError { Store, MalformedOp }`. In memory only. | `marking` (console markings on the lattice), `consoles::*` (generated read models, one module per console), `source` (the data-source trait), `demo::*` (dataset format, signature check, local-data guard, demo enclosure), `session` (persona sign-in, sealed cache, sync, chips) |
| Store | `LocalStore` (`clients/store/src/lib.rs`): AES-256-GCM under an ML-KEM-1024 key from a `TrustRoot`, refuses above-clearance `put`; in memory only | export/import of the sealed cache so it survives a relaunch |
| Keystore | `SoftwareKeystore` (in-process ML-KEM root, floor `NonExtractable`), `BrokeredKeystore`, `PlatformProfile::Apple.floor() == HardwareBacked`, `auth::{PlatformAuthenticator, verify_assertion}` (ML-DSA assertions) | `SealedRootKeystore`: an ML-KEM root whose secret is stored wrapped under a key-encryption key the platform derives (the Secure Enclave on Apple) |
| Sync | `Replica` LWW map, `SyncOp::encode/decode` | `Replica::requeue` so pending ops survive a relaunch |
| Enclosure | `EnclosureClient` (`clients/enclosure`) is a PQ-mTLS **transport** only; there is no data-source interface at the tag | `source::DataSource` (new) — the interface a real gateway client implements later; `demo::DemoEnclosure` implements it now |
| Bindings | none; `clients/shells/SHELL_BUILD.md` says `uniffi` is not vendored | `clients/ffi` (UniFFI 0.28.3), XCFramework script |
| Apple shell | `clients/shells/apple/{manifest.toml, accessibility.toml, audits/.gitkeep}` only | `MidgardKit` Swift package (root `Package.swift`), Secure Enclave key root, tokens, sign-in, lock, chips, contract test, `audit.sh` hook |

### Phase A task list

| Task | Design item |
|---|---|
| A1 Baseline, toolchain, targets | — |
| A2 Markings and visibility in the core | 1 |
| A3 Read-model generator and the six console modules | 1 |
| A4 Fixture extraction from the release's mocks | 3 |
| A5 Local-suite data guard | 3 |
| A6 Dataset file format and signature check | 2, 3 |
| A7 Dataset build step, demo signing key, committed dataset | 3 |
| A8 Data-source trait and the demo enclosure | 2 |
| A9 Sealed-root keystore, cache persistence, pending-op requeue | 4, 6 |
| A10 Session façade | 1, 2, 6 |
| A11 `clients/ffi` crate (UniFFI), vendoring, licence exceptions | 4 |
| A12 XCFramework build script | 5 |
| A13 `MidgardKit` Swift package | 6 |
| A14 Accessibility audit hook | 7 |
| A15 CI wiring, documents, full local verification | all |

---

### Task A1: Baseline, toolchain and targets

**Files:** none changed.

**Step 1: Confirm the worktree.**

```bash
cd ~/Library/Caches/midgard-build/wt-clients-shared
git status --short          # expect: nothing
git branch --show-current   # expect: clients_shared
git describe --tags         # expect: v0.1.0-rc.3-<n>-g<sha>
git verify-tag v0.1.0-rc.3  # expect: Good "git" signature
```

**Step 2: Add the Apple targets to the pinned toolchain.** `rustup target list --installed --toolchain 1.82.0` shows `aarch64-apple-darwin` and `x86_64-apple-darwin` only.

```bash
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
rustup target add aarch64-apple-ios aarch64-apple-ios-sim --toolchain 1.82.0
rustup target list --installed --toolchain 1.82.0
```

Expected: four lines — `aarch64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-darwin`. (The `x86_64-apple-ios` simulator slice is not built: the simulator runs arm64 on Apple silicon. If an Intel Mac must run the simulator later, add that target and a fourth slice in Task A12.)

**Step 3: Baseline tests of the crates Phase A touches.**

```bash
export CARGO_TARGET_DIR=$HOME/Library/Caches/midgard-build/target-clients
cargo test --offline --locked -p midgard-client-sync
cargo test --offline --locked -p midgard-client-keystore --features backend-fips
cargo test --offline --locked -p midgard-client-store --features backend-fips
cargo test --offline --locked -p midgard-client-core --features backend-fips
cargo test --offline --locked -p attest
```

Expected: every `test result:` line says `0 failed`. These are the `just test` lines 258, 259, 260, 265 and 266 of `justfile`.

**Step 4: Baseline static checks.**

```bash
bash deploy/acceptance/verify-shells-accessibility.sh   # expect: ">>> all 6 shells declare complete accessibility conformance"
bash tools/ci/vendor-check.sh                           # expect: "48 locked registry package(s), all vendored …"
bash tools/ci/release-artefacts.sh --check              # expect: "18 binaries, every one in the matrix"
node --version                                          # expect: v22.x (>= 22.15 for module.registerHooks)
xcodebuild -version                                     # expect: Xcode 26.2
```

No commit.

---

### Task A2: Markings and visibility in the core

The consoles' marking words (`MarkingLevel` in `sdk/ts/src/types.ts`: `'confidential' | 'restricted' | 'internal' | 'open'`) are ordered `open < internal < restricted < confidential` by `MARKING_RANK` in `sdk/ts/src/mock/clients.ts`. The core enforces clearance with `policy::dominates(&Clearance, &Marking)` over `policy::ClassificationLevel(u8)` (`crypto/policy/src/model.rs`). This task maps one onto the other, so the store and the consoles use one rule, and defines `Visible<T>`: a row is either shown with its marking or withheld with only its level (no id, no content).

**Files:**
- Create: `clients/core/src/marking.rs`
- Modify: `clients/core/src/lib.rs` (add `pub mod marking;`)
- Test: `clients/core/tests/marking.rs`

**Step 1: Write the failing test** — `clients/core/tests/marking.rs`:

```rust
//! Console markings on the clearance lattice: a row above the session clearance is
//! withheld, never shown. [AC-C-13] [NFR-SEC-2]

use midgard_client_core::marking::{clearance, filter_rows, MarkingLevel, RowMarking, Visible};

fn row(level: MarkingLevel, comps: &[&str]) -> RowMarking {
    RowMarking::new(level, comps.iter().copied())
}

#[test]
fn levels_follow_the_console_order() {
    use MarkingLevel::*;
    assert!(Open < Internal && Internal < Restricted && Restricted < Confidential);
    for l in MarkingLevel::ALL {
        assert_eq!(MarkingLevel::parse(l.as_str()), Some(l));
    }
    assert_eq!(MarkingLevel::parse("secret"), None);
}

#[test]
fn labels_read_like_the_consoles() {
    assert_eq!(row(MarkingLevel::Confidential, &["SIG", "OPS-N"]).label(), "CONFIDENTIAL // OPS-N, SIG");
    assert_eq!(row(MarkingLevel::Internal, &[]).label(), "INTERNAL");
}

#[test]
fn a_row_above_clearance_is_withheld_without_its_id_or_content() {
    let restricted = clearance(MarkingLevel::Restricted, &[]);
    let rows = vec![
        ("a".to_string(), row(MarkingLevel::Internal, &[]), "visible"),
        ("b".to_string(), row(MarkingLevel::Confidential, &[]), "secret body"),
    ];
    let out = filter_rows(&restricted, rows);
    assert_eq!(
        out[0],
        Visible::Shown { id: "a".into(), marking: row(MarkingLevel::Internal, &[]), item: "visible" }
    );
    assert_eq!(out[1], Visible::Withheld { level: MarkingLevel::Confidential });
}

#[test]
fn a_missing_compartment_withholds_even_at_the_right_level() {
    let ops_only = clearance(MarkingLevel::Confidential, &["OPS-N".to_string()]);
    let out = filter_rows(&ops_only, vec![("s".to_string(), row(MarkingLevel::Confidential, &["SIG"]), 1)]);
    assert!(matches!(out[0], Visible::Withheld { .. }));
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test marking
```

Expected: `error[E0432]: unresolved import \`midgard_client_core::marking\``.

**Step 3: Implement** — `clients/core/src/marking.rs`:

```rust
//! Row markings as the consoles show them, on the Bell–LaPadula lattice the core
//! enforces. `[AC-C-13]` `[NFR-SEC-2]`
//!
//! The consoles mark rows `open`, `internal`, `restricted` or `confidential`
//! (`sdk/ts/src/types.ts` `MarkingLevel`), ordered as `MARKING_RANK` in
//! `sdk/ts/src/mock/clients.ts`. They map onto `policy::ClassificationLevel` 0..=3,
//! so `policy::dominates` decides what a session sees, exactly as it decides what
//! `midgard_client_store::LocalStore` may cache.

use std::collections::BTreeSet;

use policy::{dominates, ClassificationLevel, Clearance, Compartment, Marking};

/// A console marking level, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkingLevel {
    /// `open` (unclassified).
    Open,
    /// `internal`.
    Internal,
    /// `restricted`.
    Restricted,
    /// `confidential` (the highest the consoles use).
    Confidential,
}

impl MarkingLevel {
    /// Every level, weakest first.
    pub const ALL: [MarkingLevel; 4] = [
        MarkingLevel::Open,
        MarkingLevel::Internal,
        MarkingLevel::Restricted,
        MarkingLevel::Confidential,
    ];

    /// Parse the consoles' word for a level.
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "open" => Some(Self::Open),
            "internal" => Some(Self::Internal),
            "restricted" => Some(Self::Restricted),
            "confidential" => Some(Self::Confidential),
            _ => None,
        }
    }

    /// The consoles' word for this level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Internal => "internal",
            Self::Restricted => "restricted",
            Self::Confidential => "confidential",
        }
    }

    /// The level on the policy lattice.
    pub fn classification(self) -> ClassificationLevel {
        ClassificationLevel(self as u8)
    }
}

/// A row's marking: a level and the compartments it requires, sorted and unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMarking {
    /// The level.
    pub level: MarkingLevel,
    /// Required compartments, sorted, without duplicates.
    pub compartments: Vec<String>,
}

impl RowMarking {
    /// A marking at `level` requiring `compartments`.
    pub fn new<S: Into<String>>(level: MarkingLevel, compartments: impl IntoIterator<Item = S>) -> Self {
        let set: BTreeSet<String> = compartments.into_iter().map(Into::into).collect();
        Self { level, compartments: set.into_iter().collect() }
    }

    /// The policy marking the lattice checks.
    pub fn to_policy(&self) -> Marking {
        Marking {
            level: self.level.classification(),
            compartments: self.compartments.iter().cloned().map(Compartment).collect(),
        }
    }

    /// `CONFIDENTIAL // OPS-N, SIG`, as the marking chips read.
    pub fn label(&self) -> String {
        let level = self.level.as_str().to_uppercase();
        if self.compartments.is_empty() {
            level
        } else {
            format!("{level} // {}", self.compartments.join(", "))
        }
    }
}

/// A session clearance at `level` holding `compartments`.
pub fn clearance(level: MarkingLevel, compartments: &[String]) -> Clearance {
    Clearance {
        level: level.classification(),
        compartments: compartments.iter().cloned().map(Compartment).collect(),
    }
}

/// Whether `clearance` may see a row marked `marking`.
pub fn may_see(clearance: &Clearance, marking: &RowMarking) -> bool {
    dominates(clearance, &marking.to_policy())
}

/// One row as a session may see it: in full, or withheld with its level only.
#[derive(Debug, Clone, PartialEq)]
pub enum Visible<T> {
    /// The session's clearance dominates the row's marking.
    Shown {
        /// The row id.
        id: String,
        /// The row marking (for the chip).
        marking: RowMarking,
        /// The row.
        item: T,
    },
    /// Above the session's clearance: neither the id nor the content is given.
    Withheld {
        /// The level, so the row can say what it withholds.
        level: MarkingLevel,
    },
}

impl<T> Visible<T> {
    /// The row, when shown.
    pub fn shown(&self) -> Option<&T> {
        match self {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        }
    }

    /// Map the row, keeping its visibility.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Visible<U> {
        match self {
            Visible::Shown { id, marking, item } => Visible::Shown { id, marking, item: f(item) },
            Visible::Withheld { level } => Visible::Withheld { level },
        }
    }
}

/// Apply `clearance` to `(id, marking, item)` rows, in order.
pub fn filter_rows<T>(clearance: &Clearance, rows: Vec<(String, RowMarking, T)>) -> Vec<Visible<T>> {
    rows.into_iter()
        .map(|(id, marking, item)| {
            if may_see(clearance, &marking) {
                Visible::Shown { id, marking, item }
            } else {
                Visible::Withheld { level: marking.level }
            }
        })
        .collect()
}
```

In `clients/core/src/lib.rs`, after `pub use error::CoreError;` add:

```rust
pub mod marking;
```

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test marking
cargo fmt --all --check
cargo clippy --offline --locked -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

Expected: `test result: ok. 4 passed; 0 failed`; fmt and clippy silent.

**Step 5: Commit.**

```bash
git add clients/core/src/marking.rs clients/core/src/lib.rs clients/core/tests/marking.rs
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: console markings on the clearance lattice, withheld rows [AC-C-13]"
```

---

### Task A3: Read-model generator and the six console modules

The read models must match what the web consoles render, field for field. They are the TypeScript interfaces in `sdk/ts/src/types.ts` (Midgard core, plus the shared `AuditRecord` and `PolicyContext`) and `sdk/ts/src/products/{admin,heimdall,forge,mimir,bifrost}.ts`. Writing some sixty Rust structs and their decoders by hand would drift from those files, so a small generator writes them, and a `--check` mode fails CI when the TypeScript changes and the Rust does not.

The workspace has no `serde` (`vendor/` holds 48 crates, none of them serde). JSON is read with the strict parser that already exists: `attest::json::parse` (`crypto/attest/src/json.rs`: RFC 8259 only, duplicate keys and trailing data rejected, depth bounded at 64, numbers kept as their source text). The core gains a dependency on `attest` with `default-features = false, features = ["backend-fips"]` — the verifier-only form `bifrost-artefact` and `model-server` already use — which Task A6 also needs for the signature check.

`clients/ffi` (Task A11) needs the same types again with `#[derive(uniffi::Record)]`. The core keeps `#![forbid(unsafe_code)]`, and UniFFI's derives emit `unsafe impl`s, so the derives cannot live in the core; the generator also writes FFI mirror structs and `From` conversions into `clients/ffi/src/generated/` when given `--ffi-out` (used from Task A11 on).

Type mapping (decided here): `string` → `String`; `number` → `f64`; a union of number literals (`AdminRiskClass = 1 | 2 | 3 | 4 | 5`) → `u32`; `boolean` → `bool`; a union of string literals or an alias of one (`AdminStatus`, `MarkingLevel`) → `String`, with the allowed values in the doc comment; `T | null` and `name?: T` → `Option<T>`; `T[]` → `Vec<T>`; `Record<string, string>` → `KeyValues` (ordered key/value pairs; UniFFI has no ordered map); an inline object type → a named struct `<Parent><Field>`; anything else is an error, so a new TypeScript shape fails loudly. Type names in `types.ts` get the prefix `Midgard` (so its `Marking` cannot clash with `policy::Marking` or any product type), except the shared `AuditRecord`, `PolicyContext`, which go to `consoles::common`. Field names become snake_case; `type` becomes `type_name`.

**Files:**
- Create: `clients/demo/tools/gen-read-models.mjs`
- Create: `clients/core/src/consoles/mod.rs`, `clients/core/src/consoles/decode.rs`
- Create (generated): `clients/core/src/consoles/{common,midgard,admin,heimdall,forge,mimir,bifrost}.rs`
- Modify: `clients/core/Cargo.toml`, `clients/core/src/lib.rs`
- Test: `clients/core/tests/read_models.rs`

**Step 1: Write the failing test** — `clients/core/tests/read_models.rs`:

```rust
//! The generated console read models decode what the web consoles' mocks serve, and
//! refuse a shape they do not know. [FR-C-1] [AC-C-13]

use attest::json::parse;
use midgard_client_core::consoles::admin::AdminApproval;
use midgard_client_core::consoles::decode::{DecodeError, FromJson, KeyValues};
use midgard_client_core::consoles::heimdall::HeimdallTrack;
use midgard_client_core::consoles::midgard::MidgardObjectInstance;
use midgard_client_core::consoles::mimir::MimirTraceStep;

fn decode<T: FromJson>(text: &str) -> Result<T, DecodeError> {
    T::from_json(&parse(text).expect("valid json"), "$")
}

#[test]
fn an_admin_approval_decodes_with_its_optional_field_absent() {
    let a: AdminApproval = decode(
        r#"{"id":"APR-1","action":"Rotate KEK","subsystem":"kms","risk":4,"requester":"k.rao",
            "approver":"—","state":"awaiting","justification":"quarterly"}"#,
    )
    .unwrap();
    assert_eq!(a.risk, 4);
    assert_eq!(a.denied_reason, None);
}

#[test]
fn nested_lists_nulls_and_records_decode() {
    let t: HeimdallTrack = decode(
        r#"{"id":"E-0871","cls":"UNKNOWN","conf":0.54,"x":74,"y":34,"layer":"air","track":"anomalous",
            "hdg":197,"spd":388,"alt":"FL212","feeds":2,
            "options":[{"rank":1,"title":"Re-task","why":"gain","cites":["Entity/E-0871"]}]}"#,
    )
    .unwrap();
    assert_eq!(t.options.as_ref().map(Vec::len), Some(1));

    // MimirTraceStep (sdk/ts/src/products/mimir.ts): `ms: number | null`, `ok: boolean | null`.
    let s: MimirTraceStep =
        decode(r#"{"kind":"plan","label":"p","detail":"d","ms":null,"ok":null}"#).unwrap();
    assert_eq!((s.ms, s.ok), (None, None));

    let o: MidgardObjectInstance = decode(
        r#"{"id":"aircraft/1000","typeId":"aircraft","propertyValues":{"status":"standby","updated":"2m ago"},
            "markings":[{"id":"mk-confidential","name":"CONFIDENTIAL","level":"confidential","compartments":["OPS-N"]}]}"#,
    )
    .unwrap();
    assert_eq!(o.property_values, KeyValues(vec![("status".into(), "standby".into()), ("updated".into(), "2m ago".into())]));
}

#[test]
fn an_unknown_key_or_a_wrong_type_is_refused_with_its_path() {
    let e = decode::<AdminApproval>(
        r#"{"id":"APR-1","action":"a","subsystem":"s","risk":"high","requester":"r","approver":"a",
            "state":"awaiting","justification":"j"}"#,
    )
    .unwrap_err();
    assert_eq!(e.path, "$.risk");

    let e = decode::<AdminApproval>(
        r#"{"id":"APR-1","action":"a","subsystem":"s","risk":1,"requester":"r","approver":"a",
            "state":"awaiting","justification":"j","extra":true}"#,
    )
    .unwrap_err();
    assert!(e.reason.contains("unknown key \"extra\""), "{e}");
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test read_models
```

Expected: `error[E0433]: failed to resolve: use of undeclared crate or module \`attest\`` (the dependency is not there yet), then, after Step 3a, `unresolved import \`midgard_client_core::consoles\``.

**Step 3a: Add the dependency** — in `clients/core/Cargo.toml`, under `[dependencies]`:

```toml
# The strict JSON reader for console read models and the demo dataset, and the one
# signature-acceptance decision (`attest::accept`). Verifier only: no key custody.
# [NFR-PQ-3] [AC-C-13]
attest = { path = "../../crypto/attest", default-features = false, features = ["backend-fips"] }
```

and add `"attest/backend-fips"` to the core's `backend-fips` feature list. No new crate enters `Cargo.lock` (attest is a workspace member), so `vendor/` is unchanged; confirm with `bash tools/ci/vendor-check.sh`.

**Step 3b: The hand-written decode helpers** — `clients/core/src/consoles/decode.rs`:

```rust
//! Decoding console read models from strict JSON. `[FR-C-1]`
//!
//! Every generated read model implements [`FromJson`]. Objects are decoded strictly:
//! a key the model does not know is an error, so a change to the TypeScript contract
//! that the generator has not been rerun for cannot be silently dropped.

use std::fmt;

use attest::json::Json;

/// Why a value did not decode, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// A JSONPath-like location, `$.tracks[2].options[0].rank`.
    pub path: String,
    /// What was wrong.
    pub reason: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

impl std::error::Error for DecodeError {}

fn err<T>(path: &str, reason: impl Into<String>) -> Result<T, DecodeError> {
    Err(DecodeError { path: path.to_string(), reason: reason.into() })
}

/// A value decodable from JSON at `path`.
pub trait FromJson: Sized {
    /// Decode `v`, reporting errors at `path`.
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError>;
}

impl FromJson for String {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Str(s) => Ok(s.clone()),
            _ => err(path, "expected a string"),
        }
    }
}

impl FromJson for bool {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Bool(b) => Ok(*b),
            _ => err(path, "expected a boolean"),
        }
    }
}

impl FromJson for f64 {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Num(text) => text.parse::<f64>().or_else(|_| err(path, "not a number")),
            _ => err(path, "expected a number"),
        }
    }
}

impl FromJson for u32 {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Num(text) => text.parse::<u32>().or_else(|_| err(path, "expected a whole number")),
            _ => err(path, "expected a whole number"),
        }
    }
}

impl<T: FromJson> FromJson for Vec<T> {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Arr(items) => items
                .iter()
                .enumerate()
                .map(|(i, x)| T::from_json(x, &format!("{path}[{i}]")))
                .collect(),
            _ => err(path, "expected an array"),
        }
    }
}

/// A `Record<string, string>`: ordered key/value pairs, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyValues(pub Vec<(String, String)>);

impl FromJson for KeyValues {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        match v {
            Json::Obj(pairs) => pairs
                .iter()
                .map(|(k, x)| Ok((k.clone(), String::from_json(x, &format!("{path}.{k}"))?)))
                .collect::<Result<Vec<_>, _>>()
                .map(KeyValues),
            _ => err(path, "expected an object"),
        }
    }
}

/// The pairs of an object whose keys are all in `known`.
pub fn object<'a>(v: &'a Json, path: &str, known: &[&str]) -> Result<&'a [(String, Json)], DecodeError> {
    match v {
        Json::Obj(pairs) => {
            if let Some((k, _)) = pairs.iter().find(|(k, _)| !known.contains(&k.as_str())) {
                return err(path, format!("unknown key {k:?}"));
            }
            Ok(pairs)
        }
        _ => err(path, "expected an object"),
    }
}

/// A required field.
pub fn field<T: FromJson>(pairs: &[(String, Json)], key: &str, path: &str) -> Result<T, DecodeError> {
    match pairs.iter().find(|(k, _)| k == key) {
        Some((_, v)) => T::from_json(v, &format!("{path}.{key}")),
        None => err(path, format!("missing key {key:?}")),
    }
}

/// An optional (`key?:`) or nullable (`T | null`) field: absent or `null` is `None`.
pub fn opt_field<T: FromJson>(pairs: &[(String, Json)], key: &str, path: &str) -> Result<Option<T>, DecodeError> {
    match pairs.iter().find(|(k, _)| k == key) {
        None | Some((_, Json::Null)) => Ok(None),
        Some((_, v)) => T::from_json(v, &format!("{path}.{key}")).map(Some),
    }
}
```

`clients/core/src/consoles/mod.rs`:

```rust
//! Console read models: the rows each of the six release consoles shows, as the
//! TypeScript contract in `sdk/ts/src` defines them. Generated by
//! `clients/demo/tools/gen-read-models.mjs`; only `decode` is written by hand.
//! `[FR-C-1]` `[AC-C-13]`

pub mod decode;

pub mod admin;
pub mod bifrost;
pub mod common;
pub mod forge;
pub mod heimdall;
pub mod midgard;
pub mod mimir;
```

In `clients/core/src/lib.rs`, next to `pub mod marking;`: `pub mod consoles;`.

**Step 3c: The generator** — `clients/demo/tools/gen-read-models.mjs`. It reads the six TypeScript files as text, parses only the subset they use, and fails on anything else.

```js
#!/usr/bin/env node
// Generate the Rust console read models from the TypeScript contract.
// [FR-C-1] [AC-C-13]
//
//   node clients/demo/tools/gen-read-models.mjs --write [--ffi-out clients/ffi/src/generated]
//   node clients/demo/tools/gen-read-models.mjs --check [--ffi-out clients/ffi/src/generated]
//
// Reads sdk/ts/src/types.ts and sdk/ts/src/products/{admin,bifrost,forge,heimdall,mimir}.ts
// as text and understands only what those files use: `export type X = …;` aliases and
// `export interface X { name?: Type; }` blocks. Anything else is an error, so a new
// shape in the contract fails here instead of decoding wrongly. Output is formatted
// with the pinned rustfmt, so `cargo fmt --all --check` agrees with --check.
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const CORE_OUT = join(ROOT, 'clients/core/src/consoles');
const SOURCES = [
  { module: 'midgard', file: 'sdk/ts/src/types.ts', prefix: 'Midgard',
    shared: ['AuditRecord', 'PolicyContext'],
    skip: ['QueryRequest', 'QueryCondition', 'ApplyActionRequest'] },
  { module: 'admin', file: 'sdk/ts/src/products/admin.ts' },
  { module: 'bifrost', file: 'sdk/ts/src/products/bifrost.ts' },
  { module: 'forge', file: 'sdk/ts/src/products/forge.ts' },
  { module: 'heimdall', file: 'sdk/ts/src/products/heimdall.ts' },
  { module: 'mimir', file: 'sdk/ts/src/products/mimir.ts' },
];
const RUST_KEYWORDS = new Set(['type', 'ref', 'match', 'move', 'loop', 'impl', 'fn', 'mod', 'use',
  'where', 'self', 'super', 'crate', 'in', 'as', 'const', 'static', 'struct', 'enum', 'trait',
  'async', 'await', 'dyn', 'box', 'yield', 'try', 'override', 'final', 'macro', 'priv', 'virtual']);

const die = (msg) => { console.error(`gen-read-models: ${msg}`); process.exit(1); };

// --- lexing helpers -----------------------------------------------------------------
function stripComments(src) {
  let out = '', i = 0, q = null;
  while (i < src.length) {
    const c = src[i], n = src[i + 1];
    if (q) { out += c; if (c === '\\') { out += n; i += 2; continue; } if (c === q) q = null; i++; continue; }
    if (c === "'" || c === '"' || c === '`') { q = c; out += c; i++; continue; }
    if (c === '/' && n === '*') { const e = src.indexOf('*/', i + 2); i = e < 0 ? src.length : e + 2; continue; }
    if (c === '/' && n === '/') { const e = src.indexOf('\n', i); i = e < 0 ? src.length : e; continue; }
    out += c; i++;
  }
  return out;
}
// Split `s` on `sep` at bracket depth 0, outside string literals.
function splitTop(s, sep) {
  const parts = []; let depth = 0, q = null, cur = '';
  for (const c of s) {
    if (q) { cur += c; if (c === q) q = null; continue; }
    if (c === "'" || c === '"') { q = c; cur += c; continue; }
    if ('{[(<'.includes(c)) depth++;
    if ('}])>'.includes(c)) depth--;
    if (c === sep && depth === 0) { parts.push(cur); cur = ''; continue; }
    cur += c;
  }
  parts.push(cur);
  return parts.map((p) => p.trim()).filter((p) => p !== '');
}
function blockAfter(src, openIdx) { // src[openIdx] === '{' → index of the matching '}'
  let depth = 0;
  for (let i = openIdx; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) return i;
  }
  die('unbalanced braces');
}

// --- parsing ------------------------------------------------------------------------
const aliases = new Map();     // name -> type text (all files)
const interfaces = [];         // { module, name, rustName, members:[{name, optional, type}] }
const rustNameOf = new Map();  // TS interface name -> Rust struct name

function parseMembers(body, where) {
  return splitTop(body.replace(/\n/g, ';'), ';').map((m) => {
    const mm = m.match(/^(readonly\s+)?([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([\s\S]+)$/);
    if (!mm) die(`${where}: cannot read member "${m}"`);
    if (/^\(.*\)\s*=>/.test(mm[4]) || mm[4].includes('Promise<')) die(`${where}: method member "${m}"`);
    return { name: mm[2], optional: !!mm[3], type: mm[4].trim() };
  });
}

for (const src of SOURCES) {
  const text = stripComments(readFileSync(join(ROOT, src.file), 'utf8'));
  for (const m of text.matchAll(/export\s+type\s+([A-Za-z0-9_]+)\s*=\s*([^;]+);/g)) aliases.set(m[1], m[2].trim());
  for (const m of text.matchAll(/export\s+interface\s+([A-Za-z0-9_]+)\s*\{/g)) {
    const name = m[1];
    const open = m.index + m[0].length - 1;
    const body = text.slice(open + 1, blockAfter(text, open));
    if (name.endsWith('Gateway') || (src.skip ?? []).includes(name)) continue;
    const isShared = (src.shared ?? []).includes(name);
    const module = isShared ? 'common' : src.module;
    const rustName = isShared || !src.prefix ? name : `${src.prefix}${name}`;
    rustNameOf.set(name, rustName);
    interfaces.push({ module, name, rustName, source: src.file, members: parseMembers(body, `${src.file} ${name}`) });
  }
}

// Type text -> IR: {k:'str'|'f64'|'u32'|'bool'|'kv'} | {k:'vec',of} | {k:'opt',of} | {k:'ref',name} | {k:'obj',members}
function typeOf(t, ctx) {
  t = t.trim();
  if (t.startsWith('(') && t.endsWith(')')) return typeOf(t.slice(1, -1), ctx);
  const union = splitTop(t, '|');
  if (union.length > 1) {
    const rest = union.filter((u) => u !== 'null' && u !== 'undefined');
    if (rest.length < union.length) return { k: 'opt', of: typeOf(rest.join(' | '), ctx) };
    if (rest.every((u) => /^'[^']*'$/.test(u))) return { k: 'str', doc: rest.join(' | ') };
    if (rest.every((u) => /^\d+$/.test(u))) return { k: 'u32', doc: rest.join(' | ') };
    die(`${ctx}: unsupported union "${t}"`);
  }
  if (t.endsWith('[]')) return { k: 'vec', of: typeOf(t.slice(0, -2), ctx) };
  if (t === 'string') return { k: 'str' };
  if (t === 'number') return { k: 'f64' };
  if (t === 'boolean') return { k: 'bool' };
  if (/^Record<\s*string\s*,\s*string\s*>$/.test(t)) return { k: 'kv' };
  if (/^'[^']*'$/.test(t)) return { k: 'str', doc: t };
  if (t.startsWith('{') && t.endsWith('}')) return { k: 'obj', members: parseMembers(t.slice(1, -1), ctx) };
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(t)) {
    if (rustNameOf.has(t)) return { k: 'ref', name: rustNameOf.get(t) };
    if (aliases.has(t)) return typeOf(aliases.get(t), `${ctx} (alias ${t})`);
  }
  die(`${ctx}: unsupported type "${t}"`);
}

// --- emitting -----------------------------------------------------------------------
const snake = (s) => {
  const x = s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/([A-Z])([A-Z][a-z])/g, '$1_$2').toLowerCase();
  return RUST_KEYWORDS.has(x) ? (x === 'type' ? 'type_name' : `${x}_`) : x;
};
const pascal = (s) => s.charAt(0).toUpperCase() + s.slice(1);

function rustType(ir, emitNested, parent, field) {
  switch (ir.k) {
    case 'str': return 'String';
    case 'f64': return 'f64';
    case 'u32': return 'u32';
    case 'bool': return 'bool';
    case 'kv': return 'crate::consoles::decode::KeyValues';
    case 'vec': return `Vec<${rustType(ir.of, emitNested, parent, field)}>`;
    case 'opt': return `Option<${rustType(ir.of, emitNested, parent, field)}>`;
    case 'ref': return ir.name;
    case 'obj': { const n = `${parent}${pascal(field)}`; emitNested(n, ir.members); return n; }
  }
}
function docOf(ir) { return ir.doc ? ir.doc : ir.of ? docOf(ir.of) : null; }

function structFor(name, members, source, ctx, nested) {
  const fields = members.map((m) => {
    const base = typeOf(m.type, `${ctx}.${m.name}`);
    const ir = m.optional && base.k !== 'opt' ? { k: 'opt', of: base } : base;
    const ty = rustType(ir, (n, ms) => nested.push(structFor(n, ms, source, `${ctx}.${m.name}`, nested)), name, m.name);
    return { ts: m.name, rs: snake(m.name), ty, opt: ir.k === 'opt', doc: docOf(ir) };
  });
  const decl = fields.map((f) =>
    `${f.doc ? `    /// One of: \`${f.doc}\`.\n` : `    /// \`${f.ts}\`.\n`}    pub ${f.rs}: ${f.ty},`).join('\n');
  const known = fields.map((f) => JSON.stringify(f.ts)).join(', ');
  const init = fields.map((f) =>
    `            ${f.rs}: ${f.opt ? 'opt_field' : 'field'}(o, ${JSON.stringify(f.ts)}, path)?,`).join('\n');
  return { name, fields, rust:
`/// \`${name}\`, from \`${source}\`.
#[derive(Debug, Clone, PartialEq)]
pub struct ${name} {
${decl}
}

impl FromJson for ${name} {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &[${known}])?;
        Ok(Self {
${init}
        })
    }
}
` };
}

function rustfmt(src) {
  return execFileSync('rustfmt', ['--edition', '2021', '--config-path', join(ROOT, 'rustfmt.toml')],
    { input: src, cwd: ROOT, encoding: 'utf8' });
}

const modules = ['common', ...SOURCES.map((s) => s.module)];
const outputs = new Map(); // path -> content
const ffiOutIdx = process.argv.indexOf('--ffi-out');
const FFI_OUT = ffiOutIdx > 0 ? join(ROOT, process.argv[ffiOutIdx + 1]) : null;
const ffiParts = new Map(modules.map((m) => [m, []]));

for (const mod of modules) {
  const defs = [];
  for (const itf of interfaces.filter((i) => i.module === mod)) {
    const nested = [];
    const s = structFor(itf.rustName, itf.members, itf.source, `${itf.source} ${itf.name}`, nested);
    for (const d of [s, ...nested]) { defs.push(d.rust); ffiParts.get(mod).push(d); }
  }
  const sources = [...new Set(interfaces.filter((i) => i.module === mod).map((i) => i.source))].join(', ');
  const head =
`//! @generated by clients/demo/tools/gen-read-models.mjs from ${sources || 'nothing'}.
//! Do not edit: run \`node clients/demo/tools/gen-read-models.mjs --write\`. [FR-C-1] [AC-C-13]
#![allow(clippy::all)]

#[allow(unused_imports)]
use crate::consoles::decode::{field, object, opt_field, DecodeError, FromJson};
#[allow(unused_imports)]
use attest::json::Json;
${mod === 'common' ? '' : '#[allow(unused_imports)]\nuse crate::consoles::common::*;\n'}`;
  outputs.set(join(CORE_OUT, `${mod}.rs`), rustfmt(`${head}\n${defs.join('\n')}`));
}

if (FFI_OUT) {
  // FFI mirrors: the same fields with uniffi derives, and From<core> conversions.
  const conv = (ty, expr) => {
    if (ty === 'crate::consoles::decode::KeyValues') return `${expr}.0.into_iter().map(|(key, value)| KeyValue { key, value }).collect()`;
    const v = ty.match(/^Vec<(.+)>$/); if (v) return `${expr}.into_iter().map(|x| ${conv(v[1], 'x')}).collect()`;
    const o = ty.match(/^Option<(.+)>$/); if (o) return `${expr}.map(|x| ${conv(o[1], 'x')})`;
    return ['String', 'f64', 'u32', 'bool'].includes(ty) ? expr : `${expr}.into()`;
  };
  const ffiTy = (ty) => ty.replace(/crate::consoles::decode::KeyValues/g, 'Vec<KeyValue>');
  for (const mod of modules) {
    const body = ffiParts.get(mod).map((d) =>
`#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ${d.name} {
${d.fields.map((f) => `    pub ${f.rs}: ${ffiTy(f.ty)},`).join('\n')}
}

impl From<core::${d.name}> for ${d.name} {
    fn from(x: core::${d.name}) -> Self {
        Self {
${d.fields.map((f) => `            ${f.rs}: ${conv(f.ty, `x.${f.rs}`)},`).join('\n')}
        }
    }
}
`).join('\n');
    const head =
`//! @generated by clients/demo/tools/gen-read-models.mjs: UniFFI mirrors of
//! midgard_client_core::consoles::${mod}. Do not edit. [FR-C-1] [DD-6]
#![allow(clippy::all)]

#[allow(unused_imports)]
use super::common::*;
#[allow(unused_imports)]
use super::KeyValue;
use midgard_client_core::consoles::${mod} as core;
`;
    outputs.set(join(FFI_OUT, `${mod}.rs`), rustfmt(`${head}\n${body}`));
  }
}

const mode = process.argv.includes('--write') ? 'write' : process.argv.includes('--check') ? 'check' : die('use --write or --check');
let stale = 0;
for (const [path, content] of outputs) {
  if (mode === 'write') { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, content); continue; }
  if (!existsSync(path) || readFileSync(path, 'utf8') !== content) { console.error(`stale: ${path.slice(ROOT.length + 1)}`); stale++; }
}
if (stale) die(`${stale} generated file(s) differ from the TypeScript contract; run with --write`);
console.log(`gen-read-models: ${interfaces.length} interfaces, ${outputs.size} file(s) ${mode === 'write' ? 'written' : 'up to date'}`);
```

Notes for the implementer: (1) in the FFI mirrors `core::X` only resolves for types defined in that module; for `common` types referenced from another module the `From` impl still works because `.into()` is called on the field value, whose type carries its own `From`. (2) If `typeOf` reports `unsupported type` for a name defined in `types.ts` but referenced from a product file (for example `MarkingLevel`), that name is an alias and must resolve through `aliases`; check the order of the two loops if it does not. (3) The `common` module comes first so that `use crate::consoles::common::*;` resolves in the others.

**Step 3d: Generate.**

```bash
node clients/demo/tools/gen-read-models.mjs --write
```

Expected: `gen-read-models: <n> interfaces, 7 file(s) written`. Open `clients/core/src/consoles/admin.rs` and check that `AdminApproval` has `pub risk: u32` and `pub denied_reason: Option<String>`, and that `AdminOverview.security` became `AdminOverviewSecurity`.

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test read_models
node clients/demo/tools/gen-read-models.mjs --check     # expect: "… 7 file(s) up to date"
cargo fmt --all --check
cargo clippy --offline --locked -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

Expected: `test result: ok. 3 passed; 0 failed`. If clippy flags the generated modules despite `#![allow(clippy::all)]` (for example `missing_docs` is a rustc lint, not clippy), add the named lint to the generated header in the generator, not by hand.

**Step 5: Commit.**

```bash
git add clients/demo/tools/gen-read-models.mjs clients/core/Cargo.toml clients/core/src/lib.rs \
        clients/core/src/consoles clients/core/tests/read_models.rs Cargo.lock
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: console read models generated from the TypeScript contract [FR-C-1]"
```

(`Cargo.lock` changes only by the new path edge `midgard-client-core -> attest`.)

---

### Task A4: Fixture extraction from the release's mocks

**Which fixtures.** The web consoles' demo mode loads, by dynamic import from `@midgard/sdk/mock` (`sdk/ts/src/mock/index.ts`), one in-memory gateway per console: `MockGateway` (Midgard; seed data in `sdk/ts/src/mock/fixtures.ts`), `AdminMock`, `HeimdallMock`, `ForgeMock`, `MimirMock`, `BifrostMock` (`sdk/ts/src/mock/{admin,heimdall,forge,mimir,bifrost}.ts`). Their fixtures are module-private `const`s, so they are read through the gateway methods, exactly as the consoles read them. Each mock takes `{ latencyMs }`; `latencyMs: 0` makes the reads immediate. Each collection is read from a **fresh** mock instance, because several calls emit audit records into the instance (`MockGateway.query` emits `Query · …`), and a shared instance would leak the extraction's own calls into the audit fixtures.

`ClientMock` (`sdk/ts/src/mock/clients.ts`) is deliberately **not** extracted: it simulates the client itself (sync state, key custody, tier), and in the Apple apps those readouts come from the real core, not from a fixture.

**The recorded Heimdall hour is not in the repository at the tag.** A search of `v0.1.0-rc.3` for recorded or replayed feed data (`MIDGARD_REPLAY`, `replay`, `*.ndjson`, `*.jsonl`) finds only replay *protection* (`heimdall/feed-gateway/src/replay.rs`) and the import-contract fixtures under `contracts/fixtures/import/`. Until the owner decides otherwise (see Open questions), this task substitutes a **synthetic hour**: each of the five `HeimdallMock` tracks flies a closed loop for 60 minutes, sampled every 10 s, starting at its picture position. It is labelled `synthetic` in the dataset.

**Positions.** `HeimdallTrack` carries canvas percentages (`x`, `y`), not coordinates. For MapKit they are mapped onto a fixed demonstration box, latitude 51.0–56.0 N, longitude 10.0–2.0 W (the Irish Sea area the `ClientMock` fixture already uses: `53.42 N · 6.27 W`), `y = 0` at the north edge.

**Markings.** Rows that carry a marking (`ObjectType.marking`/`compartment`, `ObjectInstance.markings[0]`, `HeimdallEntity.marking`/`compartment`, `ForgePipeline.marking`/`compartment`, …) keep it. Every other row takes its marking from a committed overlay, `clients/demo/overlay.json`, so that switching persona visibly changes what is shown. Heimdall tracks take the marking of the entity with the same id.

**Personas** (`clients/demo/personas.json`), all names taken from the fixtures themselves:

| id | role | clearance | compartments | roles |
|---|---|---|---|---|
| `k.rao` | analyst | restricted | — | analyst |
| `j.benjamin` | operator | confidential | OPS-N | operator, analyst, data-engineer |
| `k.osei` | approver | confidential | OPS-N, SIG | operator, watch-officer, approver, iam-admin |
| `a.holt` | release manager | internal | — | release-manager, operator |

**Files:**
- Create: `clients/demo/personas.json`, `clients/demo/overlay.json`
- Create: `clients/demo/tools/extract-fixtures.mjs`, `clients/demo/tools/extract-fixtures.sh`
- Create (output): `clients/demo/fixtures/0.1.0-rc.3/{manifest,personas,midgard,admin,heimdall,forge,mimir,bifrost}.json`
- Test: `tools/ci/tests/demo-fixtures.test.sh`

**Step 1: Write the failing test** — `tools/ci/tests/demo-fixtures.test.sh`:

```bash
#!/usr/bin/env bash
# The demo fixtures are what the release's mocks serve, extracted deterministically,
# and the generated read models match the TypeScript contract. [AC-C-13] [FR-C-1]
#
#   bash tools/ci/tests/demo-fixtures.test.sh
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
failures=0
pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

release="$(bash tools/ci/release-artefacts.sh --version)"
if bash clients/demo/tools/extract-fixtures.sh --check >/dev/null 2>&1; then
    pass "clients/demo/fixtures/$release is what the mocks serve"
else
    fail "clients/demo/fixtures/$release is stale: bash clients/demo/tools/extract-fixtures.sh"
fi

a="$(mktemp -d)"; b="$(mktemp -d)"
trap 'rm -rf "$a" "$b"' EXIT
bash clients/demo/tools/extract-fixtures.sh --out "$a" >/dev/null 2>&1
bash clients/demo/tools/extract-fixtures.sh --out "$b" >/dev/null 2>&1
if [ -s "$a/manifest.json" ] && diff -r "$a" "$b" >/dev/null; then
    pass "two extractions are byte-identical"
else
    fail "extraction is not deterministic"
fi

if node clients/demo/tools/gen-read-models.mjs --check >/dev/null 2>&1; then
    pass "the console read models match sdk/ts/src"
else
    fail "stale read models: node clients/demo/tools/gen-read-models.mjs --write"
fi

[ "$failures" -eq 0 ] || { echo "$failures failure(s)"; exit 1; }
```

`selftest` (`tools/ci/run.sh`, line 121) already runs every `tools/ci/tests/*.test.sh`, so this joins CI with no pipeline change.

**Step 2: Run it and see it fail.**

```bash
bash tools/ci/tests/demo-fixtures.test.sh
```

Expected: `FAIL  clients/demo/fixtures/0.1.0-rc.3 is stale: …` and `FAIL  extraction is not deterministic` (the script does not exist yet), `ok    the console read models match sdk/ts/src`, then `2 failure(s)`.

**Step 3a: Personas and overlay.** `clients/demo/personas.json`:

```json
[
  { "id": "k.rao", "display": "K. Rao", "role": "analyst", "clearance": "restricted", "compartments": [], "roles": ["analyst"] },
  { "id": "j.benjamin", "display": "J. Benjamin", "role": "operator", "clearance": "confidential", "compartments": ["OPS-N"], "roles": ["operator", "analyst", "data-engineer"] },
  { "id": "k.osei", "display": "K. Osei", "role": "approver", "clearance": "confidential", "compartments": ["OPS-N", "SIG"], "roles": ["operator", "watch-officer", "approver", "iam-admin"] },
  { "id": "a.holt", "display": "A. Holt", "role": "release-manager", "clearance": "internal", "compartments": [], "roles": ["release-manager", "operator"] }
]
```

`clients/demo/overlay.json` (a first cut; the console phases adjust it, and the extraction refuses an id the overlay names that the collection does not have):

```json
{
  "default": { "level": "internal", "compartments": [] },
  "collections": {
    "midgard.audit": { "default": { "level": "restricted", "compartments": [] } },
    "midgard.keys": { "default": { "level": "restricted", "compartments": [] },
                      "ids": { "kek-root": { "level": "confidential", "compartments": ["OPS-N"] } } },
    "admin.ledger": { "default": { "level": "restricted", "compartments": [] } },
    "admin.breakGlass": { "default": { "level": "confidential", "compartments": [] } },
    "heimdall.feeds": { "default": { "level": "restricted", "compartments": [] } },
    "heimdall.syncSites": { "default": { "level": "restricted", "compartments": [] } },
    "heimdall.decisions": { "default": { "level": "confidential", "compartments": ["OPS-N"] } },
    "mimir.sessions": { "default": { "level": "restricted", "compartments": [] } },
    "bifrost.fleet": { "default": { "level": "restricted", "compartments": [] } }
  }
}
```

**Step 3b: The extractor** — `clients/demo/tools/extract-fixtures.mjs`:

```js
// Extract the demo fixtures from the release's console mocks. [AC-C-13] [FR-C-1]
//
// Run through extract-fixtures.sh, which loads Node's type stripping and the SDK's
// extensionless-import hook (sdk/ts/scripts/ts-resolve.mjs). Output is canonical JSON
// (sorted keys, no whitespace), one file per console, plus manifest.json with the
// git blob id of every input and the SHA-256 of every output.
import { readFileSync, writeFileSync, mkdirSync, rmSync, readdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : dflt; };
const version = readFileSync(join(ROOT, 'Cargo.toml'), 'utf8').match(/^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)[1];
const release = arg('--release', version);
const out = arg('--out', join(ROOT, 'clients/demo/fixtures', release));
const die = (m) => { console.error(`extract-fixtures: ${m}`); process.exit(1); };

const mock = await import(pathToFileURL(join(ROOT, 'sdk/ts/src/mock/index.ts')).href);
const fresh = {
  midgard: () => new mock.MockGateway({ latencyMs: 0 }),
  admin: () => new mock.AdminMock({ latencyMs: 0 }),
  heimdall: () => new mock.HeimdallMock({ latencyMs: 0 }),
  forge: () => new mock.ForgeMock({ latencyMs: 0 }),
  mimir: () => new mock.MimirMock({ latencyMs: 0 }),
  bifrost: () => new mock.BifrostMock({ latencyMs: 0 }),
};
const SOURCE = {
  midgard: ['sdk/ts/src/mock/MockGateway.ts', 'sdk/ts/src/mock/fixtures.ts'],
  admin: ['sdk/ts/src/mock/admin.ts'], heimdall: ['sdk/ts/src/mock/heimdall.ts'],
  forge: ['sdk/ts/src/mock/forge.ts'], mimir: ['sdk/ts/src/mock/mimir.ts'],
  bifrost: ['sdk/ts/src/mock/bifrost.ts'],
};

// name, the Rust read model, the read, and (optionally) how to id a row.
const one = (id) => async (p) => [{ ...(await p), __id: id }];
const C = [
  ['midgard.objectTypes', 'MidgardObjectType', (g) => g.listObjectTypes(), (x) => x.apiName],
  ['midgard.objects', 'MidgardObjectInstance', async (g) => {
    const types = await g.listObjectTypes(); const rows = [];
    for (const t of types) rows.push(...(await fresh.midgard().query({ policy: g.policy(), objectType: t.apiName, maxResults: 5 })).results);
    return rows; }],
  ['midgard.searchAround', 'MidgardSearchGraph', async (g) => {
    const types = await g.listObjectTypes(); const rows = [];
    for (const t of types) rows.push(await fresh.midgard().searchAround(`${t.apiName}/1000`, 2));
    return rows; }, (x) => x.seed],
  ['midgard.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['midgard.policies', 'MidgardPolicyRule', (g) => g.listPolicies()],
  ['midgard.identities', 'MidgardIdentity', (g) => g.listIdentities(), (x) => x.subject],
  ['midgard.keys', 'MidgardKeyRecord', (g) => g.listKeys()],
  ['midgard.computeJobs', 'MidgardComputeJob', (g) => g.listComputeJobs()],
  ['midgard.observability', 'MidgardSloMetric', (g) => g.getObservability(), (x) => x.label],
  ['midgard.resilience', 'MidgardRolloutState', (g) => g.getResilience()],
  ['midgard.contractSurface', 'MidgardRpcSurface', (g) => g.getContractSurface(), (x) => x.service],
  ['admin.overview', 'AdminOverview', (g) => one('overview')(g.getOverview())],
  ['admin.approvals', 'AdminApproval', (g) => g.listApprovals()],
  ['admin.roles', 'AdminRole', (g) => g.listRoles(), (x) => x.name],
  ['admin.bindings', 'AdminBinding', (g) => g.listBindings(), (x) => `${x.user}:${x.role}`],
  ['admin.configCommits', 'AdminConfigCommit', (g) => g.listConfigCommits()],
  ['admin.configSimulations', 'AdminConfigSimulation', async (g) => {
    const rows = [];
    for (const c of await g.listConfigCommits()) rows.push({ ...(await fresh.admin().simulateConfig(c.id)), __id: c.id });
    return rows; }],
  ['admin.registry', 'AdminRegistryItem', (g) => g.listRegistry(), (x) => x.name],
  ['admin.ledger', 'AdminLedgerEntry', (g) => g.searchLedger('')],
  ['admin.breakGlass', 'AdminBreakGlass', (g) => one('breakglass')(g.getBreakGlass())],
  ['admin.copilot', 'AdminCopilotTurn', (g) => g.getCopilotThread()],
  ['heimdall.picture', 'HeimdallPicture', async (g) => [{ ...(await g.getPicture()), tracks: [], entities: [], __id: 'picture' }]],
  ['heimdall.tracks', 'HeimdallTrack', async (g) => (await g.getPicture()).tracks],
  ['heimdall.feeds', 'HeimdallFeed', (g) => g.listFeeds(), (x) => x.name],
  ['heimdall.entities', 'HeimdallEntity', (g) => g.listEntities()],
  ['heimdall.syncSites', 'HeimdallSyncSite', (g) => g.listSyncSites(), (x) => x.name],
  ['heimdall.workflows', 'HeimdallWorkflow', (g) => g.listWorkflows()],
  ['heimdall.decisions', 'HeimdallDecision', (g) => g.listDecisions()],
  ['heimdall.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['forge.pipelines', 'ForgePipeline', (g) => g.listPipelines()],
  ['forge.connectors', 'ForgeConnector', (g) => g.listConnectors()],
  ['forge.datasets', 'ForgeDataset', (g) => g.listDatasets()],
  ['forge.healthChecks', 'ForgeHealthCheck', (g) => g.listHealthChecks()],
  ['forge.schedules', 'ForgeSchedule', (g) => g.listSchedules()],
  ['forge.transform', 'ForgeTransform', (g) => one('transform')(g.getTransform())],
  ['forge.projections', 'ForgeProjection', (g) => g.listProjections()],
  ['forge.audit', 'AuditRecord', (g) => g.getAudit(50)],
  ['mimir.agents', 'MimirAgent', (g) => g.listAgents()],
  ['mimir.logic', 'MimirLogicFn', async (g) => {
    const rows = [];
    for (const a of await g.listAgents()) rows.push({ ...(await fresh.mimir().getLogic(a.id)), __id: a.id });
    return rows; }],
  ['mimir.sessions', 'MimirSession', (g) => g.listSessions()],
  ['mimir.evals', 'MimirEval', (g) => g.listEvals()],
  ['mimir.guardrails', 'MimirGuardrail', (g) => g.listGuardrails()],
  ['mimir.feedback', 'MimirFeedbackItem', (g) => g.listFeedback()],
  ['mimir.models', 'MimirModel', (g) => g.listModels()],
  ['bifrost.releases', 'BifrostRelease', (g) => g.listReleases()],
  ['bifrost.rings', 'BifrostRing', (g) => g.listRings()],
  ['bifrost.artefacts', 'BifrostArtefact', (g) => g.listArtefacts()],
  ['bifrost.channels', 'BifrostChannel', (g) => g.listChannels()],
  ['bifrost.bundles', 'BifrostBundle', (g) => g.listBundles()],
  ['bifrost.fleet', 'BifrostFleetNode', (g) => g.listFleet()],
  ['bifrost.rollbacks', 'BifrostRollback', (g) => g.listRollbacks()],
  ['bifrost.audit', 'AuditRecord', (g) => g.getAudit(50)],
];

const personas = JSON.parse(readFileSync(join(ROOT, 'clients/demo/personas.json'), 'utf8'));
const overlay = JSON.parse(readFileSync(join(ROOT, 'clients/demo/overlay.json'), 'utf8'));
const LEVELS = ['open', 'internal', 'restricted', 'confidential'];
for (const p of personas) if (!LEVELS.includes(p.clearance)) die(`persona ${p.id}: unknown clearance ${p.clearance}`);

function markingOf(name, id, x, entities) {
  const lvl = (v) => (LEVELS.includes(v) ? v : null);
  const fromItem = lvl(x.marking) ?? lvl(x.markings?.[0]?.level);
  if (fromItem) {
    const comps = x.compartments ?? x.markings?.[0]?.compartments ?? (x.compartment ? [x.compartment] : []);
    return { level: fromItem, compartments: [...new Set(comps)].sort() };
  }
  if (name === 'heimdall.tracks' && entities.has(id)) return entities.get(id);
  const c = overlay.collections[name] ?? {};
  return c.ids?.[id] ?? c.default ?? overlay.default;
}

// Canonical JSON: sorted keys, no whitespace.
const canon = (v) => JSON.stringify(v, (_, x) =>
  x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x);

const byConsole = new Map(Object.keys(fresh).map((c) => [c, {}]));
const entityMarkings = new Map();
for (const [name, type, read, idOf] of C) {
  const console_ = name.split('.')[0];
  const raw = await read(fresh[console_]());
  const seen = new Set();
  const rows = raw.map((x, i) => {
    const { __id, ...item } = x;
    const id = String(__id ?? idOf?.(item) ?? item.id ?? item.name ?? i);
    if (seen.has(id)) die(`${name}: duplicate row id ${id}; give the collection an id function`);
    seen.add(id);
    return { id, marking: markingOf(name, id, item, entityMarkings), item };
  });
  if (name === 'heimdall.entities') for (const r of rows) entityMarkings.set(r.id, r.marking);
  for (const id of Object.keys(overlay.collections[name]?.ids ?? {})) if (!seen.has(id)) die(`overlay names ${name}/${id}, which does not exist`);
  byConsole.get(console_)[name] = { type, rows };
}
// heimdall.tracks is read before heimdall.entities in C; re-mark tracks now that entity markings are known.
for (const r of byConsole.get('heimdall')['heimdall.tracks'].rows) if (entityMarkings.has(r.id)) r.marking = entityMarkings.get(r.id);

// The synthetic hour: a 60-minute closed loop per track, every 10 s.
const box = { latN: 56.0, latS: 51.0, lonW: -5.5, lonE: 2.5 }; // east of 6 W: see Task A5 (the guard reads keyed positions both ways round)
const round = (n) => Math.round(n * 1e5) / 1e5;
const motion = byConsole.get('heimdall')['heimdall.tracks'].rows.map((r) => {
  const t = r.item;
  const lat0 = box.latN - (t.y / 100) * (box.latN - box.latS);
  const lon0 = box.lonW + (t.x / 100) * (box.lonE - box.lonW);
  const radiusNm = Math.max(t.spd, 1) / (2 * Math.PI);           // one clockwise loop per hour
  const h = (t.hdg * Math.PI) / 180;
  const k = 60 * Math.cos((lat0 * Math.PI) / 180);                // nm per degree of longitude
  // Centre to the right of the heading; start on the loop at the picture position.
  const cLat = lat0 - (radiusNm * Math.sin(h)) / 60;
  const cLon = lon0 + (radiusNm * Math.cos(h)) / k;
  const a0 = Math.atan2(-Math.cos(h), Math.sin(h));               // bearing from centre to start
  const samples = [];
  for (let s = 0; s < 360; s++) {
    const a = a0 + (2 * Math.PI * s) / 360;
    samples.push({ t: s * 10, lat: round(cLat + (radiusNm * Math.cos(a)) / 60), lon: round(cLon + (radiusNm * Math.sin(a)) / k) });
  }
  return { id: r.id, marking: r.marking, item: { id: r.id, provenance: 'synthetic: HeimdallMock track, closed loop', samples } };
});
byConsole.get('heimdall')['heimdall.motion'] = { type: 'HeimdallMotion', rows: motion };

const blob = (p) => execFileSync('git', ['hash-object', join(ROOT, p)], { encoding: 'utf8' }).trim();
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const files = {};
const write = (f, v) => { const s = canon(v); writeFileSync(join(out, f), s); files[f] = createHash('sha256').update(s).digest('hex'); };
write('personas.json', personas);
for (const [c, collections] of byConsole) {
  write(`${c}.json`, { console: c, sources: Object.fromEntries(SOURCE[c].map((p) => [p, blob(p)])), collections });
}
const inputs = Object.fromEntries([...Object.values(SOURCE).flat(), 'clients/demo/personas.json',
  'clients/demo/overlay.json', 'clients/demo/tools/extract-fixtures.mjs'].map((p) => [p, blob(p)]));
writeFileSync(join(out, 'manifest.json'), canon({ format: 'midgard-demo-fixtures', version: 1, release, files, inputs }));
console.log(`extract-fixtures: ${C.length} collections + motion -> ${relative(ROOT, out) || out}`);
```

The motion arithmetic is deliberately plain trigonometry rounded to five decimals (about 1 m). V8 computes `Math.sin`/`Math.cos` with its own fdlibm port, so two runs on the same Node major give the same bytes; the determinism test above is what guards that.

`clients/demo/tools/extract-fixtures.sh` (bash 3.2 safe):

```bash
#!/usr/bin/env bash
# Extract the demo fixtures (extract-fixtures.mjs); --check compares with the committed
# copy instead of writing it. [AC-C-13]
set -euo pipefail
cd "$(dirname "$0")/../../.."
release="$(bash tools/ci/release-artefacts.sh --version)"
run() {
    node --experimental-strip-types --no-warnings --import ./sdk/ts/scripts/ts-resolve.mjs \
        clients/demo/tools/extract-fixtures.mjs --release "$release" "$@"
}
if [ "${1:-}" = --check ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    run --out "$tmp" >/dev/null
    diff -r "clients/demo/fixtures/$release" "$tmp"
else
    run "$@"
fi
```

**Step 3c: Extract.**

```bash
chmod +x clients/demo/tools/extract-fixtures.sh
bash clients/demo/tools/extract-fixtures.sh
ls clients/demo/fixtures/0.1.0-rc.3
```

Expected: `extract-fixtures: 52 collections + motion -> clients/demo/fixtures/0.1.0-rc.3`, and eight files. If a collection dies on a duplicate id, add an id function to its row in `C`; if a read's method name differs from the one in `C`, the error names it — check the gateway interface in `sdk/ts/src/products/<console>.ts`.

**Step 4: Run it and see it pass.**

```bash
bash tools/ci/tests/demo-fixtures.test.sh
```

Expected: three `ok` lines and exit 0.

**Step 5: Commit.**

```bash
git add clients/demo/personas.json clients/demo/overlay.json clients/demo/tools/extract-fixtures.mjs \
        clients/demo/tools/extract-fixtures.sh clients/demo/fixtures tools/ci/tests/demo-fixtures.test.sh
git commit -m "clients/demo: deterministic fixtures from the release's console mocks [AC-C-13]"
```

(No Rust changed, so `NUMBERS.md` is unchanged; run `bash tools/ci/numbers.sh --check` to confirm only the gate-record lines differ as before.)

---

### Task A5: Local-suite data guard

The design forbids any data from a local suite in the dataset: no live feeds, no Mauritius data, no ICIJ/Offshore Leaks data. Task A4 already reads only the tagged mocks; this guard is the second, independent check the build step runs on the assembled dataset body before it signs anything, and it refuses rather than warns.

What it refuses:
1. any string containing a marker of a local data source (case-insensitive): leak-archive names (`icij`, `offshore leaks`, `panama papers`, `pandora papers`, `paradise papers`, `mauritius leaks`), the local suite's live area (`mauritius`, `port louis`, `rodrigues`, `agalega`, `agaléga`), live-feed sources (`adsb.lol`, `adsbexchange`, `airplanes.live`, `opensky`, `aishub`, `aisstream`, `marinetraffic`, `knmi`, `usgs.gov`, `emsc-csem`) and the local suite's replay switches (`live-feeds`, `midgard_replay`);
2. any position inside the Mauritius exclusive economic zone box (25° S–6° S, 52° E–68.5° E) or the Chagos Archipelago box (9° S–3.5° S, 69° E–74.5° E): an object with `lat`/`lon` under any of their usual names, as numbers or numeric strings; a bare array of two or three numbers; or a number pair written in a string (as built, after the 16 Nov review: the guard also folds accents, full-width forms and separators before matching markers, and refuses combining marks and invisible format characters);
3. any `inputs` entry in the fixture manifest outside `sdk/ts/src/mock/`, `clients/demo/` (checked in Task A7's builder, which also checks the blob ids against the tag).

**Files:**
- Create: `clients/core/src/demo/mod.rs`, `clients/core/src/demo/guard.rs`
- Modify: `clients/core/src/lib.rs` (`pub mod demo;`)
- Test: `clients/core/tests/guard.rs`

**Step 1: Write the failing test** — `clients/core/tests/guard.rs`:

```rust
//! The demo dataset may hold only the release's mock fixtures: local-suite data is
//! refused before anything is signed. [NFR-PRV-1] [AC-C-13]

use attest::json::parse;
use midgard_client_core::demo::guard::{check, GuardError};

#[test]
fn the_release_fixture_shapes_pass() {
    let v = parse(r#"{"collections":{"heimdall.motion":{"rows":[{"item":{"samples":[{"lat":53.4,"lon":-2.2,"t":0}]}}]}},
                      "note":"NORTH-2 radar"}"#).unwrap();
    assert_eq!(check(&v), Ok(()));
}

#[test]
fn a_leak_archive_or_live_feed_marker_is_refused_wherever_it_is() {
    for text in ["ICIJ Offshore Leaks node 12000", "source: opensky-network.org", "Port Louis harbour"] {
        let v = parse(&format!(r#"{{"rows":[{{"item":{{"title":{:?}}}}}]}}"#, text)).unwrap();
        let errs = check(&v).unwrap_err();
        assert!(matches!(&errs[0], GuardError::Marker { path, .. } if path == "$.rows[0].item.title"), "{errs:?}");
    }
}

#[test]
fn a_position_in_the_mauritius_area_is_refused() {
    let v = parse(r#"{"samples":[{"lat":-20.16,"lon":57.5}]}"#).unwrap();
    assert!(matches!(&check(&v).unwrap_err()[0], GuardError::Area { area, .. } if area == "Mauritius EEZ"));
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test guard
```

Expected: `unresolved import \`midgard_client_core::demo\``.

**Step 3: Implement.** `clients/core/src/demo/mod.rs`:

```rust
//! The demo enclosure: a signed, bundled dataset served behind the same data-source
//! interface a gateway connection implements. `[AC-C-13]` `[NFR-PQ-3]`

pub mod guard;
```

`clients/core/src/demo/guard.rs`:

```rust
//! The local-suite data guard. `[NFR-PRV-1]` `[AC-C-13]`
//!
//! The demo dataset ships inside an app. It may hold only the release's deterministic
//! mock fixtures; data from a developer's local suite (live feeds, the Mauritius
//! deployment, leak-archive research data) must never reach it. The builder runs
//! [`check`] over the assembled body and refuses to sign on any finding.

use attest::json::Json;

/// Substrings (lower-case) that only local-suite data contains.
pub const DENY_MARKERS: &[&str] = &[
    "icij", "offshore leaks", "panama papers", "pandora papers", "paradise papers", "mauritius leaks",
    "mauritius", "port louis", "rodrigues", "agalega", "agaléga",
    "adsb.lol", "adsbexchange", "airplanes.live", "opensky", "aishub", "aisstream", "marinetraffic",
    "knmi", "usgs.gov", "emsc-csem",
    "live-feeds", "midgard_replay",
];

/// A latitude/longitude box, degrees.
#[derive(Debug, Clone, Copy)]
pub struct Area {
    /// Display name.
    pub name: &'static str,
    /// Southern edge.
    pub lat_min: f64,
    /// Northern edge.
    pub lat_max: f64,
    /// Western edge.
    pub lon_min: f64,
    /// Eastern edge.
    pub lon_max: f64,
}

/// Areas no demo position may fall in.
pub const DENY_AREAS: &[Area] = &[
    Area { name: "Mauritius EEZ", lat_min: -25.0, lat_max: -6.0, lon_min: 52.0, lon_max: 68.5 },
    Area { name: "Chagos Archipelago", lat_min: -9.0, lat_max: -3.5, lon_min: 69.0, lon_max: 74.5 },
];

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub enum GuardError {
    /// A string holds a denied marker.
    Marker {
        /// Where.
        path: String,
        /// Which marker.
        marker: &'static str,
    },
    /// A position falls in a denied area.
    Area {
        /// Where.
        path: String,
        /// Which area.
        area: &'static str,
    },
}

/// Walk `v`; every finding is reported, none is fatal on its own.
pub fn check(v: &Json) -> Result<(), Vec<GuardError>> {
    let mut found = Vec::new();
    walk(v, "$", &mut found);
    if found.is_empty() {
        Ok(())
    } else {
        Err(found)
    }
}

fn num(pairs: &[(String, Json)], keys: &[&str]) -> Option<f64> {
    pairs.iter().find(|(k, _)| keys.contains(&k.as_str())).and_then(|(_, v)| match v {
        Json::Num(t) => t.parse().ok(),
        _ => None,
    })
}

fn walk(v: &Json, path: &str, found: &mut Vec<GuardError>) {
    match v {
        Json::Str(s) => {
            let lower = s.to_lowercase();
            if let Some(m) = DENY_MARKERS.iter().find(|m| lower.contains(*m)) {
                found.push(GuardError::Marker { path: path.to_string(), marker: m });
            }
        }
        Json::Arr(items) => {
            for (i, x) in items.iter().enumerate() {
                walk(x, &format!("{path}[{i}]"), found);
            }
        }
        Json::Obj(pairs) => {
            if let (Some(lat), Some(lon)) = (num(pairs, &["lat", "latitude"]), num(pairs, &["lon", "longitude"])) {
                for a in DENY_AREAS {
                    if (a.lat_min..=a.lat_max).contains(&lat) && (a.lon_min..=a.lon_max).contains(&lon) {
                        found.push(GuardError::Area { path: path.to_string(), area: a.name });
                    }
                }
            }
            for (k, x) in pairs {
                // Keys are data too: a collection named after a live source is refused.
                walk(&Json::Str(k.clone()), &format!("{path}.{k}"), found);
                walk(x, &format!("{path}.{k}"), found);
            }
        }
        Json::Null | Json::Bool(_) | Json::Num(_) => {}
    }
}
```

The key walk reports a marker in a key at the key's own path; the first test's assertion is on a value path, which the value walk produces. Add `pub mod demo;` to `clients/core/src/lib.rs`.

**Step 4: Run it and see it pass.** Also run the guard over the real fixtures so a false positive in them is found now, not at signing time:

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test guard
grep -ril -e icij -e mauritius -e opensky -e 'port louis' clients/demo/fixtures || echo "fixtures clean"
```

Expected: `3 passed`; `fixtures clean`.

**Step 5: Commit.**

```bash
git add clients/core/src/demo clients/core/src/lib.rs clients/core/tests/guard.rs
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: refuse local-suite data in the demo dataset [NFR-PRV-1]"
```

---

### Task A6: Dataset file format and signature check

**Format decision.** One file per release, `clients/demo/datasets/<release>.mgdemo`:

```text
magic       7   b"MGDEMO1"
version     1   u8 = 1
release_len 4   u32 LE ┐ the release this dataset is for, e.g. "0.1.0-rc.3"
release     n          ┘
body_len    4   u32 LE ┐ canonical JSON (sorted keys, no whitespace)
body        n          ┘
env_len     4   u32 LE ┐ an attest MGSIG1 envelope (crypto/attest/src/envelope.rs)
envelope    n          ┘
```

The signature covers `attest::accept::preimage(&DEMO_DATASET, magic ‖ version ‖ release_len ‖ release ‖ body_len ‖ body)`, so the release label cannot be swapped under a valid signature, and a demo-dataset signature can never be presented as a release-artefact, Bifrost or Mimir signature (the domain label is length-prefixed; see `crypto/attest/src/accept.rs`). Why this format: it reuses the suite's one signature envelope and its one acceptance decision (`attest::accept`), so there is no second verifier; the body stays canonical JSON that the strict `attest::json` parser reads and a reviewer can diff; and it is one file, as the design asks. The `.sig` sidecar format was not used because the dataset is bundled into an app as one resource, and `.gitignore` ignores `*.sig`.

**Signing key decision.** There is **no release demo key at the tag**: `deploy/trust/` pins `release-trust.pub` (ML-DSA-87), `bifrost-trust.pub` and `model-signing.pub`, all populated only by the Dvalin key ceremony, and `deploy/trust/README.md` forbids committing development keys there. The demo dataset therefore gets its own key, generated on this machine the way `deploy/compose/stage-model.sh --development` generates a development model key: an `attest` version-3 keystore made by `mg-sign keygen`, ML-DSA-65 (one of `attest::APPROVED_RELEASE_ALGS`), its passphrase in a 0600 file outside the repository. Its public half is pinned at `clients/demo/trust/demo-dataset.pub`, outside `deploy/trust/`, and is trusted only under the `midgard-demo-dataset-v1` domain. Verifiers accept ML-DSA-65 or ML-DSA-87, so the owner can later re-sign with a ceremony key without a format change.

**Files:**
- Modify: `crypto/attest/src/accept.rs` (add `DEMO_DATASET`)
- Create: `clients/core/src/demo/canon.rs`, `clients/core/src/demo/dataset.rs`
- Modify: `clients/core/src/demo/mod.rs`
- Test: `clients/core/tests/dataset.rs`; one unit test in `crypto/attest/src/accept.rs`

**Step 1: Write the failing tests.** In `crypto/attest/src/accept.rs`, inside `mod tests`:

```rust
    #[test]
    fn the_demo_dataset_domain_is_its_own() {
        for other in [RELEASE_ARTEFACT, BIFROST_ARTEFACT, BIFROST_BUNDLE, MIMIR_MODEL, MIMIR_MODEL_FILE] {
            assert_ne!(DEMO_DATASET.label(), other.label());
            assert_ne!(preimage(&DEMO_DATASET, b"x"), preimage(&other, b"x"));
        }
    }
```

`clients/core/tests/dataset.rs`:

```rust
//! The demo dataset is accepted only under its own domain, with a pinned key, for the
//! release it names; any tamper is refused. [NFR-PQ-3] [AC-C-13]

use attest::{SignError, SigningBackend, TrustAnchors};
use midgard_client_core::demo::dataset::{self, DatasetError};
use pqc_agility::backend::fips::provider;
use pqc_agility::{OsRng, SigId, Signature, SigningKey, VerifyingKey};

struct TestSigner {
    id: String,
    sk: SigningKey,
    vk: VerifyingKey,
}
impl TestSigner {
    fn new(id: &str) -> Self {
        let (vk, sk) = provider().sig_for(SigId::MlDsa65).unwrap().keypair(&mut OsRng).unwrap();
        Self { id: id.into(), sk, vk }
    }
    fn anchors(&self) -> TrustAnchors {
        let mut a = TrustAnchors::new();
        a.insert(self.id.clone(), self.vk.clone());
        a
    }
}
impl SigningBackend for TestSigner {
    fn key_id(&self) -> &str {
        &self.id
    }
    fn sign(&self, msg: &[u8]) -> Result<Signature, SignError> {
        provider().sig_for(SigId::MlDsa65).unwrap().sign(&self.sk, msg).map_err(|e| SignError::Custodian(e.to_string()))
    }
    fn verifying_key(&self) -> &VerifyingKey {
        &self.vk
    }
}

const BODY: &[u8] = br#"{"format":"midgard-demo-dataset","version":1}"#;

#[test]
fn a_signed_dataset_round_trips() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign("0.1.0-rc.3", BODY, &s).unwrap();
    let ds = dataset::verify(&file, &s.anchors(), "0.1.0-rc.3").unwrap();
    assert_eq!(ds.body_bytes, BODY);
    assert_eq!(ds.key_id, "demo-test");
}

#[test]
fn any_flipped_byte_is_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign("0.1.0-rc.3", BODY, &s).unwrap();
    for i in [0, 8, 12, 30, file.len() - 1] {
        let mut bad = file.clone();
        bad[i] ^= 0x01;
        assert!(dataset::verify(&bad, &s.anchors(), "0.1.0-rc.3").is_err(), "byte {i} accepted");
    }
}

#[test]
fn an_unpinned_key_another_release_or_trailing_bytes_are_refused() {
    let s = TestSigner::new("demo-test");
    let file = dataset::sign("0.1.0-rc.3", BODY, &s).unwrap();
    let other = TestSigner::new("demo-test");
    assert!(matches!(dataset::verify(&file, &other.anchors(), "0.1.0-rc.3"), Err(DatasetError::Signature(_))));
    assert!(matches!(dataset::verify(&file, &TrustAnchors::new(), "0.1.0-rc.3"), Err(DatasetError::UnknownKey(_))));
    assert!(matches!(
        dataset::verify(&file, &s.anchors(), "0.1.0-rc.4"),
        Err(DatasetError::WrongRelease { .. })
    ));
    let mut long = file.clone();
    long.push(0);
    assert!(matches!(dataset::verify(&long, &s.anchors(), "0.1.0-rc.3"), Err(DatasetError::Malformed(_))));
}
```

**Step 2: Run them and see them fail.**

```bash
cargo test --offline --locked -p attest the_demo_dataset_domain_is_its_own
cargo test --offline --locked -p midgard-client-core --features backend-fips --test dataset
```

Expected: `cannot find value \`DEMO_DATASET\` in this scope`; `unresolved import \`midgard_client_core::demo::dataset\``. If the second fails first on `OsRng`/`SigningBackend` imports, check that the core's dev-dependencies include `pqc-agility` with `backend-fips` (it is already a normal dependency; tests see it) and `attest` (normal dependency since Task A3).

**Step 3: Implement.** In `crypto/attest/src/accept.rs`, after `MIMIR_MODEL_FILE`:

```rust
/// A client demo dataset (`clients/demo/datasets/<release>.mgdemo`): the bundled data
/// a demonstration app serves instead of a gateway. Its own domain, so a demo-dataset
/// signature can never stand for a release artefact. [NFR-PQ-3] [AC-C-13]
pub const DEMO_DATASET: Domain = Domain::new(b"midgard-demo-dataset-v1");
```

`clients/core/src/demo/canon.rs` (canonical JSON; `attest::json` has no encoder by design, so the core writes one for the dataset only):

```rust
//! Canonical JSON: object keys sorted by byte order, no whitespace, numbers as their
//! validated source text, strings escaped minimally (RFC 8259). Two parses of equal
//! data encode to equal bytes. `[NFR-PQ-3]`

use attest::json::Json;

/// Encode `v` canonically.
pub fn encode(v: &Json) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(t) => out.push_str(t),
        Json::Str(s) => string(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Json::Obj(pairs) => {
            let mut sorted: Vec<&(String, Json)> = pairs.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (i, (k, x)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
```

Note: the extractor sorts keys with JavaScript's default string order (UTF-16 code units) and this encoder sorts by UTF-8 bytes; the two agree for the ASCII keys the fixtures use. The builder re-encodes with this function, so the signed bytes never depend on Node's order.

`clients/core/src/demo/dataset.rs`:

```rust
//! The signed demo dataset file. `[NFR-PQ-3]` `[AC-C-13]`
//!
//! Format in Task A6 of docs/plans/2026-11-13-apple-clients-plan.md. Verification
//! order: strict parse, domain label, pinned key, `attest::accept`, release label —
//! and only then is the body parsed.

use attest::accept::{accept, preimage, AcceptError, DEMO_DATASET};
use attest::envelope::SigEnvelope;
use attest::json::{parse, Json};
use attest::{SigningBackend, TrustAnchors};
use pqc_agility::SigId;

/// File magic.
pub const MAGIC: &[u8; 7] = b"MGDEMO1";
/// Format version.
pub const VERSION: u8 = 1;
/// Algorithms a demo dataset may be signed with.
pub const APPROVED: [SigId; 2] = [SigId::MlDsa65, SigId::MlDsa87];

/// Why a dataset was refused.
#[derive(Debug)]
pub enum DatasetError {
    /// Truncated, trailing bytes, bad magic or version.
    Malformed(String),
    /// The envelope names another signing domain.
    WrongDomain,
    /// The signing key is not pinned.
    UnknownKey(String),
    /// The signature does not verify under the pinned key.
    Signature(AcceptError),
    /// The dataset is for another release.
    WrongRelease {
        /// What the file says.
        found: String,
        /// What this build expects.
        expected: String,
    },
    /// The body is not strict JSON.
    Body(String),
    /// Signing failed (build step only).
    Sign(String),
}

impl std::fmt::Display for DatasetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatasetError::Malformed(d) => write!(f, "malformed demo dataset: {d}"),
            DatasetError::WrongDomain => write!(f, "not signed as a demo dataset"),
            DatasetError::UnknownKey(k) => write!(f, "signing key {k:?} is not pinned"),
            DatasetError::Signature(e) => write!(f, "demo dataset signature refused: {e}"),
            DatasetError::WrongRelease { found, expected } => {
                write!(f, "demo dataset is for {found}, this build is {expected}")
            }
            DatasetError::Body(d) => write!(f, "demo dataset body: {d}"),
            DatasetError::Sign(d) => write!(f, "signing failed: {d}"),
        }
    }
}

impl std::error::Error for DatasetError {}

/// A verified dataset.
#[derive(Debug)]
pub struct Dataset {
    /// The release it is for.
    pub release: String,
    /// The signing key id.
    pub key_id: String,
    /// The canonical body bytes, as signed.
    pub body_bytes: Vec<u8>,
    /// The parsed body.
    pub body: Json,
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// The bytes the signature covers (before the domain preimage).
pub fn signed_bytes(release: &str, body: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(16 + release.len() + body.len());
    o.extend_from_slice(MAGIC);
    o.push(VERSION);
    put(&mut o, release.as_bytes());
    put(&mut o, body);
    o
}

/// Sign `body` for `release` through `signer` and return the file bytes.
pub fn sign(release: &str, body: &[u8], signer: &dyn SigningBackend) -> Result<Vec<u8>, DatasetError> {
    let signed = signed_bytes(release, body);
    let signature = signer
        .sign(&preimage(&DEMO_DATASET, &signed))
        .map_err(|e| DatasetError::Sign(e.to_string()))?;
    let env = SigEnvelope { domain: DEMO_DATASET.label().to_vec(), key_id: signer.key_id().to_string(), signature };
    let mut file = signed;
    put(&mut file, &env.encode());
    Ok(file)
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], DatasetError> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len());
        let end = end.ok_or_else(|| DatasetError::Malformed(format!("{what}: truncated")))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn field(&mut self, what: &str) -> Result<&'a [u8], DatasetError> {
        let n = u32::from_le_bytes(self.take(4, what)?.try_into().expect("4 bytes")) as usize;
        self.take(n, what)
    }
}

/// Verify `file` against `anchors` for `expected_release`; parse the body only after.
pub fn verify(file: &[u8], anchors: &TrustAnchors, expected_release: &str) -> Result<Dataset, DatasetError> {
    let mut c = Cursor { b: file, at: 0 };
    if c.take(MAGIC.len(), "magic")? != MAGIC {
        return Err(DatasetError::Malformed("bad magic".into()));
    }
    if c.take(1, "version")?[0] != VERSION {
        return Err(DatasetError::Malformed("unsupported version".into()));
    }
    let release = std::str::from_utf8(c.field("release")?)
        .map_err(|_| DatasetError::Malformed("release is not UTF-8".into()))?
        .to_string();
    let body_bytes = c.field("body")?.to_vec();
    let signed_len = c.at;
    let env = SigEnvelope::parse(c.field("envelope")?).map_err(|e| DatasetError::Malformed(e.to_string()))?;
    if c.at != file.len() {
        return Err(DatasetError::Malformed("trailing bytes".into()));
    }
    if env.domain != DEMO_DATASET.label() {
        return Err(DatasetError::WrongDomain);
    }
    let anchor = anchors.get(&env.key_id).ok_or_else(|| DatasetError::UnknownKey(env.key_id.clone()))?;
    accept(&DEMO_DATASET, &file[..signed_len], anchor, &env.signature, &APPROVED).map_err(DatasetError::Signature)?;
    if release != expected_release {
        return Err(DatasetError::WrongRelease { found: release, expected: expected_release.to_string() });
    }
    let text = std::str::from_utf8(&body_bytes).map_err(|_| DatasetError::Body("not UTF-8".into()))?;
    let body = parse(text).map_err(DatasetError::Body)?;
    Ok(Dataset { release, key_id: env.key_id, body_bytes, body })
}
```

`clients/core/src/demo/mod.rs` becomes:

```rust
pub mod canon;
pub mod dataset;
pub mod guard;
```

**Step 4: Run them and see them pass.**

```bash
cargo test --offline --locked -p attest the_demo_dataset_domain_is_its_own
cargo test --offline --locked -p attest --no-default-features --features backend-fips
cargo test --offline --locked -p midgard-client-core --features backend-fips --test dataset
cargo clippy --offline --locked -p attest --all-targets -- -D warnings
cargo clippy --offline --locked -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

Expected: all green (`3 passed` for `dataset`).

**Step 5: Commit.**

```bash
git add crypto/attest/src/accept.rs clients/core/src/demo clients/core/tests/dataset.rs
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: signed demo dataset format under its own attest domain [NFR-PQ-3]"
```

---

### Task A7: Dataset build step, demo signing key, committed dataset

**Files:**
- Create: `clients/core/src/demo/build.rs` (canonical body from a fixture directory)
- Create: `clients/core/examples/build_demo_dataset.rs`
- Create: `clients/demo/tools/demo-key.sh`, `clients/demo/trust/demo-dataset.pub` (by the script)
- Create (output): `clients/demo/datasets/0.1.0-rc.3.mgdemo`
- Modify: `clients/core/Cargo.toml` (`[dev-dependencies] attest` with custody), `justfile` (recipe `demo-dataset`), `.gitignore` (un-ignore the pinned anchor), `.gitattributes` (binary)
- Test: `clients/core/tests/committed_dataset.rs`

**Step 1: Write the failing test** — `clients/core/tests/committed_dataset.rs`:

```rust
//! The dataset committed for this release verifies under the pinned demo key and is
//! exactly what its committed fixtures build. [NFR-PQ-3] [AC-C-13]

use std::fs;
use std::path::PathBuf;

use attest::TrustAnchors;
use midgard_client_core::demo::{build, dataset};
use midgard_client_core::CORE_VERSION;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_committed_dataset_verifies_and_matches_its_fixtures() {
    let path = repo().join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"));
    let file = fs::read(&path)
        .unwrap_or_else(|_| panic!("no demo dataset for {CORE_VERSION}: run `just demo-dataset` (Task A7)"));
    let anchors = TrustAnchors::parse(&fs::read_to_string(repo().join("clients/demo/trust/demo-dataset.pub")).unwrap())
        .unwrap();
    let ds = dataset::verify(&file, &anchors, CORE_VERSION).unwrap();
    let body = build::canonical_body(&repo().join(format!("clients/demo/fixtures/{CORE_VERSION}"))).unwrap();
    assert_eq!(ds.body_bytes, body, "the dataset is stale: run `just demo-dataset`");
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test committed_dataset
```

Expected: `unresolved import \`midgard_client_core::demo::build\``; after Step 3a it fails with `no demo dataset for 0.1.0-rc.3: run \`just demo-dataset\``.

**Step 3a: Canonical body** — `clients/core/src/demo/build.rs`:

```rust
//! Assemble the canonical dataset body from a fixture directory. `[AC-C-13]`
//!
//! Reads `manifest.json`, checks each listed file's SHA-256, and builds
//! `{"format","version","release","inputs","personas","collections"}`; then runs the
//! local-suite guard. The result is the exact byte string the dataset signs.

use std::fs;
use std::path::Path;

use attest::json::{parse, Json};
use pqc_agility::sha256;

use super::{canon, guard};

/// Why the body could not be built.
#[derive(Debug)]
pub enum BuildError {
    /// A file is missing or unreadable.
    Io(String),
    /// A file's digest differs from the manifest.
    Digest(String),
    /// A file is not strict JSON, or lacks a required key.
    Shape(String),
    /// The local-suite guard refused it.
    Guard(Vec<guard::GuardError>),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(d) => write!(f, "{d}"),
            BuildError::Digest(d) => write!(f, "digest mismatch: {d}"),
            BuildError::Shape(d) => write!(f, "fixture shape: {d}"),
            BuildError::Guard(found) => write!(f, "local-suite data refused: {found:?}"),
        }
    }
}

impl std::error::Error for BuildError {}

fn read_json(dir: &Path, name: &str) -> Result<(Vec<u8>, Json), BuildError> {
    let bytes = fs::read(dir.join(name)).map_err(|e| BuildError::Io(format!("{name}: {e}")))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| BuildError::Shape(format!("{name}: not UTF-8")))?;
    let v = parse(text).map_err(|e| BuildError::Shape(format!("{name}: {e}")))?;
    Ok((bytes, v))
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// The canonical dataset body for the fixtures in `dir`.
pub fn canonical_body(dir: &Path) -> Result<Vec<u8>, BuildError> {
    let (_, manifest) = read_json(dir, "manifest.json")?;
    let files = match manifest.get("files") {
        Some(Json::Obj(pairs)) => pairs.clone(),
        _ => return Err(BuildError::Shape("manifest.json: no files".into())),
    };
    let mut collections = Vec::new();
    let mut personas = Json::Null;
    for (name, digest) in &files {
        let (bytes, v) = read_json(dir, name)?;
        if digest.as_str() != Some(hex(&sha256(&bytes)).as_str()) {
            return Err(BuildError::Digest(name.clone()));
        }
        if name == "personas.json" {
            personas = v;
            continue;
        }
        match v.get("collections") {
            Some(Json::Obj(pairs)) => collections.extend(pairs.iter().cloned()),
            _ => return Err(BuildError::Shape(format!("{name}: no collections"))),
        }
    }
    let pick = |k: &str| manifest.get(k).cloned().ok_or_else(|| BuildError::Shape(format!("manifest: no {k}")));
    let body = Json::Obj(vec![
        ("format".into(), Json::Str("midgard-demo-dataset".into())),
        ("version".into(), Json::Num("1".into())),
        ("release".into(), pick("release")?),
        ("inputs".into(), pick("inputs")?),
        ("personas".into(), personas),
        ("collections".into(), Json::Obj(collections)),
    ]);
    guard::check(&body).map_err(BuildError::Guard)?;
    Ok(canon::encode(&body).into_bytes())
}
```

Add `pub mod build;` to `clients/core/src/demo/mod.rs`. (`Json::get` exists in `attest::json`; if `Json` does not derive `Clone` for `Obj` pairs, it does: `#[derive(Debug, Clone, PartialEq, Eq)]` at `crypto/attest/src/json.rs:23`.)

**Step 3b: The builder example.** In `clients/core/Cargo.toml`, `[dev-dependencies]`:

```toml
# The dataset builder example signs through an attest keystore (custody); the library
# itself only verifies. Examples are not release binaries (tools/ci/release-artefacts.sh
# derives the release matrix from [[bin]], src/main.rs and src/bin/ only). [NFR-PQ-3]
attest = { path = "../../crypto/attest" }
```

`clients/core/examples/build_demo_dataset.rs`:

```rust
//! Build and sign the demo dataset for a release. [AC-C-13] [NFR-PQ-3]
//!
//!   cargo run --offline --locked -p midgard-client-core --features backend-fips \
//!     --example build_demo_dataset -- --fixtures <dir> --tag <tag> --keystore <path> \
//!     --keystore-passphrase-file <path> --anchors <pub> --out <file>
//!
//! Refuses unless every mock input is byte-identical to the tagged release's copy,
//! every other input is tracked and clean, the guard passes, and the signed file
//! verifies under the pinned anchor before it is written.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use attest::{Keystore, KmsSigner, RootWrap, TrustAnchors};
use midgard_client_core::demo::{build, dataset};

fn arg(args: &[String], name: &str) -> Result<String, String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().ok_or(format!("{name} is required"))
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn check_inputs(fixtures: &Path, tag: &str) -> Result<(), String> {
    let manifest = std::fs::read_to_string(fixtures.join("manifest.json")).map_err(|e| e.to_string())?;
    let v = attest::json::parse(&manifest)?;
    let Some(attest::json::Json::Obj(inputs)) = v.get("inputs") else { return Err("manifest has no inputs".into()) };
    for (path, blob) in inputs {
        let blob = blob.as_str().ok_or("blob is not a string")?;
        let tagged = path.starts_with("sdk/ts/src/mock/");
        if !tagged && !path.starts_with("clients/demo/") {
            return Err(format!("input {path} is outside sdk/ts/src/mock/ and clients/demo/"));
        }
        let want = if tagged { git(&["rev-parse", &format!("{tag}:{path}")])? } else { git(&["hash-object", path])? };
        if want != blob {
            return Err(format!("input {path} is not the {} copy", if tagged { tag } else { "committed" }));
        }
        if !tagged && !git(&["status", "--porcelain", "--", path])?.is_empty() {
            return Err(format!("input {path} has uncommitted changes"));
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fixtures = PathBuf::from(arg(&args, "--fixtures")?);
    let tag = arg(&args, "--tag")?;
    let out = PathBuf::from(arg(&args, "--out")?);
    let release = tag.strip_prefix('v').ok_or("--tag must look like v<release>")?.to_string();

    check_inputs(&fixtures, &tag)?;
    let body = build::canonical_body(&fixtures).map_err(|e| e.to_string())?;

    let pass = RootWrap::passphrase_from_file(Path::new(&arg(&args, "--keystore-passphrase-file")?))
        .map_err(|e| e.to_string())?;
    let ks = Keystore::read(Path::new(&arg(&args, "--keystore")?), &RootWrap::Passphrase(&pass))
        .map_err(|e| e.to_string())?;
    let signer = KmsSigner::new(ks.into_signing_root().map_err(|e| e.to_string())?);
    let file = dataset::sign(&release, &body, &signer).map_err(|e| e.to_string())?;

    let anchors = TrustAnchors::parse(&std::fs::read_to_string(arg(&args, "--anchors")?).map_err(|e| e.to_string())?)?;
    dataset::verify(&file, &anchors, &release).map_err(|e| format!("self-check failed: {e}"))?;
    std::fs::create_dir_all(out.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    std::fs::write(&out, &file).map_err(|e| e.to_string())?;
    println!("build_demo_dataset: {} ({} bytes) signed and verified", out.display(), file.len());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("build_demo_dataset: {e}");
            ExitCode::FAILURE
        }
    }
}
```

(`Keystore::into_signing_root` is at `crypto/attest/src/keystore.rs:695`; `KmsSigner` implements `SigningBackend` for any `kms::SigningRoot`, and `kms::SoftwareSigningRoot` is one.)

**Step 3c: The demo key.** `clients/demo/tools/demo-key.sh`:

```bash
#!/usr/bin/env bash
# Create the demo-dataset signing key on this machine and pin its public half in
# clients/demo/trust/demo-dataset.pub. Never prints key material. [NFR-PQ-3]
#
#   MIDGARD_DEMO_KEY_DIR=<dir> bash clients/demo/tools/demo-key.sh
set -euo pipefail
cd "$(dirname "$0")/../../.."
dir="${MIDGARD_DEMO_KEY_DIR:-$HOME/.midgard/demo-keys}"
umask 077
mkdir -p "$dir" && chmod 700 "$dir"
if [ ! -s "$dir/passphrase" ]; then
    echo "demo-key: put a passphrase of at least 16 bytes in $dir/passphrase (mode 0600) first, e.g." >&2
    echo "  ( umask 077; openssl rand -base64 32 > \"$dir/passphrase\" )" >&2
    exit 2
fi
if [ -e "$dir/demo-dataset.keystore" ]; then
    echo "demo-key: $dir/demo-dataset.keystore exists; refusing to replace a signing key" >&2
    exit 2
fi
mkdir -p clients/demo/trust
cargo run --offline --locked -q -p mg-sign -- keygen \
    --keystore "$dir/demo-dataset.keystore" \
    --key-id "midgard-demo-dataset-$(date +%Y)" --alg ML-DSA-65 \
    --keystore-passphrase-file "$dir/passphrase" \
    --export-verifying-key clients/demo/trust/demo-dataset.pub
echo "demo-key: pinned $(cut -d' ' -f1,2 clients/demo/trust/demo-dataset.pub)"
```

In `.gitignore`, after `!/deploy/trust/model-signing.pub`, add:

```gitignore
# The demo-dataset anchor: a public key, trusted only for the demo dataset domain.
!/clients/demo/trust/demo-dataset.pub
```

In `.gitattributes` (create it if absent): `clients/demo/datasets/*.mgdemo binary`.

In `justfile`, after the `verify-heimdall` recipe:

```just
# Build and sign the demo dataset for this release (Apple clients; Task A7 of
# docs/plans/2026-11-13-apple-clients-plan.md). Needs the demo key from
# clients/demo/tools/demo-key.sh. [AC-C-13] [NFR-PQ-3]
demo-dataset:
    #!/usr/bin/env bash
    set -euo pipefail
    release="$(bash tools/ci/release-artefacts.sh --version)"
    dir="${MIDGARD_DEMO_KEY_DIR:-$HOME/.midgard/demo-keys}"
    bash clients/demo/tools/extract-fixtures.sh --check
    cargo run --offline --locked -q -p midgard-client-core --features backend-fips --example build_demo_dataset -- \
        --fixtures "clients/demo/fixtures/$release" --tag "v$release" \
        --keystore "$dir/demo-dataset.keystore" --keystore-passphrase-file "$dir/passphrase" \
        --anchors clients/demo/trust/demo-dataset.pub --out "clients/demo/datasets/$release.mgdemo"
```

**Step 3d: Make the key and the dataset.** The key directory location is an open question for the owner (the default below is outside the repository and outside every backup the suite makes); confirm before running. Do not print or copy the passphrase.

Both demo keys (`demo-key.sh dataset` and, from A8, `demo-key.sh audit`) are unlocked by the one passphrase file in the key directory. That is acceptable for demo keys only (it is no separation between the two); a real signing key never shares a passphrase.

```bash
( umask 077; mkdir -p ~/.midgard/demo-keys; openssl rand -base64 32 > ~/.midgard/demo-keys/passphrase )
bash clients/demo/tools/demo-key.sh          # expect: "demo-key: pinned ML-DSA-65 midgard-demo-dataset-2026"
just demo-dataset                            # or the cargo command inside the recipe, if `just` is absent
```

Expected: `build_demo_dataset: clients/demo/datasets/0.1.0-rc.3.mgdemo (<n> bytes) signed and verified`. If it reports `input sdk/ts/src/mock/… is not the v0.1.0-rc.3 copy`, a mock changed after the tag; the dataset must be built from the tagged mocks.

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test committed_dataset
cargo test --offline --locked -p midgard-client-core --features backend-fips
bash tools/ci/release-artefacts.sh --check     # still "18 binaries": examples are not release binaries
git check-ignore -v clients/demo/trust/demo-dataset.pub || echo "anchor is tracked"
```

**Step 5: Commit.**

```bash
git add clients/core/src/demo/build.rs clients/core/src/demo/mod.rs clients/core/examples/build_demo_dataset.rs \
        clients/core/Cargo.toml clients/core/tests/committed_dataset.rs clients/demo/tools/demo-key.sh \
        clients/demo/trust/demo-dataset.pub clients/demo/datasets/0.1.0-rc.3.mgdemo \
        .gitignore .gitattributes justfile Cargo.lock
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/demo: signed demo dataset for 0.1.0-rc.3 and its build step [AC-C-13] [NFR-PQ-3]"
```

---

### Task A8: Data-source trait and the demo enclosure

At the tag, `clients/enclosure` (`EnclosureClient::connect`) is a PQ-mTLS **transport**; nothing defines what a client asks the enclosure for. This task defines that seam, `source::DataSource`, in the terms the consoles already use — personas (policy contexts), collections of marked rows, and an exchange of opaque CRDT op blobs (the `drain_pending_encoded`/`merge_remote_encoded` surface `clients/shells/SHELL_BUILD.md` describes) — and implements it with `demo::DemoEnclosure` over the verified dataset. A gateway client later implements the same trait over `EnclosureClient`; the session and the apps do not change.

**Clearance is applied at the source, before anything reaches the client:** a row the grant does not dominate is delivered as `Delivered::Withheld { level }` with no id and no body, mirroring "marking enforcement remains authoritative in the enclosure" in `clients/store/src/lib.rs`. The store's own refusal (`StoreError::AboveClearance`) stays as the client-side floor.

**Actions travel as CRDT edits.** An action is an object `action:<replica>-<n>` whose fields `verb`, `target`, `params` (canonical JSON), `by` are set with `ClientCore::edit_field`, so it queues offline like any edit. On exchange the enclosure merges the ops, runs the handler registered for `verb` once (an action that already has a `status` is skipped, so a re-push is idempotent), appends an audit row, and answers with ops that set `status` (`applied` | `refused` | `conflict`), `reason`, `audit` and `result`. The client merges them. Phase A registers one handler, `demo.ping`; each console phase registers its own.

**Review amendments (16 Nov).** Decisions are kept per `(persona, action id)` and a re-push answers with the stored ops again; another persona reusing an action id gets nothing of the first persona's result (it no longer gets `conflict`: see the re-review amendment below, where it makes its own action). The audit row names the signed-in persona (a different claimed `by` goes into the action text) and goes to the registered handler's console (unknown verbs to `midgard.audit`). An audit row for an action whose target the actor may not see — hidden or missing alike — is marked at the actor's own level (at least `restricted`) with no compartment. Unchecked signature claims outside audit rows are delivered as "not checked (demo)". `open`/`from_body` are behind the non-default `demo-unpinned` feature; production code opens with `open_pinned`.

**Re-review amendments (16 Nov).** The enclosure keeps no global owner per action id: decisions are keyed by `(persona, action id)` and each persona has its own replica, so another persona reusing an id makes its own action (applied, refused or in conflict on its own merits) and learns nothing about the first, not even that the id was used. `params` longer than 4 KiB (`MAX_ACTION_PARAMS`) is refused as malformed. Mimir's `feedback.prov = "verified"` is a provenance label, not a signature; the threat model lists it, and shells render it with a demo qualifier, never as a check mark.

**Files:**
- Create: `clients/core/src/source.rs`, `clients/core/src/demo/enclosure.rs`
- Modify: `clients/core/src/lib.rs` (`pub mod source;`), `clients/core/src/demo/mod.rs` (`pub mod enclosure;`)
- Test: `clients/core/tests/enclosure.rs`

**Step 1: Write the failing test** — `clients/core/tests/enclosure.rs`:

```rust
//! The demo enclosure serves rows filtered by the grant, refuses everything while
//! offline, and runs each queued action exactly once. [AC-C-13] [FR-C-4] [FR-C-5]

use std::fs;
use std::path::PathBuf;

use attest::json::parse;
use attest::TrustAnchors;
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::marking::MarkingLevel;
use midgard_client_core::source::{action, DataSource, Delivered, SourceError};
use midgard_client_core::sync::{Replica, SyncOp};
use midgard_client_core::CORE_VERSION;

const BODY: &str = r#"{"collections":{
  "midgard.audit":{"type":"AuditRecord","rows":[
    {"id":"AUD-1","marking":{"level":"internal","compartments":[]},"item":{"id":"AUD-1","actor":"k.rao","action":"Query","resource":"r","ts":"14:00:00","pqSignature":"ML-DSA"}},
    {"id":"AUD-2","marking":{"level":"confidential","compartments":["OPS-N"]},"item":{"id":"AUD-2","actor":"x","action":"y","resource":"z","ts":"14:00:01","pqSignature":"ML-DSA"}}]}},
 "personas":[
  {"id":"k.rao","display":"K. Rao","role":"analyst","clearance":"restricted","compartments":[],"roles":["analyst"]},
  {"id":"k.osei","display":"K. Osei","role":"approver","clearance":"confidential","compartments":["OPS-N","SIG"],"roles":["approver"]}],
 "format":"midgard-demo-dataset","version":1,"release":"test","inputs":{}}"#;

fn enclosure() -> DemoEnclosure {
    DemoEnclosure::from_body("test", &parse(BODY).unwrap()).unwrap()
}

fn ping(client: &mut Replica, id: &str, by: &str) -> Vec<Vec<u8>> {
    for (f, v) in [(action::VERB, "demo.ping"), (action::TARGET, "midgard.audit/AUD-1"), (action::PARAMS, "{}"), (action::BY, by)] {
        client.set(id, f, v.as_bytes().to_vec());
    }
    client.drain_pending().iter().map(SyncOp::encode).collect()
}

#[test]
fn rows_above_the_grant_are_withheld_at_the_source() {
    let e = enclosure();
    let analyst = e.grant("k.rao").unwrap();
    let rows = e.fetch(&analyst, "midgard.audit").unwrap();
    assert!(matches!(&rows[0], Delivered::Row { id, .. } if id == "AUD-1"));
    assert_eq!(rows[1], Delivered::Withheld { level: MarkingLevel::Confidential });
    let approver = e.grant("k.osei").unwrap();
    assert!(matches!(&e.fetch(&approver, "midgard.audit").unwrap()[1], Delivered::Row { .. }));
    assert_eq!(e.grant("nobody").unwrap_err(), SourceError::UnknownPersona("nobody".into()));
}

#[test]
fn offline_refuses_fetch_and_exchange() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    e.set_online(false);
    assert_eq!(e.fetch(&g, "midgard.audit").unwrap_err(), SourceError::Offline);
    assert_eq!(e.exchange(&g, &[]).unwrap_err(), SourceError::Offline);
}

#[test]
fn an_action_runs_once_and_is_audited() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let ops = ping(&mut client, "action:dev-1-1", "k.rao");
    let back = e.exchange(&g, &ops).unwrap();
    let back: Vec<SyncOp> = back.iter().map(|b| SyncOp::decode(b).unwrap()).collect();
    client.merge_all(back.iter());
    assert_eq!(client.get("action:dev-1-1", action::STATUS), Some(&b"applied"[..]));
    assert!(client.get("action:dev-1-1", action::AUDIT).is_some());
    // The same ops again: decided already. The stored decision comes back (same
    // versions, so a client that lost the reply gets it; merging it twice changes
    // nothing) and nothing runs twice. (Amended at review: the first draft required
    // an empty answer, which left a client that lost the reply without its status.)
    assert_eq!(e.exchange(&g, &ops).unwrap(), back);
    let audited = e.fetch(&e.grant("k.osei").unwrap(), "midgard.audit").unwrap().len();
    assert_eq!(audited, 3);
}

#[test]
fn an_action_for_another_persona_or_an_unknown_verb_is_refused() {
    let mut e = enclosure();
    let g = e.grant("k.rao").unwrap();
    let mut client = Replica::new("dev-1");
    let back = e.exchange(&g, &ping(&mut client, "action:dev-1-1", "k.osei")).unwrap();
    client.merge_all(back.iter().map(|b| SyncOp::decode(b).unwrap()).collect::<Vec<_>>().iter());
    assert_eq!(client.get("action:dev-1-1", action::STATUS), Some(&b"refused"[..]));
}

#[test]
fn the_committed_dataset_opens() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let file = fs::read(repo.join(format!("clients/demo/datasets/{CORE_VERSION}.mgdemo"))).unwrap();
    let anchors = TrustAnchors::parse(&fs::read_to_string(repo.join("clients/demo/trust/demo-dataset.pub")).unwrap()).unwrap();
    let e = DemoEnclosure::open(&file, &anchors, CORE_VERSION).unwrap();
    assert_eq!(e.personas().len(), 4);
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test enclosure
```

Expected: `unresolved imports \`midgard_client_core::demo::enclosure\`, \`midgard_client_core::source\``.

**Step 3: Implement.** `clients/core/src/source.rs`:

```rust
//! The data-source seam between a session and whatever serves it. `[FR-C-5]`
//! `[AC-C-13]` `[FR-C-4]`
//!
//! The demo enclosure (`crate::demo::enclosure`) implements it over the signed
//! dataset; a gateway client over `midgard_client_enclosure::EnclosureClient`
//! implements it later. A source applies clearance itself: above-clearance rows are
//! delivered withheld, without id or body.

use std::fmt;

use policy::Clearance;

use crate::marking::{MarkingLevel, RowMarking};

/// Field names of an action object (`action:<replica>-<n>`). `[FR-C-4]`
pub mod action {
    /// Object id prefix.
    pub const PREFIX: &str = "action:";
    /// `<console>.<verb>`, e.g. `admin.approve`.
    pub const VERB: &str = "verb";
    /// `<collection>/<row id>` the action is on.
    pub const TARGET: &str = "target";
    /// Canonical JSON object of string parameters.
    pub const PARAMS: &str = "params";
    /// The persona id that made it.
    pub const BY: &str = "by";
    /// Set by the source: `applied`, `refused` or `conflict`.
    pub const STATUS: &str = "status";
    /// Set by the source: why it was refused or in conflict.
    pub const REASON: &str = "reason";
    /// Set by the source: the audit record id.
    pub const AUDIT: &str = "audit";
    /// Set by the source: canonical JSON result.
    pub const RESULT: &str = "result";
}

/// A demonstration identity: who signs in, at what clearance.
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    /// Stable id, e.g. `k.rao`.
    pub id: String,
    /// Display name.
    pub display: String,
    /// `analyst`, `operator`, `approver` or `release-manager`.
    pub role: String,
    /// Clearance level.
    pub clearance: MarkingLevel,
    /// Held compartments.
    pub compartments: Vec<String>,
    /// Policy roles.
    pub roles: Vec<String>,
}

/// What a source grants a signed-in persona.
#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    /// The persona.
    pub persona: Persona,
    /// The clearance on the policy lattice.
    pub clearance: Clearance,
}

/// One row as a source delivers it.
#[derive(Debug, Clone, PartialEq)]
pub enum Delivered {
    /// A row the grant dominates: canonical JSON body.
    Row {
        /// Row id within the collection.
        id: String,
        /// Its marking.
        marking: RowMarking,
        /// Canonical JSON of the row.
        body: Vec<u8>,
    },
    /// A row above the grant: its level only.
    Withheld {
        /// The level withheld.
        level: MarkingLevel,
    },
}

/// Why a source refused.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceError {
    /// The source is unreachable (or the demo is simulating that).
    Offline,
    /// No such persona.
    UnknownPersona(String),
    /// No such collection.
    UnknownCollection(String),
    /// A pushed op blob did not decode.
    Malformed(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Offline => write!(f, "offline"),
            SourceError::UnknownPersona(p) => write!(f, "unknown persona {p}"),
            SourceError::UnknownCollection(c) => write!(f, "unknown collection {c}"),
            SourceError::Malformed(d) => write!(f, "malformed op: {d}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// A source of console data for a session.
pub trait DataSource {
    /// The personas that may sign in.
    fn personas(&self) -> Vec<Persona>;
    /// Sign `persona_id` in: the clearance and roles it is granted.
    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError>;
    /// The rows of `collection`, clearance-filtered for `grant`.
    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError>;
    /// Push opaque op blobs; receive the source's ops to merge. Idempotent.
    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError>;
    /// Whether the source is reachable.
    fn online(&self) -> bool;
    /// Simulate (demo) or record (gateway) reachability.
    fn set_online(&mut self, online: bool);
}
```

`clients/core/src/demo/enclosure.rs`:

```rust
//! The demo enclosure: the signed dataset behind [`DataSource`]. `[AC-C-13]` `[FR-C-4]`
//!
//! State lives in memory and starts from the dataset each launch; actions mutate it
//! through registered handlers, each run once per action object, each audited.

use std::collections::BTreeMap;

use attest::json::{parse, Json};
use attest::TrustAnchors;

use crate::demo::{canon, dataset};
use crate::marking::{clearance, may_see, MarkingLevel, RowMarking};
use crate::source::{action, DataSource, Delivered, Grant, Persona, SourceError};
use crate::sync::{Replica, SyncOp};

/// A stored row.
#[derive(Debug, Clone)]
pub struct StoredRow {
    /// Row id.
    pub id: String,
    /// Marking.
    pub marking: RowMarking,
    /// The row as JSON.
    pub item: Json,
}

/// A named collection of rows of one read-model type.
#[derive(Debug, Clone)]
pub struct Collection {
    /// The Rust read-model name, e.g. `AdminApproval`.
    pub type_name: String,
    /// The rows, in dataset order.
    pub rows: Vec<StoredRow>,
}

/// The mutable demo state handlers work on.
#[derive(Debug, Default)]
pub struct DemoState {
    /// Collections by name.
    pub collections: BTreeMap<String, Collection>,
    audit_seq: u64,
}

impl DemoState {
    /// Append an audit row to `<console>.audit` (or `midgard.audit` when the console
    /// keeps no `AuditRecord` collection) and return its id.
    pub fn audit(&mut self, console: &str, actor: &str, what: &str, resource: &str) -> String {
        self.audit_seq += 1;
        let id = format!("AUD-D{:04}", self.audit_seq);
        let secs = 15 * 3600 + self.audit_seq;
        let ts = format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60);
        let name = format!("{console}.audit");
        let key = if self.collections.contains_key(&name) { name } else { "midgard.audit".to_string() };
        let item = Json::Obj(vec![
            ("id".into(), Json::Str(id.clone())),
            ("actor".into(), Json::Str(actor.into())),
            ("action".into(), Json::Str(what.into())),
            ("resource".into(), Json::Str(resource.into())),
            ("ts".into(), Json::Str(ts)),
            ("pqSignature".into(), Json::Str("ML-DSA ✓".into())),
        ]);
        let coll = self
            .collections
            .entry(key)
            .or_insert_with(|| Collection { type_name: "AuditRecord".into(), rows: Vec::new() });
        coll.rows.insert(0, StoredRow { id: id.clone(), marking: RowMarking::new(MarkingLevel::Restricted, Vec::<String>::new()), item });
        id
    }
}

/// A parsed action.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    /// The action object id.
    pub id: String,
    /// `<console>.<verb>`.
    pub verb: String,
    /// `<collection>/<row id>`.
    pub target: String,
    /// String parameters.
    pub params: Vec<(String, String)>,
    /// The persona that made it.
    pub by: String,
}

/// What a handler decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Done; the result is returned to the client.
    Applied(Json),
    /// Refused, with a reason the screen shows.
    Refused(String),
    /// The target changed under the action (decided twice, stale version).
    Conflict(String),
}

/// An action handler.
pub type Handler = fn(&mut DemoState, &Grant, &ActionRequest) -> Outcome;

/// The demo enclosure.
pub struct DemoEnclosure {
    release: String,
    personas: Vec<Persona>,
    state: DemoState,
    replica: Replica,
    online: bool,
    handlers: BTreeMap<&'static str, Handler>,
}

fn s(v: Option<&Json>) -> Option<String> {
    v.and_then(Json::as_str).map(str::to_string)
}

fn strings(v: Option<&Json>) -> Vec<String> {
    v.and_then(Json::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn marking_of(v: Option<&Json>) -> Option<RowMarking> {
    let v = v?;
    Some(RowMarking::new(MarkingLevel::parse(v.get("level")?.as_str()?)?, strings(v.get("compartments"))))
}

impl DemoEnclosure {
    /// Verify `file` against the pinned `anchors` for `release`, then load it.
    pub fn open(file: &[u8], anchors: &TrustAnchors, release: &str) -> Result<Self, dataset::DatasetError> {
        let ds = dataset::verify(file, anchors, release)?;
        Self::from_body(&ds.release, &ds.body).map_err(|e| dataset::DatasetError::Body(e.to_string()))
    }

    /// Load an already verified body. Tests use this with an unsigned body.
    pub fn from_body(release: &str, body: &Json) -> Result<Self, SourceError> {
        let bad = |what: &str| SourceError::Malformed(format!("dataset body: {what}"));
        let personas = body
            .get("personas")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("personas"))?
            .iter()
            .map(|p| {
                Some(Persona {
                    id: s(p.get("id"))?,
                    display: s(p.get("display"))?,
                    role: s(p.get("role"))?,
                    clearance: MarkingLevel::parse(p.get("clearance")?.as_str()?)?,
                    compartments: strings(p.get("compartments")),
                    roles: strings(p.get("roles")),
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| bad("a persona"))?;
        let mut state = DemoState::default();
        let Some(Json::Obj(colls)) = body.get("collections") else { return Err(bad("collections")) };
        for (name, c) in colls {
            let rows = c
                .get("rows")
                .and_then(Json::as_array)
                .ok_or_else(|| bad(name))?
                .iter()
                .map(|r| Some(StoredRow { id: s(r.get("id"))?, marking: marking_of(r.get("marking"))?, item: r.get("item")?.clone() }))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad(name))?;
            state.collections.insert(name.clone(), Collection { type_name: s(c.get("type")).unwrap_or_default(), rows });
        }
        let mut e = Self { release: release.to_string(), personas, state, replica: Replica::new("enclosure"), online: true, handlers: BTreeMap::new() };
        e.register("demo.ping", |_, _, req| Outcome::Applied(Json::Obj(vec![("pong".into(), Json::Str(req.target.clone()))])));
        Ok(e)
    }

    /// Register (or replace) the handler for `verb`.
    pub fn register(&mut self, verb: &'static str, handler: Handler) {
        self.handlers.insert(verb, handler);
    }

    /// The release the dataset is for.
    pub fn release(&self) -> &str {
        &self.release
    }

    fn request(&self, id: &str) -> Option<ActionRequest> {
        let field = |f: &str| self.replica.get(id, f).and_then(|b| std::str::from_utf8(b).ok()).map(str::to_string);
        let params = match parse(&field(action::PARAMS)?).ok()? {
            Json::Obj(pairs) => pairs.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect(),
            _ => return None,
        };
        Some(ActionRequest { id: id.to_string(), verb: field(action::VERB)?, target: field(action::TARGET)?, params, by: field(action::BY)? })
    }
}

impl DataSource for DemoEnclosure {
    fn personas(&self) -> Vec<Persona> {
        self.personas.clone()
    }

    fn grant(&self, persona_id: &str) -> Result<Grant, SourceError> {
        let p = self.personas.iter().find(|p| p.id == persona_id).ok_or_else(|| SourceError::UnknownPersona(persona_id.into()))?;
        Ok(Grant { clearance: clearance(p.clearance, &p.compartments), persona: p.clone() })
    }

    fn fetch(&self, grant: &Grant, collection: &str) -> Result<Vec<Delivered>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        let c = self.state.collections.get(collection).ok_or_else(|| SourceError::UnknownCollection(collection.into()))?;
        Ok(c.rows
            .iter()
            .map(|r| {
                if may_see(&grant.clearance, &r.marking) {
                    Delivered::Row { id: r.id.clone(), marking: r.marking.clone(), body: canon::encode(&r.item).into_bytes() }
                } else {
                    Delivered::Withheld { level: r.marking.level }
                }
            })
            .collect())
    }

    fn exchange(&mut self, grant: &Grant, ops: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, SourceError> {
        if !self.online {
            return Err(SourceError::Offline);
        }
        let decoded: Vec<SyncOp> =
            ops.iter().map(|b| SyncOp::decode(b)).collect::<Result<_, _>>().map_err(|e| SourceError::Malformed(e.to_string()))?;
        self.replica.merge_all(decoded.iter());
        let mut ids: Vec<&str> = decoded.iter().map(|o| o.object.as_str()).filter(|o| o.starts_with(action::PREFIX)).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            if self.replica.get(id, action::STATUS).is_some() {
                continue; // decided already: a re-push changes nothing
            }
            let Some(req) = self.request(id) else { continue }; // incomplete: wait for its other fields
            let outcome = if req.by != grant.persona.id {
                Outcome::Refused("an action is made only by the persona signed in".into())
            } else {
                match self.handlers.get(req.verb.as_str()) {
                    Some(h) => h(&mut self.state, grant, &req),
                    None => Outcome::Refused(format!("no such action: {}", req.verb)),
                }
            };
            let (status, reason, result) = match outcome {
                Outcome::Applied(r) => ("applied", String::new(), canon::encode(&r)),
                Outcome::Refused(why) => ("refused", why, "{}".into()),
                Outcome::Conflict(why) => ("conflict", why, "{}".into()),
            };
            let console = req.verb.split('.').next().unwrap_or("midgard").to_string();
            let audit = self.state.audit(&console, &req.by, &format!("{} · {}", status, req.verb), &req.target);
            self.replica.set(id, action::STATUS, status.as_bytes().to_vec());
            self.replica.set(id, action::REASON, reason.into_bytes());
            self.replica.set(id, action::AUDIT, audit.into_bytes());
            self.replica.set(id, action::RESULT, result.into_bytes());
        }
        Ok(self.replica.drain_pending().iter().map(SyncOp::encode).collect())
    }

    fn online(&self) -> bool {
        self.online
    }

    fn set_online(&mut self, online: bool) {
        self.online = online;
    }
}
```

(`Json::as_array` is at `crypto/attest/src/json.rs:65`; `ClientCore`'s sync re-export is `midgard_client_core::sync`.)

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test enclosure
cargo clippy --offline --locked -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

Expected: `5 passed`.

**Step 5: Commit.**

```bash
git add clients/core/src/source.rs clients/core/src/demo clients/core/src/lib.rs clients/core/tests/enclosure.rs
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: data-source seam and the demo enclosure over the signed dataset [FR-C-5] [AC-C-13]"
```

---

### Task A9: Sealed-root keystore, cache persistence, pending-op requeue

**Why a new keystore.** The core's custody root is an ML-KEM-1024 `kms::TrustRoot`: `unwrap` must decapsulate (`services/kms/src/lib.rs:47`). The Secure Enclave holds only P-256 keys; it cannot hold or decapsulate ML-KEM. So the root is generated in Rust, and at rest its secret is AES-256-GCM-encrypted under a key-encryption key (KEK) derived (HKDF-SHA-256) from an ECDH agreement that the Enclave key performs, gated by Face ID / Touch ID with the passcode as fallback. Swift implements the agreement callback only and never sees the root secret; Rust never sees the Enclave key. This is how "a Secure Enclave key wraps the core's data key" is realised; the design's "seal and unseal inside the Enclave" is not possible literally (the Enclave does not encrypt arbitrary data), and the plan says so to the owner (Risks).

**Floor.** While unlocked, the ML-KEM secret is in mlocked process memory (`pqc_agility::SecretBytes`), as `SoftwareKeystore`'s is, so `SealedRootKeystore` reports `SecurityFloor::NonExtractable` — not `HardwareBacked` — in line with "the floor is honest" (`clients/keystore/src/lib.rs`, `[VLD-ANB-MIDGARD-0056]`). It also reports `hardware_wrapped()` so the custody chip can say "Secure Enclave–wrapped". Whether to add a floor between `NonExtractable` and `HardwareBacked` is an open question.

**Persistence.** At the tag the store and the CRDT queue are memory-only (the `client-core` claims row says so: "It keeps its state in memory"). The design needs the sealed cache and the offline queue to survive a relaunch, and "Reset demo" to wipe them. `LocalStore` gains `export_sealed`/`import_sealed` (the whole file is itself sealed, so ids and markings are not readable at rest either; import drops rows the current clearance does not dominate, and refuses a file sealed to another root). `Replica` gains `requeue` (restores queued ops with their original versions, so a re-push is idempotent).

**As built (16 Nov).** Four changes from the steps below; the steps are kept as written for the record.

1. *Key-root interface.* `RootSealer` has three methods: `seal(&SecretBytes) -> Vec<u8>` and `unseal(&[u8]) -> SecretBytes` of a 32-byte data key, and `hardware_backed()`. The data key is the only thing that crosses to the platform side; the ML-KEM secret is AES-256-GCM-sealed under a key derived (HKDF-SHA-256) from it, and the blob carries the platform's sealed data key where the plan had an ECDH ephemeral key. On Apple the Enclave key seals with `SecKeyCreateEncryptedData` (`.eciesEncryptionCofactorVariableIVX963SHA256AESGCM`, public key, no prompt) and unseals with `SecKeyCreateDecryptedData` (private key, user presence), so the plan's `ensure_key`/`agree_new`/`agree_existing` and its shared-secret crossing are gone. A platform side that returns the key unsealed is refused. The floor stays `NonExtractable` (owner decision 5), with the reason in the `floor()` doc comment.
2. *Per-persona cache.* `LocalStore` and `ClientCore` carry a scope (the persona id): the state file's seal is bound to it, so one persona's file does not open for another, and `rescope(scope, clearance)` to another persona drops every cached row and the CRDT view (`Replica::forget`), whatever the new clearance. A switch up or sideways therefore keeps nothing (the plan kept dominated rows). `ClientCore::rescope` refuses a change of persona while ops are pending.
3. *All or nothing.* `import_sealed` replaces the cache and leaves the store unchanged on any error (root, scope, flipped byte, truncation, format version, duplicate id); `ClientCore::import_state` leaves the cache empty and the queue unchanged on any error.
4. *Pulled forward from A10.* `ClientCore::{open_scoped, scope, replica_id, rescope, export_state, import_state, cache_delivery, cached_delivery}` (`clients/core/src/cache.rs`), so the at-rest, withheld-row, persona-switch and requeue requirements are tested here (`clients/core/tests/cache.rs`). A withheld row is cached as its level in the collection index and nothing else. A10's `Session` uses these instead of its own index code.

5. *Review round (16 Nov).* The state file also carries the Lamport clock and the action counter (`\0sys/clock`, at the lowest marking so a lower clearance still restores them), and `ClientCore::new_action_id` makes ids `action:<replica>-<nonce>-<n>` with a random 64-bit nonce per launch (the persisted counter alone would repeat after a discarded file, and a source answers a reused id with its old decision). The sealed contents carry a generation, one higher on each export; `import_state(file, min_generation)` refuses an older file as `CoreError::Stale` and always raises the next generation above `min_generation`. `ImportReport::pending_dropped` says when the pending queue was in the file but the current clearance cannot read it. Contents are zero-padded to a power-of-two bucket of at least 64 KiB. The core's own objects start with `\0`; `cache_delivery` refuses a collection name that is empty or holds `\0`, `/` or `#`, and `edit_field` refuses a `\0` object. Lowering the same persona's clearance also drops the CRDT view. There is no unscoped `LocalStore::open`/`ClientCore::open`: production and tests use `open_scoped`. The platform side's sealed key must be at least 48 bytes (key + tag), a guard only.

No device signing key is made here (the sealed root is an encryption root): run-time audit rows stay `none` until A10 adds a device-audit key under its own domain and sets `SignatureStatus::Device` only after verifying.

**Files:**
- Create: `clients/keystore/src/sealed_root.rs`; modify `clients/keystore/src/lib.rs`, `clients/keystore/src/error.rs`
- Modify: `clients/store/src/lib.rs`, `clients/store/src/error.rs`
- Modify: `clients/sync/src/lib.rs`
- Test: `clients/keystore/tests/sealed_root.rs`, `clients/store/tests/persist.rs`, a unit test in `clients/sync/src/lib.rs`

**Step 1: Write the failing tests.** `clients/keystore/tests/sealed_root.rs`:

```rust
//! A sealed root reopens only through the platform key that sealed it, and is the same
//! ML-KEM root afterwards. [FR-C-2] [NFR-PQ-2]
#![cfg(feature = "backend-fips")]

use kms::{Kms, TrustRoot};
use midgard_client_keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_keystore::{Keystore, KeystoreError, SecurityFloor};
use pqc_agility::backend::fips::provider;
use pqc_agility::{sha256, OsRng, Rng, SecretBytes};

/// A software stand-in for the Enclave: shared = SHA-256(device secret ‖ ephemeral).
pub struct TestSealer([u8; 32]);
impl RootSealer for TestSealer {
    fn ensure_key(&self) -> Result<(), KeystoreError> {
        Ok(())
    }
    fn agree_new(&self) -> Result<(Vec<u8>, SecretBytes), KeystoreError> {
        let mut eph = vec![0u8; 32];
        OsRng.fill(&mut eph);
        let shared = self.agree_existing(&eph)?;
        Ok((eph, shared))
    }
    fn agree_existing(&self, eph: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Ok(SecretBytes::new(sha256(&[&self.0[..], eph].concat()).to_vec()))
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

#[test]
fn a_sealed_root_reopens_as_the_same_root() {
    let p = provider();
    let sealer = TestSealer([7; 32]);
    let (ks, blob) = SealedRootKeystore::create(&p, "device-root", &sealer, &mut OsRng).unwrap();
    assert_eq!(ks.floor(), SecurityFloor::NonExtractable);
    let sealed = Kms::new(provider(), ks).seal(b"aad", b"payload", &mut OsRng).unwrap();
    let again = SealedRootKeystore::open(&blob, &sealer).unwrap();
    assert_eq!(Kms::new(provider(), again).unseal(b"aad", &sealed).unwrap(), b"payload");
}

#[test]
fn another_platform_key_or_a_tampered_blob_cannot_reopen_it() {
    let (_, blob) = SealedRootKeystore::create(&provider(), "device-root", &TestSealer([7; 32]), &mut OsRng).unwrap();
    assert!(matches!(SealedRootKeystore::open(&blob, &TestSealer([8; 32])), Err(KeystoreError::Unseal)));
    let mut bad = blob.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(SealedRootKeystore::open(&bad, &TestSealer([7; 32])).is_err());
    let _: &dyn TrustRoot = &SealedRootKeystore::open(&blob, &TestSealer([7; 32])).unwrap();
}
```

`clients/store/tests/persist.rs`:

```rust
//! The sealed cache survives a relaunch, drops what the new clearance may not hold,
//! and refuses a file sealed to another root. [AC-C-2] [AC-C-13]
#![cfg(feature = "backend-fips")]

use midgard_client_keystore::sealed_root::{RootSealer, SealedRootKeystore};
use midgard_client_keystore::{KeystoreError, SecurityFloor};
use midgard_client_store::{LocalStore, StoreError};
use pqc_agility::backend::fips::provider;
use pqc_agility::{sha256, OsRng, Rng, SecretBytes};
use policy::{ClassificationLevel as L, Clearance, Marking};

struct TestSealer([u8; 32]);
impl RootSealer for TestSealer {
    fn ensure_key(&self) -> Result<(), KeystoreError> {
        Ok(())
    }
    fn agree_new(&self) -> Result<(Vec<u8>, SecretBytes), KeystoreError> {
        let mut eph = vec![0u8; 32];
        OsRng.fill(&mut eph);
        Ok((eph.clone(), self.agree_existing(&eph)?))
    }
    fn agree_existing(&self, eph: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Ok(SecretBytes::new(sha256(&[&self.0[..], eph].concat()).to_vec()))
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

fn cl(level: L) -> Clearance {
    Clearance { level, compartments: Default::default() }
}
fn mk(level: L) -> Marking {
    Marking { level, compartments: Default::default() }
}

#[test]
fn the_cache_round_trips_and_drops_above_clearance_rows() {
    let sealer = TestSealer([1; 32]);
    let (ks, root) = SealedRootKeystore::create(&provider(), "device-root", &sealer, &mut OsRng).unwrap();
    let mut s = LocalStore::open(provider(), ks, cl(L::SECRET), SecurityFloor::NonExtractable).unwrap();
    s.put("a", mk(L::UNCLASSIFIED), b"open row", &mut OsRng).unwrap();
    s.put("b", mk(L::SECRET), b"secret row", &mut OsRng).unwrap();
    let file = s.export_sealed(&mut OsRng).unwrap();
    assert!(!file.windows(8).any(|w| w == b"open row"), "the file must not hold plaintext");

    let ks = SealedRootKeystore::open(&root, &sealer).unwrap();
    let mut lower = LocalStore::open(provider(), ks, cl(L::CONFIDENTIAL), SecurityFloor::NonExtractable).unwrap();
    assert_eq!(lower.import_sealed(&file).unwrap(), (1, 1));
    assert_eq!(lower.get("a").unwrap(), b"open row");
    assert!(matches!(lower.get("b"), Err(StoreError::NotFound)));
}

#[test]
fn a_file_sealed_to_another_root_is_refused() {
    let (ks, _) = SealedRootKeystore::create(&provider(), "device-root", &TestSealer([1; 32]), &mut OsRng).unwrap();
    let mut s = LocalStore::open(provider(), ks, cl(L::SECRET), SecurityFloor::NonExtractable).unwrap();
    s.put("a", mk(L::UNCLASSIFIED), b"x", &mut OsRng).unwrap();
    let file = s.export_sealed(&mut OsRng).unwrap();
    let (other, _) = SealedRootKeystore::create(&provider(), "device-root", &TestSealer([2; 32]), &mut OsRng).unwrap();
    let mut t = LocalStore::open(provider(), other, cl(L::SECRET), SecurityFloor::NonExtractable).unwrap();
    assert!(t.import_sealed(&file).is_err());
}
```

(The store's dev-dependencies already include `midgard-client-keystore`; add `policy` is a normal dependency, `pqc-agility` too — no manifest change is needed for the test.)

In `clients/sync/src/lib.rs`, inside `mod tests`:

```rust
    #[test]
    fn requeued_ops_are_pending_again_with_their_versions() {
        let mut a = Replica::new("dev");
        a.set("o", "f", b"1".to_vec());
        let saved = a.drain_pending();
        let mut b = Replica::new("dev");
        b.requeue(saved.clone());
        assert_eq!(b.pending(), &saved[..]);
        assert_eq!(b.get("o", "f"), Some(&b"1"[..]));
        let next = b.set("o", "g", b"2".to_vec());
        assert!(next.version.lamport > saved[0].version.lamport);
    }
```

**Step 2: Run them and see them fail.**

```bash
cargo test --offline --locked -p midgard-client-keystore --features backend-fips --test sealed_root
cargo test --offline --locked -p midgard-client-store --features backend-fips --test persist
cargo test --offline --locked -p midgard-client-sync requeued_ops
```

Expected: `unresolved import \`midgard_client_keystore::sealed_root\``; then (after keystore) `no method named \`export_sealed\``; `no method named \`requeue\``.

**Step 3a: Keystore.** Add to `KeystoreError` (`clients/keystore/src/error.rs`) and to its `Display`:

```rust
    /// The user cancelled the platform prompt (Face ID / Touch ID / passcode).
    UserCancelled,
    /// The platform key is unavailable (no Enclave, key deleted, not enrolled).
    PlatformUnavailable(String),
    /// A sealed root blob is malformed.
    Malformed(String),
    /// The sealed root did not open: another platform key, or tampered.
    Unseal,
```

```rust
            KeystoreError::UserCancelled => write!(f, "the user cancelled authentication"),
            KeystoreError::PlatformUnavailable(d) => write!(f, "platform key unavailable: {d}"),
            KeystoreError::Malformed(d) => write!(f, "sealed root malformed: {d}"),
            KeystoreError::Unseal => write!(f, "sealed root did not open"),
```

In `clients/keystore/src/lib.rs` after `pub mod auth;`:

```rust
#[cfg(feature = "backend-fips")]
pub mod sealed_root;
```

`clients/keystore/src/sealed_root.rs`:

```rust
//! A custody root sealed by a platform key. `[FR-C-2]` `[NFR-PQ-2]`
//!
//! Platform key stores (the Apple Secure Enclave, Android StrongBox) hold P-256 keys
//! and cannot hold or decapsulate ML-KEM-1024. So the ML-KEM root is generated here
//! and, at rest, its secret is AES-256-GCM-encrypted under a key-encryption key
//! derived by HKDF-SHA-256 from an ECDH agreement the platform key performs (with user
//! presence). The platform never sees the root secret; this crate never sees the
//! platform key. While unlocked the secret is in mlocked process memory, so the floor
//! reported is `NonExtractable`; the hardware binding is at rest. `[VLD-ANB-MIDGARD-0056]`
//!
//! ```text
//! magic 8 "MGSROOT1" | key_id 4+n | suite 1 | root_public 4+n | ephemeral 4+n
//! | nonce 12 | ciphertext 4+n      (AAD = everything before the nonce)
//! ```

use kms::{KmsError, TrustRoot, WRAP_SUITE};
use pqc_agility::{
    hkdf_sha256_32, Aes256GcmAead, Ciphertext, CryptoProvider, Kem, KemPublicKey, KemSecretKey, KemSuiteId, Rng,
    SecretBytes, SharedSecret, Zeroizing, GCM_NONCE_LEN,
};

use crate::{Keystore, KeystoreError, SecurityFloor};

/// File magic.
pub const MAGIC: &[u8; 8] = b"MGSROOT1";
const KEK_INFO: &[u8] = b"midgard-client/sealed-root/v1";

/// The platform side: a key that agrees secrets and never leaves its hardware.
pub trait RootSealer {
    /// Create the platform key if it does not exist. Idempotent.
    fn ensure_key(&self) -> Result<(), KeystoreError>;
    /// A fresh agreement with the platform key's public half: `(ephemeral public key,
    /// shared secret)`. Needs no user presence.
    fn agree_new(&self) -> Result<(Vec<u8>, SecretBytes), KeystoreError>;
    /// The shared secret for `ephemeral_public`, computed by the platform key. Prompts
    /// for user presence.
    fn agree_existing(&self, ephemeral_public: &[u8]) -> Result<SecretBytes, KeystoreError>;
    /// Whether the platform key is held in hardware.
    fn hardware_backed(&self) -> bool;
}

/// An ML-KEM-1024 root, sealed at rest by a [`RootSealer`].
pub struct SealedRootKeystore {
    key_id: String,
    public_key: KemPublicKey,
    secret: KemSecretKey,
    hardware_wrapped: bool,
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn header(key_id: &str, public: &KemPublicKey, eph: &[u8]) -> Vec<u8> {
    let mut o = MAGIC.to_vec();
    put(&mut o, key_id.as_bytes());
    o.push(public.suite().as_u8());
    put(&mut o, public.as_bytes());
    put(&mut o, eph);
    o
}

fn kek(eph: &[u8], shared: &SecretBytes) -> Zeroizing<[u8; 32]> {
    Zeroizing::new(hkdf_sha256_32(eph, shared.expose(), KEK_INFO))
}

/// A bounds-checked reader: `None` on any short read, never a panic.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len())?;
        let s = &self.b[self.at..end];
        self.at = end;
        Some(s)
    }
    fn field(&mut self) -> Option<&'a [u8]> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize;
        self.take(n)
    }
}

impl SealedRootKeystore {
    /// Generate a root and seal it. Returns the keystore and the blob to store.
    pub fn create(
        provider: &CryptoProvider,
        key_id: &str,
        sealer: &dyn RootSealer,
        rng: &mut dyn Rng,
    ) -> Result<(Self, Vec<u8>), KeystoreError> {
        sealer.ensure_key()?;
        let kem = provider.kem_for(WRAP_SUITE).map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        let (public_key, secret) = kem.keypair(rng).map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        let (eph, shared) = sealer.agree_new()?;
        let mut blob = header(key_id, &public_key, &eph);
        let mut nonce = [0u8; GCM_NONCE_LEN];
        rng.fill(&mut nonce);
        let ct = Aes256GcmAead::new(&kek(&eph, &shared))
            .seal(&nonce, &blob, secret.expose())
            .map_err(|e| KeystoreError::RootGeneration(e.to_string()))?;
        blob.extend_from_slice(&nonce);
        put(&mut blob, &ct);
        let hardware_wrapped = sealer.hardware_backed();
        Ok((Self { key_id: key_id.to_string(), public_key, secret, hardware_wrapped }, blob))
    }

    /// Reopen a sealed root through the platform key that sealed it.
    pub fn open(blob: &[u8], sealer: &dyn RootSealer) -> Result<Self, KeystoreError> {
        let bad = |d: &str| KeystoreError::Malformed(d.to_string());
        let mut c = Cursor { b: blob, at: 0 };
        if c.take(MAGIC.len()).ok_or_else(|| bad("truncated"))? != MAGIC {
            return Err(bad("bad magic"));
        }
        let key_id = String::from_utf8(c.field().ok_or_else(|| bad("key id"))?.to_vec()).map_err(|_| bad("key id"))?;
        let suite = c
            .take(1)
            .and_then(|b| KemSuiteId::from_u8(b[0]))
            .filter(|s| *s == WRAP_SUITE)
            .ok_or_else(|| bad("suite"))?;
        let public = c.field().ok_or_else(|| bad("public key"))?.to_vec();
        let eph = c.field().ok_or_else(|| bad("ephemeral key"))?.to_vec();
        let header_len = c.at;
        let nonce: [u8; GCM_NONCE_LEN] = c.take(GCM_NONCE_LEN).ok_or_else(|| bad("nonce"))?.try_into().expect("nonce length");
        let ct = c.field().ok_or_else(|| bad("ciphertext"))?;
        if c.at != blob.len() {
            return Err(bad("trailing bytes"));
        }
        let shared = sealer.agree_existing(&eph)?;
        let secret = Aes256GcmAead::new(&kek(&eph, &shared))
            .open(&nonce, &blob[..header_len], ct)
            .map_err(|_| KeystoreError::Unseal)?;
        Ok(Self {
            key_id,
            public_key: KemPublicKey::new(suite, public),
            secret: KemSecretKey::new(suite, SecretBytes::new(secret)),
            hardware_wrapped: sealer.hardware_backed(),
        })
    }

    /// Whether the root is sealed by a hardware-held platform key.
    pub fn hardware_wrapped(&self) -> bool {
        self.hardware_wrapped
    }
}

impl Keystore for SealedRootKeystore {
    fn floor(&self) -> SecurityFloor {
        SecurityFloor::NonExtractable
    }
    fn holds_key(&self) -> bool {
        true
    }
    fn key_id(&self) -> &str {
        &self.key_id
    }
}

impl TrustRoot for SealedRootKeystore {
    fn key_id(&self) -> &str {
        &self.key_id
    }
    fn public_key(&self) -> &KemPublicKey {
        &self.public_key
    }
    fn unwrap(&self, kem: &dyn Kem, wrapped: &Ciphertext) -> Result<SharedSecret, KmsError> {
        kem.decapsulate(&self.secret, wrapped).map_err(|e| KmsError::Kem(e.to_string()))
    }
}
```

Keep `Unseal` for an AES-GCM failure and `Malformed` for structure, because the FFI maps them differently (signature/tamper versus corrupt store).

**Step 3b: Store.** Add `Malformed(String)` to `StoreError` (with Display `"cache file malformed: {d}"`). In `clients/store/src/lib.rs`:

```rust
/// File magic of an exported cache (inside the outer seal).
pub const CACHE_MAGIC: &[u8; 8] = b"MGCACHE1";
/// AAD binding the outer seal to its purpose.
const CACHE_FILE_AAD: &[u8] = b"midgard-client/cache-file/v1";

impl<R: TrustRoot + Keystore> LocalStore<R> {
    /// The whole cache as one sealed blob: entries (id, marking, sealed object) inside
    /// an outer seal, so neither ids nor markings are readable at rest. `[AC-C-2]`
    pub fn export_sealed(&self, rng: &mut dyn Rng) -> Result<Vec<u8>, StoreError> {
        fn put(o: &mut Vec<u8>, b: &[u8]) {
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        }
        let mut inner = CACHE_MAGIC.to_vec();
        inner.extend_from_slice(&(self.objects.len() as u32).to_le_bytes());
        for (id, obj) in &self.objects {
            put(&mut inner, id.as_bytes());
            inner.push(obj.marking.level.0);
            inner.extend_from_slice(&(obj.marking.compartments.len() as u32).to_le_bytes());
            for c in &obj.marking.compartments {
                put(&mut inner, c.0.as_bytes());
            }
            put(&mut inner, &obj.sealed.encode());
        }
        Ok(self.kms.seal(CACHE_FILE_AAD, &inner, rng)?.encode())
    }

    /// Load an exported cache. Rows the current clearance does not dominate are
    /// dropped, never stored; a file sealed to another root is refused.
    /// Returns `(kept, dropped)`. `[AC-C-13]`
    pub fn import_sealed(&mut self, file: &[u8]) -> Result<(usize, usize), StoreError> {
        let outer = SealedBlob::decode(file)?;
        let inner = self.kms.unseal(CACHE_FILE_AAD, &outer)?;
        let bad = |d: &str| StoreError::Malformed(d.to_string());
        let mut c = Cursor { b: &inner, at: 0 };
        if c.take(CACHE_MAGIC.len()).ok_or_else(|| bad("truncated"))? != CACHE_MAGIC {
            return Err(bad("magic"));
        }
        let count = c.u32().ok_or_else(|| bad("count"))? as usize;
        let mut entries = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            let id = String::from_utf8(c.field().ok_or_else(|| bad("id"))?.to_vec()).map_err(|_| bad("id"))?;
            let level = policy::ClassificationLevel(c.take(1).ok_or_else(|| bad("level"))?[0]);
            let nc = c.u32().ok_or_else(|| bad("compartments"))? as usize;
            let mut compartments = std::collections::BTreeSet::new();
            for _ in 0..nc {
                let name = c.field().ok_or_else(|| bad("compartment"))?;
                compartments.insert(policy::Compartment(String::from_utf8(name.to_vec()).map_err(|_| bad("compartment"))?));
            }
            let sealed = SealedBlob::decode(c.field().ok_or_else(|| bad("object"))?)?;
            entries.push((id, Marking { level, compartments }, sealed));
        }
        if c.at != inner.len() {
            return Err(bad("trailing bytes"));
        }
        let (mut kept, mut dropped) = (0, 0);
        for (id, marking, sealed) in entries {
            if sealed.root_key_id != self.kms.root_key_id() {
                return Err(StoreError::Kms(kms::KmsError::WrongTrustRoot));
            }
            if !dominates(&self.clearance, &marking) {
                dropped += 1;
                continue;
            }
            self.objects.insert(id, CachedObject { marking, sealed });
            kept += 1;
        }
        Ok((kept, dropped))
    }

    /// Re-scope the store to `clearance` (a persona switch): rows it does not dominate
    /// are dropped now. Returns the store and how many rows were dropped. `[AC-C-13]`
    pub fn rescope(mut self, clearance: Clearance) -> (Self, usize) {
        let before = self.objects.len();
        self.objects.retain(|_, o| dominates(&clearance, &o.marking));
        self.clearance = clearance;
        let dropped = before - self.objects.len();
        (self, dropped)
    }

    /// Remove one cached object.
    pub fn remove(&mut self, id: &str) {
        self.objects.remove(id);
    }
}
```

Two root checks happen: the outer unseal already fails with `KmsError::WrongTrustRoot` for a file from another device root (`Kms::unseal` compares `root_key_id` first) — but the test above uses the same key id `device-root` for two different roots, which is exactly the case where `Kms::unseal` reaches AES-GCM and fails with `KmsError::Aead`; both are errors, which is what the test asserts. Add the same private `Cursor` struct as in Step 3a to `clients/store/src/lib.rs`, plus `fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }`.

**Step 3c: Sync.** In `clients/sync/src/lib.rs`, in `impl Replica` after `drain_pending`:

```rust
    /// Restore ops that were queued but never acknowledged (for example across a
    /// relaunch): applied and queued again with their original versions, so pushing
    /// them again is idempotent at the enclosure. `[FR-C-4]`
    pub fn requeue(&mut self, ops: Vec<SyncOp>) {
        for op in ops {
            if op.version.lamport > self.lamport {
                self.lamport = op.version.lamport;
            }
            self.apply(&op);
            if !self.pending.contains(&op) {
                self.pending.push(op);
            }
        }
    }
```

**Step 4: Run them and see them pass**, and re-run the whole of each crate:

```bash
cargo test --offline --locked -p midgard-client-sync
cargo test --offline --locked -p midgard-client-keystore --features backend-fips
cargo test --offline --locked -p midgard-client-store --features backend-fips
cargo clippy --offline --locked -p midgard-client-sync -p midgard-client-keystore -p midgard-client-store \
    -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

(The last line is the `just lint` client group, `justfile:145`, minus the crates this task does not touch.)

**Step 5: Commit.**

```bash
git add clients/keystore clients/store clients/sync
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients: sealed-root custody, persistent sealed cache, requeue of pending ops [FR-C-2] [FR-C-4]"
```

---

### Task A10: Session façade

**Already built in A9 — read first.** A9 (see its "As built" paragraph) added to `ClientCore`: `open_scoped` (persona scope; there is no unscoped `open`), `scope`, `replica_id`, `rescope(scope, clearance)` (refuses while pending when the view would be dropped; drops everything on a persona change), `new_action_id`, `export_state -> ExportedState { file, generation }`, `import_state(file, min_generation) -> ImportReport { kept, dropped, pending_dropped, generation }` (all or nothing, `CoreError::Stale` on a rollback), and `cache_delivery`/`cached_delivery` (rows plus a fail-closed index; withheld rows by level only). A10 builds `Session` on these and must **not** re-add `cache_snapshot`, `remove_cached`, `rescope(clearance)`, its own `<collection>#index` code, the lenient `unwrap_or("[]")` index parse, its own `next_action` counter or `action:<replica>-<n>` ids. Its test that a switch up keeps the old indexes changes: a persona switch keeps nothing. A10 surfaces `pending_dropped` to the user and passes the shell's stored generation to `import_state`. It also owns the device-audit signing key (own domain; `SignatureStatus::Device` only after verifying).

**As built (16 Nov).** The steps below are kept as written for the record; the code differs as follows.

1. *Built on A9.* `Session` uses `ClientCore::{open_scoped, cache_delivery, cached_delivery, new_action_id, rescope(scope, clearance), export_state, import_state(file, min_generation)}`; none of the step-3a core code was added. `export_state` returns `ExportedState { file, generation }`; `import_state(file, min_generation)` returns `ImportReport` (with `pending_dropped`) unchanged to the caller. On any import error the cache is empty and the queue unchanged; the caller discards the file and the next read refetches.
2. *Device-audit key (owner decision 11, refined).* New module `device_audit`: `DeviceAuditKey::create(root, rng) -> (key, blob)` / `open(root, blob)`, an ML-DSA-65 key made in the core and sealed to the device's ML-KEM root (`kms::seal_to_root`; the root is the one the `RootSealer` seals), so it never crosses to Swift and is never at rest in the clear. It signs only audit-row preimages under its own domain `attest::accept::DEVICE_AUDIT` (`midgard-device-audit-v1`, added to attest's distinct-domain tests). `Session::open(keystore, device_key, source, persona_id, replica_id)` takes it. `request_action` records each action (id, verb, target, SHA-256 of the canonical parameters) in the persona's sealed record (`\0sys/device-audit`) before queueing it. A sync considers only answers about recorded actions, takes verb and target from the record and status and audit id from the merged view, and signs a *device statement* (the row's signed bytes plus `{actionId, verb, target, paramsSha256, status}`), only if the row's actor is the persona, its resource the recorded target and its `action` text exactly `"<status> · <verb>"`. So a source cannot have the device sign content of its choosing (a made-up action, an overwritten target or audit id, an altered action text). On every read, online or offline, the same fields are re-checked and the statement verified; only then is a row `device`. Owed rows are keyed by audit id (the source picks the collection, so it is known only when the row is seen); signatures by `<collection>/<id>`. Only collections the source serves as `AuditRecord` carry a status. `verified` is believed for a row only when `DataSource::in_core_verified(&Seal, grant, collection, id)` returns a `VerifiedDigest` (SHA-256 of the row's signed bytes as the in-core source verified it) that matches the row exactly as delivered; `Seal` and `VerifiedDigest` can be made only inside the crate, so an outside source gives `None` (rows `none`), and a wrapper that passes the seal on to the enclosure keeps `verified` only for rows it did not change. One action binds to at most one audit id and one audit id to at most one action (an action answered with two ids, and every action claiming a contested id, is refused, its owed row or signature dropped). Decided actions leave the record (signed, answered with no audit row, row seen and not matching, refused); each signature keeps its own copy of the action's fields; at most `MAX_DEVICE_SIGS` (1024) signatures and as many owed rows are kept, oldest dropped first (an owed row dropped takes its action's record with it). A source that answers two identical actions with their audit ids swapped from the start (one op each) cannot be told apart from an honest one: both rows say the same thing, and each statement then names the other action id (gateway-era follow-up: Risks, 16). A source's `device` claim becomes `none`; every audit row's `pqSignature` text is reset to its status word. `DeviceAuditKey::sign_statement` is crate-private.
   `request_action(verb, target, params)` takes no marking: it reads the target's marking from the persona's cached delivery (`NothingCached` if never read, `AboveClearance` if withheld or missing, `BadTarget` if not `<collection>/<id>`).
3. *Persona switch keeps nothing* (A9): cache, CRDT view and device signatures are dropped; a switch up is no different. Refused with `SessionError::PendingOps(n)` while ops are pending, and the session comes back unchanged.
4. *Sync* decodes the whole answer first, merges, and only then drains, so a malformed answer loses nothing either.
5. *Test fixture* is the A8 body; `DemoEnclosure::from_body` takes the audit anchors (A8).

`Session` composes `ClientCore` + a `DataSource` + one persona. It is the only thing the FFI talks to.

- **Reads** (`rows::<T>(collection)`): online, fetch from the source, seal every delivered row into the cache under `"<collection>/<id>"` with its marking, and seal an index of the delivery (ids, markings, withheld levels) under `"<collection>#index"` at the session's own clearance; offline, serve from the index and the sealed rows. Either way rows are decoded into the generated read model `T`.
- **Actions** (`request_action`): four `edit_field` calls on `action:<replica>-<n>`, at the target row's marking — so acting on a row above clearance is refused by `ClientCore::edit_field` before anything is queued.
- **Sync** (`sync`): push the pending ops (encoded, not yet drained), and only on success drain and merge the answer — so a failed push loses nothing.
- **Persona switch**: refused while ops are pending (sync first); otherwise the store is re-scoped and everything above the new clearance is dropped, as is every collection index sealed at the old clearance.
- **State export/import**: pending ops are sealed into the cache as object `sys/pending`, and the whole cache is exported as one sealed file.

**Files:**
- Modify: `clients/core/src/lib.rs` (`ClientCore::cache_snapshot`, `remove_cached`, `rescope`, `export_state`, `import_state`, `replica_id`)
- Create: `clients/core/src/session.rs`; `clients/core/src/lib.rs` gains `pub mod session;`
- Test: `clients/core/tests/session.rs`

**Step 1: Write the failing test** — `clients/core/tests/session.rs`:

```rust
//! A session reads clearance-filtered rows online and from the sealed cache offline,
//! queues actions offline and syncs them once, refuses a persona switch with unsent
//! work, and drops above-clearance rows on a switch down. [AC-C-13] [FR-C-4] [AC-C-2]
#![cfg(feature = "backend-fips")]

use attest::json::parse;
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::{MarkingLevel, RowMarking, Visible};
use midgard_client_core::session::{ActionState, Session, SessionError, SyncPhase};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

const BODY: &str = include_str!("fixtures/session_body.json");

fn session(persona: &str) -> Session<SoftwareKeystore, DemoEnclosure> {
    let e = DemoEnclosure::from_body("test", &parse(BODY).unwrap()).unwrap();
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    Session::open(ks, e, persona, "dev-1").unwrap()
}

#[test]
fn rows_online_then_from_the_sealed_cache_offline() {
    let mut s = session("k.rao");
    let online: Vec<Visible<AuditRecord>> = s.rows("midgard.audit").unwrap();
    assert!(matches!(online[1], Visible::Withheld { level: MarkingLevel::Confidential }));
    s.set_online(false);
    assert_eq!(s.rows::<AuditRecord>("midgard.audit").unwrap(), online);
    assert_eq!(s.sync_status().phase, SyncPhase::Offline);
}

#[test]
fn an_offline_action_queues_then_applies_once() {
    let mut s = session("k.rao");
    let target = RowMarking::new(MarkingLevel::Internal, Vec::<String>::new());
    s.set_online(false);
    let id = s.request_action("demo.ping", "midgard.audit/AUD-1", &[], &target).unwrap();
    assert_eq!(s.action_state(&id), ActionState::Queued);
    assert!(matches!(s.sync(), Err(SessionError::Offline)));
    assert_eq!(s.sync_status().pending, 4);
    s.set_online(true);
    let r = s.sync().unwrap();
    assert_eq!((r.pushed, s.sync_status().pending), (4, 0));
    assert!(matches!(s.action_state(&id), ActionState::Applied { .. }));
}

#[test]
fn acting_on_a_row_above_clearance_is_refused_before_queueing() {
    let mut s = session("k.rao");
    let secret = RowMarking::new(MarkingLevel::Confidential, ["OPS-N"]);
    assert!(matches!(s.request_action("demo.ping", "midgard.audit/AUD-2", &[], &secret), Err(SessionError::AboveClearance)));
    assert_eq!(s.sync_status().pending, 0);
}

#[test]
fn a_switch_with_unsent_work_is_refused_and_a_switch_down_drops_rows() {
    let mut s = session("k.osei");
    let _: Vec<Visible<AuditRecord>> = s.rows("midgard.audit").unwrap();
    s.set_online(false);
    s.request_action("demo.ping", "midgard.audit/AUD-1", &[], &RowMarking::new(MarkingLevel::Internal, Vec::<String>::new()))
        .unwrap();
    let (s, err) = match s.switch_persona("k.rao") {
        Ok(_) => panic!("a switch with pending work must be refused"),
        Err(refused) => refused,
    };
    assert!(matches!(err, SessionError::PendingOps(4)));
    let mut s = s;
    s.set_online(true);
    s.sync().unwrap();
    let mut s = s.switch_persona("k.rao").map_err(|(_, e)| e).unwrap();
    s.set_online(false);
    assert!(matches!(s.rows::<AuditRecord>("midgard.audit"), Err(SessionError::NothingCached(_))));
}
```

Create `clients/core/tests/fixtures/session_body.json` with the same body as the `BODY` constant in `clients/core/tests/enclosure.rs` (Task A8), as a file.

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips --test session
```

Expected: `unresolved import \`midgard_client_core::session\``.

**Step 3a: Core additions** — in `impl<K: Keystore + TrustRoot> ClientCore<K>` (`clients/core/src/lib.rs`):

```rust
    /// The replica id (stable per device and session).
    pub fn replica_id(&self) -> &str {
        self.replica.id()
    }

    /// Seal a delivered row into the local cache. Refused above clearance. `[AC-C-13]`
    pub fn cache_snapshot(&mut self, id: &str, marking: Marking, bytes: &[u8], rng: &mut dyn Rng) -> Result<(), CoreError> {
        Ok(self.store.put(id, marking, bytes, rng)?)
    }

    /// Re-scope to a new clearance; rows it does not dominate are dropped.
    pub fn rescope(self, clearance: Clearance) -> (Self, usize) {
        let (store, dropped) = self.store.rescope(clearance);
        (Self { store, replica: self.replica }, dropped)
    }

    /// Export the sealed cache with the pending queue sealed inside it. `[FR-C-4]`
    pub fn export_state(&mut self, rng: &mut dyn Rng) -> Result<Vec<u8>, CoreError> {
        let mut list = Vec::new();
        for op in self.replica.pending() {
            let b = op.encode();
            list.extend_from_slice(&(b.len() as u32).to_le_bytes());
            list.extend_from_slice(&b);
        }
        let own = Marking { level: self.store.clearance().level, compartments: self.store.clearance().compartments.clone() };
        self.store.put(PENDING_OBJECT, own, &list, rng)?;
        Ok(self.store.export_sealed(rng)?)
    }

    /// Import an exported state: the cache (above-clearance rows dropped) and the
    /// pending queue (requeued). Returns `(kept, dropped)`.
    pub fn import_state(&mut self, file: &[u8]) -> Result<(usize, usize), CoreError> {
        let counts = self.store.import_sealed(file)?;
        if let Ok(list) = self.store.get(PENDING_OBJECT) {
            let mut ops = Vec::new();
            let mut at = 0usize;
            while at < list.len() {
                let n = list.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4")) as usize);
                let blob = n.and_then(|n| list.get(at + 4..at + 4 + n)).ok_or_else(|| CoreError::MalformedOp("pending list".into()))?;
                ops.push(SyncOp::decode(blob)?);
                at += 4 + blob.len();
            }
            self.replica.requeue(ops);
        }
        Ok(counts)
    }
```

with `const PENDING_OBJECT: &str = "sys/pending";` and the imports `policy::Clearance` added to the existing `use policy::{dominates, Clearance, Marking};`.

**Step 3b: Session** — `clients/core/src/session.rs`:

```rust
//! A signed-in session: one persona, one sealed cache, one data source. `[FR-C-1]`
//! `[AC-C-13]` `[FR-C-4]`

use attest::json::{parse, Json};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

use crate::consoles::decode::{DecodeError, FromJson};
use crate::demo::canon;
use crate::keystore::{Keystore, SecurityFloor, TrustRoot};
use crate::marking::{MarkingLevel, RowMarking, Visible};
use crate::source::{action, DataSource, Delivered, Grant, SourceError};
use crate::store::StoreError;
use crate::sync::SyncOp;
use crate::{ClientCore, CoreError};

/// Coarse sync phase (the words `ClientSyncPhase` uses in sdk/ts/src/products/clients.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    /// Nothing pending.
    Synced,
    /// Edits are queued and the source is reachable.
    Syncing,
    /// The source is unreachable; edits queue.
    Offline,
    /// The last sync returned a conflict.
    Conflict,
}

/// Sync readout for the chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    /// Phase.
    pub phase: SyncPhase,
    /// Ops queued locally.
    pub pending: usize,
    /// Unix seconds of the last successful sync.
    pub last_sync: Option<u64>,
}

/// What a sync did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Ops pushed.
    pub pushed: usize,
    /// Ops merged back.
    pub merged: usize,
}

/// An action's state as this device knows it.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionState {
    /// Queued or pushed, not yet answered.
    Queued,
    /// Applied by the source.
    Applied {
        /// Audit record id.
        audit: String,
        /// Canonical JSON result.
        result: String,
    },
    /// Refused by the source.
    Refused {
        /// Why.
        reason: String,
    },
    /// The target changed under it.
    Conflict {
        /// Why.
        reason: String,
    },
    /// No such action on this device.
    Unknown,
}

/// A session error, each kind a screen state.
#[derive(Debug)]
pub enum SessionError {
    /// The row or action is above the session's clearance.
    AboveClearance,
    /// The source is unreachable.
    Offline,
    /// Offline and nothing cached for this collection.
    NothingCached(String),
    /// A persona switch with unsent work.
    PendingOps(usize),
    /// The source refused.
    Source(SourceError),
    /// The core refused (custody, tamper, malformed op).
    Core(CoreError),
    /// A row did not decode into its read model.
    Decode(DecodeError),
}

impl From<CoreError> for SessionError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Store(StoreError::AboveClearance) => SessionError::AboveClearance,
            e => SessionError::Core(e),
        }
    }
}

impl From<SourceError> for SessionError {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Offline => SessionError::Offline,
            e => SessionError::Source(e),
        }
    }
}

impl From<DecodeError> for SessionError {
    fn from(e: DecodeError) -> Self {
        SessionError::Decode(e)
    }
}

/// A signed-in session.
pub struct Session<K: Keystore + TrustRoot, S: DataSource> {
    core: ClientCore<K>,
    source: S,
    grant: Grant,
    next_action: u64,
    last_sync: Option<u64>,
    conflict: bool,
}

fn own_marking(grant: &Grant) -> RowMarking {
    RowMarking::new(grant.persona.clearance, grant.persona.compartments.iter().cloned())
}

impl<K: Keystore + TrustRoot, S: DataSource> Session<K, S> {
    /// Sign `persona_id` in over `keystore` and `source`.
    pub fn open(keystore: K, source: S, persona_id: &str, replica_id: &str) -> Result<Self, SessionError> {
        let grant = source.grant(persona_id)?;
        let core = ClientCore::open(provider(), keystore, grant.clearance.clone(), SecurityFloor::NonExtractable, replica_id)?;
        Ok(Self { core, source, grant, next_action: 0, last_sync: None, conflict: false })
    }

    /// The grant (persona, clearance).
    pub fn grant(&self) -> &Grant {
        &self.grant
    }

    /// The data source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Simulate (demo) going offline or back online.
    pub fn set_online(&mut self, online: bool) {
        self.source.set_online(online);
    }

    /// The rows of `collection`, decoded into `T`.
    pub fn rows<T: FromJson>(&mut self, collection: &str) -> Result<Vec<Visible<T>>, SessionError> {
        let delivered = match self.source.fetch(&self.grant, collection) {
            Ok(d) => {
                self.remember(collection, &d)?;
                d
            }
            Err(SourceError::Offline) => self.cached(collection)?,
            Err(e) => return Err(e.into()),
        };
        delivered
            .into_iter()
            .map(|d| match d {
                Delivered::Row { id, marking, body } => {
                    let text = String::from_utf8(body).map_err(|_| DecodeError { path: id.clone(), reason: "not UTF-8".into() })?;
                    let json = parse(&text).map_err(|e| DecodeError { path: id.clone(), reason: e })?;
                    let item = T::from_json(&json, &format!("{collection}/{id}"))?;
                    Ok(Visible::Shown { id, marking, item })
                }
                Delivered::Withheld { level } => Ok(Visible::Withheld { level }),
            })
            .collect()
    }

    fn remember(&mut self, collection: &str, delivered: &[Delivered]) -> Result<(), SessionError> {
        let mut index = Vec::new();
        for d in delivered {
            match d {
                Delivered::Row { id, marking, body } => {
                    self.core.cache_snapshot(&format!("{collection}/{id}"), marking.to_policy(), body, &mut OsRng)?;
                    index.push(Json::Obj(vec![
                        ("id".into(), Json::Str(id.clone())),
                        ("level".into(), Json::Str(marking.level.as_str().into())),
                        ("compartments".into(), Json::Arr(marking.compartments.iter().cloned().map(Json::Str).collect())),
                    ]));
                }
                Delivered::Withheld { level } => {
                    index.push(Json::Obj(vec![("withheld".into(), Json::Str(level.as_str().into()))]));
                }
            }
        }
        let bytes = canon::encode(&Json::Arr(index)).into_bytes();
        self.core.cache_snapshot(&format!("{collection}#index"), own_marking(&self.grant).to_policy(), &bytes, &mut OsRng)?;
        Ok(())
    }

    fn cached(&self, collection: &str) -> Result<Vec<Delivered>, SessionError> {
        let index = self
            .core
            .load_cached(&format!("{collection}#index"))
            .map_err(|_| SessionError::NothingCached(collection.to_string()))?;
        let index = parse(std::str::from_utf8(&index).unwrap_or("[]")).unwrap_or(Json::Arr(Vec::new()));
        let mut out = Vec::new();
        for e in index.as_array().unwrap_or(&[]) {
            if let Some(level) = e.get("withheld").and_then(Json::as_str).and_then(MarkingLevel::parse) {
                out.push(Delivered::Withheld { level });
                continue;
            }
            let id = e.get("id").and_then(Json::as_str).unwrap_or_default().to_string();
            let level = e.get("level").and_then(Json::as_str).and_then(MarkingLevel::parse).unwrap_or(MarkingLevel::Confidential);
            let comps: Vec<String> =
                e.get("compartments").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(|c| c.as_str().map(str::to_string)).collect();
            let body = self.core.load_cached(&format!("{collection}/{id}"))?;
            out.push(Delivered::Row { id, marking: RowMarking::new(level, comps), body });
        }
        Ok(out)
    }

    /// Queue an action on `target` (a `<collection>/<id>` whose row carries `marking`).
    pub fn request_action(
        &mut self,
        verb: &str,
        target: &str,
        params: &[(String, String)],
        marking: &RowMarking,
    ) -> Result<String, SessionError> {
        self.next_action += 1;
        let id = format!("{}{}-{}", action::PREFIX, self.core.replica_id(), self.next_action);
        let params = canon::encode(&Json::Obj(params.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect()));
        let m = marking.to_policy();
        for (field, value) in
            [(action::VERB, verb), (action::TARGET, target), (action::PARAMS, params.as_str()), (action::BY, self.grant.persona.id.as_str())]
        {
            self.core.edit_field(&id, field, value.as_bytes().to_vec(), m.clone(), &mut OsRng)?;
        }
        Ok(id)
    }

    /// What this device knows about action `id`.
    pub fn action_state(&self, id: &str) -> ActionState {
        let field = |f: &str| self.core.read_field(id, f).map(|b| String::from_utf8_lossy(&b).into_owned());
        match field(action::STATUS).as_deref() {
            None if field(action::VERB).is_some() => ActionState::Queued,
            None => ActionState::Unknown,
            Some("applied") => ActionState::Applied { audit: field(action::AUDIT).unwrap_or_default(), result: field(action::RESULT).unwrap_or_default() },
            Some("conflict") => ActionState::Conflict { reason: field(action::REASON).unwrap_or_default() },
            Some(_) => ActionState::Refused { reason: field(action::REASON).unwrap_or_default() },
        }
    }

    /// Push queued ops and merge the answer. On failure nothing is lost.
    pub fn sync(&mut self) -> Result<SyncReport, SessionError> {
        let blobs: Vec<Vec<u8>> = self.core.pending_ops().iter().map(SyncOp::encode).collect();
        let back = self.source.exchange(&self.grant, &blobs)?;
        self.core.drain_pending();
        self.core.merge_remote_encoded(&back)?;
        self.conflict = back
            .iter()
            .filter_map(|b| SyncOp::decode(b).ok())
            .any(|op| op.field == action::STATUS && op.value.as_deref() == Some(b"conflict"));
        self.last_sync = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
        Ok(SyncReport { pushed: blobs.len(), merged: back.len() })
    }

    /// The sync chip.
    pub fn sync_status(&self) -> SyncStatus {
        let pending = self.core.pending_ops().len();
        let phase = if !self.source.online() {
            SyncPhase::Offline
        } else if self.conflict {
            SyncPhase::Conflict
        } else if pending > 0 {
            SyncPhase::Syncing
        } else {
            SyncPhase::Synced
        };
        SyncStatus { phase, pending, last_sync: self.last_sync }
    }

    /// Switch persona. Refused while ops are pending; otherwise rows above the new
    /// clearance (and the old clearance's collection indexes) are dropped. `[AC-C-13]`
    pub fn switch_persona(self, persona_id: &str) -> Result<Self, (Self, SessionError)> {
        let pending = self.core.pending_ops().len();
        if pending > 0 {
            return Err((self, SessionError::PendingOps(pending)));
        }
        let grant = match self.source.grant(persona_id) {
            Ok(g) => g,
            Err(e) => return Err((self, e.into())),
        };
        let (core, _dropped) = self.core.rescope(grant.clearance.clone());
        Ok(Self { core, source: self.source, grant, next_action: self.next_action, last_sync: self.last_sync, conflict: false })
    }

    /// End the session, keeping the source (its demo state outlives a lock). The
    /// keystore, and with it the unlocked root secret, is dropped and zeroised.
    pub fn into_source(self) -> S {
        self.source
    }

    /// The sealed state to write to disk.
    pub fn export_state(&mut self) -> Result<Vec<u8>, SessionError> {
        Ok(self.core.export_state(&mut OsRng)?)
    }

    /// Load sealed state written by [`export_state`](Self::export_state).
    pub fn import_state(&mut self, file: &[u8]) -> Result<(usize, usize), SessionError> {
        Ok(self.core.import_state(file)?)
    }
}
```

Two details to check while implementing: (1) a switch *down* leaves `<collection>#index` objects sealed at the higher clearance — `rescope` drops them because the index's marking is the old persona's clearance, which the new one does not dominate; the last test relies on that. A switch *up* or sideways keeps the old indexes, which is correct (they are dominated). (2) `ClientCore::edit_field` already pre-checks clearance and leaves the CRDT untouched on refusal, which the third test relies on.

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-core --features backend-fips
cargo clippy --offline --locked -p midgard-client-core --features backend-fips --all-targets -- -D warnings
```

**Step 5: Commit.**

```bash
git add clients/core
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/core: session over a data source with sealed cache, offline queue and persona switch [FR-C-4] [AC-C-13]"
```

---

### Task A11: `clients/ffi` crate (UniFFI), vendoring, licence exceptions

**Changed by A10.** `Session::open` takes the device-audit key: open it with `DeviceAuditKey::open(&keystore, &blob)` (or `create` on first run, storing the blob beside the sealed root) before moving the keystore into the session. `Session::export_state` returns `ExportedState`; `import_state(file, min_generation)` returns `ImportReport` (surface `pending_dropped`); map `SessionError::Device` and `SessionError::BadTarget` to their own FFI errors. `Session::request_action(verb, target, params)` takes no marking: drop the `rows::<Raw>` lookup in `MidgardClient::request_action` below and pass the input straight through (the session reads the marking from the cache). Audit rows reach Swift with `pqSignature` set to `verified`, `device` or `none` only.

**Changed by A9.** The key-root callback is `seal(data_key) -> sealed` / `unseal(sealed) -> data_key` / `hardware_backed()` (A9, "As built"), not `agree_new`/`agree_existing`; the `KeyRoot`/`KeyAgreement` code below is superseded and is rewritten to that shape, wrapping returned bytes in `SecretBytes` at once. Map `UserCancelled`, `PlatformUnavailable`, `Malformed` (corrupt) and `Unseal` (tamper, wrong key) to distinct FFI errors, and `CoreError::{Stale, PendingOps, Reserved}` likewise.

**As built (16 Nov).** The steps below are kept as written for the record; the code differs as follows.

1. *Bytes, not files.* The crate writes no file and reads only the bundled dataset (`DemoEnclosure::open_pinned`). `ClientConfig` is `{ dataset_path, replica_id, device }` (no `store_dir`, no `demo_on_device_model`: owner decision 6, offline shows "AI off"). `enrol() -> SealedKeys { root, device_audit }` makes the sealed root and the device-audit key sealed to it (`DeviceAuditKey::create`), for the shell to store; `unlock(keys, persona_id)` checks the persona first (an unknown one never prompts), opens the root (one prompt) and the device-audit key, and signs in. `export_state() -> ExportedState { file, generation }` and `import_state(file, min_generation) -> ImportReport { kept, dropped, pending_dropped, generation }` hand the sealed state file to the shell, which keeps it and its generation per persona (A13). `reset_demo()` locks and reloads the dataset; the shell deletes its files and its platform key. `switch_persona`, `session_persona`, `collections`, `lock`, `sync_now`, `set_offline`, `status_chips`, `request_action` (no marking: the session reads it from the cache), `action_status` as in the steps.
2. *Key root.* `KeyRoot` (with_foreign, `Send + Sync`) is `seal(data_key) -> sealed`, `unseal(sealed) -> data_key`, `hardware_backed()`; `KeyRootError { UserCancelled, PlatformUnavailable { detail }, Malformed { detail }, Unseal }` maps one to one onto `KeystoreError`. The unsealed bytes go into `SecretBytes` at once. The custody chip takes `floor()` and `hardware_wrapped()` from the keystore before it moves into the session.
3. *Errors.* `ClientError` keeps every case apart: `AboveClearance`, `Offline`, `NothingCached`, `PendingOps`, `BadTarget`, `Stale { found, min }`, `Reserved`, `DeviceKey`, `SignatureFailure` (dataset), `Integrity` (a sealed state file that does not open), `Malformed`, `Locked`, `AlreadyUnlocked`, `UnknownPersona`, `UnknownCollection`, `Cancelled`, `KeyRootUnavailable`, `SealedRootMalformed`, `UnsealFailed`, `NotFound`, `Internal`. `StorageFull`, `Conflict`, `NotEnrolled` and `KeyRoot` are gone (no file writes; a conflict is an action state; enrolment is the shell's file).
4. *Typed rows.* `collection_rows(collection) -> [ConsoleRow { position, id?, marking, withheld, signature?, item? }]`: `item` is `RowItem`, one variant per read-model type (the generated mirrors plus a hand-written `HeimdallMotion` mirror, `clients/ffi/src/motion.rs`), kept equal to the core's `ROW_TYPES` by a test; `signature` is `SignatureStatus { Verified, Device, Unsigned }` for audit rows only, read from the status word the session writes into `pqSignature`; a withheld row has its level and nothing else.
5. *Generator.* First real use of `--ffi-out`: the `common` mirror imported itself (`use super::common::*`); fixed in `gen-read-models.mjs`. `gen-read-models.test.sh`'s positive control now covers `--ffi-out`.
6. *Vendoring.* `cargo update -p midgard-client-ffi` cannot name a member not yet in the lock; `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +stable update --workspace` added 51 packages and changed none; then `smawk` was held at 0.3.2. `serde_json 1.0.154` brings `zmij 1.0.23` (MIT). `cargo vendor` on 1.82 **deleted** `vendor/advisory-db` and `vendor/advisory-db-npm`; restore them with `git checkout -- vendor/advisory-db vendor/advisory-db-npm` before `vendor-check.sh`. A third informational advisory appears beside `paste` and `bincode`: `basic-toml` RUSTSEC-2026-0374 (unmaintained, under `uniffi_bindgen` only); all three are noted in `supply-chain/advisories.toml`, none suppressed.
7. *Commits.* The vendor commit also carries the workspace member, a minimal `lib.rs`, the licence exceptions and the crate counts, so it builds and passes every check on its own.
8. *Review round (16 Nov).* `unlock` no longer holds the client's lock while the platform side prompts: under the lock it checks the persona, takes the source and sets `State::Unlocking`; it opens the sealed root and the device-audit key unlocked; it re-locks to install the session, or puts the source back on an error. Meanwhile every other call sees a locked client (`personas()` and `collections()` are read once at construction), a second `unlock` is `UnlockInProgress`, a `reset_demo` wins (the waiting unlock returns `Locked`), and a panic in the platform side leaves the client `Gone` (the next `unlock` or `reset_demo` reloads the dataset). `reset_demo` also clears a poisoned lock. If reloading the dataset fails after a failed `Session::open`, the original error is returned. The licence exceptions' reason now names the generated Swift bindings (MPL-2.0 files, kept unmodified, acknowledged in the apps: A13). `pending_dropped` is tested at the FFI through an unsigned test body (a dev-dependency on the core with `demo-unpinned`, outside the release graph).

**UniFFI version.** The toolchain is pinned at 1.82.0 (`rust-toolchain.toml`; CI image `midgard-acceptance:1.82.0`). Checked while writing this plan, in a scratch crate outside the repository, with Cargo's MSRV-aware resolver (`CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback`, from stable Cargo) and then `cargo +1.82.0 check`:

| uniffi | builds on 1.82.0 | crates added (with the `bindgen` feature) |
|---|---|---|
| 0.32.2 | no (`askama_macros 0.16` needs edition 2024) | — |
| 0.31.0 | no (`rustc 1.82.0 is not supported`) | — |
| 0.30.0 | yes | ~70, including `tempfile`, `rustix`, `windows-sys` |
| 0.29.5 | yes | ~70, including `tempfile`, `rustix`, `windows-sys` |
| **0.28.3** | **yes** | **~51**, no Windows or `rustix` crates |

**Choice: `uniffi = "=0.28.3"`**, `default-features = false`. Its runtime closure is about 20 crates (`uniffi_core`, `uniffi_macros`, `uniffi_meta`, `uniffi_checksum_derive`, `anyhow`, `bytes`, `once_cell`, `static_assertions`, `paste`, `log`, `siphasher`, `camino`, `fs-err`, `toml 0.5`, `serde`, `bincode`, …). The Swift generator comes from `uniffi_bindgen 0.28.3` used as a dev-dependency by an **example** (examples are not release binaries — `tools/ci/release-artefacts.sh` derives the release matrix from `[[bin]]`, `src/main.rs` and `src/bin/` only), which keeps `clap` and its Windows crates out of the lockfile. 0.28.3 has library mode, `#[uniffi::export(with_foreign)]` callback traits, and `uniffi_bindgen::bindings::generate_swift_bindings` with `SwiftBindingsOptions { xcframework, module_name, modulemap_filename, … }` (verified in the 0.28.3 sources).

Pins the resolution needs on 1.82: `smawk 0.3.2` (0.3.3 is edition 2024 without a `rust-version`, so the MSRV-aware resolver cannot see the problem). Keep the vendored `proc-macro2 1.0.106`, `quote 1.0.46`, `syn 2.0.118`, `unicode-ident 1.0.24`. `serde 1.0.229`/`serde_derive` pull `syn 3.0.10`; it builds on 1.82, and an attempt to hold `serde` at 1.0.219 failed (`serde_json`/`serde_core` want `^1.0.220`), so accept it. Advisories in `vendor/advisory-db` for the new crates are all informational (`paste` RUSTSEC-2024-0436 and `bincode` RUSTSEC-2025-0141 unmaintained) or patched in the resolved versions (`anyhow` RUSTSEC-2026-0190 ≥ 1.0.103, `bytes` RUSTSEC-2026-0007 ≥ 1.11.1); informational advisories never block `mg-scan` (`tools/mg-scan/src/advisory.rs:78`).

**Licences.** Every new crate is `MIT`, `Apache-2.0`, `MIT OR Apache-2.0`, `MIT/Apache-2.0`, `Unlicense OR MIT` or `(MIT OR Apache-2.0) AND Unicode-3.0` — all satisfiable by `supply-chain/licences.toml` — **except the eight UniFFI crates, which are `MPL-2.0`**: `uniffi`, `uniffi_core`, `uniffi_macros`, `uniffi_meta`, `uniffi_checksum_derive`, `uniffi_bindgen`, `uniffi_udl`, `uniffi_testing`. `licences.toml` says MPL-2.0 is "permitted only via an exception below, with the obligations understood". And because `mg-sbom` lists **every** `[[package]]` of `Cargo.lock` in every release binary's SBOM (`tools/mg-sbom/src/main.rs:288`), adding UniFFI to the lockfile without exceptions would fail the licence scan of all 18 release binaries, not just the apps. This step therefore needs the owner's decision (Open questions, Q1); the plan writes the entries, the owner's name goes in `owner`.

**Unsafe code.** UniFFI's derives and scaffolding generate `unsafe` FFI code, so `clients/ffi` cannot carry `#![forbid(unsafe_code)]`; it carries `#![deny(unsafe_code)]` with an `#[allow]` only where the macros need it, and holds no hand-written `unsafe`. The core and every crate below it keep `forbid`.

**Files:**
- Create: `clients/ffi/Cargo.toml`, `clients/ffi/src/{lib.rs,error.rs,key_root.rs,client.rs}`, `clients/ffi/src/generated/mod.rs` (+ generated `common.rs … bifrost.rs`), `clients/ffi/examples/uniffi-bindgen.rs`
- Modify: `Cargo.toml` (workspace member), `Cargo.lock`, `vendor/` (new crates), `supply-chain/licences.toml` (exceptions), `justfile` (lint and test lines), `README.md:9` and `GETTING_STARTED.md:5` ("70-crate" becomes "71-crate"), `tools/ci/tests/demo-fixtures.test.sh` (generator check covers `--ffi-out`)
- Test: `clients/ffi/tests/ffi_surface.rs`

**Step 1: Write the failing test** — `clients/ffi/tests/ffi_surface.rs`:

```rust
//! The binding surface, exercised from Rust exactly as Swift calls it: enrol, unlock,
//! rows with withheld entries, offline queue, sync, lock, reset. [FR-C-1] [DD-6] [AC-C-13]

use std::path::PathBuf;
use std::sync::Arc;

use midgard_client_ffi::*;
use pqc_agility::{sha256, OsRng, Rng};

struct TestKeyRoot {
    device: [u8; 32],
    cancel: bool,
}
impl KeyRoot for TestKeyRoot {
    fn ensure_key(&self) -> Result<(), KeyRootError> {
        Ok(())
    }
    fn agree_new(&self) -> Result<KeyAgreement, KeyRootError> {
        let mut eph = vec![0u8; 32];
        OsRng.fill(&mut eph);
        // No prompt on this path, so `cancel` does not apply (it models the unlock prompt).
        let shared = sha256(&[&self.device[..], &eph].concat()).to_vec();
        Ok(KeyAgreement { ephemeral_public: eph, shared_secret: shared })
    }
    fn agree_existing(&self, eph: Vec<u8>) -> Result<Vec<u8>, KeyRootError> {
        if self.cancel {
            return Err(KeyRootError::Cancelled);
        }
        Ok(sha256(&[&self.device[..], &eph].concat()).to_vec())
    }
    fn destroy_key(&self) -> Result<(), KeyRootError> {
        Ok(())
    }
    fn hardware_backed(&self) -> bool {
        false
    }
}

fn client(cancel: bool) -> (Arc<MidgardClient>, PathBuf) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let store = std::env::temp_dir().join(format!("midgard-ffi-{}-{}", std::process::id(), cancel as u8));
    let _ = std::fs::remove_dir_all(&store);
    let config = ClientConfig {
        dataset_path: repo.join(format!("clients/demo/datasets/{}.mgdemo", core_version())).display().to_string(),
        store_dir: store.display().to_string(),
        replica_id: "test-device".into(),
        device: DeviceInfo { ram_mb: 8192, has_accelerator: true },
        demo_on_device_model: true,
    };
    (MidgardClient::new(config, Arc::new(TestKeyRoot { device: [3; 32], cancel })).unwrap(), store)
}

#[test]
fn the_whole_session_surface() {
    let (c, store) = client(false);
    assert_eq!(c.personas().len(), 4);
    assert!(matches!(c.unlock("k.rao".into()), Err(ClientError::NotEnrolled)));
    c.enrol().unwrap();
    let info = c.unlock("k.rao".into()).unwrap();
    assert_eq!(info.persona.id, "k.rao");
    assert_eq!(info.chips.custody.floor, "NonExtractable");

    let rows = c.collection_rows("midgard.objectTypes".into()).unwrap();
    assert!(rows.iter().any(|r| r.withheld), "k.rao (restricted) must see withheld rows");
    let visible = rows.iter().find(|r| !r.withheld).unwrap().id.clone().unwrap();

    let chips = c.set_offline(true).unwrap();
    assert_eq!(chips.sync.phase, SyncPhase::Offline);
    let id = c
        .request_action(ActionInput { verb: "demo.ping".into(), target: format!("midgard.objectTypes/{visible}"), params: vec![] })
        .unwrap();
    assert!(matches!(c.sync_now(), Err(ClientError::Offline)));
    c.set_offline(false).unwrap();
    assert_eq!(c.sync_now().unwrap().pushed, 4);
    assert_eq!(c.action_status(id).unwrap().state, ActionState::Applied);

    c.lock().unwrap();
    assert!(c.is_locked());
    assert!(matches!(c.collection_rows("midgard.objectTypes".into()), Err(ClientError::Locked)));
    c.unlock("k.rao".into()).unwrap();
    c.reset_demo().unwrap();
    assert!(!c.is_enrolled());
    let _ = std::fs::remove_dir_all(store);
}

#[test]
fn a_cancelled_prompt_leaves_the_client_locked() {
    let (c, store) = client(true);
    c.enrol().unwrap();
    assert!(matches!(c.unlock("k.rao".into()), Err(ClientError::Cancelled)));
    assert!(c.is_locked());
    let _ = std::fs::remove_dir_all(store);
}

#[test]
fn a_tampered_dataset_is_refused_at_open() {
    let (c, store) = client(false);
    drop(c);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut bytes = std::fs::read(repo.join(format!("clients/demo/datasets/{}.mgdemo", core_version()))).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    let bad = store.join("tampered.mgdemo");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(&bad, bytes).unwrap();
    let config = ClientConfig {
        dataset_path: bad.display().to_string(),
        store_dir: store.display().to_string(),
        replica_id: "t".into(),
        device: DeviceInfo { ram_mb: 8192, has_accelerator: true },
        demo_on_device_model: true,
    };
    assert!(matches!(
        MidgardClient::new(config, Arc::new(TestKeyRoot { device: [3; 32], cancel: false })),
        Err(ClientError::SignatureFailure { .. })
    ));
    let _ = std::fs::remove_dir_all(store);
}
```

**Step 2: Run it and see it fail.**

```bash
cargo test --offline --locked -p midgard-client-ffi
```

Expected: `error: package ID specification \`midgard-client-ffi\` did not match any packages`.

**Step 3a: The manifest and workspace entry.** `clients/ffi/Cargo.toml`:

```toml
[package]
name = "midgard-client-ffi"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version = "1.82"
description = "UniFFI bindings over midgard-client-core for the Apple (and later Android) shells: the shell calls the core and never re-implements it. [FR-C-1] [DD-6]"

[lib]
name = "midgard_client_ffi"
# `staticlib` for the XCFramework; `lib` for the Rust tests.
crate-type = ["lib", "staticlib"]
path = "src/lib.rs"

[features]
default = ["backend-fips"]
backend-fips = ["midgard-client-core/backend-fips", "midgard-client-keystore/backend-fips", "pqc-agility/backend-fips"]

[dependencies]
midgard-client-core = { path = "../core", default-features = false }
midgard-client-keystore = { path = "../keystore", default-features = false }
# Only the deterministic tier heuristic and guard: no backend feature is needed.
midgard-client-inference = { path = "../inference", default-features = false }
pqc-agility = { path = "../../crypto/pqc-agility", default-features = false }
attest = { path = "../../crypto/attest", default-features = false, features = ["backend-fips"] }
# MPL-2.0: see the exceptions in supply-chain/licences.toml. Exact pin: the Swift
# bindings are generated by the same version (examples/uniffi-bindgen.rs).
uniffi = { version = "=0.28.3", default-features = false }

[dev-dependencies]
uniffi_bindgen = { version = "=0.28.3", default-features = false }
camino = "1"
pqc-agility = { path = "../../crypto/pqc-agility", features = ["backend-fips"] }
```

In the root `Cargo.toml`, add `"clients/ffi",` after `"clients/core",` in `members`.

**Step 3b: Resolve and vendor (the only networked step in this plan).** Follow `docs/dev/BUILD_ENV.md` ("Offline / vendored dependencies"):

```bash
cp .cargo/config.toml /tmp/cargo-config.keep
# 1. comment out the [source.crates-io] and [source.vendored-sources] blocks in .cargo/config.toml
CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +stable update -p midgard-client-ffi
cargo +stable update -p smawk --precise 0.3.2
for p in proc-macro2@1.0.106 quote@1.0.46 unicode-ident@1.0.24; do cargo +stable update -p "${p%@*}" --precise "${p#*@}"; done
cargo +stable update -p syn@2.0.119 --precise 2.0.118 2>/dev/null || true   # only if a newer syn 2 appeared
head -3 Cargo.lock                                    # must still say: version = 3
cargo +1.82.0 metadata --format-version 1 --locked >/dev/null   # every manifest parses on 1.82
cargo vendor --versioned-dirs vendor > /dev/null      # 1.82's cargo; refreshes vendor/
cp /tmp/cargo-config.keep .cargo/config.toml          # 2. restore the source replacement
bash tools/ci/vendor-check.sh                          # expect: "<n> locked registry package(s), all vendored …; nothing extra."
git status --short vendor | wc -l
```

Expected: about 51 new `vendor/<crate>-<version>` directories; `vendor/advisory-db*` untouched (they have no `.cargo-checksum.json`, so `cargo vendor` leaves them). `+stable` is used only for the MSRV-aware resolver (Cargo ≥ 1.84); everything that builds uses 1.82. If `cargo +1.82.0 metadata` fails on an edition-2024 manifest, pin that crate one release back the same way as `smawk` and record it in `docs/dev/BUILD_ENV.md` beside the `zeroize`/`base64ct` notes.

**Step 3c: Licence exceptions** — append to `supply-chain/licences.toml` under `[exceptions]`, one table per crate, replacing `# (empty)`:

```toml
[exceptions."uniffi"]
licence = "MPL-2.0"
owner   = "<owner's name, as in clients/shells/audit-waivers.txt>"
reason  = "UniFFI generates the Swift bindings the Apple shell links (SAD_06 KDD-1, DD-6). MPL-2.0 is file-level copyleft: the UniFFI files are used unmodified from vendor/ and their source stays available there; no Midgard file becomes MPL. Revisit if a UniFFI file is modified or the shells move to another binding."
expires = "2027-05-13"
```

…and the same table for `uniffi_core`, `uniffi_macros`, `uniffi_meta`, `uniffi_checksum_derive`, `uniffi_bindgen`, `uniffi_udl`, `uniffi_testing` (all `MPL-2.0`, same owner, reason and expiry). Then check the exceptions parse and the scan passes on a release binary's SBOM:

```bash
cargo run --offline --locked -q --release -p mg-sbom -- --out /tmp/mg.sbom.cdx.json Cargo.lock
cargo run --offline --locked -q --release -p mg-scan -- /tmp/mg.sbom.cdx.json --out /tmp/mg-findings.json; echo "exit $?"
```

Expected: exit 0 and no `licence` finding. (Confirm `mg-sbom`'s artefact argument form with `cargo run -p mg-sbom -- --help` first; the scan is what matters.)

**Step 3d: The code.** `clients/ffi/src/lib.rs`:

```rust
//! # midgard-client-ffi — the UniFFI surface over the client core
//!
//! Swift (and later Kotlin) calls this crate; this crate calls `midgard-client-core`.
//! Nothing here holds key material, merges sync state or decides clearance: those
//! stay in the core (KDD-1). The only foreign code the core calls back is
//! [`KeyRoot`], the platform key agreement that seals the custody root.
//! `[FR-C-1]` `[DD-6]` `[AC-C-13]`

// No `#![forbid(unsafe_code)]` here: UniFFI's scaffolding and derives emit `unsafe`
// FFI glue, and `forbid` cannot be lifted for generated code. This crate writes no
// `unsafe` by hand: `grep -rn unsafe clients/ffi/src` must stay empty.

uniffi::setup_scaffolding!();

mod client;
mod error;
pub mod generated;
mod key_root;

pub use client::*;
pub use error::ClientError;
pub use key_root::{KeyAgreement, KeyRoot, KeyRootError};

/// The core version every shell links. `[AC-C-1]`
#[uniffi::export]
pub fn core_version() -> String {
    midgard_client_core::CORE_VERSION.to_string()
}
```


`clients/ffi/src/key_root.rs`:

```rust
//! The platform key root the core calls back: an ECDH agreement with a key held in
//! the Secure Enclave (Apple) or StrongBox (Android). The shell implements this trait
//! and nothing else that touches keys. `[FR-C-2]` `[DD-6]`

use std::sync::Arc;

use midgard_client_keystore::sealed_root::RootSealer;
use midgard_client_keystore::KeystoreError;
use pqc_agility::SecretBytes;

/// A fresh agreement with the platform key's public half.
#[derive(Debug, Clone, uniffi::Record)]
pub struct KeyAgreement {
    /// The ephemeral public key (X9.63 P-256 on Apple).
    pub ephemeral_public: Vec<u8>,
    /// The shared secret; zeroised by the core after use.
    pub shared_secret: Vec<u8>,
}

/// Why the platform key could not answer.
#[derive(Debug, uniffi::Error)]
pub enum KeyRootError {
    /// The user cancelled Face ID / Touch ID / passcode.
    Cancelled,
    /// No usable platform key (no Enclave, biometry not enrolled, key deleted).
    Unavailable,
    /// Anything else.
    Failed {
        /// The platform's message.
        detail: String,
    },
}

impl std::fmt::Display for KeyRootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyRootError::Cancelled => write!(f, "cancelled"),
            KeyRootError::Unavailable => write!(f, "platform key unavailable"),
            KeyRootError::Failed { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for KeyRootError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for KeyRootError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        KeyRootError::Failed { detail: e.reason }
    }
}

/// The platform key, implemented in Swift by `SecureEnclaveKeyRoot`.
#[uniffi::export(with_foreign)]
pub trait KeyRoot: Send + Sync {
    /// Create the platform key if it does not exist.
    fn ensure_key(&self) -> Result<(), KeyRootError>;
    /// Agree a fresh secret with the platform key's public half (no prompt).
    fn agree_new(&self) -> Result<KeyAgreement, KeyRootError>;
    /// The secret for `ephemeral_public`, computed by the platform key (prompts).
    fn agree_existing(&self, ephemeral_public: Vec<u8>) -> Result<Vec<u8>, KeyRootError>;
    /// Delete the platform key ("Reset demo").
    fn destroy_key(&self) -> Result<(), KeyRootError>;
    /// Whether the key is held in hardware.
    fn hardware_backed(&self) -> bool;
}

impl From<KeyRootError> for KeystoreError {
    fn from(e: KeyRootError) -> Self {
        match e {
            KeyRootError::Cancelled => KeystoreError::UserCancelled,
            KeyRootError::Unavailable => KeystoreError::PlatformUnavailable("no platform key".into()),
            KeyRootError::Failed { detail } => KeystoreError::PlatformUnavailable(detail),
        }
    }
}

/// Adapts the foreign [`KeyRoot`] to the keystore's [`RootSealer`].
pub(crate) struct Sealer(pub Arc<dyn KeyRoot>);

impl RootSealer for Sealer {
    fn ensure_key(&self) -> Result<(), KeystoreError> {
        Ok(self.0.ensure_key()?)
    }
    fn agree_new(&self) -> Result<(Vec<u8>, SecretBytes), KeystoreError> {
        let a = self.0.agree_new()?;
        Ok((a.ephemeral_public, SecretBytes::new(a.shared_secret)))
    }
    fn agree_existing(&self, ephemeral_public: &[u8]) -> Result<SecretBytes, KeystoreError> {
        Ok(SecretBytes::new(self.0.agree_existing(ephemeral_public.to_vec())?))
    }
    fn hardware_backed(&self) -> bool {
        self.0.hardware_backed()
    }
}
```

`clients/ffi/src/error.rs`:

```rust
//! Typed errors for the shells: each variant is a screen state, never a crash.
//! `[FR-C-1]` `[AC-C-13]`

use midgard_client_core::demo::dataset::DatasetError;
use midgard_client_core::session::SessionError;
use midgard_client_keystore::KeystoreError;

/// An error a binding returns.
#[derive(Debug, uniffi::Error)]
pub enum ClientError {
    /// Above the session's clearance: shown as a withheld row or a refused action.
    AboveClearance,
    /// The dataset or the sealed root failed its signature or integrity check.
    SignatureFailure {
        /// What failed.
        detail: String,
    },
    /// The source reported a conflict.
    Conflict {
        /// Why.
        detail: String,
    },
    /// The client is locked: unlock first.
    Locked,
    /// No space left to write the sealed store.
    StorageFull,
    /// The source is unreachable (or the demo is offline).
    Offline,
    /// Offline and nothing is cached for this collection.
    NothingCached {
        /// The collection.
        collection: String,
    },
    /// A persona switch while edits are unsent.
    PendingOps {
        /// How many.
        count: u32,
    },
    /// No key root yet: enrol first.
    NotEnrolled,
    /// The user cancelled the biometric prompt.
    Cancelled,
    /// The platform key failed.
    KeyRoot {
        /// The platform's message.
        detail: String,
    },
    /// Malformed data.
    Malformed {
        /// What.
        detail: String,
    },
    /// Not found.
    NotFound {
        /// What.
        what: String,
    },
    /// An internal error.
    Internal {
        /// What.
        detail: String,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ClientError {}

impl From<SessionError> for ClientError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::AboveClearance => ClientError::AboveClearance,
            SessionError::Offline => ClientError::Offline,
            SessionError::NothingCached(c) => ClientError::NothingCached { collection: c },
            SessionError::PendingOps(n) => ClientError::PendingOps { count: n as u32 },
            SessionError::Decode(d) => ClientError::Malformed { detail: d.to_string() },
            SessionError::Source(s) => ClientError::Malformed { detail: s.to_string() },
            SessionError::Core(c) => match c {
                midgard_client_core::CoreError::Store(midgard_client_core::store::StoreError::Kms(k)) => {
                    ClientError::SignatureFailure { detail: k.to_string() }
                }
                other => ClientError::Internal { detail: other.to_string() },
            },
        }
    }
}

impl From<DatasetError> for ClientError {
    fn from(e: DatasetError) -> Self {
        match e {
            DatasetError::Body(d) => ClientError::Malformed { detail: d },
            other => ClientError::SignatureFailure { detail: other.to_string() },
        }
    }
}

impl From<KeystoreError> for ClientError {
    fn from(e: KeystoreError) -> Self {
        match e {
            KeystoreError::UserCancelled => ClientError::Cancelled,
            KeystoreError::Unseal => ClientError::SignatureFailure { detail: "the sealed root did not open".into() },
            KeystoreError::Malformed(d) => ClientError::Malformed { detail: d },
            other => ClientError::KeyRoot { detail: other.to_string() },
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        // ENOSPC is 28 on Darwin and Linux. (`ErrorKind::StorageFull` is unstable on 1.82.)
        if e.raw_os_error() == Some(28) {
            ClientError::StorageFull
        } else {
            ClientError::Internal { detail: e.to_string() }
        }
    }
}
```

`clients/ffi/src/client.rs`:

```rust
//! `MidgardClient`: the one object a shell holds. `[FR-C-1]` `[AC-C-13]` `[FR-C-4]`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use attest::json::Json;
use midgard_client_core::consoles::decode::{DecodeError, FromJson};
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::marking::{MarkingLevel, RowMarking, Visible};
use midgard_client_core::session::{self, Session};
use midgard_client_core::source::DataSource;
use midgard_client_core::CORE_VERSION;
use midgard_client_inference::tier::{
    guard, heuristic, Connectivity, DeviceCapability, Heuristic, PrivacyPreference, TaskComplexity, Tier, TierContext,
};
use midgard_client_keystore::sealed_root::SealedRootKeystore;
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

use crate::key_root::{KeyRoot, Sealer};
use crate::ClientError;

// The pinned demo-dataset and demo-audit anchors are compiled into the core
// (`DemoEnclosure::open_pinned`, Task A8): a dataset signed by any other key is
// refused, and this crate cannot choose other anchors (`demo-unpinned` is off).
const ROOT_FILE: &str = "root.sealed";
const STATE_FILE: &str = "state.sealed";

/// How the shell configures the client.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ClientConfig {
    /// Path of the bundled `<release>.mgdemo`.
    pub dataset_path: String,
    /// A private directory for `root.sealed` and `state.sealed`.
    pub store_dir: String,
    /// Stable per install.
    pub replica_id: String,
    /// The device, for the inference tier.
    pub device: DeviceInfo,
    /// Demo only: treat the device as having a verified on-device model, so going
    /// offline shows the on-device tier the design describes (Open questions, Q6).
    pub demo_on_device_model: bool,
}

/// Device capability as the shell measures it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceInfo {
    /// Physical memory, MiB.
    pub ram_mb: u32,
    /// A Neural Engine / GPU is present.
    pub has_accelerator: bool,
}

/// A persona the sign-in screen lists.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PersonaInfo {
    /// Id.
    pub id: String,
    /// Display name.
    pub display: String,
    /// Role word.
    pub role: String,
    /// `CONFIDENTIAL // OPS-N, SIG`.
    pub clearance_label: String,
    /// Policy roles.
    pub roles: Vec<String>,
}

/// Marking level for chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MarkingLevelChip {
    /// open
    Open,
    /// internal
    Internal,
    /// restricted
    Restricted,
    /// confidential
    Confidential,
}

/// A row's marking chip.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MarkingChip {
    /// Level (fixed colour).
    pub level: MarkingLevelChip,
    /// Full label.
    pub label: String,
}

/// Sync phase for the chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncPhase {
    /// Nothing pending.
    Synced,
    /// Pending and reachable.
    Syncing,
    /// Unreachable.
    Offline,
    /// Last sync had a conflict.
    Conflict,
}

/// The sync chip.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncChip {
    /// Phase.
    pub phase: SyncPhase,
    /// Ops queued.
    pub pending: u32,
    /// Unix seconds of the last sync.
    pub last_sync: Option<u64>,
}

/// Inference tier for the chip (`clients/inference/src/tier.rs` `Tier`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InferenceTier {
    /// Tier 1.
    OnDevice,
    /// Tier 2.
    Remote,
    /// Tier 3.
    Hybrid,
    /// Disabled cleanly.
    Disabled,
}

/// The inference-tier chip.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TierChip {
    /// Tier.
    pub tier: InferenceTier,
    /// Why, in a phrase.
    pub reason: String,
}

/// The custody chip.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CustodyChip {
    /// The keystore's honest floor (`SecurityFloor` debug name).
    pub floor: String,
    /// Whether the root is sealed by a hardware key.
    pub hardware_wrapped: bool,
}

/// All chips a screen header shows.
#[derive(Debug, Clone, uniffi::Record)]
pub struct StatusChips {
    /// Sync.
    pub sync: SyncChip,
    /// Inference tier.
    pub tier: TierChip,
    /// Custody.
    pub custody: CustodyChip,
}

/// After unlock or switch.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SessionInfo {
    /// Who.
    pub persona: PersonaInfo,
    /// Chips.
    pub chips: StatusChips,
}

/// A row of any collection, for generic lists and the contract test.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RowSummary {
    /// Position in the collection.
    pub position: u32,
    /// `None` when withheld.
    pub id: Option<String>,
    /// Marking chip.
    pub marking: MarkingChip,
    /// Above clearance.
    pub withheld: bool,
}

/// An action to queue.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ActionInput {
    /// `<console>.<verb>`.
    pub verb: String,
    /// `<collection>/<row id>`.
    pub target: String,
    /// String parameters.
    pub params: Vec<crate::generated::KeyValue>,
}

/// An action's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ActionState {
    /// Queued or in flight.
    Queued,
    /// Applied.
    Applied,
    /// Refused.
    Refused,
    /// Conflict.
    Conflict,
    /// Unknown here.
    Unknown,
}

/// An action's status for the screen.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ActionStatus {
    /// Id.
    pub action_id: String,
    /// State.
    pub state: ActionState,
    /// Reason when refused or in conflict.
    pub reason: Option<String>,
    /// Audit id when applied.
    pub audit_id: Option<String>,
    /// Canonical JSON result when applied.
    pub result: Option<String>,
}

/// A sync's report.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncReport {
    /// Ops pushed.
    pub pushed: u32,
    /// Ops merged.
    pub merged: u32,
    /// Chips after.
    pub chips: StatusChips,
}

type Live = Session<SealedRootKeystore, DemoEnclosure>;

enum State {
    Locked(Box<DemoEnclosure>),
    Open(Box<Live>),
    Gone,
}

/// The client a shell holds: locked or signed in.
#[derive(uniffi::Object)]
pub struct MidgardClient {
    config: ClientConfig,
    key_root: Arc<dyn KeyRoot>,
    state: Mutex<State>,
}

/// A row kept as JSON, for [`RowSummary`].
struct Raw;
impl FromJson for Raw {
    fn from_json(_: &Json, _: &str) -> Result<Self, DecodeError> {
        Ok(Raw)
    }
}

fn chip(m: &RowMarking) -> MarkingChip {
    MarkingChip { level: level_chip(m.level), label: m.label() }
}

fn level_chip(l: MarkingLevel) -> MarkingLevelChip {
    match l {
        MarkingLevel::Open => MarkingLevelChip::Open,
        MarkingLevel::Internal => MarkingLevelChip::Internal,
        MarkingLevel::Restricted => MarkingLevelChip::Restricted,
        MarkingLevel::Confidential => MarkingLevelChip::Confidential,
    }
}

fn persona_info(p: &midgard_client_core::source::Persona) -> PersonaInfo {
    PersonaInfo {
        id: p.id.clone(),
        display: p.display.clone(),
        role: p.role.clone(),
        clearance_label: RowMarking::new(p.clearance, p.compartments.iter().cloned()).label(),
        roles: p.roles.clone(),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ClientError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Read and verify the bundled dataset against the anchors compiled into the core.
fn load_source(config: &ClientConfig) -> Result<DemoEnclosure, ClientError> {
    let file = std::fs::read(&config.dataset_path)
        .map_err(|e| ClientError::NotFound { what: format!("dataset {}: {e}", config.dataset_path) })?;
    Ok(DemoEnclosure::open_pinned(&file)?)
}

impl MidgardClient {
    fn dir(&self) -> PathBuf {
        PathBuf::from(&self.config.store_dir)
    }

    fn chips(&self, s: &Live) -> StatusChips {
        let st = s.sync_status();
        let online = s.source().online();
        let ctx = TierContext {
            capability: DeviceCapability {
                ram_mb: self.config.device.ram_mb,
                has_accelerator: self.config.device.has_accelerator,
                on_device_model: self.config.demo_on_device_model,
            },
            connectivity: if online { Connectivity::Online } else { Connectivity::Offline },
            allow_egress: true,
            privacy: PrivacyPreference::Standard,
            complexity: TaskComplexity::Light,
        };
        let proposed = match heuristic(&ctx) {
            Heuristic::Decided(t) => t,
            Heuristic::Ambiguous => Tier::Remote,
        };
        let (tier, reason) = match guard(proposed, &ctx) {
            Tier::OnDevice => (InferenceTier::OnDevice, if online { "on-device" } else { "offline: on-device" }),
            Tier::Remote => (InferenceTier::Remote, "enclosure models"),
            Tier::Hybrid => (InferenceTier::Hybrid, "on-device draft, enclosure check"),
            Tier::Disable => (InferenceTier::Disabled, "no safe tier"),
        };
        StatusChips {
            sync: SyncChip {
                phase: match st.phase {
                    session::SyncPhase::Synced => SyncPhase::Synced,
                    session::SyncPhase::Syncing => SyncPhase::Syncing,
                    session::SyncPhase::Offline => SyncPhase::Offline,
                    session::SyncPhase::Conflict => SyncPhase::Conflict,
                },
                pending: st.pending as u32,
                last_sync: st.last_sync,
            },
            tier: TierChip { tier, reason: reason.to_string() },
            custody: CustodyChip { floor: "NonExtractable".into(), hardware_wrapped: self.key_root.hardware_backed() },
        }
    }

    fn with_session<T>(&self, f: impl FnOnce(&mut Live) -> Result<T, ClientError>) -> Result<T, ClientError> {
        let mut st = self.state.lock().map_err(|_| ClientError::Internal { detail: "poisoned".into() })?;
        match &mut *st {
            State::Open(s) => f(s),
            _ => Err(ClientError::Locked),
        }
    }

    fn save(&self, s: &mut Live) -> Result<(), ClientError> {
        let bytes = s.export_state()?;
        write_private(&self.dir().join(STATE_FILE), &bytes)
    }
}

#[uniffi::export]
impl MidgardClient {
    /// Verify the bundled dataset against the pinned anchor and start locked.
    #[uniffi::constructor]
    pub fn new(config: ClientConfig, key_root: Arc<dyn KeyRoot>) -> Result<Arc<Self>, ClientError> {
        let source = load_source(&config)?;
        std::fs::create_dir_all(&config.store_dir)?;
        Ok(Arc::new(Self { config, key_root, state: Mutex::new(State::Locked(Box::new(source))) }))
    }

    /// The personas the sign-in screen lists.
    pub fn personas(&self) -> Vec<PersonaInfo> {
        match &*self.state.lock().expect("state") {
            State::Locked(src) => src.personas().iter().map(persona_info).collect(),
            State::Open(s) => s.source().personas().iter().map(persona_info).collect(),
            State::Gone => Vec::new(),
        }
    }

    /// Whether a sealed root exists.
    pub fn is_enrolled(&self) -> bool {
        self.dir().join(ROOT_FILE).exists()
    }

    /// First use: create the platform key and the sealed custody root.
    pub fn enrol(&self) -> Result<(), ClientError> {
        let sealer = Sealer(self.key_root.clone());
        let (_ks, blob) = SealedRootKeystore::create(&provider(), "device-root", &sealer, &mut OsRng)?;
        write_private(&self.dir().join(ROOT_FILE), &blob)
    }

    /// Unlock (prompts through the key root) and sign `persona_id` in.
    pub fn unlock(&self, persona_id: String) -> Result<SessionInfo, ClientError> {
        let root = std::fs::read(self.dir().join(ROOT_FILE)).map_err(|_| ClientError::NotEnrolled)?;
        let mut st = self.state.lock().map_err(|_| ClientError::Internal { detail: "poisoned".into() })?;
        let State::Locked(_) = &*st else { return Err(ClientError::Internal { detail: "already unlocked".into() }) };
        let ks = SealedRootKeystore::open(&root, &Sealer(self.key_root.clone()))?;
        let State::Locked(src) = std::mem::replace(&mut *st, State::Gone) else { unreachable!() };
        let opened = Session::open(ks, *src, &persona_id, &self.config.replica_id).map_err(ClientError::from).and_then(|mut s| {
            if let Ok(saved) = std::fs::read(self.dir().join(STATE_FILE)) {
                s.import_state(&saved)?;
            }
            Ok(s)
        });
        let s = match opened {
            Ok(s) => s,
            Err(e) => {
                // The source moved into the failed session: load it again so the client
                // stays usable, locked.
                *st = State::Locked(Box::new(load_source(&self.config)?));
                return Err(e);
            }
        };
        let info = SessionInfo { persona: persona_info(&s.grant().persona), chips: self.chips(&s) };
        *st = State::Open(Box::new(s));
        Ok(info)
    }

    /// Switch persona without re-prompting. Refused while edits are unsent.
    pub fn switch_persona(&self, persona_id: String) -> Result<SessionInfo, ClientError> {
        let mut st = self.state.lock().map_err(|_| ClientError::Internal { detail: "poisoned".into() })?;
        let State::Open(s) = std::mem::replace(&mut *st, State::Gone) else { return Err(ClientError::Locked) };
        match s.switch_persona(&persona_id) {
            Ok(mut s) => {
                self.save(&mut s)?;
                let info = SessionInfo { persona: persona_info(&s.grant().persona), chips: self.chips(&s) };
                *st = State::Open(Box::new(s));
                Ok(info)
            }
            Err((s, e)) => {
                *st = State::Open(Box::new(s));
                Err(e.into())
            }
        }
    }

    /// Lock: write the sealed state, drop the unlocked root.
    pub fn lock(&self) -> Result<(), ClientError> {
        let mut st = self.state.lock().map_err(|_| ClientError::Internal { detail: "poisoned".into() })?;
        if let State::Open(mut s) = std::mem::replace(&mut *st, State::Gone) {
            let saved = self.save(&mut s);
            *st = State::Locked(Box::new(s.into_source()));
            saved?;
        }
        Ok(())
    }

    /// Whether locked.
    pub fn is_locked(&self) -> bool {
        !matches!(&*self.state.lock().expect("state"), State::Open(_))
    }

    /// The header chips.
    pub fn status_chips(&self) -> Result<StatusChips, ClientError> {
        self.with_session(|s| Ok(self.chips(s)))
    }

    /// Simulate going offline (or back online).
    pub fn set_offline(&self, offline: bool) -> Result<StatusChips, ClientError> {
        self.with_session(|s| {
            s.set_online(!offline);
            Ok(self.chips(s))
        })
    }

    /// Push queued edits and merge.
    pub fn sync_now(&self) -> Result<SyncReport, ClientError> {
        self.with_session(|s| {
            let r = s.sync()?;
            self.save(s)?;
            Ok(SyncReport { pushed: r.pushed as u32, merged: r.merged as u32, chips: self.chips(s) })
        })
    }

    /// Any collection as summaries (position, id, marking, withheld).
    pub fn collection_rows(&self, collection: String) -> Result<Vec<RowSummary>, ClientError> {
        self.with_session(|s| {
            Ok(s.rows::<Raw>(&collection)?
                .into_iter()
                .enumerate()
                .map(|(i, v)| match v {
                    Visible::Shown { id, marking, .. } => {
                        RowSummary { position: i as u32, id: Some(id), marking: chip(&marking), withheld: false }
                    }
                    Visible::Withheld { level } => RowSummary {
                        position: i as u32,
                        id: None,
                        marking: MarkingChip { level: level_chip(level), label: level.as_str().to_uppercase() },
                        withheld: true,
                    },
                })
                .collect())
        })
    }

    /// Queue an action on a visible row. The row's marking is looked up here, not
    /// taken from the shell.
    pub fn request_action(&self, action: ActionInput) -> Result<String, ClientError> {
        self.with_session(|s| {
            let (collection, id) = action
                .target
                .split_once('/')
                .ok_or_else(|| ClientError::Malformed { detail: "target must be <collection>/<id>".into() })?;
            let marking = s
                .rows::<Raw>(collection)?
                .into_iter()
                .find_map(|v| match v {
                    Visible::Shown { id: rid, marking, .. } if rid == id => Some(marking),
                    _ => None,
                })
                .ok_or(ClientError::AboveClearance)?;
            let params: Vec<(String, String)> = action.params.into_iter().map(|kv| (kv.key, kv.value)).collect();
            let id = s.request_action(&action.verb, &action.target, &params, &marking)?;
            self.save(s)?;
            Ok(id)
        })
    }

    /// An action's state.
    pub fn action_status(&self, action_id: String) -> Result<ActionStatus, ClientError> {
        self.with_session(|s| {
            let st = s.action_state(&action_id);
            let (state, reason, audit_id, result) = match st {
                session::ActionState::Queued => (ActionState::Queued, None, None, None),
                session::ActionState::Applied { audit, result } => (ActionState::Applied, None, Some(audit), Some(result)),
                session::ActionState::Refused { reason } => (ActionState::Refused, Some(reason), None, None),
                session::ActionState::Conflict { reason } => (ActionState::Conflict, Some(reason), None, None),
                session::ActionState::Unknown => (ActionState::Unknown, None, None, None),
            };
            Ok(ActionStatus { action_id, state, reason, audit_id, result })
        })
    }

    /// "Reset demo": lock, delete the sealed state and root, delete the platform key.
    pub fn reset_demo(&self) -> Result<(), ClientError> {
        self.lock()?;
        for f in [STATE_FILE, ROOT_FILE] {
            let _ = std::fs::remove_file(self.dir().join(f));
        }
        self.key_root.destroy_key().map_err(|e| ClientError::KeyRoot { detail: e.to_string() })
    }
}
```

Two behaviours to note: (1) a failed `Session::open` inside `unlock` consumes the source, so `unlock` reloads and re-verifies it (`load_source`) and stays locked; add a test that unlocks an unknown persona and then unlocks a known one. (2) `with_session` holds the mutex for the whole call; a Swift caller must call the client off the main thread (Task A13 does this through an actor).

`clients/ffi/src/generated/mod.rs` (hand-written; the modules are generated):

```rust
//! UniFFI mirrors of the console read models (generated) and the one shared helper
//! type. `[FR-C-1]` `[DD-6]`

/// One key/value pair of a TypeScript `Record<string, string>`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KeyValue {
    /// Key.
    pub key: String,
    /// Value.
    pub value: String,
}

pub mod admin;
pub mod bifrost;
pub mod common;
pub mod forge;
pub mod heimdall;
pub mod midgard;
pub mod mimir;
```

Generate the mirrors and add the `--ffi-out` form to the drift test:

```bash
node clients/demo/tools/gen-read-models.mjs --write --ffi-out clients/ffi/src/generated
sed -i '' 's#node clients/demo/tools/gen-read-models.mjs --check#node clients/demo/tools/gen-read-models.mjs --check --ffi-out clients/ffi/src/generated#' tools/ci/tests/demo-fixtures.test.sh
```

`clients/ffi/examples/uniffi-bindgen.rs`:

```rust
//! Generate the Swift bindings from the built static library (library mode).
//! [DD-6] [FR-C-1]
//!
//!   cargo run -p midgard-client-ffi --example uniffi-bindgen -- --library <lib.a> --out <dir>
//!
//! Writes midgard_client_ffi.swift, midgard_client_ffiFFI.h and module.modulemap
//! (module `midgard_client_ffiFFI`, XCFramework form).

use camino::Utf8PathBuf;
use uniffi_bindgen::bindings::{generate_swift_bindings, SwiftBindingsOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| {
            eprintln!("usage: uniffi-bindgen --library <lib.a> --out <dir>");
            std::process::exit(2)
        })
    };
    generate_swift_bindings(SwiftBindingsOptions {
        generate_swift_sources: true,
        generate_headers: true,
        generate_modulemap: true,
        library_path: Utf8PathBuf::from(get("--library")),
        out_dir: Utf8PathBuf::from(get("--out")),
        xcframework: true,
        module_name: Some("midgard_client_ffiFFI".into()),
        modulemap_filename: Some("module.modulemap".into()),
        metadata_no_deps: true,
    })
    .unwrap_or_else(|e| {
        eprintln!("uniffi-bindgen: {e:#}");
        std::process::exit(1)
    });
}
```

**Step 3e: CI recipes.** In `justfile`, add to `lint` (after the client group at line 145):

```just
    cargo clippy -p midgard-client-ffi --features backend-fips --all-targets -- -D warnings
```

and to `test` (after `cargo test -p midgard-client-core --features backend-fips`):

```just
    cargo test -p midgard-client-ffi --features backend-fips
```

Then update the crate count quoted in `README.md` line 9 and `GETTING_STARTED.md` line 5 from "70-crate" to "71-crate"; `numbers.sh --check` sweeps both files and would otherwise fail.

**Step 4: Run it and see it pass.**

```bash
cargo test --offline --locked -p midgard-client-ffi --features backend-fips
cargo clippy --offline --locked -p midgard-client-ffi --features backend-fips --all-targets -- -D warnings
bash tools/ci/no-test-backend.sh                     # the workspace graph still never enables test-backend
bash tools/ci/release-artefacts.sh --check            # still 18 binaries
bash tools/ci/tests/demo-fixtures.test.sh
```

Expected: `3 passed` (plus any you added); no-test-backend passes; 18 binaries.

**Step 5: Commit** (two commits keep the vendor diff reviewable on its own):

```bash
git add vendor Cargo.lock
git commit -m "vendor: uniffi 0.28.3 and its closure for the client bindings [DD-6]"
git add Cargo.toml clients/ffi supply-chain/licences.toml justfile README.md GETTING_STARTED.md \
        tools/ci/tests/demo-fixtures.test.sh
bash tools/ci/numbers.sh --write
git add docs/compliance/NUMBERS.md
git commit -m "clients/ffi: UniFFI surface over the client core, typed errors, key-root callback [FR-C-1] [DD-6]"
```

(The first commit leaves `Cargo.lock` naming a member that the tree does not yet have; if `vendor-check.sh` or a bisect needs every commit buildable, commit both together instead.)

---

### Task A12: XCFramework build script

**Files:**
- Create: `clients/shells/apple/tools/build-xcframework.sh`, `clients/shells/apple/tools/clean.sh`
- Create: `clients/shells/apple/.gitignore`
- Test: `tools/ci/tests/apple-xcframework.test.sh` (static checks only; the real build runs on a Mac)

**Step 1: Write the failing test** — `tools/ci/tests/apple-xcframework.test.sh`:

```bash
#!/usr/bin/env bash
# The XCFramework script builds the four Apple slices offline and locked, writes only
# under ~/Library/Caches/midgard-build/clients-*, and stays bash 3.2 safe. [DD-6]
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 2
f=clients/shells/apple/tools/build-xcframework.sh
failures=0
check() { if eval "$2"; then echo "ok    $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi; }
check "script exists" "[ -f $f ]"
for t in aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim; do
    check "builds $t" "grep -q '$t' $f"
done
check "cargo is offline and locked" "! grep -E '^[^#]*cargo (build|run)' $f | grep -vq -- '--offline --locked'"
check "outputs under clients-*" "grep -q 'midgard-build/clients-' $f"
check "no bash-4 builtins" "! grep -Eq 'mapfile|readarray|declare -A' $f"
check "parses under /bin/bash" "/bin/bash -n $f"
[ "$failures" -eq 0 ] || exit 1
```

**Step 2: Run it and see it fail.** `bash tools/ci/tests/apple-xcframework.test.sh` → `FAIL  script exists` and more.

**Step 3: Implement.** `clients/shells/apple/tools/build-xcframework.sh`:

```bash
#!/usr/bin/env bash
# Build MidgardCoreFFI.xcframework and the Swift bindings from clients/ffi.
# [DD-6] [FR-C-1]
#
#   bash clients/shells/apple/tools/build-xcframework.sh [--keep-target]
#
# Slices: macOS (arm64 + x86_64, lipo'd), iOS device (arm64), iOS simulator (arm64).
# Cargo builds into ~/Library/Caches/midgard-build/clients-target, deleted on exit
# unless --keep-target; the XCFramework and bindings go to
# ~/Library/Caches/midgard-build/clients-xcframework and are linked into the package.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
target_dir="$cache/clients-target"
out="$cache/clients-xcframework"
keep=0
[ "${1:-}" = --keep-target ] && keep=1
cleanup() { [ "$keep" = 1 ] || rm -rf "$target_dir"; rm -rf "$out/staging"; }
trap cleanup EXIT

export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export CARGO_TARGET_DIR="$target_dir"
export MACOSX_DEPLOYMENT_TARGET=14.0 IPHONEOS_DEPLOYMENT_TARGET=17.0
lib=libmidgard_client_ffi.a
targets="aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim"
for t in $targets; do
    if ! rustup target list --installed | grep -qx "$t"; then
        echo "build-xcframework: missing target $t (rustup target add $t --toolchain 1.82.0)" >&2
        exit 2
    fi
done
for t in $targets; do
    cargo build --offline --locked --release -p midgard-client-ffi --target "$t"
done

rm -rf "$out"
mkdir -p "$out/staging/macos" "$out/staging/headers" "$out/swift"
lipo -create "$target_dir/aarch64-apple-darwin/release/$lib" "$target_dir/x86_64-apple-darwin/release/$lib" \
    -output "$out/staging/macos/$lib"
cargo run --offline --locked -q -p midgard-client-ffi --example uniffi-bindgen -- \
    --library "$target_dir/aarch64-apple-darwin/release/$lib" --out "$out/swift"
cp "$out/swift/midgard_client_ffiFFI.h" "$out/swift/module.modulemap" "$out/staging/headers/"
xcodebuild -create-xcframework \
    -library "$out/staging/macos/$lib" -headers "$out/staging/headers" \
    -library "$target_dir/aarch64-apple-ios/release/$lib" -headers "$out/staging/headers" \
    -library "$target_dir/aarch64-apple-ios-sim/release/$lib" -headers "$out/staging/headers" \
    -output "$out/MidgardCoreFFI.xcframework" >/dev/null

# Link into the Swift package (both paths are in clients/shells/apple/.gitignore).
apple=clients/shells/apple
mkdir -p "$apple/Frameworks" "$apple/Sources/MidgardCore"
ln -sfn "$out/MidgardCoreFFI.xcframework" "$apple/Frameworks/MidgardCoreFFI.xcframework"
cp "$out/swift/midgard_client_ffi.swift" "$apple/Sources/MidgardCore/midgard_client_ffi.swift"
echo "build-xcframework: $out/MidgardCoreFFI.xcframework ($(du -sh "$out" | cut -f1))"
```

`clients/shells/apple/tools/clean.sh`:

```bash
#!/usr/bin/env bash
# Delete every local Apple-client build output. [DD-6]
set -euo pipefail
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
rm -rf "$cache"/clients-target "$cache"/clients-xcframework "$cache"/clients-derived "$cache"/clients-archive
cd "$(dirname "$0")/.."
rm -rf Frameworks Sources/MidgardCore/midgard_client_ffi.swift .build
echo "clean: done"
```

`clients/shells/apple/.gitignore`:

```gitignore
# Build outputs linked in by tools/build-xcframework.sh; never committed.
/Frameworks/
/Sources/MidgardCore/midgard_client_ffi.swift
/.build/
/.swiftpm/
*.xcresult
```

**Step 4: Run it and see it pass**, then build once for real (about 4 release builds; the cargo target directory is deleted at the end):

```bash
chmod +x clients/shells/apple/tools/*.sh
bash tools/ci/tests/apple-xcframework.test.sh
bash clients/shells/apple/tools/build-xcframework.sh
ls ~/Library/Caches/midgard-build/clients-xcframework/MidgardCoreFFI.xcframework
grep -c 'public func\|public init\|open func' clients/shells/apple/Sources/MidgardCore/midgard_client_ffi.swift
```

Expected: the test prints only `ok`; the XCFramework lists `ios-arm64`, `ios-arm64-simulator`, `macos-arm64_x86_64`; the Swift file exists. If SwiftPM later (A13) refuses the symlinked `Frameworks/MidgardCoreFFI.xcframework` as a binary target, replace the `ln -sfn` with `rm -rf … && cp -R …` (the copy is gitignored and removed by `clean.sh`). Check free disk first (`df -h ~`): four release builds of the crypto stack need roughly 2–3 GB of target directory while running.

**Step 5: Commit.**

```bash
git add clients/shells/apple/tools clients/shells/apple/.gitignore tools/ci/tests/apple-xcframework.test.sh
git commit -m "clients/shells/apple: XCFramework build for macOS, iOS and the simulator [DD-6]"
```

---

### Task A13: `MidgardKit` Swift package

**As built (16 Nov).** The steps below and the "Changed by" notes are kept as written for the record; the code differs as follows.

1. *Package.* `clients/shells/apple/Package.swift` as in Step 3a, without `MidgardAuditKit` (Task A14 adds the target and product). The test target depends on `MidgardKit` and `MidgardCore`. `Sources/MidgardCore/.gitkeep` is committed; the bindings and the framework link stay gitignored (A12). Every `swift build`/`swift test` runs with `--scratch-path ~/Library/Caches/midgard-build/clients-swift`, which `clean.sh` now removes.
2. *Key roots* (`Sources/MidgardKit/KeyRoot/`). `SecureEnclaveKeyRoot` uses the Security framework, not CryptoKit's `SecureEnclave.P256.KeyAgreement`: a permanent Enclave key (application tag `io.veldris.midgard.key-root.device-root`, data-protection keychain, `.privateKeyUsage` + `.userPresence`, `WhenUnlockedThisDeviceOnly`) made on the first `seal`; `seal` is `SecKeyCreateEncryptedData(.eciesEncryptionCofactorVariableIVX963SHA256AESGCM)` and refuses any length but 113; `unseal` refuses a wrong length or a first byte other than `0x04` as `Malformed` before any prompt, then `SecKeyCreateDecryptedData` with an `LAContext` carrying the prompt text. `SoftwareKeyRoot` (simulator, no Enclave, or no access group) seals with AES-256-GCM (60 bytes, fixed AAD) under a key in a `ThisDeviceOnly` keychain item, asks `DeviceOwnerPresence` (`.deviceOwnerAuthentication`) before unsealing, and carries a label ("Software key (simulator)", "(no Secure Enclave)", "(unsigned build)"). Both implement `PlatformKeyRoot` (the generated `KeyRoot` plus `custodyLabel` and `destroyKey()`, which Reset demo needs). `KeyRootErrors` maps `errSecUserCanceled` (-128) and `LAError.userCancel` (-2) — and also `systemCancel` (-4) and `appCancel` (-9) — to `UserCancelled`; other LocalAuthentication codes and missing-key/entitlement statuses to `PlatformUnavailable`; any other decryption failure to `Unseal`. Details name only the error domain and code.
3. *Choosing the root.* An unsigned process reads the data-protection keychain as "not found" but gets `errSecMissingEntitlement` (-34018) on any write, so `KeyRootFactory` probes by adding and deleting an empty generic-password item — only at enrolment (no sealed root yet). The Enclave is chosen only when that write succeeds on a device with one; the simulator and Macs without an Enclave get the labelled software root. `errSecMissingEntitlement` is `KeyRootFactory.Failure.unsignedBuild` ("This build is not signed for the keychain, so it cannot keep keys"): an unsigned build cannot keep keys at all, since the state-file key and generations need the same keychain, so there is no "unsigned build" software root. Any other probe failure (a locked device) is `keychainUnavailable`, never a silent downgrade. (Review round, below.)
4. *Wiping.* `seal` zeroes the storage of the `Data` it was handed (`memset_s` through its buffer, no copy first); `unseal` returns a `Data` with a custom deallocator that zeroes it when released, and the decrypted `CFData` (Enclave) or opened `Data` (software) is zeroed too. Keys read from or written to the keychain (software root key, state-naming key) are zeroed the same way in the buffer the keychain returned or the one handed to it. Not wiped: the copies the generated code makes (the `[UInt8]`/`RustBuffer` it lowers into, and the buffer it lifts the data key from before `seal`) — Risks, 17.
5. *State files* (`Sources/MidgardKit/Storage/`). `PersonaStateStore` (not in the file list above) keeps `root.sealed`, `device-audit.sealed` and `<hmac>.state` per persona, and each persona's generation in keychain service `io.veldris.midgard.state-generation` (account = the same HMAC, 8 bytes big-endian); the naming key is service `io.veldris.midgard.state-names`. `AtomicFile` writes a 0600 temp file in the same directory, `fsync`s, renames and `fsync`s the directory (complete file protection on iOS). `discard` deletes the file and keeps the generation. When a persona has a kept generation but no file, sign-in still calls `importState(file: Data(), minGeneration:)` so the core numbers its next export above it (the store raises its generation even when the import fails, `clients/store/src/lib.rs` `import_sealed`); a test shows the keychain value would otherwise go backwards. `KeychainSecretStore` never falls back to the legacy macOS keychain.
6. *Gateway.* `CoreGateway` is a class over a private serial `DispatchQueue`, not an actor: `unlock` blocks while the platform prompts, which would hold a thread of the Swift concurrency pool. `SessionController` (new) does the ordered steps on that queue — unlock then restore, export then write then generation, persist then lock, persist then switch — and depends on the generated `MidgardClientProtocol`, so tests can wrap the real client.
7. *AppModel.* `start(AppEnvironment)` (dataset, store directory, secret store, key root, device, replica id, client factory; `AppEnvironment.live(reason:device:)` for the apps). Enrolment is the shell's `root.sealed` (the core has no `isEnrolled`). It persists after reads, actions and syncs; tells the user when a state file was a rollback, unreadable, or restored without its unsent changes (`pendingDropped`); keeps the last persona in memory only (no persona id in `UserDefaults`). `ErrorText` follows the A11 `ClientError` cases and keeps platform details off the screen. `IdleLock` uses a cancellable `Task` sleep instead of a `Timer`.
8. *Presentation and views.* Added `StatusChipsPresentation` (offline is "AI OFF", owner decision 6), `RowPresentation` (withheld: "Withheld — above your clearance" and the level only), `SignatureBadge` (a check mark only for `verified`/`device`; the core's "not checked (demo)" claims shown as such and any other check mark stripped from free text; Mimir's provenance "verified" shown as "verified (demo label)", never a check — owner decision 11 and A8's amendment), `DemoBanner` (the web consoles' `DEMONSTRATION_NOTICE` word for word, owner decision 10), `Acknowledgements` (the UniFFI 0.28.3 MPL-2.0 line and the three OFL fonts), and the views `ConsoleRowView`, `SignatureBadgeView`, `DemoBannerView`, `AcknowledgementsView`, `EnrolView` and `SessionRootView` beside the planned ones. The offline coastline (owner decision 7) belongs to the Operating Picture and is left to Phase E.
9. *Tokens.* `Tokens.swift` imports `CoreGraphics` (not `SwiftUI`); `.mode-light` aliases resolve against the light scope; `--check` also fails on unexpected files in `Resources/Fonts`. `MGFonts` exposes the family names the font files declare (Oxanium, Space Grotesk, JetBrains Mono), and a test checks each resolves to its bundled family.
10. *Contract.* `ContractTests` uses the generated names (`enrol`, `unlock(keys:personaId:)`, `exportState`, `importState(file:minGeneration:)`, `collections`, `sessionPersona`, …); `check-contract.sh` also requires every Rust object constructor (`MidgardClient(config:keyRoot:)`): 22 in all. The `Stale` check comes after the device-signed audit row because a failed import empties the session's cache and device record.
11. *iOS.* The iOS 26.2 platform is not installed on the build Mac, so `xcodebuild -destination 'generic/platform=iOS Simulator'` refuses (Step 4). New `tools/typecheck-ios.sh` typechecks the bindings and `MidgardKit` with `swiftc` against the iPhoneOS and iPhoneSimulator SDKs (`arm64-apple-ios17.0` and `-simulator`), so every `#if os(iOS)` branch is compiled; it does not build, link or run. `apple-xcframework.test.sh` holds both new scripts to bash 3.2.
12. *Tests* (macOS, `swift test`): 69 at first; 85 after the review rounds — AppModel 19, Contract 2, IdleLock 3, KeyRoot 26, Presentation 12, SourceHygiene 1 (no logging in `MidgardKit`), StateStore 17, Tokens 4, ViewSmoke 1.
13. *Review round (16 Nov).* (a) `SignatureBadge` strips check marks per Unicode scalar (U+2713, U+2714, U+2705, U+2611, U+1F5F8, U+1F5F9 and the emoji selector U+FE0F), so "✔️" and "☑️" cannot survive as one `Character`. (b) Once enrolled, the root is never re-chosen by a probe: `SessionController.enrol` records `PlatformKeyRoot.kind` ("enclave"/"software") in keychain service `io.veldris.midgard.key-root.choice`; `KeyRootFactory.make(reason:secrets:sealedRoot:platform:)` follows that record, cross-checked with the sealed root's shape (`KeyRootKind(sealedRoot:)`: a 113-byte sealed data key is the Enclave, 60 the software root, read from the `MGSROOT1` layout), and refuses a contradiction (`mismatch`). Enrolment records the kind before it saves the keys (a failed record saves no keys), so a sealed root with no record is always refused as `mismatch(stored: nil, sealed: <shape>)` — never followed, so a planted software-shaped blob cannot downgrade an Enclave device — and the user is sent to Reset demo. Reset demo forgets the record. (c) If a persona's kept generation cannot be read, sign-in (and a switch) fails with `SessionController.Failure.keychainUnavailable`, the client is locked again and the file kept; it never imports with 0. A state file that exists but cannot be read now (for example file protection while the device is locked) is kept and sign-in fails with the retryable `stateFileUnavailable`; only a file that reads but does not import is discarded. `PersonaStateStore.save` reads the kept generation first and stores `max(kept, new)`; a naming key of the wrong length is `namingKeyDamaged`, never regenerated. (d) `AtomicFile` flushes with `F_FULLFSYNC` (falling back to `fsync`) and reports a failed directory flush. (e) Reset demo runs every step (core reset, files and generations, platform key, recorded choice) even when one fails, and reports the first error. (f) `AppModel.start(making:)` builds the environment off the main thread and turns a failure (for example `unsignedBuild`) into the failed screen state.

**Changed by A9 — key root and state file.** The Swift key root implements `seal`/`unseal` of the core's 32-byte data key, not a key agreement (the `agree*` code below is superseded):
- Create the Enclave P-256 key on the first `seal`, with access control `.privateKeyUsage` + `.userPresence` (not `.biometryCurrentSet`, which would lose the data after a biometry change). Seal with `SecKeyCreateEncryptedData(publicKey, .eciesEncryptionCofactorVariableIVX963SHA256AESGCM, …)` (no prompt) and assert the result is 113 bytes; unseal with `SecKeyCreateDecryptedData` (prompts).
- Wipe Swift's `Data` copy of the data key after handing it over (`withUnsafeMutableBytes` + `memset_s`); it cannot be wiped reliably, so keep it as short-lived as possible.
- Map `errSecUserCanceled` (-128) and `LAError.userCancel` (-2) to `UserCancelled`.
- On macOS an Enclave key needs the data-protection keychain (`kSecUseDataProtectionKeychain`) and the `keychain-access-groups` entitlement in a signed build; without them use the software key root and report `hardwareBacked() == false` truthfully.
- Keep the latest state-file generation (`ExportedState.generation`) **per persona, that is per state file**, never as one device-wide value: one `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` keychain item per persona, whose `kSecAttrAccount` is `HMAC-SHA-256(device-local key, persona id)` in hex, so no persona name is stored in plaintext. The device-local key is 32 random bytes made on first launch and kept in its own `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` keychain item (never in a file, never exported). A bare SHA-256 would not do: the persona set is small and known (four demo personas, later a directory), so a plain digest is reversed by hashing each candidate. Pass that persona's value to `importState` as `minGeneration`; on `Stale` or any other import error, delete that persona's file and refetch. Why per persona: with one shared value, persona A exports generation 5 with unsent ops, the app is killed, the relaunch signs in as B and B exports generation 6; switching back to A would then pass 6 and see A's generation-5 file as `Stale`, discarding A's unsent ops. With a value per persona, A's item still says 5 and A's file opens. Name each state file by the same HMAC, never by the persona id or a plain digest of it, and create the root with the constant key id `device-root`.
- Write order on every export: write the state file atomically first (write a temp file in the same directory, `fsync`, then rename it over the old file), and only after the rename succeeds update that persona's keychain generation. Never the other way round: a keychain value ahead of the file on disk makes the next launch refuse the only file it has as `Stale`.
- Open the sealed root once per unlock (each open prompts).

**Changed by A11 — calling the client.** Never call the `MidgardClient` from inside the `KeyRoot` implementation, and call `unlock` (which waits on the `unseal` prompt) off the main thread; `CoreGateway` does both through its actor. The Swift side also wipes its own `Data` copy of the data key after `seal` returns and after handing `unseal`'s result back (the Rust side cannot wipe the copies UniFFI makes: Risks, 17).

**Changed by A11 — acknowledgements.** The generated `midgard_client_ffi.swift` and `midgard_client_ffiFFI.h` are MPL-2.0 files (generated by UniFFI). Keep them unmodified (regenerate, never hand-edit), and add an acknowledgements entry to both apps (Settings → Acknowledgements, and the release notes' third-party list): "UniFFI 0.28.3, Mozilla Public License 2.0, source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3". The release checklist (A15) checks that the entry is present and that the bindings match a fresh generation.

**Layout decision.** `tools/ci/run.sh` (`job_shell_audit`, the `apple)` branch) accepts the Apple shell as buildable when `clients/shells/apple/Package.swift` **or** `clients/shells/apple/*.xcodeproj` exists. So the package is rooted at `clients/shells/apple/` itself, and the two apps (Phase B) live in `clients/shells/apple/Apps/MidgardMac/` and `clients/shells/apple/Apps/MidgardiOS/`, each with its own project referencing the package at `../..`. The existing `manifest.toml` (`links_core = "midgard-client-core"`, `binding = "uniffi"`) and `accessibility.toml` stay as they are, so `verify-shells-accessibility.sh` keeps passing.

**Names UniFFI 0.28.3 gives the Swift side** (from `uniffi_bindgen-0.28.3/src/bindings/swift/gen_swift/mod.rs`, `object_names`): a `with_foreign` trait becomes a Swift `protocol KeyRoot` (the Rust-backed wrapper is `KeyRootImpl`); a plain object becomes `class MidgardClient` with `protocol MidgardClientProtocol`; methods and fields are lowerCamelCase (`agreeExisting(ephemeralPublic:)`, `collectionRows(collection:)`); `Vec<u8>` is `Data`; the constructor named `new` is the class's `init`. Check these against the generated `midgard_client_ffi.swift` before writing the Swift below, and follow the file if it differs.

**Swift language mode.** `swift-tools-version:5.10` keeps every target in Swift 5 mode under the Swift 6.2.3 compiler: UniFFI 0.28's generated Swift predates strict concurrency checking and does not compile cleanly in Swift 6 mode.

**Files:**
- Create: `clients/shells/apple/Package.swift`
- Create: `clients/shells/apple/Sources/MidgardCore/.gitkeep` (the generated bindings land beside it)
- Create: `clients/shells/apple/Sources/MidgardKit/KeyRoot/{SecureEnclaveKeyRoot.swift,SoftwareKeyRoot.swift,KeyRootFactory.swift}`
- Create: `clients/shells/apple/Sources/MidgardKit/DesignSystem/{Tokens.swift (generated),Color+Midgard.swift,Fonts.swift}`, `clients/shells/apple/Sources/MidgardKit/Resources/Fonts/*` (copied by the generator)
- Create: `clients/shells/apple/Sources/MidgardKit/Session/{CoreGateway.swift,AppModel.swift,IdleLock.swift,StorePaths.swift}`
- Create: `clients/shells/apple/Sources/MidgardKit/Views/{PersonaPickerView.swift,LockView.swift,StatusChipsView.swift,MarkingChipView.swift,WithheldRowView.swift}`
- Create: `clients/shells/apple/tools/gen-tokens.mjs`, `clients/shells/apple/tools/check-contract.sh`
- Test: `clients/shells/apple/Tests/MidgardKitTests/{ContractTests.swift,TokensTests.swift,TestKeyRoot.swift}`

**Step 1: Write the failing tests.** `clients/shells/apple/Tests/MidgardKitTests/TestKeyRoot.swift`:

```swift
import CryptoKit
import Foundation
import MidgardCore

/// A software key root for tests: P-256 in memory, no prompt. Never in an app build.
final class TestKeyRoot: KeyRoot {
    private let key = P256.KeyAgreement.PrivateKey()
    func ensureKey() throws {}
    func agreeNew() throws -> KeyAgreement {
        let eph = P256.KeyAgreement.PrivateKey()
        let s = try eph.sharedSecretFromKeyAgreement(with: key.publicKey)
        return KeyAgreement(ephemeralPublic: eph.publicKey.x963Representation, sharedSecret: s.withUnsafeBytes { Data($0) })
    }
    func agreeExisting(ephemeralPublic: Data) throws -> Data {
        let peer = try P256.KeyAgreement.PublicKey(x963Representation: ephemeralPublic)
        return try key.sharedSecretFromKeyAgreement(with: peer).withUnsafeBytes { Data($0) }
    }
    func destroyKey() throws {}
    func hardwareBacked() -> Bool { false }
}
```

`clients/shells/apple/Tests/MidgardKitTests/ContractTests.swift` — calls every binding, so a change on either side fails here:

```swift
import XCTest
import MidgardCore
@testable import MidgardKit

/// The Swift–Rust contract: every exported binding is called once with real data.
/// tools/check-contract.sh fails if a binding is added without a call here. [FR-C-1]
final class ContractTests: XCTestCase {
    private var repo: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent()      // MidgardKitTests
            .deletingLastPathComponent().deletingLastPathComponent()      // apple
            .deletingLastPathComponent().deletingLastPathComponent()      // shells, clients
            .deletingLastPathComponent()                                  // repo root
    }

    private func makeClient() throws -> (MidgardClient, URL) {
        let store = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let dataset = repo.appendingPathComponent("clients/demo/datasets/\(coreVersion()).mgdemo")
        let config = ClientConfig(datasetPath: dataset.path, storeDir: store.path, replicaId: "contract",
                                  device: DeviceInfo(ramMb: 8192, hasAccelerator: true), demoOnDeviceModel: true)
        return (try MidgardClient(config: config, keyRoot: TestKeyRoot()), store)
    }

    func testEveryBinding() throws {
        XCTAssertFalse(coreVersion().isEmpty)
        let (client, store) = try makeClient()
        defer { try? FileManager.default.removeItem(at: store) }

        XCTAssertEqual(client.personas().count, 4)
        XCTAssertFalse(client.isEnrolled())
        try client.enrol()
        XCTAssertTrue(client.isEnrolled())
        let info = try client.unlock(personaId: "k.rao")
        XCTAssertEqual(info.persona.role, "analyst")
        XCTAssertFalse(client.isLocked())

        let chips = try client.statusChips()
        XCTAssertEqual(chips.sync.phase, .synced)
        let rows = try client.collectionRows(collection: "midgard.objectTypes")
        XCTAssertTrue(rows.contains { $0.withheld })
        let target = try XCTUnwrap(rows.first { !$0.withheld }?.id)

        XCTAssertEqual(try client.setOffline(offline: true).sync.phase, .offline)
        let id = try client.requestAction(action: ActionInput(verb: "demo.ping", target: "midgard.objectTypes/\(target)", params: []))
        XCTAssertThrowsError(try client.syncNow()) { error in
            guard case ClientError.Offline = error else { return XCTFail("expected offline, got \(error)") }
        }
        _ = try client.setOffline(offline: false)
        XCTAssertEqual(try client.syncNow().pushed, 4)
        XCTAssertEqual(try client.actionStatus(actionId: id).state, .applied)

        XCTAssertEqual(try client.switchPersona(personaId: "a.holt").persona.id, "a.holt")
        try client.lock()
        XCTAssertTrue(client.isLocked())
        XCTAssertThrowsError(try client.collectionRows(collection: "midgard.objectTypes")) { error in
            guard case ClientError.Locked = error else { return XCTFail("expected locked, got \(error)") }
        }
        try client.resetDemo()
        XCTAssertFalse(client.isEnrolled())
    }
}
```

(UniFFI 0.28 spells error cases after the Rust variant names — `ClientError.Offline`, `ClientError.Locked` — while plain enums are lowerCamelCase (`.synced`, `.applied`). If the generated file differs, follow it.)

`clients/shells/apple/Tests/MidgardKitTests/TokensTests.swift`:

```swift
import SwiftUI
import XCTest
@testable import MidgardKit

/// The generated tokens are the design system's values. [DD-7]
final class TokensTests: XCTestCase {
    func testMarkingColoursAreFixed() {
        XCTAssertEqual(MGToken.markingConfidential, 0xCA1D25)
        XCTAssertEqual(MGToken.markingRestricted, 0xF0A824)
        XCTAssertEqual(MGToken.markingInternal, 0x3D8BFF)
        XCTAssertEqual(MGToken.markingOpen, 0x21C07A)
    }
    func testFontsAreBundledAndRegister() {
        XCTAssertTrue(MGFonts.registerAll(), "Oxanium, Space Grotesk and JetBrains Mono must register")
    }
}
```

**Step 2: Run them and see them fail.**

```bash
bash clients/shells/apple/tools/build-xcframework.sh        # Task A12: bindings and XCFramework
cd clients/shells/apple && swift test 2>&1 | tail -5; cd -
```

Expected: `error: Could not find Package.swift in this directory or any of its parent directories`.

**Step 3a: The package** — `clients/shells/apple/Package.swift`:

```swift
// swift-tools-version:5.10
// MidgardKit: the shared SwiftUI layer of the Apple shell (SAD_06 §5.1). Presentation
// only (KDD-1): crypto, keys, sync merge and clearance are calls into MidgardCore,
// the UniFFI bindings over midgard-client-core. Build the XCFramework first:
//   bash tools/build-xcframework.sh
import PackageDescription

let package = Package(
    name: "MidgardKit",
    defaultLocalization: "en",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "MidgardKit", targets: ["MidgardKit"]),
        .library(name: "MidgardAuditKit", targets: ["MidgardAuditKit"]),
    ],
    targets: [
        .binaryTarget(name: "MidgardCoreFFI", path: "Frameworks/MidgardCoreFFI.xcframework"),
        .target(name: "MidgardCore", dependencies: ["MidgardCoreFFI"], path: "Sources/MidgardCore",
                exclude: [".gitkeep"]),
        .target(name: "MidgardKit", dependencies: ["MidgardCore"], path: "Sources/MidgardKit",
                resources: [.copy("Resources/Fonts")]),
        .target(name: "MidgardAuditKit", path: "Sources/MidgardAuditKit"),
        .testTarget(name: "MidgardKitTests", dependencies: ["MidgardKit"], path: "Tests/MidgardKitTests"),
    ]
)
```

(`MidgardAuditKit` is Task A14's; create `Sources/MidgardAuditKit/AccessibilityAudit.swift` as an empty `import XCTest` file now so the manifest resolves, or add the target in A14.)

**Step 3b: Key root.** `Sources/MidgardKit/KeyRoot/SecureEnclaveKeyRoot.swift`:

```swift
import CryptoKit
import Foundation
import LocalAuthentication
import MidgardCore
import Security

/// The Secure Enclave key root. A P-256 key-agreement key created inside the Enclave
/// with `.privateKeyUsage` and `.userPresence` (Face ID / Touch ID, passcode fallback);
/// only its opaque `dataRepresentation` is stored, in the data-protection keychain,
/// this device only. It implements the core's `KeyRoot` callback and nothing else:
/// the core derives its key-encryption key from the agreed secret, and this type never
/// sees the core's custody root. [FR-C-2] [FR-C-9] [DD-6]
public final class SecureEnclaveKeyRoot: KeyRoot {
    private let service = "io.veldris.midgard.key-root"
    private let account: String
    private let reason: String

    public init(account: String = "device-root", reason: String) {
        self.account = account
        self.reason = reason
    }

    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account,
         kSecUseDataProtectionKeychain as String: true]
    }

    private func stored() throws -> Data? {
        var q = query
        q[kSecReturnData as String] = true
        var out: AnyObject?
        switch SecItemCopyMatching(q as CFDictionary, &out) {
        case errSecSuccess: return out as? Data
        case errSecItemNotFound: return nil
        case let status: throw KeyRootError.Failed(detail: "keychain read \(status)")
        }
    }

    public func ensureKey() throws {
        guard SecureEnclave.isAvailable else { throw KeyRootError.Unavailable }
        if try stored() != nil { return }
        var error: Unmanaged<CFError>?
        guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                                                           [.privateKeyUsage, .userPresence], &error) else {
            throw KeyRootError.Failed(detail: "access control: \(String(describing: error?.takeRetainedValue()))")
        }
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: access)
        var add = query
        add[kSecValueData as String] = key.dataRepresentation
        add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeyRootError.Failed(detail: "keychain write \(status)") }
    }

    /// The public half, without a prompt: reading `publicKey` does not use the private key.
    private func publicKey() throws -> P256.KeyAgreement.PublicKey {
        guard let data = try stored() else { throw KeyRootError.Unavailable }
        return try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: data).publicKey
    }

    public func agreeNew() throws -> KeyAgreement {
        let eph = P256.KeyAgreement.PrivateKey()
        let secret = try eph.sharedSecretFromKeyAgreement(with: try publicKey())
        return KeyAgreement(ephemeralPublic: eph.publicKey.x963Representation,
                            sharedSecret: secret.withUnsafeBytes { Data($0) })
    }

    public func agreeExisting(ephemeralPublic: Data) throws -> Data {
        guard let data = try stored() else { throw KeyRootError.Unavailable }
        let context = LAContext()
        context.localizedReason = reason
        do {
            let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: data, authenticationContext: context)
            let peer = try P256.KeyAgreement.PublicKey(x963Representation: ephemeralPublic)
            return try key.sharedSecretFromKeyAgreement(with: peer).withUnsafeBytes { Data($0) }
        } catch let error as LAError where [.userCancel, .appCancel, .systemCancel].contains(error.code) {
            throw KeyRootError.Cancelled
        }
    }

    public func destroyKey() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeyRootError.Failed(detail: "keychain delete \(status)")
        }
    }

    public func hardwareBacked() -> Bool { SecureEnclave.isAvailable }
}
```

`Sources/MidgardKit/KeyRoot/SoftwareKeyRoot.swift` — for the iOS simulator and Macs without an Enclave (the simulator has none, and the UI tests run there): the same keychain item holds a software P-256 key (`rawRepresentation`), and `agreeExisting` first evaluates `LAContext().evaluatePolicy(.deviceOwnerAuthentication, …)`; `hardwareBacked()` is `false`, so the custody chip says "software-wrapped". Write it as the mirror of the class above, with `P256.KeyAgreement.PrivateKey(rawRepresentation:)`.

`Sources/MidgardKit/KeyRoot/KeyRootFactory.swift`:

```swift
import CryptoKit
import MidgardCore

/// Chooses the key root for this device: the Enclave when present, software otherwise.
public enum KeyRootFactory {
    public static func make(reason: String) -> KeyRoot {
        SecureEnclave.isAvailable ? SecureEnclaveKeyRoot(reason: reason) : SoftwareKeyRoot(reason: reason)
    }
}
```

**Step 3c: Gateway, model, idle lock.** All core calls happen on one actor, off the main thread (`MidgardClient` holds a mutex for each call):

`Sources/MidgardKit/Session/CoreGateway.swift`:

```swift
import Foundation
import MidgardCore

/// Serialises every call into the core off the main thread. [FR-C-1]
public actor CoreGateway {
    public let client: MidgardClient
    public init(client: MidgardClient) { self.client = client }
    public func run<T>(_ body: (MidgardClient) throws -> T) throws -> T { try body(client) }
}
```

`Sources/MidgardKit/Session/StorePaths.swift`:

```swift
import Foundation
import MidgardCore

/// Where the sealed store lives: Application Support/Midgard/<release>, excluded from
/// backup, complete file protection on iOS. Holds only `root.sealed` and `state.sealed`.
public enum StorePaths {
    public static func storeDirectory() throws -> URL {
        var url = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                              appropriateFor: nil, create: true)
            .appendingPathComponent("Midgard", isDirectory: true)
            .appendingPathComponent(coreVersion(), isDirectory: true)
        #if os(iOS)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true,
                                                attributes: [.protectionKey: FileProtectionType.complete])
        #else
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        #endif
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try url.setResourceValues(values)
        return url
    }

    /// The dataset bundled in the app as `<release>.mgdemo`.
    public static func bundledDataset(in bundle: Bundle = .main) -> URL? {
        bundle.url(forResource: coreVersion(), withExtension: "mgdemo")
    }
}
```

`Sources/MidgardKit/Session/IdleLock.swift`:

```swift
import Foundation

/// Locks after `timeout` seconds without user activity, and when the app leaves the
/// foreground. Any interaction calls `touch()`. [FR-C-9]
@MainActor
public final class IdleLock {
    private let timeout: TimeInterval
    private var timer: Timer?
    private let onExpire: @MainActor () -> Void

    public init(timeout: TimeInterval = 300, onExpire: @escaping @MainActor () -> Void) {
        self.timeout = timeout
        self.onExpire = onExpire
    }

    public func touch() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: timeout, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.onExpire() }
        }
    }

    public func stop() { timer?.invalidate(); timer = nil }
}
```

`Sources/MidgardKit/Session/AppModel.swift`:

```swift
import Foundation
import MidgardCore
import SwiftUI

/// The app's session state: enrol, sign in as a persona, lock, chips, offline toggle,
/// reset. Errors become screen states, never crashes. [FR-C-1] [FR-C-9] [AC-C-13]
@MainActor
public final class AppModel: ObservableObject {
    public enum Phase: Equatable {
        case loading, needsEnrolment, locked, unlocked(PersonaInfo), failed(String)
    }

    @Published public private(set) var phase: Phase = .loading
    @Published public private(set) var personas: [PersonaInfo] = []
    @Published public private(set) var chips: StatusChips?
    @Published public var message: String?

    private var gateway: CoreGateway?
    private lazy var idle = IdleLock { [weak self] in Task { await self?.lock() } }

    public init() {}

    public func start(keyRoot: KeyRoot, dataset: URL? = StorePaths.bundledDataset(), device: DeviceInfo) async {
        do {
            guard let dataset else { throw ClientError.NotFound(what: "bundled dataset") }
            let config = ClientConfig(datasetPath: dataset.path, storeDir: try StorePaths.storeDirectory().path,
                                      replicaId: Self.replicaId(), device: device, demoOnDeviceModel: true)
            let client = try MidgardClient(config: config, keyRoot: keyRoot)
            gateway = CoreGateway(client: client)
            personas = client.personas()
            phase = client.isEnrolled() ? .locked : .needsEnrolment
        } catch {
            phase = .failed(Self.describe(error))
        }
    }

    public func enrol() async { await call { try $0.enrol() }; if case .needsEnrolment = phase { phase = .locked } }

    public func signIn(_ persona: PersonaInfo) async {
        guard let info = await call({ try $0.unlock(personaId: persona.id) }) else { return }
        chips = info.chips
        phase = .unlocked(info.persona)
        idle.touch()
    }

    public func switchPersona(_ persona: PersonaInfo) async {
        guard let info = await call({ try $0.switchPersona(personaId: persona.id) }) else { return }
        chips = info.chips
        phase = .unlocked(info.persona)
    }

    public func lock() async {
        _ = await call { try $0.lock() }
        idle.stop()
        chips = nil
        if gateway != nil { phase = .locked }
    }

    public func setOffline(_ offline: Bool) async { chips = await call { try $0.setOffline(offline: offline) } ?? chips }
    public func syncNow() async { chips = await call { try $0.syncNow().chips } ?? chips }
    public func resetDemo() async { _ = await call { try $0.resetDemo() }; phase = .needsEnrolment }
    public func touch() { idle.touch() }

    /// Run `body` on the core actor; map a `ClientError` to a message and a phase.
    @discardableResult
    public func call<T>(_ body: @escaping (MidgardClient) throws -> T) async -> T? {
        guard let gateway else { return nil }
        do {
            return try await gateway.run(body)
        } catch ClientError.Locked {
            phase = .locked
        } catch ClientError.Cancelled {
            message = String(localized: "Unlock cancelled.")
        } catch {
            message = Self.describe(error)
        }
        return nil
    }

    static func describe(_ error: Error) -> String {
        switch error {
        case ClientError.AboveClearance: return String(localized: "Withheld: above your clearance.")
        case ClientError.SignatureFailure: return String(localized: "The demo data failed its signature check and was not loaded.")
        case ClientError.Offline: return String(localized: "Offline. Changes are queued and will sync when you reconnect.")
        case ClientError.StorageFull: return String(localized: "The device is out of space; the sealed store could not be written.")
        case let ClientError.PendingOps(count): return String(localized: "Sync \(count) queued change(s) before switching persona.")
        case ClientError.Conflict: return String(localized: "This changed elsewhere. Review and try again.")
        default: return String(describing: error)
        }
    }

    private static func replicaId() -> String {
        let key = "midgard.replica-id"
        if let id = UserDefaults.standard.string(forKey: key) { return id }
        let id = UUID().uuidString
        UserDefaults.standard.set(id, forKey: key)
        return id
    }
}
```

**Step 3d: Views.** Each view uses only tokens and accessibility identifiers prefixed `mg.` (Task A14's focus-order check reads them).

`Sources/MidgardKit/Views/MarkingChipView.swift`:

```swift
import MidgardCore
import SwiftUI

/// A marking chip: the fixed governance colour of its level (never themed). [AC-C-13]
public struct MarkingChipView: View {
    let chip: MarkingChip
    public init(_ chip: MarkingChip) { self.chip = chip }
    public var body: some View {
        Text(chip.label)
            .font(MGFonts.mono(.caption2))
            .padding(.horizontal, MGSpace.s2).padding(.vertical, 2)
            .foregroundStyle(.white)
            .background(Capsule().fill(Color(mgHex: MGToken.markingHex(chip.level))))
            .accessibilityLabel(Text("Marking \(chip.label)"))
            .accessibilityIdentifier("mg.marking")
    }
}
```

`Sources/MidgardKit/Views/WithheldRowView.swift`: a row reading "Withheld — above your clearance" with the level's chip and no other content; accessibility label "Withheld row, <LEVEL>".

`Sources/MidgardKit/Views/StatusChipsView.swift`: three chips in a row — sync (`SYNCED`, `SYNCING · <n>`, `OFFLINE · <n> QUEUED`, `CONFLICT`), tier (`ON-DEVICE`, `REMOTE`, `HYBRID`, `AI OFF`), custody (`SECURE ENCLAVE` when `hardwareWrapped`, else `SOFTWARE KEY`) — each with an `accessibilityLabel` that spells the state out ("Sync: offline, 4 changes queued") and identifiers `mg.chip.sync`, `mg.chip.tier`, `mg.chip.custody`. Colours from `--secure`, `--warn`, `--danger`, `--info` tokens.

`Sources/MidgardKit/Views/PersonaPickerView.swift`:

```swift
import MidgardCore
import SwiftUI

/// Sign in as a demo persona. Signing in prompts for Face ID / Touch ID (or the
/// passcode) through the key root. [FR-C-9]
public struct PersonaPickerView: View {
    @ObservedObject var model: AppModel
    public init(model: AppModel) { self.model = model }

    public var body: some View {
        VStack(alignment: .leading, spacing: MGSpace.s4) {
            Text("Sign in").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            ForEach(Array(model.personas.enumerated()), id: \.element.id) { index, persona in
                Button {
                    Task { await model.signIn(persona) }
                } label: {
                    HStack {
                        VStack(alignment: .leading) {
                            Text(persona.display).font(MGFonts.sans(.body))
                            Text(persona.role.uppercased()).font(MGFonts.mono(.caption))
                        }
                        Spacer()
                        Text(persona.clearanceLabel).font(MGFonts.mono(.caption2))
                    }
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                }
                .accessibilityLabel(Text("\(persona.display), \(persona.role), clearance \(persona.clearanceLabel)"))
                .accessibilityIdentifier("mg.\(index).persona.\(persona.id)")
            }
            if let message = model.message {
                Text(message).font(MGFonts.sans(.footnote)).accessibilityIdentifier("mg.message")
            }
        }
        .padding(MGSpace.s5)
    }
}
```

`Sources/MidgardKit/Views/LockView.swift`: "Locked" header, the persona last used, an "Unlock" button (`mg.0.unlock`, ≥ 44 pt) calling `model.signIn`, and a "Reset demo" button behind a confirmation dialog (`mg.1.reset`).

**Step 3e: Tokens from the design system.** The design system's authority is `docs/design/` and its bundled port is `ui/design-system/src/tokens/*.css` (`ui/design-system/PORTING.md`); fonts are the OFL-1.1 builds in `ui/design-system/src/fonts/` (`Oxanium.ttf`, `SpaceGrotesk.ttf`, `JetBrainsMono.ttf` with their `-OFL.txt`). `clients/shells/apple/tools/gen-tokens.mjs` reads the CSS custom properties and writes `Sources/MidgardKit/DesignSystem/Tokens.swift`, and copies the fonts and their licences into `Sources/MidgardKit/Resources/Fonts/`; `--check` fails if either differs.

```js
#!/usr/bin/env node
// Generate MidgardKit's design tokens from ui/design-system/src/tokens. [DD-7]
//   node clients/shells/apple/tools/gen-tokens.mjs --write | --check
import { readFileSync, writeFileSync, copyFileSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../../..');
const TOK = join(ROOT, 'ui/design-system/src/tokens');
const OUT = join(ROOT, 'clients/shells/apple/Sources/MidgardKit/DesignSystem/Tokens.swift');
const FONT_SRC = join(ROOT, 'ui/design-system/src/fonts');
const FONT_OUT = join(ROOT, 'clients/shells/apple/Sources/MidgardKit/Resources/Fonts');
const FONTS = ['Oxanium.ttf', 'SpaceGrotesk.ttf', 'JetBrainsMono.ttf', 'Oxanium-OFL.txt', 'SpaceGrotesk-OFL.txt', 'JetBrainsMono-OFL.txt'];

// Custom properties per selector block: { ':root': {name: value}, '.mode-light': {...}, '.theme-forge': {...} }
function blocks(file) {
  const css = readFileSync(join(TOK, file), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  const out = {};
  for (const m of css.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const sel = m[1].trim();
    for (const d of m[2].matchAll(/--([a-z0-9-]+)\s*:\s*([^;]+);/g)) (out[sel] ??= {})[d[1]] = d[2].trim();
  }
  return out;
}
const colors = blocks('colors.css'), effects = blocks('effects.css'), spacing = blocks('spacing.css'), type = blocks('typography.css');
const root = { ...colors[':root'], ...effects[':root'], ...spacing[':root'], ...type[':root'] };
const resolve = (v, scope = root) => {
  for (let i = 0; i < 8 && /var\(--/.test(v); i++) v = v.replace(/var\(--([a-z0-9-]+)\)/g, (_, n) => scope[n] ?? root[n] ?? `var(--${n})`);
  return v;
};
// '#ca1d25' -> {hex: 0xCA1D25, a: 1}; 'rgba(202, 29, 37, 0.14)' -> {hex, a}
function color(v) {
  v = resolve(v);
  let m = v.match(/^#([0-9a-f]{6})$/i);
  if (m) return { hex: parseInt(m[1], 16), a: 1 };
  m = v.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/);
  if (m) return { hex: (+m[1] << 16) | (+m[2] << 8) | +m[3], a: m[4] === undefined ? 1 : +m[4] };
  return null;
}
const camel = (n) => n.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
const hex = (n) => `0x${n.toString(16).toUpperCase().padStart(6, '0')}`;
const lines = [];
lines.push('// @generated by clients/shells/apple/tools/gen-tokens.mjs from ui/design-system/src/tokens.');
lines.push('// Do not edit: run `node clients/shells/apple/tools/gen-tokens.mjs --write`. [DD-7]');
lines.push('import MidgardCore', 'import SwiftUI', '', 'public enum MGToken {');
for (const [n, v] of Object.entries(colors[':root'])) {
  const c = color(v);
  if (c && c.a === 1) lines.push(`    public static let ${camel(n)}: UInt32 = ${hex(c.hex)}`);
  else if (c) lines.push(`    public static let ${camel(n)}: (UInt32, Double) = (${hex(c.hex)}, ${c.a})`);
}
lines.push('', '    /// Light-mode values (`.mode-light`).', '    public enum Light {');
for (const [n, v] of Object.entries(colors['.mode-light'] ?? {})) {
  const c = color(v);
  if (c) lines.push(`        public static let ${camel(n)}: (UInt32, Double) = (${hex(c.hex)}, ${c.a})`);
}
lines.push('    }', '');
lines.push('    /// The fixed marking colour for a level.');
lines.push('    public static func markingHex(_ level: MarkingLevelChip) -> UInt32 {');
lines.push('        switch level {', '        case .open: return markingOpen', '        case .internal: return markingInternal',
  '        case .restricted: return markingRestricted', '        case .confidential: return markingConfidential', '        }', '    }', '}', '');
lines.push('public enum MGSpace {');
for (const [n, v] of Object.entries(spacing[':root'])) {
  const m = v.match(/^([\d.]+)rem$/) ?? (v === '0' ? ['', '0'] : null);
  if (m && n.startsWith('space-')) lines.push(`    public static let s${n.slice(6)}: CGFloat = ${+m[1] * 16}`);
}
lines.push('}', '', 'public enum MGRadius {');
for (const [n, v] of Object.entries(effects[':root'])) {
  const m = v.match(/^(\d+)px$/);
  if (m && n.startsWith('radius-')) lines.push(`    public static let ${camel(n.slice(7))}: CGFloat = ${m[1]}`);
}
lines.push('}', '', '/// Product accents: one hue per console (`.theme-<product>`). Admin uses Midgard\'s.');
lines.push('public enum MGAccent: String, CaseIterable {', '    case midgard, forge, mimir, bifrost, heimdall');
lines.push('    public var hex: UInt32 {', '        switch self {');
for (const p of ['midgard', 'forge', 'mimir', 'bifrost', 'heimdall']) lines.push(`        case .${p}: return MGToken.${p}500`);
lines.push('        }', '    }', '}', '');
const swift = lines.join('\n');

const mode = process.argv.includes('--write') ? 'write' : 'check';
let stale = 0;
if (mode === 'write') {
  writeFileSync(OUT, swift);
  mkdirSync(FONT_OUT, { recursive: true });
  for (const f of FONTS) copyFileSync(join(FONT_SRC, f), join(FONT_OUT, f));
} else {
  if (!existsSync(OUT) || readFileSync(OUT, 'utf8') !== swift) { console.error('stale: Tokens.swift'); stale++; }
  for (const f of FONTS) {
    const out = join(FONT_OUT, f);
    if (!existsSync(out) || !readFileSync(out).equals(readFileSync(join(FONT_SRC, f)))) { console.error(`stale: Fonts/${f}`); stale++; }
  }
}
if (stale) process.exit(1);
console.log(`gen-tokens: ${mode === 'write' ? 'written' : 'up to date'}`);
```

`Sources/MidgardKit/DesignSystem/Color+Midgard.swift`:

```swift
import SwiftUI
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

public extension Color {
    /// A colour from a 0xRRGGBB token.
    init(mgHex hex: UInt32, opacity: Double = 1) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: opacity)
    }

    /// A colour that follows light and dark mode (the console's `.mode-light` scope).
    init(mgLight light: (UInt32, Double), dark: (UInt32, Double)) {
        #if canImport(UIKit)
        self.init(UIColor { $0.userInterfaceStyle == .dark ? UIColor(Color(mgHex: dark.0, opacity: dark.1))
                                                           : UIColor(Color(mgHex: light.0, opacity: light.1)) })
        #else
        self.init(NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(Color(mgHex: dark.0, opacity: dark.1)) : NSColor(Color(mgHex: light.0, opacity: light.1))
        })
        #endif
    }
}
```

`Sources/MidgardKit/DesignSystem/Fonts.swift`:

```swift
import CoreText
import SwiftUI

/// The house type (Oxanium display, Space Grotesk UI, JetBrains Mono labels), bundled and
/// registered at launch; sizes follow Dynamic Type via `relativeTo:`. [DD-7]
public enum MGFonts {
    @discardableResult
    public static func registerAll() -> Bool {
        ["Oxanium", "SpaceGrotesk", "JetBrainsMono"].allSatisfy { name in
            guard let url = Bundle.module.url(forResource: name, withExtension: "ttf", subdirectory: "Fonts") else { return false }
            var error: Unmanaged<CFError>?
            let ok = CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error)
            // Already registered (a second call) is fine.
            return ok || (error?.takeRetainedValue()).map { CFErrorGetCode($0) == 105 } == true
        }
    }
    public static func display(_ style: Font.TextStyle) -> Font { .custom("Oxanium", size: size(style), relativeTo: style) }
    public static func sans(_ style: Font.TextStyle) -> Font { .custom("Space Grotesk", size: size(style), relativeTo: style) }
    public static func mono(_ style: Font.TextStyle) -> Font { .custom("JetBrains Mono", size: size(style), relativeTo: style) }
    /// Base sizes from typography.css (console default body 14, chips 12, eyebrow 11).
    static func size(_ style: Font.TextStyle) -> CGFloat {
        switch style {
        case .largeTitle: return 32
        case .title: return 24
        case .title2: return 20
        case .headline, .body: return 14
        case .callout, .subheadline, .footnote: return 13
        case .caption: return 12
        default: return 11
        }
    }
}
```

(`105` is `kCTFontManagerErrorAlreadyRegistered`.) `Font.custom` needs the names the font files declare; read them once with `CTFontManagerCreateFontDescriptorsFromURL` (or open the files in Font Book) and use the family or PostScript names they report — if `"Space Grotesk"` is not one of them, SwiftUI silently falls back to the system font. Add an assertion to `TokensTests` that `CTFontCreateWithName` for each name returns a font whose family is the expected one.

Generate:

```bash
node clients/shells/apple/tools/gen-tokens.mjs --write
```

**Step 3f: The contract coverage check** — `clients/shells/apple/tools/check-contract.sh` (bash 3.2):

```bash
#!/usr/bin/env bash
# Every function the generated Swift bindings export is called by ContractTests.swift.
# [FR-C-1] [DD-6]
set -euo pipefail
cd "$(dirname "$0")/.."
gen=Sources/MidgardCore/midgard_client_ffi.swift
test=Tests/MidgardKitTests/ContractTests.swift
[ -f "$gen" ] || { echo "check-contract: run tools/build-xcframework.sh first" >&2; exit 2; }
missing=0
# Object methods (`open func name(`) and top-level functions (`public func name(`),
# minus UniFFI's own converters and helpers.
for name in $(grep -Eo '(open|public) func [a-zA-Z0-9_]+\(' "$gen" | sed -E 's/.* func ([a-zA-Z0-9_]+)\(/\1/' \
              | grep -Ev '^(FfiConverter|uniffi|lift|lower|read|write|allocationSize|hash)' | sort -u); do
    # KeyRoot's methods are implemented (not called) by the test key root.
    if ! grep -q "[.( ]$name(" "$test" Tests/MidgardKitTests/TestKeyRoot.swift; then
        echo "check-contract: binding $name is not called in $test"
        missing=$((missing + 1))
    fi
done
[ "$missing" -eq 0 ] && echo "check-contract: every binding is exercised"
[ "$missing" -eq 0 ]
```

The generated `KeyRootImpl` class has `open func ensureKey()` and the other callback methods; the script finds them in `TestKeyRoot.swift`, which implements them.

**Step 4: Run them and see them pass.**

```bash
cd clients/shells/apple
swift build 2>&1 | tail -3
swift test 2>&1 | tail -5
bash tools/check-contract.sh
node tools/gen-tokens.mjs --check
cd -
xcodebuild build -scheme MidgardKit -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived -quiet -workspace clients/shells/apple/.swiftpm/xcode/package.xcworkspace 2>&1 | tail -3
```

Expected: `swift test` runs `ContractTests` and `TokensTests` green on macOS (the host has no Enclave requirement: the tests use `TestKeyRoot`); `check-contract: every binding is exercised`; `gen-tokens: up to date`; the iOS-simulator build of the package succeeds. (If `xcodebuild` cannot find the scheme, run `xcodebuild -list` inside `clients/shells/apple` and use the listed one.) Afterwards: `bash clients/shells/apple/tools/clean.sh`.

**Step 5: Commit.**

```bash
git add clients/shells/apple/Package.swift clients/shells/apple/Sources clients/shells/apple/Tests clients/shells/apple/tools
git commit -m "clients/shells/apple: MidgardKit package with Secure Enclave key root, tokens, sign-in, lock, chips [FR-C-9] [DD-7]"
```

---

### Task A14: Accessibility audit hook

**As built (16 Nov).** The steps below are kept as written for the record; the code differs as follows.

1. *Fail closed on an empty run.* `audit.sh` also exits non-zero (no record) when a result bundle holds no `test_affordance_` case at all — the UI tests did not build or launch — instead of reporting seven failures that look measured. `record-audit.sh` removes the record and exits 2 if the stamping rewrite fails, so a `PLACEHOLDER_OVERALL` record is never left behind.
2. *`AccessibilityAuditBase`.* As in Step 3d, with: a test with no `screens` fails ("nothing was measured") instead of passing vacuously; `focus_order` fails unless at least one audited screen has two `mg.<n>.` elements; `reduced_motion` waits for `mg.motion` and checks every screen for `mg.anim.`; `keyboard_navigation` asserts that Tab puts focus on an `mg.<n>.` element (iOS `hasFocus`; macOS has no `hasFocus`, so a `hasKeyboardFocus` predicate — unverified until a Mac app runs, and it fails rather than passes if wrong) and presses ⌘1…⌘6 after the **last** screen (inside a console, where `mg.console.selected` exists), not on the sign-in screen; the iOS contrast test restores the device appearance. `Package.swift` gains the `MidgardAuditKit` target and product; `tools/typecheck-ios.sh` now also typechecks it against the iPhoneOS and iPhoneSimulator XCTest, so its `#if os(iOS)` branches compile.
3. *What `swift test` measures now* (`AccessibilityPrecheckTests`, 10 tests, and `AuditKitTests`, 3; 98 tests in all). These are prechecks of the shared views, never an audit record. **contrast_aa**: WCAG ratios from the tokens for every pair the shared views draw (marking chips, status chips, signature badges, demo banner) and heading/body/muted text on every surface in both modes. **hit_target_size**: on macOS, the laid-out controls of `EnrolView`, `PersonaPickerView` and `LockView` (their focus-ring frames, which is what AppKit hit-tests) are at least 44 × 44 pt, and the test fails if it finds fewer controls than expected. **dynamic_type, reduced_motion, screen_reader_labels**: source rules (every view font is an `MGFonts` style built `relativeTo:`, no fixed sizes; no animation anywhere in `MidgardKit`; every image hidden or labelled, every combined element labelled). macOS ignores `dynamicTypeSize` (a rendered `Text` is the same size at `.large` and `.accessibility5`), so Dynamic Type scaling is measured only by the iOS audit. **focus_order, keyboard_navigation**: not measurable without a running app. `AuditKitTests` checks that the base class runs nothing and that a subclass has exactly one `test_affordance_<name>` per affordance in `accessibility.toml`, the same list `audit-failures.js` reads.
4. *Views changed by the prechecks.* The dark ink on the confidential red is 3.6:1, below AA: `MGContrast.ink(on:)` (new, `DesignSystem/Contrast.swift`) picks the darkest ink or white, whichever reads better, so the confidential chip is white on red (the web chip uses white there too; its white on the internal blue, 3.3:1, is not followed). Status chips and signature badges drew tone-coloured text on whatever the host put behind them, which fails AA for `danger` on the dark surfaces (2.9–3.5:1) and for `secure`, `warn` and `info` on every light surface (1.7–3.3:1); they are now filled with their tone and lettered with `MGContrast.ink(on:)`, so their contrast no longer depends on the screen. `LockView` and `EnrolView` had `.frame(minWidth: 44, minHeight: 44)` outside their buttons, which pads the layout but leaves the hit region at the 24 pt bezel; the frame is now inside the label (`HitTarget`), as `PersonaPickerView` already did. `text-faint` (4.0:1 dark, 2.8:1 light) is for decoration only and no shared view uses it.
5. *Tests.* `tools/ci/tests/shell-audit-apple.test.sh` covers more than Step 1: record stamping for pass and fail with no temp file left; `audit.sh` against stub `xcodebuild`/`xcrun`/`osascript` (macOS app alone reports `dynamic_type`; both apps let the iOS result stand; no bundle or no affordance test fails closed); and the CI job `run.sh shell-audit apple` end to end (package without an app: no record; both apps passing: a record with all seven `pass`). It runs on `/bin/bash` 3.2 and in the Linux `selftest` job (the parser case is skipped where there is no `osascript`).
6. *Locally* (no app targets, no iOS platform): `audit.sh` and `run.sh shell-audit apple` both stop with "no app project under clients/shells/apple/Apps; nothing was audited", exit 1, and write no record. No `audits/<release>.toml` is committed: per owner decision 4 the release record comes from a CI run on a merge of `clients_mac` and `clients_iphone`.
7. *CI.* `shell-audit` runs only in the audit/release pipelines (`shell-audit.yml` on dispatch or from `release.yml`; GitLab `.on-audit`), never per commit, with `continue-on-error`/`allow_failure`; the gate decides. Until Phase B adds the apps, the Apple audit job fails as it did before A14 (then because `audit.sh` was missing) and leaves no record, so the gate fails closed unless the pre-release is waived. The device field is `MIDGARD_AUDIT_DEVICE` or "iOS simulator" even when the macOS app was audited too; A15 or Phase B should set it to name both.
8. *Review round (16 Nov).* (a) `audit.sh` refuses a tree without **both** apps (owner decision 4) before anything runs; the Dynamic Type exception therefore always has the iOS result standing for it, and a `clients_mac`-only or `clients_iphone`-only run records nothing. (b) It refuses UI-test sources under `Apps/` that override `record(_:)` or mention `invokeTest` (they could drop or rewrite failures). (c) It keeps both apps' summaries as evidence (`shell-audit/apple-<release>-ios.json`, `-mac.json`; the `report` itself is the two merged as `{"ios":…,"mac":…}`) and writes `shell-audit/apple-<release>.device` — "iPhone 16 simulator (iOS <runtime from simctl>) + macOS <sw_vers> (<arch>)" — which `run.sh` records as the device unless `MIDGARD_AUDIT_DEVICE` is set; a simulator `simctl` does not list fails the audit. Item 7's device sentence is superseded. (d) `verify-shell-audits.sh` refuses a record committed to the tree (records reach the gate only as the audit jobs' artifacts; the repository has only `.gitkeep` under `audits/`, and the new `tools/ci/tests/shell-audit-gate.test.sh`, run in `selftest` on every change, fails if that changes), and in CI (`CI` set) refuses a record whose `commit` is neither the gated commit (`GITHUB_SHA`/`CI_COMMIT_SHA`) nor an ancestor of it; outside CI it warns. In the release workflow the audit jobs and the gate check out the same commit, so the equality test decides and a shallow checkout is enough; an older record needs the history. (e) `focus_order`'s reading-order test is `a.minY < b.minY - 1 || (same row && a.minX <= b.minX)` (Step 3d had `+ 1`, which let a right-to-left pair on one row pass); Step 3d is corrected. (f) Loading is `LoadingView`: a spinner tagged `mg.anim.loading`, or the static text "Loading…" (`mg.loading`) when `MotionPolicy.reduced` (the system's Reduce Motion or `MIDGARD_REDUCE_MOTION=1`); `test_affordance_reduced_motion` walks `auditedScreens()` and also requires no activity indicator (iOS) or progress indicator (macOS); the precheck counts an untagged `ProgressView(` as motion and measures `LoadingView` on macOS (no `NSProgressIndicator` when reduced). (g) The macOS `keyboard_navigation` run launches with `-AppleKeyboardUIMode 2` (keyboard navigation reaches every control). (h) The parser fixture adds a skipped test, an expected failure and a test that failed once and then passed; none counts as a pass. (i) Limits of the prechecks: the hit-size probe finds controls through SwiftUI's private `_FocusRingView` (or `NSControl`); it fails rather than passes if that changes, and a public-API alternative is an `NSHostingView` sized to each control alone (`fittingSize` of the button with its label) or the XCUITest `.hitRegion` audit, which is the measurement of record. The source rules read one line (images, spinners) or one file (fonts, labels, ink) at a time, so a modifier split across lines or a view in another file is not seen; they guard against regressions and do not replace the audit. (j) The chip styling is superseded by item 9. (k) For A15: `tools/ci/tests/claims.test.sh` has 18 failures on this Mac, the same at `a3356c55` and at `v0.1.0-rc.3`, so not caused by Phase A; A15 must explain or fix them before CI `selftest` runs it.
9. *Chip styling decided (16 Nov): tinted, outlined.* The owner kept the web consoles' outlined look for status chips and signature badges, replacing item 4's filled chips: the tone's tint (`--<tone>-tint`, 14%) behind, a 1 pt outline in the tone, and text in a per-mode text token. The design system has no darker text for the tones, and the tone as text on its own tint fails AA (danger and info in dark, every tone in light), so `gen-tokens.mjs` derives `MGToken.ToneText.<tone>Dark`/`<tone>Light`: the tone mixed toward white (dark) or black (light) in 1% steps, channels rounded, first mix reaching 4.6:1 (AA plus 0.1) against the tint composited source-over (rounded) on each of the mode's five surfaces (`bg-page`, `bg-sunken`, `surface-card`, `surface-raised`, `surface-inset`). Result, worst–best over the surfaces: secure dark `#21C07A` (the tone itself) 5.39–7.29, light `#136D46` 4.67–5.62; warn dark `#F0A824` (the tone) 6.06–8.25, light `#7F5913` 4.75–5.71; danger dark `#DD6C71` 4.64–5.85, light `#AE1920` 4.66–5.62; info dark `#5298FF` 4.63–6.18, light `#295DAB` 4.62–5.57. `ChipTone` gains `tint` and `text`; `MGContrast.over` composites as the generator does. The prechecks measure every tone's text on the composited tint over every surface in both modes, show that the tone colour itself would fail there, and check that both views are tinted and outlined; reverting either view to the coloured-text-on-any-background style, or the text token to the tone, fails them. The marking chips stay filled in their governance colour, white on confidential red as on the web.

`job_shell_audit apple` (`tools/ci/run.sh`) runs `bash clients/shells/apple/audit.sh <release> <report>` on the `macos-15` runner, takes its **stdout** as the list of failed affordances, and passes it to `clients/shells/tools/record-audit.sh`, which writes `clients/shells/apple/audits/<release>.toml` in the `AUDIT_SCHEMA.md` format (runner, device `${MIDGARD_AUDIT_DEVICE:-iOS simulator}`, tool `XCUITest + XCUIAccessibilityAudit`, tool version from `xcodebuild -version`). `deploy/acceptance/verify-shell-audits.sh` then requires every affordance declared in `accessibility.toml` to be measured `pass`. Under `set -euo pipefail` in `run.sh`, a non-zero exit from `audit.sh` stops the job before a record is written — which is the required behaviour when nothing could run.

Two defects found on the way, fixed here:
1. `record-audit.sh` ends with `sed -i "s/PLACEHOLDER_OVERALL/$overall/" "$out"`. On the macOS runner `sed` is BSD `sed`, which reads the expression as a backup suffix and fails, so an Apple record would never be stamped. It becomes a portable temp-file rewrite.
2. Nothing at the tag implements `clients/shells/apple/audit.sh`.

**What is measured, per affordance** (XCUITest's `performAccessibilityAudit(for:)` needs iOS 17 / macOS 14, which the deployment targets already are):

| affordance | measurement |
|---|---|
| `dynamic_type` | iOS: app launched at `UICTContentSizeCategoryAccessibilityXXXL`; `performAccessibilityAudit(for: [.dynamicType, .textClipped])` on the sign-in, console list and one detail screen. macOS has no Dynamic Type audit type: the macOS run records this affordance from the iOS run only. |
| `screen_reader_labels` | `[.sufficientElementDescription, .elementDetection]` on the same screens |
| `contrast_aa` | `[.contrast]`, in light and dark appearance |
| `hit_target_size` | `[.hitRegion]` |
| `focus_order` | the elements whose identifier starts `mg.<n>.` appear in accessibility order with `n` increasing, and their frames are in reading order |
| `reduced_motion` | launched with `MIDGARD_REDUCE_MOTION=1` (mirrors `accessibilityReduceMotion`), the hidden element `mg.motion` reads `reduced` and no element with an identifier starting `mg.anim.` exists |
| `keyboard_navigation` | ⌘1…⌘6 switch the selected console (`mg.console.selected`'s value changes), Tab moves focus to the next `mg.<n>.` element (iOS simulator with the hardware keyboard connected; macOS natively) |

**Files:**
- Create: `clients/shells/apple/audit.sh`, `clients/shells/apple/tools/audit-failures.js`
- Create: `clients/shells/apple/Sources/MidgardAuditKit/AccessibilityAudit.swift` (the base class each app's UI-test target subclasses in Phase B)
- Modify: `clients/shells/tools/record-audit.sh` (portable stamping)
- Test: `tools/ci/tests/shell-audit-apple.test.sh`, fixture `tools/ci/tests/fixtures/xcresult-tests.json`

**Step 1: Write the failing test** — `tools/ci/tests/shell-audit-apple.test.sh`:

```bash
#!/usr/bin/env bash
# The Apple audit hook fails closed without an app, reports failed affordances from an
# xcresult summary, and record-audit.sh stamps `overall` on BSD and GNU sed alike.
# [RC-49] [AC-C-11]
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; root="$here/../../.."
failures=0
pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

# 1. No app project: non-zero exit, nothing on stdout (so no record can be written).
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/clients/shells/apple/tools"
cp "$root/clients/shells/apple/audit.sh" "$work/clients/shells/apple/"
out="$(bash "$work/clients/shells/apple/audit.sh" 0.0.0-test shell-audit/x.json 2>/dev/null)"; rc=$?
[ "$rc" -ne 0 ] && [ -z "$out" ] && pass "no app: fails closed" || fail "no app: rc=$rc out='$out'"

# 2. The xcresult parser (macOS only: JavaScript for Automation).
if command -v osascript >/dev/null 2>&1; then
    got="$(osascript -l JavaScript "$root/clients/shells/apple/tools/audit-failures.js" "$here/fixtures/xcresult-tests.json")"
    [ "$got" = "contrast_aa keyboard_navigation" ] && pass "parser lists failed and missing affordances" || fail "parser said '$got'"
else
    echo "skip  parser (no osascript on this host)"
fi

# 3. record-audit.sh stamps overall with whatever sed this host has.
mkdir -p "$work/r/clients/shells/apple" "$work/r/clients/shells/tools"
cp "$root/clients/shells/tools/record-audit.sh" "$work/r/clients/shells/tools/"
cp "$root/clients/shells/apple/accessibility.toml" "$work/r/clients/shells/apple/"
bash "$work/r/clients/shells/tools/record-audit.sh" apple 0.0.0-test runner-1 "iPhone 16" XCUITest "Xcode 26.2" r.json \
    contrast_aa >/dev/null 2>&1
rec="$work/r/clients/shells/apple/audits/0.0.0-test.toml"
grep -q '^overall *= "fail"' "$rec" 2>/dev/null && ! grep -q PLACEHOLDER "$rec" \
    && pass "record-audit stamps overall" || fail "record-audit left $(grep overall "$rec" 2>/dev/null)"

[ "$failures" -eq 0 ] || exit 1
```

`tools/ci/tests/fixtures/xcresult-tests.json` — a trimmed `xcrun xcresulttool get test-results tests` document with five affordance test cases passed, `test_affordance_contrast_aa()` failed and no `keyboard_navigation` case:

```json
{"testNodes":[{"name":"MidgardiOSUITests","nodeType":"Unit test bundle","result":"Failed","children":[
 {"name":"AccessibilityAudit","nodeType":"Test Suite","result":"Failed","children":[
  {"name":"test_affordance_dynamic_type()","nodeType":"Test Case","result":"Passed"},
  {"name":"test_affordance_screen_reader_labels()","nodeType":"Test Case","result":"Passed"},
  {"name":"test_affordance_contrast_aa()","nodeType":"Test Case","result":"Failed"},
  {"name":"test_affordance_focus_order()","nodeType":"Test Case","result":"Passed"},
  {"name":"test_affordance_reduced_motion()","nodeType":"Test Case","result":"Passed"},
  {"name":"test_affordance_hit_target_size()","nodeType":"Test Case","result":"Passed"}]}]}]}
```

**Step 2: Run it and see it fail.** `bash tools/ci/tests/shell-audit-apple.test.sh` → `cp: …/audit.sh: No such file or directory`, then three `FAIL` lines (the third because BSD `sed -i` fails: on this Mac it prints `FAIL  record-audit left overall      = "PLACEHOLDER_OVERALL"`).

**Step 3a: Fix `record-audit.sh`.** Replace its last-but-two line

```bash
sed -i "s/PLACEHOLDER_OVERALL/$overall/" "$out"
```

with

```bash
# Portable across GNU and BSD sed (the Apple audit runs on macOS): rewrite, then move.
sed "s/PLACEHOLDER_OVERALL/$overall/" "$out" > "$out.tmp" && mv "$out.tmp" "$out"
```

**Step 3b: The parser** — `clients/shells/apple/tools/audit-failures.js` (JavaScript for Automation, present on every Mac; no Python or Node needed on the runner):

```js
// Print the accessibility affordances whose audit test failed or did not run, from
// `xcrun xcresulttool get test-results tests` JSON. [RC-49] [AC-C-11]
//   osascript -l JavaScript audit-failures.js <tests.json>
function run(argv) {
  const text = $.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null).js;
  const all = ['dynamic_type', 'screen_reader_labels', 'contrast_aa', 'focus_order', 'reduced_motion',
               'keyboard_navigation', 'hit_target_size'];
  const seen = {}; const failed = [];
  (function walk(nodes) {
    (nodes || []).forEach(function (n) {
      const m = /^test_affordance_([a-z_]+)\(/.exec(n.name || '');
      if (m && n.nodeType === 'Test Case') {
        seen[m[1]] = true;
        if (n.result !== 'Passed') failed.push(m[1]);
      }
      walk(n.children);
    });
  })(JSON.parse(text).testNodes);
  all.forEach(function (a) { if (!seen[a]) failed.push(a); }); // not measured = not passed
  return all.filter(function (a) { return failed.indexOf(a) >= 0; }).join(' ');
}
```

**Step 3c: The hook** — `clients/shells/apple/audit.sh`:

```bash
#!/usr/bin/env bash
# On-device accessibility audit of the Apple shell, run by `tools/ci/run.sh shell-audit
# apple` on a macOS runner. Prints the failed affordances (space-separated) on stdout;
# exits non-zero when no app could be built and tested, so no record is written.
# [RC-49] [AC-C-11]
#
#   bash clients/shells/apple/audit.sh <release> <report-path>
set -euo pipefail
release="${1:?release}"; report="${2:?report path}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
cache="${MIDGARD_BUILD_CACHE:-$HOME/Library/Caches/midgard-build}"
derived="$cache/clients-derived"

apps=""
[ -d "$here/Apps/MidgardiOS/MidgardiOS.xcodeproj" ] && apps="$apps ios"
[ -d "$here/Apps/MidgardMac/MidgardMac.xcodeproj" ] && apps="$apps mac"
if [ -z "$apps" ]; then
    echo "audit: no app project under clients/shells/apple/Apps; nothing was audited" >&2
    exit 1
fi

# On a CI runner, provision the pinned toolchain's Apple targets (the workflow has no
# build logic of its own; see .github/workflows/shell-audit.yml).
if [ -n "${CI:-}" ]; then
    rustup toolchain install 1.82.0 --profile minimal >&2
    rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim \
        --toolchain 1.82.0 >&2
fi
bash "$here/tools/build-xcframework.sh" >&2

failed=""
first=""
for app in $apps; do
    case "$app" in
        ios) proj="$here/Apps/MidgardiOS/MidgardiOS.xcodeproj"; scheme=MidgardiOS
             dest="platform=iOS Simulator,name=${MIDGARD_AUDIT_SIMULATOR:-iPhone 16}" ;;
        mac) proj="$here/Apps/MidgardMac/MidgardMac.xcodeproj"; scheme=MidgardMac; dest="platform=macOS" ;;
    esac
    bundle="$derived/$app-$release.xcresult"
    rm -rf "$bundle"
    # Test failures are data here, not a reason to stop: the parser reads them.
    xcodebuild test -project "$proj" -scheme "$scheme" -destination "$dest" \
        -only-testing:"${scheme}UITests/AccessibilityAudit" \
        -derivedDataPath "$derived/$app" -resultBundlePath "$bundle" >&2 || true
    [ -d "$bundle" ] || { echo "audit: the $app audit did not run" >&2; exit 1; }
    xcrun xcresulttool get test-results tests --path "$bundle" > "$derived/$app-tests.json"
    f="$(osascript -l JavaScript "$here/tools/audit-failures.js" "$derived/$app-tests.json")"
    # Dynamic Type is measured on iOS only; the macOS run does not claim it. When the
    # iOS app was audited too, its result stands for the affordance.
    if [ "$app" = mac ]; then
        case "$apps" in *ios*) f="$(echo " $f " | sed 's/ dynamic_type / /')" ;; esac
    fi
    failed="$failed $f"
    [ -n "$first" ] || first="$derived/$app-tests.json"
done

mkdir -p "$(dirname "$root/$report")"
cp "$first" "$root/$report"
# Each affordance once, in a stable order.
for a in $failed; do echo "$a"; done | sort -u | tr '\n' ' '
```

On macOS the `dynamic_type` test fails by design (there is no Dynamic Type audit type), and `audit.sh` discards that failure only when the iOS app was audited in the same run. If only the macOS app exists (branch `clients_mac` alone), `dynamic_type` is reported failed — the record must not claim what was not measured. That is why the release record should come from a branch that has both apps (Open questions, Q4).

**Step 3d: The test base class** — `clients/shells/apple/Sources/MidgardAuditKit/AccessibilityAudit.swift`:

```swift
import XCTest

/// The seven affordances of clients/shells/apple/accessibility.toml, measured on the
/// running app. Each app's UI-test target subclasses this as `AccessibilityAudit` and
/// supplies `screens`: closures that navigate to each screen to audit. [RC-49] [AC-C-11]
open class AccessibilityAuditBase: XCTestCase {
    /// The base class is abstract: run its tests only through a subclass.
    override open class var defaultTestSuite: XCTestSuite {
        self == AccessibilityAuditBase.self ? XCTestSuite(name: "AccessibilityAuditBase (abstract)") : super.defaultTestSuite
    }

    /// Navigations, each ending on a screen to audit. The first is the sign-in screen.
    open var screens: [(XCUIApplication) -> Void] { [] }
    open var launchArguments: [String] { ["-MidgardUITesting"] }

    public func launch(_ extra: [String] = [], environment: [String: String] = [:]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = launchArguments + extra
        app.launchEnvironment = environment
        app.launch()
        return app
    }

    func audit(_ types: XCUIAccessibilityAuditType, extra: [String] = []) throws {
        let app = launch(extra)
        for go in screens {
            go(app)
            try app.performAccessibilityAudit(for: types)
        }
    }

    public func test_affordance_screen_reader_labels() throws { try audit([.sufficientElementDescription, .elementDetection]) }
    public func test_affordance_contrast_aa() throws {
        #if os(iOS)
        for style in [XCUIDevice.Appearance.light, .dark] {
            XCUIDevice.shared.appearance = style
            try audit(.contrast)
        }
        #else
        try audit(.contrast, extra: ["-AppleInterfaceStyle", "Light"])
        try audit(.contrast, extra: ["-AppleInterfaceStyle", "Dark"])
        #endif
    }
    public func test_affordance_hit_target_size() throws { try audit(.hitRegion) }

    public func test_affordance_dynamic_type() throws {
        #if os(iOS)
        try audit([.dynamicType, .textClipped],
                  extra: ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"])
        #else
        XCTFail("Dynamic Type is measured on iOS; the macOS run does not claim it")
        #endif
    }

    public func test_affordance_focus_order() throws {
        let app = launch()
        for go in screens {
            go(app)
            let ordered = app.descendants(matching: .any)
                .matching(NSPredicate(format: "identifier MATCHES 'mg\\\\.[0-9]+\\\\..*'"))
                .allElementsBoundByAccessibilityElement
            let indices = ordered.compactMap { Int($0.identifier.split(separator: ".")[1]) }
            XCTAssertEqual(indices, indices.sorted(), "accessibility order must follow mg.<n>. order")
            let frames = ordered.map(\.frame)
            for (a, b) in zip(frames, frames.dropFirst()) {
                XCTAssertTrue(a.minY < b.minY - 1 || (abs(a.minY - b.minY) < 1 && a.minX <= b.minX),
                              "elements must be in reading order")
            }
        }
    }

    public func test_affordance_reduced_motion() throws {
        let app = launch(environment: ["MIDGARD_REDUCE_MOTION": "1"])
        XCTAssertEqual(app.descendants(matching: .any)["mg.motion"].value as? String, "reduced")
        let animated = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'mg.anim.'"))
        XCTAssertEqual(animated.count, 0)
    }

    public func test_affordance_keyboard_navigation() throws {
        let app = launch()
        screens.first?(app)
        let selected = app.descendants(matching: .any)["mg.console.selected"]
        guard selected.waitForExistence(timeout: 5) else { return XCTFail("no console selector") }
        for n in 1...6 {
            app.typeKey("\(n)", modifierFlags: .command)
            XCTAssertEqual(selected.value as? String, "\(n)", "⌘\(n) must select console \(n)")
        }
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
    }
}
```

The `MIDGARD_REDUCE_MOTION`, `mg.motion`, `mg.console.selected` and `-MidgardUITesting` hooks are implemented in the apps' root views in Phase B (`-MidgardUITesting` selects a non-prompting test key root and a fresh store, compiled in Debug builds only).

**Step 4: Run it and see it pass.**

```bash
chmod +x clients/shells/apple/audit.sh
bash tools/ci/tests/shell-audit-apple.test.sh
bash tools/ci/tests/shell-audit.test.sh               # the existing web/extension test still passes
bash tools/ci/tests/shell-audit-waiver.test.sh
bash deploy/acceptance/verify-shells-accessibility.sh
```

Expected: three `ok` lines; the existing tests unchanged; all six shells still declare conformance.

**Step 5: Commit.**

```bash
git add clients/shells/apple/audit.sh clients/shells/apple/tools/audit-failures.js \
        clients/shells/apple/Sources/MidgardAuditKit clients/shells/tools/record-audit.sh \
        tools/ci/tests/shell-audit-apple.test.sh tools/ci/tests/fixtures/xcresult-tests.json
git commit -m "clients/shells/apple: on-device accessibility audit hook; portable audit record stamping [RC-49] [AC-C-11]"
```

---

### Task A15: CI wiring, documents, full local verification

**Changed by A11.** The release checklist gains: (1) both apps carry the acknowledgements entry "UniFFI 0.28.3, Mozilla Public License 2.0, source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3"; (2) the committed or bundled Swift bindings equal a fresh `uniffi-bindgen` run (never hand-edited, MPL-2.0 §3.2); (3) the licence exceptions in `supply-chain/licences.toml` expire 2027-05-13, so renew or remove them before then. Also update `clients/shells/SHELL_BUILD.md` (it still says UniFFI is not vendored).

**Files:**
- Modify: `clients/shells/SHELL_BUILD.md` (the Apple binding is built: what, where, how), `clients/shells/README.md` (Apple row: status), `clients/README.md` (the "scaffold only" note for `clients/ffi` and the Apple shell)
- Modify: `docs/compliance/claims.toml` rows `client-core` (persistence now exists: the fallback's "It keeps its state in memory" is no longer true) and `client-shells` (language bindings exist for Apple; status stays `spec` until an app runs), then `bash tools/ci/claims.sh --render`
- Modify: `docs/compliance/NUMBERS.md` (regenerated)

**Step 1: Write the failing check.** `claims.sh --check` does not test wording, so the "test" here is the full verification list below run once before editing: record which lines fail.

**Step 2: Run the full local verification.**

```bash
export PATH=/opt/homebrew/opt/rustup/bin:$PATH CARGO_TARGET_DIR=$HOME/Library/Caches/midgard-build/target-clients
cargo fmt --all --check
cargo clippy --offline --locked -p midgard-client-sync -p midgard-client-keystore -p midgard-client-store \
    -p midgard-client-broker -p midgard-client-delivery -p midgard-client-core --features backend-fips --all-targets -- -D warnings
cargo clippy --offline --locked -p midgard-client-enclosure -p midgard-client-inference --features test-backend --all-targets -- -D warnings
cargo clippy --offline --locked -p midgard-client-ffi --features backend-fips --all-targets -- -D warnings
cargo clippy --offline --locked -p attest -p mg-sign -p mg-verify --all-targets -- -D warnings
for p in midgard-client-sync; do cargo test --offline --locked -p $p; done
cargo test --offline --locked -p midgard-client-keystore --features backend-fips
cargo test --offline --locked -p midgard-client-store --features backend-fips
cargo test --offline --locked -p midgard-client-core --features backend-fips
cargo test --offline --locked -p midgard-client-ffi --features backend-fips
cargo test --offline --locked -p attest
cargo test --offline --locked -p attest --no-default-features --features backend-fips
bash deploy/acceptance/verify-clients.sh        # AC-C-1..13 still pass (runs cargo test per criterion)
bash tools/ci/no-test-backend.sh
bash tools/ci/vendor-check.sh
bash tools/ci/release-artefacts.sh --check
bash deploy/acceptance/verify-shells-accessibility.sh
for t in demo-fixtures apple-xcframework shell-audit-apple shell-audit shell-audit-waiver release-artefacts vendor-check numbers; do
    bash tools/ci/tests/$t.test.sh >/dev/null && echo "ok    $t" || echo "FAIL  $t"
done
/opt/homebrew/bin/bash tools/ci/provenance-lint.sh v0.1.0-rc.3
/opt/homebrew/bin/bash tools/ci/claims.sh --check
bash tools/ci/numbers.sh --check
(cd clients/shells/apple && bash tools/build-xcframework.sh && swift test && bash tools/check-contract.sh && node tools/gen-tokens.mjs --check)
bash clients/shells/apple/tools/clean.sh
```

Everything must pass except `numbers.sh --check`'s gate-record section, which reports "not run at this commit" until a gate run is recorded for this branch (that is the sheet being honest, not a defect; `--write` makes the committed sheet say so). `verify-clients.sh` exports `PATH=/usr/local/cargo/bin:$PATH` for the toolchain image; on this Mac it finds `cargo` through the rustup `PATH` set above. The full `just test` and `just lint` (every package) and `repro` run in the toolchain image: run `bash tools/ci/run.sh toolchain` once, then `bash tools/ci/run.sh test` and `bash tools/ci/run.sh repro`, before asking for review — they are the jobs most likely to notice the new `staticlib` crate type (see Risks: reproducibility).

**Step 3: Update the documents.** In `clients/shells/SHELL_BUILD.md`, replace "the binding crates (`uniffi`, `wasm-bindgen`) are **not yet vendored**" with what is now true for UniFFI (vendored at 0.28.3; `clients/ffi`; `clients/shells/apple/tools/build-xcframework.sh`; `wasm-bindgen` still not vendored), and add a short "Apple" section: prerequisites (Xcode 26.2 or 16.x, the two iOS Rust targets), the build command, the package layout, and that the audit runs from `clients/shells/apple/audit.sh`. In `claims.toml`, change the `client-core` fallback's last sentence to say the sealed cache and offline queue persist across a relaunch, and the `client-shells` fallback to say the Apple bindings and package exist and no app has been audited; keep both statuses; then `claims.sh --render`.

**Step 4: Re-run the checks that read these files.**

```bash
/opt/homebrew/bin/bash tools/ci/claims.sh --check
```

**Step 5: Commit.**

```bash
git add clients/shells/SHELL_BUILD.md clients/shells/README.md clients/README.md docs/compliance/claims.toml \
        docs/compliance/CLAIMS.md
bash tools/ci/numbers.sh --write && bash tools/ci/numbers.sh --check | tail -2
git add docs/compliance/NUMBERS.md
git commit -m "docs: Apple binding and shared base built; claims and numbers follow [RC-49] [FR-C-1]"
```

**Phase A is done when** every command in Step 2 passes on this Mac, `swift test` passes in `clients/shells/apple`, and `git log --show-signature v0.1.0-rc.3..clients_shared` shows only good signatures.


**As built (16 Nov).** The steps above are kept as written for the record; the work differs as follows.

1. *macOS test failures (A14 item 8k).* `claims.test.sh` (18), `numbers.test.sh` (3), `release-artefacts.test.sh` (2), and also `gate-record.test.sh` (16) and `advisory-refresh.test.sh` (8), failed on this Mac at the tag because the tests used GNU-only `sed`: `sed -i EXPR FILE` (BSD `sed` reads EXPR as the backup suffix and the file name as the script, hence "invalid command code f"), `\n` in a replacement, `0,/re/` and `{n;d}`. Each test now edits through a temp file and `mv` (`sed_i`), and uses backslash-newline, `1,/re/` and `{n;d;}`, which GNU and BSD `sed` read alike; the scripts under test were not changed, so Linux CI is unaffected (`68aa7930`). Still failing on macOS, and not changed because they test Linux-only or online tooling: `edge-egress` (iptables stubs and the installer's GNU `sed`), `helm-digests` (`set-digests` uses a GNU-only `sed` block), `mg-install` (the Linux installer uses `readlink -m` and `stat -c`), `js-artefacts` (corepack's pnpm is not cached and there is no network). They pass in the Linux `selftest` job, where they belong.
2. *MPL-2.0 release checklist.* New `clients/shells/apple/tools/check-bindings.sh` (`23ccc857`): a fresh `uniffi-bindgen` run over the built macOS arm64 library must equal `clients-xcframework/swift/*` and the package's `Sources/MidgardCore/midgard_client_ffi.swift`; `MidgardKit.Acknowledgements` must carry the UniFFI line (it does since A13, with `testAcknowledgementsNameUniFFI`); the UniFFI exceptions in `supply-chain/licences.toml` must not have expired, with a warning from 60 days before 2027-05-13. `apple-xcframework.test.sh` runs it against a scratch tree with a stub cargo, so `selftest` covers it on Linux. The checklist itself is in `clients/shells/SHELL_BUILD.md` ("Apple release checklist"); the apps' Settings → Acknowledgements screens and the release-notes line are Phase B.
3. *Documents.* `SHELL_BUILD.md` says UniFFI is vendored and `wasm-bindgen` is not, and gains the "Apple (built)" section (prerequisites, layout, every script: `build-xcframework.sh`, `verify-xcframework.sh`, `check-contract.sh`, `check-bindings.sh`, `gen-tokens.mjs`, `typecheck-ios.sh`, `clean.sh`, `audit.sh` and `MidgardAuditKit`). `clients/README.md` lists the real crates and shells and drops "scaffold only"; `clients/shells/README.md` gives the Apple status. Claims: `client-core` now cites the sealed per-persona cache and queue that survive a relaunch, device-audit signing (device-signed only after a check), and the release graph's refusal of `demo-unpinned` (evidence and four more tests), its limit records the Apple root's `NonExtractable` custody (owner decision 5), and its milestone becomes an application on the core; `client-shells` says the Apple bindings and package exist and no app is built or audited. Both statuses are unchanged; `claims.sh --check --strict` passes. `docs/security/THREAT_MODEL.md` gains four client rows (persistent sealed cache and Apple custody; unwiped FFI copies of the data key, risk 17; audit rows without an action id, risk 16; `demo-unpinned` kept out of release builds).
4. *Already done by earlier tasks.* `clean.sh` already removes `clients-derived` and `clients-archive` (A12) and `clients-swift` (A13), and its test checks it. The audit record's device field names both devices (A14 item 8c: `audit.sh` writes `apple-<release>.device`, which `run.sh` records unless `MIDGARD_AUDIT_DEVICE` is set). CI wiring needs no pipeline change: `selftest` (both forges) runs every `tools/ci/tests/*.test.sh`, including `apple-xcframework`, `shell-audit-apple`, `shell-audit-gate`, `demo-fixtures` and `gen-read-models` (the last two skip without node and run for real in `js-build`); `just test`/`just lint` (the `test`/`lint` jobs) include `midgard-client-ffi`; `build`/`check-backends` run `no-test-backend.sh`, which refuses `demo-unpinned`; `repro` runs on every change on both forges; the Apple audit runs only on the macOS runners (`shell-audit.yml`'s `macos-15`, GitLab's `macos` tag). `pipelines.sh --check` passes. No per-change CI job builds the XCFramework or runs `swift test`; they run on a Mac (locally, and in the audit once the apps exist).
5. *Reproducibility (risk 10) and Linux checks.* Docker is not running on this Mac and origin's push URL is disabled, so no CI runs this branch. The coordinator ran the jobs in Docker (16 Nov): **`test` passed** (1,860 tests, `rust:1.82` toolchain image); **`repro` passed** (140 of 140 artefacts bit-reproducible, `libmidgard_client_ffi.a` included); **`selftest` on `ubuntu:24.04`** (the GitHub hygiene job; node 20.18.1) **failed** `demo-fixtures.test.sh` and `gen-read-models.test.sh` with `node: bad option: --experimental-strip-types` (node 22.6 or later is needed; `package.json` engines say `>=22.12`). Fixed in the commit after this note: the hygiene job installs node 22 (`actions/setup-node`, the same pinned action and `'22'` as `release.yml`'s js-build step); both tests fail with a clear message on a node older than 22.6 rather than a node error (and still skip without node, as GitLab's alpine `selftest` has none); and the read-model generator checks every module's shapes before running rustfmt, so its refusal cases pass without rustfmt on PATH (one did not). **Pending:** re-run `selftest` on `ubuntu:24.04` with node 22 (Docker or a private CI remote) before Phase B is shared or merged. Failures on the alpine `selftest` (GitLab's hygiene image) are **pre-existing at `v0.1.0-rc.3`** and left as they are: `gate-record.test.sh` ("the difference is named"), `helm-digests.test.sh`, and `numbers.test.sh` (a row count that varies between runs).
6. *Verification on this Mac* (16 Nov, `numbers.sh --check` green at each commit): every Step 2 command passed — fmt; the four clippy lines; the client, ffi and attest tests; `verify-clients.sh` (14 passed, Phase 8 definition of done met); `no-test-backend`, `vendor-check`, `release-artefacts --check`, `verify-shells-accessibility`; provenance, `claims --check --strict`, numbers, `pipelines --check`; `build-xcframework.sh` (verify-xcframework passed), `swift test` (104 tests, 0 failures), `check-contract` (22 bindings), `check-bindings`, `gen-tokens --check`, `typecheck-ios` (device and simulator); and 37 of the 41 `tools/ci/tests` (the four Linux-only ones of item 1 excepted). `clean.sh` was not run at the end: Phase B reuses `target-clients`, `clients-swift` and `clients-xcframework`.

7. *Final review fixes (16 Nov).* `client-shells` says the Enclave key root is built on a Mac and tested with a software key (the Enclave path first runs in a signed app, Phase B); `advisory-refresh.test.sh` builds the forge variables in a function (`forge_env`), since bash 3.2 misreads a `case` inside `$(...)`, and passes on `/bin/bash` 3.2 and bash 5; `SecureEnclaveKeyRoot`'s doc says an unsigned build is refused (`.unsignedBuild`) and the software root is only for the simulator or a Mac without an Enclave; the root is in memory "mlocked where the OS allows" (THREAT_MODEL, `sealed_root.rs`); `ClientError::NotFound` names the dataset by file name only (`a_missing_dataset_is_named_by_file_name_only`); `verify-xcframework.sh` refuses a slice with test-only symbols (`from_body`, `*_for_tests`) by `nm`; `check-bindings.sh` refuses an expiry that is not `YYYY-MM-DD`.

## Phase A complete (16 Nov 2026)

Branch `clients_shared` (on `v0.1.0-rc.3`, all commits signed) holds the shared base: the Rust core gains markings, the six generated console read models, the data-source trait, the signed demo dataset and pinned demo key, the demo enclosure with signed audit rows, the sealed persistent per-persona cache and queue, the device-audit key and the session façade (A2–A10); `clients/ffi` exports it through vendored UniFFI 0.28.3 under time-limited MPL-2.0 exceptions (A11); `build-xcframework.sh` makes and verifies the four-slice XCFramework (A12); the `MidgardKit`/`MidgardAuditKit` Swift package carries the tokens, Secure Enclave key root, sign-in, lock, chips, acknowledgements and the accessibility prechecks, with 104 macOS tests (A13, A14); `audit.sh` and the CI gate record an audit only from both apps (A14). A15 adds the release checklist's binding check, the documents, and macOS-safe CI tests. **Pending gate before Phase B is shared or merged:** `test` (1,860 passed) and `repro` (140/140) have passed in Docker; re-run `run.sh selftest` on `ubuntu:24.04` with node 22 (Docker or a private CI remote), since its first run failed on node 20 (fixed, As built item 5). The alpine `selftest` failures in `gate-record`, `helm-digests` and `numbers` are pre-existing at the tag and stay open. Open for Phase B: the two app projects (team ID from the owner), the first Secure Enclave enrol/unlock/relaunch on a signed build, Mac and iPhone (the package tests use a software key), the acknowledgements screen and release-notes line, the iOS simulator runtime (about 8 GB, after `df -h ~` and the owner's go-ahead), the first `shell-audit apple` run on `macos-15`, and the toolchain-image `test`/`repro` results from CI.

---

## Phase B — app targets (branches `clients_mac` and `clients_iphone`)

Moderate detail: each task names its files, what it must do, its tests and its acceptance. Before starting, expand each task into Phase-A-style steps.

**Shared conventions for both apps.**
- Project generation: **XcodeGen** (`brew install xcodegen`), with `project.yml` committed **and** the generated `.xcodeproj` committed, because `audit.sh` and CI look for the project directory and must not depend on a generator being installed. A test (`tools/ci/tests/apple-projects.test.sh`) regenerates into a temp directory and diffs, so the two never drift. (Alternative if the owner prefers no extra tool: create the projects once in Xcode and commit them; the drift test then goes away. Open questions, Q8.)
- Signing: automatic. `Apps/<App>/Config/Signing.xcconfig` (committed) sets `CODE_SIGN_STYLE = Automatic`, `DEVELOPMENT_TEAM = $(MIDGARD_TEAM_ID)` and ends with `#include? "Signing.local.xcconfig"`; `Signing.local.xcconfig` (gitignored) holds the one line `MIDGARD_TEAM_ID = <team id>` that the owner fills in. Without it, builds for the simulator still work; device and notarised builds stop with Xcode's "requires a development team" error.
- The dataset: a "Copy demo dataset" build phase copies `clients/demo/datasets/<release>.mgdemo` (release read from the root `Cargo.toml`) into the app's resources and fails the build if it is missing — so an app can never ship without the signed dataset for its release. The core verifies it again at launch.
- The XCFramework: a pre-build phase runs `tools/build-xcframework.sh` only when `Frameworks/MidgardCoreFFI.xcframework` is missing or older than any file under `clients/core/src`, `clients/ffi/src` or `clients/demo/datasets`.
- Launch: build the environment with `AppModel.start(making: { try AppEnvironment.live(reason:device:) })`, never by calling `AppEnvironment.live` on the main thread — it reads the keychain and the disk, and at enrolment probes the keychain with a write (A13, "As built" 3 and 13). An unsigned build shows the failed state "cannot keep keys"; the Mac app therefore needs the owner's team ID even for local runs.
- Merging: A13 adds `clients/shells/apple/Package.swift`, which makes `tools/ci/run.sh shell-audit apple` look for `clients/shells/apple/audit.sh` (Task A14). Do not merge `clients_shared` to `dev` with A13 and without A14.
- Root-view hooks used by the audit (Task A14): `-MidgardUITesting` (Debug only) selects `TestKeyRoot`-style non-prompting key root and a fresh temporary store; `MIDGARD_REDUCE_MOTION=1` or `accessibilityReduceMotion` sets a `MotionPolicy` that disables every `withAnimation`; as built (B2/B3, *pending owner approval of the visible status line*), each app's root view ends in a visible status line of static text whose accessibility value the tests read: "Animations on" / "Animations off (Reduce Motion)" is `mg.motion` = `full`/`reduced` (every phase), and, once signed in, "Console n of 6: Name" is `mg.console.selected` = `1…6`. Not hidden elements, as first planned: a hidden one-point marker was an AppKit group with no readable value on macOS, and every hidden-text variant either failed the contrast audit or left the accessibility tree; focusable elements on audited screens carry identifiers `mg.<n>.<name>` in reading order. The iOS `keyboard_navigation` audit needs the simulator's hardware keyboard connected and Full Keyboard Access on: set them before `xcodebuild test` (for example `defaults write com.apple.iphonesimulator ConnectHardwareKeyboard -bool true`, and Full Keyboard Access through `xcrun simctl spawn <device> defaults write com.apple.Accessibility FullKeyboardAccessEnabled -bool true`), confirm on the runner that Tab then moves focus, and add the steps to `audit.sh`; until that is confirmed, expect the iOS keyboard test to fail (it never passes by default). The root view uses MidgardKit's `MotionPolicy`/`LoadingView` (A14, "As built" 8f).

### Task B1: Branches and worktrees

**Files:** none in the tree.

```bash
cd ~/Library/Caches/midgard-build/wt-clients-shared
git branch clients_mac v0.1.0-rc.3
git branch clients_iphone v0.1.0-rc.3
git worktree add ~/Library/Caches/midgard-build/wt-clients-mac clients_mac
git worktree add ~/Library/Caches/midgard-build/wt-clients-iphone clients_iphone
for w in wt-clients-mac wt-clients-iphone; do
    (cd ~/Library/Caches/midgard-build/$w && git merge --no-ff clients_shared -m "Merge clients_shared: shared Apple client base" && git log --show-signature -1 | head -3)
done
```

Acceptance: both merges are signed; `git diff clients_shared clients_mac` is empty. Later changes to `clients_shared` are merged the same way (never cherry-picked), so the shared base stays one history.

### Task B2: macOS app project (`clients_mac`)

**Files:** `clients/shells/apple/Apps/MidgardMac/{project.yml, MidgardMac.xcodeproj, Config/Signing.xcconfig, Config/MidgardMac.entitlements, Info.plist, Sources/MidgardMacApp.swift, Sources/RootView.swift, Sources/Commands.swift, UITests/AccessibilityAudit.swift, UITests/SessionFlowTests.swift}`, `clients/shells/apple/.gitignore` (+ `Signing.local.xcconfig`), `tools/ci/tests/apple-projects.test.sh`.

What it does:
- Target `MidgardMac` (macOS 14, `PRODUCT_BUNDLE_IDENTIFIER` from Open questions Q8, Hardened Runtime, App Sandbox with no network entitlement — the app fetches nothing; `keychain-access-groups` = `$(AppIdentifierPrefix)<bundle id>` for the data-protection keychain the Secure Enclave key record needs). Depends on the local package `MidgardKit` at `../..`.
- `RootView`: `AppModel.start(keyRoot: KeyRootFactory.make(reason: "Unlock Midgard"), device: DeviceInfo(ramMb: physicalMemory/1 MiB, hasAccelerator: true))`; shows `PersonaPickerView` / `LockView` / the console shell by `phase`; header carries `StatusChipsView` and the persona menu (switch persona, lock, offline toggle).
- Console shell: `NavigationSplitView` — sidebar lists the six consoles (Midgard, Admin, Heimdall, Forge, Mimir, Bifrost; ⌘1…⌘6) and, under the selected console, its screens; content column lists rows (sortable `Table` where the web uses a table), detail column is the inspector. Each console screen is a placeholder view ("Arrives in Phase C…H") until its phase; the shell itself is complete.
- Commands: ⌘1–⌘6 consoles, ⌘R sync now, ⌘L lock, ⇧⌘O toggle offline, ⌘N new window (multiple windows via `WindowGroup(for: ConsoleRoute.self)`), Settings scene with "Reset demo" (confirmation).
- Idle lock: `AppModel.touch()` from an `NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .scrollWheel])` monitor; lock on `NSApplication.willResignActive` after the grace period.
- UI tests: `AccessibilityAudit: AccessibilityAuditBase` with `screens` = sign-in, console list (each console's first screen), one detail; `SessionFlowTests`: launch with `-MidgardUITesting`, enrol, sign in as `k.rao`, see a withheld row, go offline, see the sync chip say `OFFLINE`, lock, unlock.

Acceptance: `xcodebuild -project … -scheme MidgardMac -destination platform=macOS test` passes; `bash clients/shells/apple/audit.sh 0.1.0-rc.3 shell-audit/apple.json` prints only `dynamic_type` (measured on iOS; see A14) and exits 0; the app launches from a Developer-ID-less local build with the owner's team set.

### Task B3: iOS app project (`clients_iphone`)

**Files:** `clients/shells/apple/Apps/MidgardiOS/{project.yml, MidgardiOS.xcodeproj, Config/Signing.xcconfig, Config/MidgardiOS.entitlements, Info.plist, Sources/MidgardiOSApp.swift, Sources/RootView.swift, UITests/AccessibilityAudit.swift, UITests/SessionFlowTests.swift}`.

What it does:
- Target `MidgardiOS` (iOS 17, iPhone; runs scaled on iPad per the design's scope), `NSFaceIDUsageDescription` ("Midgard uses Face ID to unlock the encrypted demo store on this device."), keychain access group as macOS, `UIFileSharingEnabled = NO`, no network usage strings.
- Root: same phases as macOS. Console shell: `TabView` with the six consoles (Admin and Midgard first, as the build order), each a `NavigationStack` (list → detail), `.searchable` on lists, `.refreshable` → `syncNow`, sheets for actions and confirmations, the persona/lock/offline menu in the toolbar. Hardware-keyboard shortcuts ⌘1…⌘6 via `.keyboardShortcut` on hidden buttons (the keyboard-navigation audit uses them).
- Idle lock: touch on any tap via a root `simultaneousGesture`, and lock when `scenePhase` becomes `.background`; the app switcher snapshot shows a privacy cover while locked.
- UI tests as macOS, on the `iPhone 16` simulator (or `MIDGARD_AUDIT_SIMULATOR`).

**Disk check before installing a simulator runtime** (about 8 GB, design §4):

```bash
df -h ~ | tail -1
xcrun simctl runtime list
# only if free space allows, and after confirming the 10 GB suite budget with the owner:
xcodebuild -downloadPlatform iOS
```

Acceptance: `xcodebuild test -scheme MidgardiOS -destination 'platform=iOS Simulator,name=iPhone 16'` passes; `audit.sh` on this branch prints nothing (all seven measured on iOS) — or names what failed.

### Task B4: Developer ID DMG and notarisation script (macOS)

**Files:** `clients/shells/apple/tools/release-mac.sh`, `clients/shells/apple/Apps/MidgardMac/Config/ExportOptions-DeveloperID.plist`.

What it does: `xcodebuild archive` (Release, automatic signing with the team) into `~/Library/Caches/midgard-build/clients-archive/`; `xcodebuild -exportArchive` with `method = developer-id`; `hdiutil create -fs HFS+ -srcfolder … -format UDZO Midgard-<release>.dmg`; `codesign --verify --deep --strict` and `spctl -a -t open --context context:primary-signature` on the result. **Notarisation is a separate, gated step**: it runs only with `--notarise`, only when `MIDGARD_APPLE_UPLOAD_OK=1` is set for that one invocation, and only after printing exactly what will be sent and reading the word `upload` from the terminal; it uses `xcrun notarytool submit --keychain-profile <profile> --wait` (credentials stored once by the owner with `notarytool store-credentials`, never in the repository), then `xcrun stapler staple`. Without the flag the script stops after the local DMG and says that nothing was uploaded.

Acceptance: a dry run (`release-mac.sh`) produces a signed, verified DMG and uploads nothing; `--notarise` without `MIDGARD_APPLE_UPLOAD_OK=1` refuses.

### Task B5: TestFlight archive script (iOS)

**Files:** `clients/shells/apple/tools/release-ios.sh`, `clients/shells/apple/Apps/MidgardiOS/Config/ExportOptions-AppStore.plist` (`method = app-store-connect`, `destination = export`).

What it does: archive and export an `.ipa` locally. Upload is the same gated pattern as B4 (`--upload`, `MIDGARD_APPLE_UPLOAD_OK=1`, typed confirmation), using `xcrun altool --upload-app` with an App Store Connect API key the owner provides through the environment at that moment. App Store submission is out of scope.

Acceptance: the default run produces an `.ipa` under `clients-archive/` and uploads nothing.

### Task B6: Clean-up and documentation

`clean.sh` also removes `clients-archive` and `clients-derived` (already listed in A12); `clients/shells/apple/README.md` (new) explains the layout, prerequisites, the two apps, signing set-up (`Signing.local.xcconfig`), and the gated upload steps. Each local run ends with `bash clients/shells/apple/tools/clean.sh` so `~/Library/Caches/midgard-build/clients-*` does not persist.

---

## Phases C to H — the consoles

One phase per console, in the design's build order: **C** Midgard, **D** Admin (C and D together are the design's first step, "Midgard and Admin"), **E** Heimdall, **F** Forge, **G** Mimir, **H** Bifrost; a short cross-console finish follows Phase H. Each console is finished on **both** apps before the next starts: work lands on `clients_shared` (Rust, bindings, `MidgardKit` view models and shared views) and is merged into `clients_mac` and `clients_iphone`, where only the platform shells' navigation entries change. **Each phase is expanded into full step detail, in Phase A's style, before it starts.**

### The same six tasks in every console phase

| # | Task | Files (pattern) | Tests | Acceptance |
|---|---|---|---|---|
| 1 | **Typed bindings.** For each collection the console reads, an FFI accessor returning `Vec<<Type>Row>` where `<Type>Row { position, id: Option<String>, marking: MarkingChip, item: Option<<Type>> }` (`item` is `None` when withheld). Phase C extends `gen-read-models.mjs` with `--rows` so the row wrappers and the accessor bodies are generated from a collection→type table (`clients/demo/collections.json`, written by the extractor). | `clients/ffi/src/consoles/<console>.rs`, `clients/ffi/src/generated/*` | `clients/ffi/tests/<console>.rs`: every accessor returns the committed dataset's rows, withheld for the analyst persona where the overlay says so | `check-contract.sh` passes with the new bindings called in `ContractTests` |
| 2 | **Enclosure behaviour.** Computed reads and actions, **ported from the console's TypeScript mock** method by method (the mock is the specification the web demo shows). Computed reads go through a new `DataSource::compute(grant, verb, params) -> Result<Vec<u8>, SourceError>` (added in Phase C; online only, the session caches the last answer per input); actions through the existing action pipeline (`DemoEnclosure::register`). Parity: the extractor records the mock's answers for a fixed list of inputs into `clients/demo/fixtures/<release>/golden/<console>.json` (ids and clock fields normalised), and the Rust tests compare. | `clients/core/src/demo/actions/<console>.rs`, `clients/demo/tools/extract-fixtures.mjs` | `clients/core/tests/<console>_actions.rs`: golden parity; refusals (above clearance, wrong role, self-approval, missing human gate); idempotent re-push; conflict on a decided item | `cargo test -p midgard-client-core` green; the dataset is rebuilt (`just demo-dataset`) if fixtures changed |
| 3 | **View models.** One `@MainActor ObservableObject` per screen in `MidgardKit`, calling the core only through `AppModel.call`; state enums `loading / rows / empty / withheld / offline(cached) / refused(reason) / failed`. | `clients/shells/apple/Sources/MidgardKit/Consoles/<Console>/<Screen>Model.swift` | `Tests/MidgardKitTests/<Console>/<Screen>ModelTests.swift` against the real core with `TestKeyRoot` and the committed dataset | every screen state reachable in a test |
| 4 | **Shared views.** SwiftUI views used by both apps: rows with marking chips, withheld rows, detail inspectors, action sheets with the human-gate step, the console accent (`MGAccent`), identifiers `mg.<n>.<name>` on focusable elements. | `…/Sources/MidgardKit/Consoles/<Console>/Views/*.swift` | snapshot-free: covered by the UI tests | VoiceOver labels on every control; no fixed font sizes |
| 5 | **Platform layouts.** macOS: sidebar entry, `Table` with sortable columns, inspector, Swift Charts where the web shows a chart, menu commands. iOS: tab, list → detail, sheets, search, pull to refresh; the graph editors are view-and-configure only. | `Apps/MidgardMac/Sources/<Console>/*`, `Apps/MidgardiOS/Sources/<Console>/*` (on the app branches) | — | builds on both branches |
| 6 | **UI flows and audit screens.** XCUITest per console on both apps: sign in, browse, open a detail, take an action, see it sync (and, offline, see it queue). Add the console's main screens to `AccessibilityAudit.screens`. | `Apps/*/UITests/<Console>FlowTests.swift`, `Apps/*/UITests/AccessibilityAudit.swift` | the flows | `audit.sh` reports no failure for the console's screens |

The web screens are the reference for what each screen shows and does; each is cited below by file.

### Phase C — Midgard (11 screens, `products/midgard/ui/src/screens/`) — accent `MGAccent.midgard`

Phase C also does the one-off groundwork the later phases reuse: `gen-read-models.mjs --rows`, `clients/demo/collections.json`, `DataSource::compute` and the session's cache of computed answers, and the `golden/` fixtures in the extractor.

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Object Fabric (`ObjectFabricScreen.tsx`) | The registry's object types (properties, instance count, marking/compartment, links, actions); actions on **one** object; a human-gated action is requested, approved by a **second** person, then run by the requester with that approval's id; every outcome or refusal shown | `midgard.objectTypes`, `midgard.objects`, `midgard.audit` | actions `midgard.requestApproval` (type, target), `midgard.decideApproval` (approval id, approve), `midgard.applyAction` (type, target, gate token); runtime collection `midgard.approvals` (`Approval`); port of `MockGateway.applyAction/requestApproval/decideApproval` incl. one-target rule and TTL |
| Data Search (`DataSearchScreen.tsx`) | Pick a type, add conditions (`eq ne lt lte gt gte contains`, `*` any text), order, page; `?q=` searches all types; counts cover only what the user may read | `midgard.objectTypes`, `midgard.objects` | compute `midgard.query` (`QueryRequest` semantics; the mock ignores conditions, the port applies them to `propertyValues` — note the difference in the golden test) |
| Search Around (`SearchAroundScreen.tsx`) | Seed + hop depth → graph grouped by hop, each node's properties; the heading names the seed; failures replace results | `midgard.searchAround` | compute `midgard.searchAround` (from the recorded graph, truncated to the requested hops) |
| Policies (`PoliciesScreen.tsx`) | Rules table; dry run: purpose + marking against the session's policy → signed grant/deny with matched rule; New/Edit disabled | `midgard.policies` | compute `midgard.simulatePolicy` (port of `MockGateway.simulatePolicy`) |
| Identity (`IdentityScreen.tsx`) | Users, services, agents with clearance, roles, groups, status — SAMPLE DATA, controls disabled | `midgard.identities` | — |
| Keys (`KeysScreen.tsx`) | PQ key hierarchy with custody, wrap state, rotation — SAMPLE DATA | `midgard.keys` | — |
| Audit (`AuditScreen.tsx`) | Signed ledger, substring filter, category tags (actions, queries, writes, sign-ins, administration, agent), "Export chain" | `midgard.audit` | export → share sheet (iOS) / save panel (macOS) of the visible rows only |
| Compute (`ComputeScreen.tsx`) | Jobs, backend, state, progress — SAMPLE DATA | `midgard.computeJobs` | — |
| Observability (`ObservabilityScreen.tsx`) | SLO readouts with meters — SAMPLE DATA | `midgard.observability` | Swift Charts gauges |
| Resilience (`ResilienceScreen.tsx`) | Rollouts and health gates — SAMPLE DATA | `midgard.resilience` | — |
| SDK (`SdkScreen.tsx`) | Contract surface (services, RPCs); "Download SDKs" disabled with where they are | `midgard.contractSurface` | — |

Flows to test on both apps (Task 6): the dual-control object action end to end with persona switching (`j.benjamin` requests; sync; switch to `k.osei`; approve; sync; switch back; run with the gate token); an analyst (`k.rao`) sees `Sensor Feed` (confidential // SIG) withheld in the registry; Data Search offline serves the cached last query and says so.

### Phase D — Admin (8 screens, `products/admin/ui/src/screens/`) — no product accent in the design system: uses Midgard's

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Overview (`OverviewScreen.tsx`) | Subsystem health, SLOs, security counters, pending controls; "super-admin NOT DEFINED" | `admin.overview` | — |
| Approvals (`ApprovalsScreen.tsx`) | Dual-control queue; approving routes a **second** approver; self-approval denied live (AC-AD-4); denials audited (AC-AD-6) | `admin.approvals` | action `admin.approve` (id) — port of `AdminMock.approve`, approver = signed-in persona |
| IAM (`IamScreen.tsx`) | Roles (scope, ceiling, read-only, holders) and bindings (user, role, scope, expiry, MFA); no super-admin | `admin.roles`, `admin.bindings` | — |
| Config (`ConfigScreen.tsx`) | Versioned config: diff → simulate → review → commit → rollback, each signed | `admin.configCommits`, `admin.configSimulations` | compute `admin.simulateConfig`; actions `admin.commitConfig`, `admin.rollbackConfig` |
| Ledger (`LedgerScreen.tsx`) | One ledger across subsystems, search, live tail, read-only assistant | `admin.ledger` | compute `admin.searchLedger` (port) |
| Registry (`RegistryScreen.tsx`) | Model registry, agent scopes, evals gates, release admin; lifecycle changes dual-controlled | `admin.registry` | — |
| Break-Glass (`BreakGlassScreen.tsx`) | Request (justification) → time-boxed active → post-hoc review | `admin.breakGlass` | action `admin.requestBreakGlass` (reason) |
| Copilot (`CopilotScreen.tsx`) | Advisory thread; drafts/explains/simulates, never executes; refusals with a named code | `admin.copilot` | — (read-only thread; no free-text generation in v1) |

Flows to test on both apps (Task 6): an approval decided by a second persona; self-approval refused live and the refusal visible in the Ledger; a config commit simulated, committed and rolled back; break-glass requested and shown active with its time box.

### Phase E — Heimdall (6 screens, `products/heimdall/ui/src/screens/`) — accent `MGAccent.heimdall`

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Operating Picture (`OperatingPictureScreen.tsx`) | Tracks on a map with toggleable layers (air, maritime, ground, feed coverage) and range rings; alerts; the inspector ranks AI options, each cited, each authorised through a human gate (rank 3 "hold" opens no workflow) | `heimdall.picture`, `heimdall.tracks`, `heimdall.entities`, `heimdall.motion` | MapKit: `Map` with `MapPolyline` trails and `Annotation`s from `heimdall.motion` (the synthetic hour; a time scrubber replays it); authorising an option calls `heimdall.authoriseWorkflow` on the workflow its rank maps to (`WORKFLOW_FOR_RANK` in the screen file: 1 → `W-0912`, 2 → `W-0913`, 3 opens none) |
| Entities (`EntitiesScreen.tsx`) | Tracked entities with class, confidence, corroborating feeds, anomalous state; search | `heimdall.entities` | — |
| Feeds (`FeedsScreen.tsx`) | Feeds with rate, latency, fusion weight, PQ authentication; unverified feeds quarantined | `heimdall.feeds` | — |
| Picture Sync (`PictureSyncScreen.tsx`) | Sites, roles, lag, entities visible, marking-filtered replication | `heimdall.syncSites` | — |
| Workflows (`WorkflowsScreen.tsx`) | Human-gated action workflows; authorise with a gate token → signed audit id | `heimdall.workflows` | actions `heimdall.authoriseWorkflow` (port of `HeimdallMock.authoriseWorkflow`, `HumanGateRequiredError` without a token) |
| Decision Lineage (`DecisionLineageScreen.tsx`) | Signed decision records, chain verification counters, live lineage tail | `heimdall.decisions`, `heimdall.audit` | — |

Map tiles: see Risks (MapKit fetches Apple's tiles at runtime). Until the owner decides (Q7), the map uses `.mapStyle(.standard(emphasis: .muted))` and a bundled offline fallback is not built. Tracks above clearance are withheld before they reach the map (the track rows carry their entity's marking).

### Phase F — Forge (7 screens, `products/forge/ui/src/screens/`) — accent `MGAccent.forge`

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Connectors (`ConnectorsScreen.tsx`) | Typed inbound-pull sources over PQ-mTLS, no egress | `forge.connectors` | — |
| Datasets (`DatasetsScreen.tsx`) | Versioned, branched, sealed datasets with diffs and lineage | `forge.datasets` | — |
| Data Health (`DataHealthScreen.tsx`) | Expectations, drift monitors, freshness SLOs gating projections | `forge.healthChecks` | Swift Charts for drift |
| Pipeline Builder (`PipelineBuilderScreen.tsx`) | DAG source → transform → dataset → projection; "Run" steps through staged phases with a signed build log | `forge.pipelines` | action `forge.runPipeline` (recorded `ForgeBuildResult` per pipeline replayed step by step; port of `ForgeMock.runPipeline`). macOS: canvas with draggable nodes and connections (draw new graphs); iOS: graph view + node settings sheet only |
| Transform IDE (`TransformIdeScreen.tsx`) | Pro-code transform; **preview before commit**: sandboxed diff, then human-gated apply | `forge.transform` | compute `forge.previewTransform`; action `forge.applyTransform` (gate token required) |
| Schedules (`SchedulesScreen.tsx`) | Batch and streaming cadence, triggers, run history | `forge.schedules` | — |
| Object Projection (`ObjectProjectionScreen.tsx`) | Columns → objects, links and markings; governance carried to query time | `forge.projections` | — |

The design's "drawing new graphs is macOS only" applies to Pipeline Builder and (Phase G) Logic Builder; on iOS the Transform IDE is a read-and-preview editor with a monospaced `TextEditor`, and applying stays human-gated on both.

### Phase G — Mimir (7 screens, `products/mimir/ui/src/screens/`) — accent `MGAccent.mimir`

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Model Plane (`ModelPlaneScreen.tsx`) | Served open-weight families, ML-DSA verified on load, never leaving the enclosure | `mimir.models` | — |
| Session Traces (`SessionTracesScreen.tsx`) | Plan–act–observe traces with per-step evals, guardrail screens, PQ-encrypted memory records | `mimir.sessions` | — |
| Agent Studio (`AgentStudioScreen.tsx`) | Agents; a canned, grounded exchange with live object citations and a human-gated state mutation | `mimir.agents` | the canned exchange is the `CANNED` constant and the follow-up answer in `AgentStudioScreen.tsx` (lines 25 and 97), not in the mock: copy them once into `clients/demo/studio.json` (citing those lines), add that file to the extractor's inputs so it is blob-checked like the others, and serve it as `mimir.studioExchange`; the gated mutation is an action `mimir.applyProposal` |
| Logic Builder (`LogicBuilderScreen.tsx`) | No-code agent logic; a mandatory, undeletable human gate between any mutating proposal and execution | `mimir.logic` | macOS: edit steps on a canvas (the gate step cannot be deleted — enforced in the core's handler, not only in the UI); iOS: view + step settings |
| Evals (`EvalsScreen.tsx`) | Suites with a 90% regression gate; Run → per-case pass/fail and score, gate verdict | `mimir.evals` | compute `mimir.runEval` (recorded `MimirEvalRun` per eval; golden parity) |
| Guardrails (`GuardrailsScreen.tsx`) | Rail inventory; live interception: submit an input, a hostile prompt is blocked at a rail with a refusal code before any tool call | `mimir.guardrails` | compute `mimir.simulateGuardrail` — Rust port of the two regexes in `MimirMock.simulateGuardrail` (`sdk/ts/src/mock/mimir.ts:320`) without a regex crate (literal alternations), golden-tested on the mock's own examples |
| Feedback (`FeedbackScreen.tsx`) | Governed feedback; poisoning attempts fail provenance and never enter retrieval | `mimir.feedback` | — |

The inference-tier chip matters here: offline, the tier shows on-device (demo flag, Q6), and the Studio exchange still plays from the fixture.

### Phase H — Bifrost (6 screens, `products/bifrost/ui/src/screens/`) — accent `MGAccent.bifrost`

| Screen | Shows / does (web) | Collections | Computed reads / actions |
|---|---|---|---|
| Release Console (`ReleaseConsoleScreen.tsx`) | Zero-downtime rollout; Promote advances the broad ring canary → broad one health-gated step at a time, with a per-ring meter, health-gate pill and the signed audit id of each step | `bifrost.releases`, `bifrost.rings`, `bifrost.fleet` | action `bifrost.promoteRelease` (release, ring) — port of `BifrostMock.promoteRelease`; role `release-manager` required (`a.holt`) |
| Channels (`ChannelsScreen.tsx`) | Cadence, subscribers, promotion policy | `bifrost.channels` | — |
| Fleet (`FleetScreen.tsx`) | Desired vs actual state, drift, signed on-host agents | `bifrost.fleet` | — |
| Artefacts (`ArtefactsScreen.tsx`) | Reproducible builds, SBOMs, ML-DSA signatures verified fail-closed before promotion | `bifrost.artefacts` | — |
| Bundles (`BundlesScreen.tsx`) | Offline bundles: cut, export, courier, verify, apply, receipt | `bifrost.bundles` | — |
| Rollback (`RollbackScreen.tsx`) | Standing plans, automatic triggers, drill history; a drill is signed lineage | `bifrost.rollbacks` | action `bifrost.rollback` (id) |

### After Phase H — finish across consoles

| Task | What | Acceptance |
|---|---|---|
| H1 Accessibility pass | Run `audit.sh` on a branch with both apps; fix every reported affordance; VoiceOver walk-through of each console on device | `audit.sh` prints nothing |
| H2 Offline and conflict pass | For every action: queue offline, reconnect, merge; provoke a conflict (approve the same item from two personas before syncing) and check the `CONFLICT` chip and the refused second decision | flows green on both apps |
| H3 Release build of both apps | `release-mac.sh` and `release-ios.sh` dry runs; check sizes; check the app bundles contain exactly one `.mgdemo` for the release and no other data | signed local DMG and `.ipa`; nothing uploaded |
| H4 Records | `shell-audit.yml` run (owner triggers it) produces `clients/shells/apple/audits/<release>.toml`; `verify-shell-audits.sh <release>` reports apple OK; the apple waiver line is no longer needed for that release | record committed by CI, not by hand |

---

## Risks, gaps and open questions

### What the tag does not have that the design assumes

| # | Gap | What the plan does |
|---|---|---|
| G1 | **No data-source interface.** `clients/enclosure` is a PQ-mTLS transport (`EnclosureClient::connect`); nothing defines what a client fetches. | Defines `source::DataSource` (A8); the demo enclosure implements it, a gateway client implements it later. |
| G2 | **No persistence.** `LocalStore` and `Replica` are memory-only (the `client-core` claims row says so). | Sealed cache export/import, `Replica::requeue`, sealed pending queue (A9, A10). |
| G3 | **No release demo key.** `deploy/trust/` holds only ceremony-made anchors and forbids development keys. | A separate demo-dataset key and domain, pinned under `clients/demo/trust/` (A6, A7). |
| G4 | **No recorded Heimdall hour** in the repository at the tag. | A synthetic, labelled hour from `HeimdallMock`'s tracks (A4). |
| G5 | **UniFFI is not vendored** (`clients/shells/SHELL_BUILD.md` says so). | Vendors UniFFI 0.28.3 (A11). |
| G6 | **No Apple project or `audit.sh`**; `record-audit.sh` cannot stamp a record on macOS (`sed -i` without a suffix is GNU-only). | Package root `Package.swift`, `audit.sh`, portable stamping (A13, A14). |
| G7 | The design counts **42 screens**; its own table lists **45** (11 + 6 + 7 + 7 + 6 + 8), matching the 45 screen files at the tag. | The plan covers all 45. |
| G8 | The mocks' Midgard **object instances are synthetic and thin** (`MockGateway.query`: five per type, two properties; conditions ignored). | The Apple apps show the same data as the web demo; Phase C's port applies conditions for real and its golden test records where it differs from the mock. Enriching the fixtures would make the apps diverge from the web demo. |

### Risks

1. **Licence (blocking until decided).** The eight UniFFI crates are MPL-2.0, outside `supply-chain/licences.toml`'s allow-list. Because `mg-sbom` lists the whole `Cargo.lock` in every release binary's SBOM, adding them without exceptions fails the licence scan of all 18 release binaries. A11 writes time-limited exceptions; they need an owner (Q1).
2. **Supply chain growth.** About 51 crates join `vendor/` (UniFFI's closure, including `serde`, `syn 3`, `goblin`, `askama`, `nom`, `weedle2`). Two carry informational "unmaintained" advisories (`paste`, `bincode`), which do not block but will appear in every scan and the weekly refresh. Resolution on 1.82 needs pins (`smawk 0.3.2`); any future `cargo update` must use the MSRV-aware resolver as A11 does.
3. **Custody floor.** The Secure Enclave cannot hold ML-KEM-1024, so the core's root is in (mlocked) process memory while unlocked and Enclave-wrapped at rest. Reported honestly as `NonExtractable`, it does **not** meet `PlatformProfile::Apple.floor() == HardwareBacked` (SAD_06 §11.2), so the apps open the store at `NonExtractable`. Either accept and document that for the demo, or add a floor between `NonExtractable` and `HardwareBacked` (Q5). The design's "seal and unseal inside the Enclave" is realised as Enclave ECIES: the Enclave key seals the core's 32-byte data key (`SecKeyCreateEncryptedData`) and unseals it with user presence (`SecKeyCreateDecryptedData`); the core derives its key-encryption key from that data key (A9, "As built"). Owner decision 5 accepted `NonExtractable`.
4. **Biometric gate.** Face ID / Touch ID is enforced by the Enclave key's access control (`.userPresence`), not by the core's `keystore::auth::verify_assertion`, whose assertions are ML-DSA (the Enclave signs only P-256). AC-C-9 ("no secret leaves the secure boundary") holds for the Enclave key; the core's own authenticator seam stays unused on Apple in v1.
5. **Secrets in Swift memory.** The ECDH shared secret crosses the FFI as `Data` and cannot be reliably zeroised in Swift; Rust wraps it in `SecretBytes` at once. The core's root secret never enters Swift.
6. **Keychain and signing.** The data-protection keychain used for the Enclave key record requires a signed app with a team (unsigned builds get `errSecMissingEntitlement`); development runs therefore need the owner's team ID. The iOS simulator has no Enclave: simulator and UI-test runs use the software key root and say so on the custody chip.
7. **Map tiles are fetched.** MapKit downloads Apple's map tiles at run time; with no network the Operating Picture shows tracks on a blank map. This conflicts with "nothing is fetched" (Q7).
8. **The bundle holds every row.** The dataset in the app holds all rows for all personas; clearance is enforced by the core (cache, UI), not by the bundle. Acceptable for demo data, but it is not what a real enclosure does.
9. **"ML-DSA ✓" is a label.** The mocks' audit rows carry the display string `ML-DSA ✓`; the apps would show the same claim without a signature behind it (Q11).
10. **Release build and reproducibility.** `clients/ffi` adds a `staticlib` to `cargo build --workspace --release` on Linux; `just repro` and `check-backends` have not seen a staticlib crate before. Run `tools/ci/run.sh repro` in the toolchain image before review (A15).
11. **CI audit runner.** `shell-audit.yml` runs on GitHub's `macos-15` image (Xcode 16.x, not 26.2); the simulator name, Xcode's `xcresulttool` JSON shape and rustup installation are runner facts that only a run confirms. One `apple` record covers both apps, but the apps live on two branches (Q4).
12. **Disk.** Four release builds of the crypto stack use 2–3 GB of target directory while running; an iOS simulator runtime is about 8 GB. With the 10 GB suite budget, install the runtime only after `df -h ~` and the owner's go-ahead; `clean.sh` after every session.
13. **Generated Swift and Swift 6.** UniFFI 0.28's Swift is not strict-concurrency clean; the package stays in Swift 5 mode (`swift-tools-version:5.10`). Moving to Swift 6 mode needs a newer UniFFI, which needs a newer Rust than 1.82.
14. **Determinism of the synthetic hour.** It uses `Math.sin`/`Math.cos` in Node; the determinism test guards it per Node version, but a Node major upgrade could change the last rounded digit of some samples and make the fixtures "stale".
15. **Local check tooling.** `provenance-lint.sh` and `claims.sh` use `mapfile` (bash 4); macOS's `/bin/bash` is 3.2. Use Homebrew bash or the toolchain image for those two.
16. **Swapped identical actions (gateway-era follow-up).** A source can answer two identical actions (same verb, target and parameters) with their audit ids swapped, and the device cannot tell: the rows carry no action id, so each device statement then names the other action. When a real gateway replaces the demo enclosure, the audit row the source (gateway) writes must carry the action id it answers, and `row_is_for` (`clients/core/src/session.rs`) must require that field to equal the recorded action id; that closes the gap. Until then the demo enclosure writes both rows itself, so the gap has no effect on the demo.
17. **Unwiped copies of the data key.** The 32-byte data key crosses the binding twice: to `seal` at enrolment and back from `unseal` at each unlock. Rust moves the unsealed bytes into `SecretBytes` (mlocked, zeroised) at once, but the copies UniFFI makes on the way (the `Vec<u8>` lowered into a `RustBuffer`, and the buffer freed after Swift reads it) cannot be wiped from Rust without `unsafe`, which this crate forbids itself. A memory-disclosure bug in the app after a lock could therefore recover the data key from freed memory, and with the sealed root, the ML-KEM root. The Swift side wipes its own `Data` copies (A13); a later UniFFI with custom buffer types, or a small audited `unsafe` wipe, would close the rest.

### Open questions for the owner

| # | Question | Plan's default until answered |
|---|---|---|
| Q1 | Approve MPL-2.0 exceptions for the eight UniFFI crates in `supply-chain/licences.toml` (owner name, expiry; the plan proposes 2027-05-13)? | A11 stops before Step 3c. |
| Q2 | Create the demo-dataset signing key on this Mac with `mg-sign keygen` (ML-DSA-65)? Where should the keystore and its passphrase live (default `~/.midgard/demo-keys`, outside the repository and the suite's backups)? Re-sign later with a ceremony key? | A7 stops before Step 3d. |
| Q3 | The recorded Heimdall hour: it is not at the tag. Use the synthetic hour, or bring a recorded hour in? A recording of live feeds is itself live-feed data, which the design excludes, and may carry feed licence terms that limit it to the local demo. | Synthetic hour. |
| Q4 | One `apple` audit record per release covers both apps, but they live on `clients_mac` and `clients_iphone`. Which branch produces the release record — a branch that merges both? | Audit runs on each branch; the record is produced where both apps exist. |
| Q5 | Custody floor: accept `NonExtractable` for the Enclave-wrapped root in the demo, or add a floor (for example `HardwareWrapped`) and update `PlatformProfile::Apple`? | `NonExtractable`, documented. |
| Q6 | The design says offline drops the tier to on-device, but no on-device model ships in v1. Keep the demo flag (`demo_on_device_model = true`) so the chip shows on-device, or show "AI off" honestly? | Demo flag on, chip labelled. |
| Q7 | Map tiles: allow Apple's online tiles, or bundle an offline coastline (for example a public-domain Natural Earth outline) as the fallback? | Online tiles; no offline basemap. |
| Q8 | Bundle identifiers, display names, the Apple team ID, and whether XcodeGen (Homebrew) may be used. | Placeholders `io.veldris.midgard.mac` / `.ios`; XcodeGen. |
| Q9 | Personas: the four in A4 reuse names from the fixtures (`k.rao`, `j.benjamin`, `k.osei`, `a.holt`). Keep them? | Keep. |
| Q10 | The web consoles show a demonstration banner in mock mode. Should the apps show one, given that the design wants the apps unaware the data is demo data? | No banner; the persona screen says "Demo personas". |
| Q11 | Show the mocks' `ML-DSA ✓` labels as the web does, or have the demo enclosure actually sign its audit rows with the demo key so the mark is true? | Show as the web does, in Phase C; revisit. |
| Q12 | The design's screen total (42) versus its table (45). | Build all 45. |

## Owner decisions (16 Nov 2026)

The owner accepted the recommended answer to every open question:

1. **UniFFI licence:** approve time-limited MPL-2.0 exceptions for the eight UniFFI crates in `supply-chain/licences.toml`, with the repository owner as `owner`.
2. **Demo signing key:** created on the development Mac in `~/.midgard/demo-keys`, separate from every real key; only the public key is committed (`clients/demo/trust/`).
3. **Heimdall hour:** the labelled synthetic hour; no recording of live feeds.
4. **Release audit record:** written from a merge of `clients_mac` and `clients_iphone` before a release.
5. **Key custody:** accept `NonExtractable` for the Apple key root and record the reason; no new custody level.
6. **Offline AI:** show "AI off" when offline; no simulated on-device tier.
7. **Map:** online MapKit tiles, with a plain bundled coastline when offline.
8. **Projects:** bundle IDs `io.veldris.midgard.mac` and `io.veldris.midgard.ios`; XcodeGen from Homebrew; the owner enters the team ID.
9. **Personas:** keep the four from the release's demo fixtures.
10. **Demo banner:** yes, as in the web consoles.
11. **Audit rows:** the demo enclosure signs its audit rows with the demo key, so the check mark is real.
12. **Screens:** build all 45.

## Owner decision (19 Nov 2026): the apps ship with no data

The owner asked for the clients to be built out **without any data or feeds for now**, and
chose "nothing at all" over a personas-only dataset. This replaces decisions 3, 9, 10 and 11
for the apps (until data returns) and changes Phases B to H as below. Phase A stays as
built: the signed demo dataset, its key and the enclosure remain in the tree, unused by the
apps, so that adding data later is a configuration change.

- **No dataset in either app.** No `.mgdemo` is bundled and no "Copy demo dataset" phase
  exists. Each app's `Info.plist` carries `MidgardDataSource = none`; the only other
  accepted value is `bundled` (the Phase A path, unchanged). A missing or unknown value is
  a failed state ("app misconfigured"), never a silent switch between modes.
- **One built-in local user.** With no source the core signs in one persona, `local`
  ("Local user", role `operator`, clearance `open`, no compartments, no policy roles):
  the lowest grant there is, so the mode can never show more than the data allows.
- **Every collection exists and is empty.** Screens show their empty state; actions are
  refused by the core with "no data source connected" and the UI shows them disabled
  with that reason.
- **Tests may use the Phase A dataset** to prove that rows, withheld rows and markings
  render; the apps never bundle it.
- **Banner:** "No data source connected" replaces the demo banner in this mode.

### Task B0 (on `clients_shared`): no-source mode

**Files:** `clients/core/src/local_source.rs` (new), `clients/core/src/lib.rs`,
`clients/ffi/src/client.rs`, `clients/ffi/tests/*`, the Swift package
(`Session/AppEnvironment.swift`, `Session/AppModel.swift`, `Storage/StorePaths.swift`,
the banner view, tests), regenerated bindings, `ContractTests.swift`.

- `LocalSource: DataSource`. `personas()` returns only `local`; `grant("local")` gives
  clearance `open`; any other id is `UnknownPersona`. `collections()` returns the
  compile-time collection table (empty until Phase C adds it), and `fetch` returns `[]`
  for a listed collection and `UnknownCollection` otherwise. `exchange` with no ops
  returns `[]`; with any op it is refused, and nothing is queued. `online`/`set_online`
  toggle a flag, so the offline chip still works. `in_core_verified` keeps the default
  `None`.
- FFI: `ClientConfig.dataset_path` becomes `Option<String>`. `None` builds a
  `LocalSource`; `Some(path)` takes the verified path exactly as today and fails closed.
  An internal `enum Source { Demo(DemoEnclosure), Local(LocalSource) }` implements
  `DataSource` by delegation (passing the seal through for `in_core_verified`), so
  `Live = Session<SealedRootKeystore, Source>`. `request_action` in local mode returns
  `ClientError::Refused` with "no data source connected". `reset_demo` makes a fresh
  `LocalSource`. A new `data_source_kind() -> DataSourceKind { None, Demo }` drives the
  banner.
- Swift: `AppEnvironment` holds a `DataSourceChoice` (`.none` or `.bundled(URL)`) read
  from `MidgardDataSource`; `AppModel.start` passes `nil` or the path.
- Tests (Rust): one persona; an unknown persona is refused; every listed collection
  fetches empty; an action is refused and queues nothing; offline toggles; enrol,
  unlock, lock, export and import state round-trip; `Some(<bad path>)` still fails and
  never falls back to local. Tests (Swift): `none` reaches `needsEnrolment` with one
  persona; a missing or unknown key reaches `failed`.
- Acceptance: `cargo test` for core and ffi green; the XCFramework rebuilt and verified;
  `swift test` green; `check-contract.sh` passes with `data_source_kind` called.

### Changes to Phase B

- **B2/B3:** no dataset phase; `MidgardDataSource = none`. The UI tests sign in as
  `local`, see the "No data source connected" banner, open each console and toggle
  offline (the `OFFLINE` chip); the withheld-row check moves to Phase C's model tests.
  Without a team ID, Debug builds sign ad hoc with an entitlements file that leaves out
  `keychain-access-groups` (a restricted entitlement that stops an ad-hoc app from
  launching); such a build shows "cannot keep keys" outside UI testing, which uses
  `-MidgardUITesting`. The first Secure Enclave enrol and unlock waits for the team ID.
- **B4/B5 (release scripts):** deferred until the team ID, since neither can be checked
  without it.

### Changes to Phases C to H

- **Task 2 (enclosure behaviour and golden parity) is deferred** until data returns.
- **Task 1** still generates the typed bindings, and the collection table that
  `LocalSource` lists.
- **Task 3** view models keep every state. The empty state is tested against
  `LocalSource`; rows, withheld and refused are tested against the Phase A dataset.
- **Task 4** adds an empty-state view per screen that names what will appear there.
  Action controls are present but disabled, with the core's reason.
- **Task 6** UI flows: sign in, visit every screen, see its empty state, and see that a
  disabled action explains why. The screens join `AccessibilityAudit.screens`.
- **Heimdall map:** MapKit with no tracks; the offline coastline is deferred with the data.
- **After Phase H:** H1 (accessibility) and H4 (records) stay; H2 (offline and conflict)
  waits for data; H3 (release builds) waits for the team ID.

## Owner decision (22 Nov 2026): two editions, Midgard and Midgard 3D

The owner asked for two clients on each platform, four apps in all:

1. **Midgard** (the release edition): only the signed official release, tag
   `v0.1.0-rc.3`. All six consoles and 45 screens, exactly as Phases B to H build them.
2. **Midgard 3D:** the same release plus a 3D globe in Heimdall.

The owner chose two separate apps over one app with a switch. They also chose online
tiles for the globe, fetched only while the 3D view is open, and asked for the globe to
land in two separate commits: the globe first, then the tiles. Everything else in the
19 Nov decision still holds for both editions: no dataset, the `local` user and empty
collections.

| | Midgard | Midgard 3D |
|---|---|---|
| Content | `v0.1.0-rc.3`, 45 screens | The same, plus a Heimdall globe view |
| Bundle IDs | `io.veldris.midgard.mac`, `.ios` | `io.veldris.midgard3d.mac`, `.ios` |
| `MidgardEdition` | `release` | `3d` |
| Network | None (sandbox with no network entitlement) | Client network entitlement, used only by the globe's tile loader (Task I2) |
| Banner | "No data source connected" | The same, plus "3D edition, not in the signed release" |

**Where the globe comes from.** The release tag has no globe. The 3D view ports the
behaviour of the three.js globe on the unsigned branch `md_unified`
(`products/heimdall/ui/src/app/globe/`, about 2,300 lines in `GlobeView.ts` and its
helpers). That code is not in the signed release, which is why it ships only in the
3D edition. If it later lands in a signed release, the editions can merge back into
one app.

**Renderer.** MetalKit (`MTKView` in an `NSViewRepresentable` or
`UIViewRepresentable`), because SAD_06 requires native rendering rather than a web view.

- RealityKit's `RealityView` would need macOS 15 and iOS 18, above the deployment
  targets.
- SceneKit is deprecated.
- Metal also matches the web globe's own vertex and index buffer model most directly.

### Task B7 (on `clients_shared`, then both app branches): the 3D edition targets

**Package:** a new `MidgardGlobeKit` library in `Package.swift`, depending on
`MidgardKit`. Until Phase I it holds only a placeholder view that says "3D view".

**Apps:** each `project.yml` gains a second app target, `MidgardMac3D` and `MidgardiOS3D`.

- These targets link `MidgardGlobeKit`; the release targets do not.
- Each target has its own `Info.plist` with `MidgardEdition`, its own entitlements and
  its own scheme, with UI tests that subclass the same audit base class.

**Swift:**
- `AppEnvironment` reads `MidgardEdition`. A missing or unknown value is the "app
  misconfigured" failed state, as with `MidgardDataSource`.
- The About screen names the edition, and for `release` the signed tag and its commit.
- Heimdall shows a "3D" entry only when the edition is `3d` and the globe library
  registered itself. The registration is a link-time hook, never a runtime switch.

**Release-binary check:** `clients/shells/apple/tools/check-edition.sh` builds both
release apps and fails if their binaries or bundles contain any `MidgardGlobeKit` symbol,
any `Globe` type, or any network entitlement. `tools/ci/tests/apple-edition.test.sh`
tests the script against a seeded failure.

**Acceptance:**
- All four schemes build and their UI tests pass.
- `apple-projects.test.sh` covers both targets in each project.
- `check-edition.sh` passes, and `audit.sh` runs all four apps.

### Phase I (after Phase E, Heimdall): the globe, in two commits

#### Task I1: 3D globe, no tiles (its own signed commit)

The 3D view with nothing fetched:

- **Globe:** a lit sphere with a graticule, and a land and sea outline from Natural
  Earth 1:110m land. That file is public domain, about 100 KB, and bundled in
  `MidgardGlobeKit`'s resources.
- **Camera:** behaves like `GlobeCamera.ts`, with drag to orbit, pinch or scroll to zoom,
  and the same easing. It has the same presets and REGION menu as `presets.ts`, ported
  as a pure Swift module with tests that compare against the web values.
- **Style and layers:** only the CLEAN style, because TOPOGRAPHIC needs terrain. With no
  feeds the globe shows an empty picture and the same empty-state wording as the 2D
  map. Layers are listed and disabled with the core's reason.
- **Network:** none. The 3D app's network entitlement is present but unused until I2,
  and a test asserts that no URL session is created.
- **Tests:**
  - unit tests for projection, the camera limits and the presets;
  - snapshot tests of three camera presets, so the outline lands on the right places;
  - a UI test that opens Heimdall, switches to 3D, moves the camera and uses each preset;
  - the accessibility audit, where the globe has a text summary of the camera position
    and region.

#### Task I2: online globe tiles (a separate signed commit on top of I1)

**Sources:** the same as the web globe. Each source's attribution is shown in a readable
credits strip.

| Source | Use | Licence |
|---|---|---|
| EOX Sentinel‑2 cloudless, layer `s2cloudless_3857` (2016) | Imagery | CC BY 4.0 (the 2018-onwards yearly layers are CC BY‑NC‑SA; use only the 2016 layer, and confirm against EOX's capabilities) |
| Mapzen Terrain Tiles (AWS Open Data) | World terrain | Attribution |
| Environment Agency | UK close-up terrain | OGL v3.0 |
| VersaTiles | Vector tiles: coast, roads, labels | OpenStreetMap, ODbL |

All four allow commercial use with attribution, provided the imagery stays on the 2016
EOX layer.

**Loader:**
- A list of the four host names. Any other host is refused.
- HTTPS only, and no cookies.
- A generic User-Agent with no user or machine detail.
- Requests are made only while the 3D view is on screen, and are cancelled when it
  closes.

**Cache:** in `~/Library/Caches/<bundle id>/tiles`, capped at 500 MB with
least-recently-used eviction. A "Clear map cache" control goes under About.

**Adds:**
- terrain on the globe;
- TOPOGRAPHIC: tint, hillshade and contours;
- the satellite imagery style;
- zoom to 1.5 km above the ground, with the camera kept 150 m above the terrain;
- the UK close-up patch, as on `md_unified`.

**Offline:** the OFFLINE chip, or no network, falls back to the I1 globe with
"Map tiles need a connection", never an error.

**Tests:**
- the loader refuses other hosts, HTTP and redirects to other hosts;
- cache eviction stays under the cap;
- terrain sampling matches the web globe's bilinear sampler on fixed tiles;
- tiles come from recorded fixtures in tests, with no live network in CI;
- the UI test from I1 is extended to show the credits strip.

**Checks for Phase I:** the review steps are the same as for every other task (spec
review, then quality review). Once I2 is done, `check-edition.sh` must still show that
the release apps carry no globe code and no network entitlement.

### Publishing on completion (owner decision, 22 Nov 2026)

When both editions are complete and reviewed, the work goes to branches of the main
repository, `veldris-ltd/midgard-app`. This replaces the "do not push" line above for
that final step only; until then, nothing is pushed.

**Branches**, with the names this plan uses:

| Branch | Holds |
|---|---|
| `clients_shared` | The core, the FFI and the shared Swift packages, including `MidgardGlobeKit` |
| `clients_mac` | The Mac apps, `MidgardMac` and `MidgardMac3D` |
| `clients_iphone` | The iPhone apps, `MidgardiOS` and `MidgardiOS3D` |

**Signed tags**, annotated, on the final reviewed commits:

| Tag | Message names |
|---|---|
| `apple-midgard-v0.1.0-rc.3` | Midgard, the release edition; macOS and iOS; built from the signed `v0.1.0-rc.3` (`ea5899ab`); no data source |
| `apple-midgard3d-v0.1.0-rc.3` | Midgard 3D at Task I1; macOS and iOS; the same release plus the Heimdall globe ported from `md_unified`; no tiles and no network use |
| `apple-midgard3dmaps-v0.1.0-rc.3` | Midgard 3D at Task I2; macOS and iOS; the I1 globe plus online map tiles (EOX imagery, Mapzen and Environment Agency terrain, VersaTiles) |

Each tag points at the commit where both platforms of that stage pass `audit.sh` and
`check-edition.sh`. The 3D tag is on the I1 commit and the maps tag on the I2 commit, so
the globe without tiles stays checkable on its own.

**How the push is done:**
- The checkout's push URL stays blocked. The push names the repository URL and the
  three branches and two tags explicitly, so no other branch can go with it.
- Before pushing:
  - verify every commit's signature with `git log --format=%G?`, and all three tags with
    `git verify-tag`;
  - confirm that no commit message mentions an AI tool;
  - run gitleaks over the range;
  - ask the owner for a final go-ahead.

**Public by choice:** the repository is public. The 3D edition puts a port of the globe
work, which was taken off it on 27 Oct, back in public. The owner chose this.

### Branch base before publishing (owner decision, 22 Nov 2026)

`dev` is left untouched: nothing merges into it or out of it, and syncing it is deferred.

The work is built from `main`, as `CONTRIBUTING.md` allows ("Branch from `main` (or `dev`)"),
and as the three release candidates were made. Since `v0.1.0-rc.3`, `main` has one commit,
PR #97 (`7493bbac`), a CI-only fix with no client code.

**Before the push:**
- Merge `origin/main` into `clients_shared`, `clients_mac` and `clients_iphone`, with
  signed merge commits.
- Run each branch's checks again.
- Put the three `apple-*` tags on the merged commits.

The release edition's content is still exactly `v0.1.0-rc.3`, because #97 changes nothing
in the apps.

**Later, not part of this plan:**
- The `apple-*` tags are labels, not release tags.
- To make the clients part of a signed release:
  1. a pull request from each branch into `main`;
  2. a version bump to `0.2.0` (the clients are new features);
  3. the release pipeline, then a `v0.2.0-rc.1` tag from `main`.
- The 3D globe reaches a signed release the same way, from its own feature branch off
  `main`. It is never a wholesale merge of `md_unified` or `dev_mac`.
- No pull requests are opened without the owner's request.

## Owner decision (22 Nov 2026): only the three clients; everything else paused

Only these three builds continue:

1. **Midgard:** clients of the signed release `v0.1.0-rc.3` only, with no other additions
   or changes. Phases B to H build the rc.3 screens in the no-data mode the owner chose
   on 19 Nov. Tag `apple-midgard-v0.1.0-rc.3`.
2. **Midgard 3D:** the same, plus the 3D globe only (Task B7, then Task I1). Tag
   `apple-midgard3d-v0.1.0-rc.3`.
3. **Midgard 3D with map tiles:** item 2 plus online map tiles (Task I2). Tag
   `apple-midgard3dmaps-v0.1.0-rc.3`.

**The release apps get no visible change beyond the rc.3 screens.** There is no edition
or tag line in their About screen, and no new banner, menu entry or behaviour. The
`MidgardEdition` key is read silently. The edition and tag lines and the 3D banner
appear only in the 3D apps.

**Paused until the owner resumes them:**
- development signing setup, Team ID included;
- the Secure Enclave check;
- B4/B5, the release scripts;
- publishing:
  - the `main` merge before the push;
  - the push;
  - the tags;
- any pull request;
- `dev` and `main` work;
- the defensive cyber option (`md_cyber_dco`);
- every other project.

**Signing resumed (owner, 22 Nov 2026):** all six apps are signed under team
`64JX2CTLJR`. That means the release, 3D and 3D-with-tiles editions, on Mac and iPhone.

- **Development signing:** set up as soon as B7 has finished.
  - The Team ID goes in each app's gitignored `Config/Signing.local.xcconfig`.
  - Each app gets one Debug build with `-allowProvisioningUpdates`.
  - Then the Secure Enclave enrol and unlock check runs.
- **Release signing:** B4/B5 build each edition's signed Release app.
  - Mac apps: signed with Developer ID and Hardened Runtime.
  - iPhone apps: an Ad Hoc or Development export for test devices.
- **Still needs a go-ahead each time:**
  - notarisation (an upload to Apple);
  - TestFlight;
  - any App Store submission.

Signing changes nothing inside the apps. The release edition still contains only the
rc.3 screens.

**Distribution approved (owner, 22 Nov 2026):**

- **Mac:** notarisation of the Developer ID builds.
- **iPhone and Mac:** uploads to TestFlight.
- **When:** each edition is notarised and uploaded at its tag, once it is complete and
  reviewed:
  - the release edition after Phase H;
  - the 3D edition after I1;
  - the 3D-with-tiles edition after I2.
- **Bundle IDs:** the 3D and 3D-with-tiles stages share one bundle ID per platform, so
  they appear as successive builds of the same apps.
- **App Store review:** submitting a build for review publishes the app to the public
  once it is approved, so that one step is confirmed with the owner when it comes.

**Prerequisites (owner):**
- **Notary credential:** an App Store Connect API key, stored in the login keychain with
  `xcrun notarytool store-credentials midgard-notary`. Scripts refer to the profile name
  only, and never read or print the key.
- **Developer ID Application certificate:** created by the Account Holder (or an Admin
  allowed to).
- **App Store Connect records:** one per bundle ID: `io.veldris.midgard.mac`,
  `io.veldris.midgard.ios`, `io.veldris.midgard3d.mac` and `io.veldris.midgard3d.ios`.
- **Agreements:** the current agreements accepted.
- **Export compliance:** the apps use encryption (keys stored at rest, PQ mTLS code in
  the core). `ITSAppUsesNonExemptEncryption` is set only after the owner decides
  whether an exemption applies.

**Three apps per platform (owner, 22 Nov 2026).** This replaces the shared 3D bundle ID
above.

| Edition | `MidgardEdition` | Mac / iPhone targets | Bundle IDs | Network | Tag |
|---|---|---|---|---|---|
| Midgard | `release` | `MidgardMac`, `MidgardiOS` | `io.veldris.midgard.mac`, `.ios` | None | `apple-midgard-v0.1.0-rc.3` |
| Midgard 3D | `3d` | `MidgardMac3D`, `MidgardiOS3D` | `io.veldris.midgard3d.mac`, `.ios` | **None** | `apple-midgard3d-v0.1.0-rc.3` |
| Midgard Maps | `maps` | `MidgardMacMaps`, `MidgardiOSMaps` | `io.veldris.midgardmaps.mac`, `.ios` | Client network entitlement, used only by the tile loader | `apple-midgard3dmaps-v0.1.0-rc.3` |

**Where each piece lands:**
- B7 adds all six targets.
- I1 puts the globe in both the 3D and Maps apps.
- I2 adds the tile loader and turns it on only in the Maps apps. The 3D apps never fetch
  anything.

**`check-edition.sh` asserts:**
- the release apps have no globe code and no network entitlement;
- the 3D apps have no network entitlement;
- only the Maps apps have one.

**App Store Connect** needs six app records, one per bundle ID.

**Midgard 3D and Midgard Maps follow the newest build (owner, 22 Nov 2026).**

- **Reference.** Phase I ports the newest globe and map code. The reference order is:
  1. the core branches `heimdall_3d` and `heimdall_maps`, once they exist. Their
     carve-out from `md_unified` 857c3bcb (5 Nov) is the newest globe and map code
     across all branches;
  2. until then, `md_unified` 857c3bcb itself.
- **Port order.** I1 follows `heimdall_3d`, and I2 follows `heimdall_maps`.
- **Before each edition's tag**, the Mac and iPhone 3D and Maps apps are compared against
  the newest core branch, and any later globe or map change is ported.
- **What the Midgard Maps app has.** It matches `heimdall_maps` in full:
  - the close-up;
  - the TOPOGRAPHIC and satellite styles;
  - the folding ⓘ CREDITS strip;
  - the regenerated UK images (516bed75);
  - **the 2D geographic map**, from VersaTiles vector tiles, native in MapKit or Metal
    with the same tile sources.
- **What the Midgard 3D app has.** It matches `heimdall_3d`: the globe with bundled data
  only, and no network.
- **Shared defaults:** height ×4, and the REGION menu without the feed regions.

## Owner's stages (22 Nov 2026): this replaces every earlier source note for the globe and maps

1. **Stage 1 (core repo):** a Heimdall 3D globe (`heimdall_3d`), and then maps
   (`heimdall_maps`), written fresh on `main`. Pushed, with pull requests into `dev`. No
   work branch is read or ported, and there are no feeds.
2. **Stage 2 (these clients):** the signed release plus the 3D function. Phase I1 ports
   the behaviour of Stage 1's `heimdall_3d`, and nothing else. Pushed to the `clients_*`
   branches, tag `apple-midgard3d-v0.1.0-rc.3`.
3. **Stage 3 (these clients):** Stage 2 plus maps. Phase I2 follows `heimdall_maps`,
   including the 2D geographic map. Pushed to the same branches as an update, tag
   `apple-midgard3dmaps-v0.1.0-rc.3`.

**Superseded:** any earlier mention in this plan of porting from a work branch.

## B7 review notes (22 Nov 2026)

- **Edition code stays out of the release apps.** Everything an edition shows (banner,
  About, Heimdall's 3D view, the signed tag and commit) lives in `MidgardGlobeKit`, which
  only the 3D and Maps apps link, and reaches MidgardKit through the link-time hook
  `midgard_edition_hook` as an `EditionProvider`. The release apps keep only the silent
  `MidgardEdition` read and the weak hook, which returns nothing.
- **B4 and B5 run `check-edition.sh` on the signed Release archives**, not only on Debug
  builds: `check-edition.sh --release <archive app> --3d <app> --maps <app>` for each
  platform, before notarisation or upload. Release builds strip local symbols, so the
  hook rule may find no symbol at all there; that is accepted for release apps (weak or
  absent) and must still find the strong hook in the 3D and Maps apps.
- **I2, the tile loader, is its own module, `MidgardTileKit`, linked only by the Maps
  apps.** `check-edition.sh` already refuses, in release and 3D apps on both platforms,
  any `MidgardTileKit` symbol and the Info.plist keys `NSAppTransportSecurity`,
  `NSLocalNetworkUsageDescription` and `NSBonjourServices`; I2 must keep those out of
  them and add no ATS exception to the Maps apps beyond what the four tile hosts need.

## Owner decisions (22 Nov 2026): look matched to the web suite, with its sample data

**Dev signing and the Secure Enclave.** Development signing is set up under team
64JX2CTLJR and the three Mac apps are signed. The owner enrolled, locked and unlocked
on the signed release Mac app: the Secure Enclave key works.

**The apps should look like the web suite.** The owner saw the shell and found it
"nothing like the suite". Two decisions follow.

1. **Native, matched to the web consoles.** Every screen stays native SwiftUI, per
   SAD_06. Each is matched closely to its web console's layout, density, tokens and
   type.
2. **The signed release's own sample data.** The apps show exactly the built-in sample
   data that the `v0.1.0-rc.3` web consoles ship with (`sdk/ts/src/mock/`), under the
   same "Demonstration data" banner. There are no feeds and no network. This replaces
   the 19 Nov "nothing at all" choice for what the apps display.
   - **Mode:** `MidgardDataSource = sample`, a new `SampleSource` in the core.
   - **Data source:** the data is generated from the SDK mocks at the tag by an
     extraction script (Phase A style), so there is one source of truth. A drift check
     compares it with the tag.
   - **Actions:** they behave as they do in the web mock.

### Task R1: reference screen (before Phases C–H)

1. **`SampleSource`.** Build it in `midgard-client-core` from the rc.3 SDK mocks, with
   the extraction script and drift check, the FFI and the Swift wiring. All six apps use
   `sample`. The banner text is identical to the web's.
2. **Heimdall Operating Picture.** Build it natively, matched to the web console's 2D
   reference layout:
   - the navigation (PICTURE / DECIDE, LAYERS);
   - the picture with its five tracks;
   - the header chips;
   - the inspector with the ranked options and Authorise/Reject;
   - the stats footer;
   - the classification banner.

   It has both Mac and iPhone layouts.
3. **Side-by-side screenshots.** Web, Mac and iPhone, for the owner's sign-off. Phases
   C–H follow the standard the owner approves.

#### R1 deliberate deviations (23 Nov 2026)

Where the apps knowingly differ from the web console's 2D reference layout, for the
owner's sign-off:

- **Added for the iPhone and accessibility:** the "N TRACKS WITHHELD · ABOVE YOUR
  CLEARANCE" readout for a persona below some tracks' clearance; the track list under
  the picture at accessibility text sizes (where the picture draws no text); the
  iPhone's "Inspect" button, which opens the inspector sheet; and, at compact width,
  the picture's readouts (north, scale, vector, the withheld count and the focus track)
  as a strip directly above the picture instead of over it, because over it they came
  over the tracks and their labels. The Mac keeps the web layout.
- **Reject is screen-only**, as in rc.3's web console: it is the operator's own
  decision, shown on screen and not audited. To be raised for Phase D.
- **The 3D entry is in the navigation bar** in the 3D and Maps iPhone apps, not a row
  above the picture, so the picture's lowest track label stays above the console bar.
- **The Authorise button has dark text** on the accent fill, for contrast.

## 23 Nov: two apps, one branch

Owner decision (23 Nov 2026), replacing the three editions per platform of 22 Nov:

- **Two apps only**, both the Maps edition (the 3D view now, Phase I's map tiles to come):
  the Mac app `MidgardMacMaps` (bundle `io.veldris.midgardmaps.mac`) and the iPhone app
  `MidgardiOSMaps` (`io.veldris.midgardmaps.ios`), each shown as "Midgard". The
  release-only apps (`io.veldris.midgard.*`) and the 3D-only apps
  (`io.veldris.midgard3d.*`) are dropped, with their targets, schemes, Info.plists,
  entitlements and UI-test targets.
- **One branch, `clients_apple`**, holds both apps and replaces `clients_mac` and
  `clients_iphone`, which are kept unchanged until the owner decides about them.
  `clients_shared` stays the shared base and is merged into `clients_apple` as before.
- `tools/check-edition.sh`, `apple-projects.test.sh`, `apple-edition.test.sh`,
  `audit.sh` and its test cover the one edition. The sandbox, own-entitlements and
  no-unexpected-network checks stay: the client network entitlement only (required on
  the Mac), and no server, networking or temporary-exception entitlement.
- `MidgardGlobeKit` and its link-time hook are unchanged.
- The planned tag is `apple-midgard3dmaps-v0.1.0-rc.3` (Stage 3); it is not created yet.

## Phase I done (23 Nov 2026)

Heimdall's MAP and 3D views are in both apps, ported from the web console on
`heimdall_3dmaps` (its public demo build) and nothing else; the `md_unified` and
Environment Agency sources above are superseded.

- **View switch:** 2D / MAP / 3D, as the web's `ViewSwitch`, in the Mac's header and the
  iPhone's navigation bar; the placeholder "3D view" entry is gone. MAP and 3D share the
  picture's layers, selection and inspector.
- **3D:** Metal: the lit sphere with day and night from the subsolar point, the
  graticule, the bundled Natural Earth outlines, the atmosphere; the web's camera
  (limits, easing, tilt and heading, the presets and REGION list); the five sample
  tracks at the web build's positions with altitude stems; terrain relief from the
  terrarium tiles, and the CLEAN, TOPOGRAPHIC and SATELLITE styles.
- **MAP:** VersaTiles vector tiles over the bundled land, with the web's caps (500
  labels a tile, 400 candidates a frame, labels cut to 200 characters) and the tracks
  on top.
- **Tiles:** one loader in `MidgardGlobeKit` (no separate `MidgardTileKit`: the release
  and 3D-only apps it was to be kept out of are gone): the three hosts, HTTPS, no
  cookies, no redirects, a generic User-Agent, the web's byte, size, feature, point and
  time limits, requests only while a view is open, a 500 MB least-recently-used disk
  cache, the credits strip, the tile pause after track-driven moves, and the offline
  fallback.
- **Entitlements:** unchanged: the Mac app's client network entitlement only; the
  iPhone app has none and keeps App Transport Security's default.
- **Tests:** package tests for the geometry, camera and presets against the web values,
  tile maths, the terrarium sampler, decoding and its limits, the loader's host, scheme,
  redirect and byte refusals, and cache eviction, on three recorded tiles; one UI test
  per app (`HeimdallMapsFlowTests`).

The README's "Heimdall's MAP and 3D views" section lists where the apps knowingly differ
from the web.
