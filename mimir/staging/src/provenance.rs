//! Stage-1 native-provenance verification. [FR-MS-2] [STD:NIST SA-12]
//!
//! The enclosure will trust only Veldris's signature, but before we re-sign we
//! must record *how much we could independently establish* about the upstream
//! artefact — so a human accepts a known level of risk, not a blank one. We
//! verify native provenance **where it exists** and record, per check, whether it was
//! `Verified`, `Unverifiable` (nothing to check against, or no way to check it), or a
//! `Mismatch`.
//!
//! A check is `Verified` only when bytes were actually compared or a signature actually
//! checked — never because a signature or commit *string* is present. [RT-21]
//!
//! A `Mismatch` is the one hard stop: it means the bytes we hold are not the
//! bytes the upstream published, i.e. possible tampering, so we must **not**
//! re-sign. Absence of native provenance is *not* a stop — it is recorded as
//! `Unverifiable` for explicit human risk-acceptance.

use pqc_agility::hash::sha256;

use crate::fetch::FetchedModel;

/// The in-bundle path the weights are recorded under, in a pinned file listing and
/// in the SBOM.
pub const WEIGHTS_PATH: &str = "model.weights";

/// The outcome of one native-provenance check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verifiability {
    /// Independently verified against upstream-published material.
    Verified,
    /// Nothing was published to verify against, or it could not be checked —
    /// recorded, not fatal.
    Unverifiable,
    /// Published material was present but did **not** match — possible tampering.
    Mismatch,
}

impl Verifiability {
    /// The contract string.
    pub fn as_str(self) -> &'static str {
        match self {
            Verifiability::Verified => "verified",
            Verifiability::Unverifiable => "unverifiable",
            Verifiability::Mismatch => "mismatch",
        }
    }
}

/// A single named provenance check and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceCheck {
    /// The check name, e.g. `sha256-weights`, `vendor-signature`, `hf-commit`.
    pub check: String,
    /// The outcome.
    pub result: Verifiability,
    /// A short, non-secret detail line.
    pub detail: String,
}

/// The recorded provenance of a staged artefact: the source it came from, the
/// SHA-256 we computed over the weights (the digest that goes into the signed
/// manifest), and every native check we ran. [FR-MS-2]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceRecord {
    /// The upstream source label.
    pub source: String,
    /// SHA-256 of the weights, computed in staging (32 bytes).
    pub weights_sha256: [u8; 32],
    /// Every native-provenance check and its verifiability.
    pub checks: Vec<ProvenanceCheck>,
}

impl ProvenanceRecord {
    /// True if any check found a `Mismatch` — the re-sign hard stop.
    pub fn has_mismatch(&self) -> bool {
        self.checks
            .iter()
            .any(|c| c.result == Verifiability::Mismatch)
    }

    /// True if at least one check independently `Verified` the artefact.
    pub fn any_verified(&self) -> bool {
        self.checks
            .iter()
            .any(|c| c.result == Verifiability::Verified)
    }

    /// Lowercase-hex of the computed weights digest.
    pub fn weights_sha256_hex(&self) -> String {
        hex(&self.weights_sha256)
    }
}

/// Lowercase-hex encode bytes (dependency-free).
pub(crate) fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

/// Verify the native provenance of a fetched model, recording verifiability for
/// each dimension. Computes the weights SHA-256 either way (it anchors the signed
/// manifest); compares it to the upstream-published digest when one exists. [FR-MS-2]
pub fn verify_provenance(fetched: &FetchedModel) -> ProvenanceRecord {
    let weights_sha256 = sha256(&fetched.weights);
    let computed_hex = hex(&weights_sha256);
    let mut checks = Vec::new();

    // 1. Published SHA-256 over the weights.
    match &fetched.declared.published_sha256_hex {
        Some(published) => {
            let published_norm = published.trim().to_ascii_lowercase();
            if published_norm == computed_hex {
                checks.push(ProvenanceCheck {
                    check: "sha256-weights".into(),
                    result: Verifiability::Verified,
                    detail: "computed weights digest matches upstream-published SHA-256".into(),
                });
            } else {
                checks.push(ProvenanceCheck {
                    check: "sha256-weights".into(),
                    result: Verifiability::Mismatch,
                    detail: "computed weights digest does NOT match upstream-published SHA-256"
                        .into(),
                });
            }
        }
        None => checks.push(ProvenanceCheck {
            check: "sha256-weights".into(),
            result: Verifiability::Unverifiable,
            detail: "upstream published no SHA-256 to verify against".into(),
        }),
    }

    // 2. Vendor native signature (GPG / vendor-signed). No verifier for any vendor
    //    signature format is implemented, so a signature that is present is recorded as
    //    present and unchecked — never as verified. [RT-21]
    checks.push(ProvenanceCheck {
        check: "vendor-signature".into(),
        result: Verifiability::Unverifiable,
        detail: match &fetched.declared.vendor_signature {
            Some(sig) if !sig.trim().is_empty() => {
                "a vendor signature is present but was NOT cryptographically checked: \
                 no verifier for its format is implemented"
                    .into()
            }
            _ => "no vendor native signature published (unsigned upstream)".into(),
        },
    });

    // 3. The upstream commit, checked against the files pinned at that revision.
    checks.push(check_pinned_revision(fetched, &computed_hex));

    ProvenanceRecord {
        source: fetched.source.label(),
        weights_sha256,
        checks,
    }
}

/// The commit check: `Verified` only when every fetched file — the weights and each
/// component — matches the SHA-256 the pinned listing records for it at that commit.
/// A file that differs is a `Mismatch`; no listing, or a listing that does not cover
/// every fetched file, is `Unverifiable`. Files the listing names that were not fetched
/// (a README, other quantisations) are not needed. [RT-21]
fn check_pinned_revision(fetched: &FetchedModel, weights_hex: &str) -> ProvenanceCheck {
    let outcome = |result, detail: String| ProvenanceCheck {
        check: "hf-commit".into(),
        result,
        detail,
    };
    let commit = match fetched.declared.hf_commit.as_deref().map(str::trim) {
        Some(commit) if !commit.is_empty() => commit,
        _ => {
            return outcome(
                Verifiability::Unverifiable,
                "no upstream commit hash to pin".into(),
            )
        }
    };
    let Some(pinned) = &fetched.declared.pinned_revision_files else {
        return outcome(
            Verifiability::Unverifiable,
            format!(
                "commit {commit} is recorded, but no pinned file listing was supplied to \
                 check the fetched files against"
            ),
        );
    };

    let mut fetched_files = vec![(WEIGHTS_PATH, weights_hex.to_string())];
    for (path, bytes) in &fetched.components {
        fetched_files.push((path.as_str(), hex(&sha256(bytes))));
    }
    let mut uncovered = Vec::new();
    for (path, digest) in &fetched_files {
        match pinned.get(*path) {
            Some(expected) if expected.trim().eq_ignore_ascii_case(digest) => {}
            Some(_) => {
                return outcome(
                    Verifiability::Mismatch,
                    format!("{path} does NOT match the pinned listing for commit {commit}"),
                )
            }
            None => uncovered.push(*path),
        }
    }
    if !uncovered.is_empty() {
        return outcome(
            Verifiability::Unverifiable,
            format!(
                "the pinned listing for commit {commit} does not cover {}",
                uncovered.join(", ")
            ),
        );
    }
    outcome(
        Verifiability::Verified,
        format!("every fetched file matches the pinned listing for commit {commit}"),
    )
}
