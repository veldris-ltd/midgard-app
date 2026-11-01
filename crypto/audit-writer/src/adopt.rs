//! Adopting a ledger written before ledgers had checkpoints. [MG-SEC-05]
//!
//! A ledger from an earlier release has entries and no `<ledger>.checkpoint`, so
//! [`FileLedger::open`] refuses it. [`adopt_ledger`] is the one-off upgrade step: it
//! reads the ledger, verifies its whole chain under a key the caller supplies (a pinned
//! key, kept apart from the signing keystore, where there is one), and records its first
//! signed checkpoint. From then on the ledger opens normally. It is an operator step,
//! never a boot path, and safe to repeat: a ledger that already has a checkpoint, or
//! that does not exist, is reported and left alone.

use std::path::Path;

use pqc_agility::{CryptoProvider, SigId, SignatureScheme, SigningKey, VerifyingKey};

use crate::error::AuditError;
use crate::file::{checkpoint_path, FileLedger, Sealer};
use crate::ledger::{AuditWriter, LedgerStore};
use crate::record::LedgerId;
use crate::verify::verify_chain;

/// What [`adopt_ledger`] found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// No ledger at the path: nothing to adopt.
    Absent,
    /// The ledger already has a checkpoint: nothing was written.
    AlreadyCheckpointed,
    /// The ledger verified and its first checkpoint, vouching for `entries`, was
    /// recorded.
    Adopted {
        /// How many entries the new checkpoint vouches for.
        entries: usize,
    },
}

/// The key that signs the checkpoint, and the one the chain is verified under.
pub struct AdoptionKeys<'a> {
    /// The scheme of the ledger's signing key.
    pub scheme: Box<dyn SignatureScheme>,
    /// The ledger's signing key, which signs the new checkpoint.
    pub signing_key: SigningKey,
    /// Its verifying key.
    pub verifying_key: VerifyingKey,
    /// The key each entry must verify under, by algorithm.
    pub verify_under: &'a dyn Fn(SigId) -> Option<VerifyingKey>,
}

/// Adopt the ledger at `path`: verify every entry under `keys.verify_under` and
/// record its first signed checkpoint. A checkpoint without its ledger is an error
/// (the ledger was deleted); everything else that does not verify is an error too, and
/// nothing is written. [MG-SEC-05]
pub fn adopt_ledger(
    path: impl AsRef<Path>,
    sealer: Box<dyn Sealer>,
    ledger_id: LedgerId,
    provider: &CryptoProvider,
    keys: AdoptionKeys<'_>,
) -> Result<Adoption, AuditError> {
    let path = path.as_ref();
    let has_checkpoint = checkpoint_path(path).exists();
    if !path.exists() {
        if has_checkpoint {
            return Err(AuditError::LedgerMissing(path.display().to_string()));
        }
        return Ok(Adoption::Absent);
    }
    if has_checkpoint {
        return Ok(Adoption::AlreadyCheckpointed);
    }
    let ledger = FileLedger::open_uncheckpointed(path, sealer, ledger_id.clone())?;
    verify_chain(ledger.entries(), provider, &ledger_id, keys.verify_under)?;
    let mut writer = AuditWriter::new(
        ledger,
        keys.scheme,
        keys.signing_key,
        keys.verifying_key,
        ledger_id,
    );
    writer.seal_checkpoint()?;
    Ok(Adoption::Adopted {
        entries: writer.entries().len(),
    })
}
