//! Bundle import errors — all fail closed. An air-gapped site that cannot fully
//! verify a bundle offline imports nothing.

use bifrost_artefact::VerifyError;

/// Why an offline bundle import failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// The bundle-level ML-DSA signature did not verify (unsigned, wrong key, or a
    /// tampered manifest). [AC-B-3]
    BadBundleSignature,
    /// The manifest's table of contents does not match the contained artefacts
    /// (count or per-entry digests) — the bundle was altered after signing.
    ManifestMismatch,
    /// A contained artefact failed its own verification, at the given index.
    Artefact {
        /// The index of the failing artefact within the bundle.
        index: usize,
        /// The artefact-level verification error.
        error: VerifyError,
    },
    /// A chunk of a streamed payload does not hash to the digest recorded beside it: the
    /// file was damaged in transit or storage. Chunk digests are not signed; they locate
    /// damage early, and the signed payload digest still decides authenticity.
    ChunkDigestMismatch {
        /// The index of the artefact within the bundle.
        index: usize,
        /// The damaged chunk, counting from 0.
        chunk: u64,
    },
    /// A file-format error reading or writing the on-disk `.mgbundle` (bad magic,
    /// unsupported version, or a truncated/oversized frame). Fail closed. [AC-B-3]
    Io(String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::BadBundleSignature => {
                write!(f, "bundle signature did not verify")
            }
            BundleError::ManifestMismatch => {
                write!(f, "bundle manifest does not match its contents")
            }
            BundleError::Artefact { index, error } => {
                write!(f, "artefact {index} failed verification: {error}")
            }
            BundleError::ChunkDigestMismatch { index, chunk } => {
                write!(
                    f,
                    "artefact {index}: chunk {chunk} is damaged (it does not match its digest)"
                )
            }
            BundleError::Io(m) => write!(f, "bundle file error: {m}"),
        }
    }
}

impl std::error::Error for BundleError {}
