//! Algorithm identifiers. These are **stable, versioned tags** carried by every
//! key, ciphertext, and signature: an artefact decodes by its embedded id for as
//! long as that backend stays registered, so enabling a new algorithm never
//! rewrites stored data. That is the mechanism behind AC-M-8 ("no data
//! migration"). Changing a discriminant is a breaking format change. [NFR-PQ-4]

/// A KEM *component* algorithm (a building block; hybrids combine two).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum KemId {
    /// Classical ECDH; never offered alone (see [`KemSuiteId`]).
    X25519 = 1,
    /// ML-KEM-768 (FIPS 203).
    MlKem768 = 2,
    /// ML-KEM-1024 (FIPS 203).
    MlKem1024 = 3,
}

impl KemId {
    /// Stable string id (wire/at-rest tag).
    pub const fn as_str(self) -> &'static str {
        match self {
            KemId::X25519 => "X25519",
            KemId::MlKem768 => "ML-KEM-768",
            KemId::MlKem1024 => "ML-KEM-1024",
        }
    }

    /// Whether this component resists a cryptographically-relevant quantum computer.
    pub const fn is_post_quantum(self) -> bool {
        matches!(self, KemId::MlKem768 | KemId::MlKem1024)
    }
}

/// A negotiable KEM **suite**. Only PQ and hybrid suites are enumerated: a
/// classical-only suite is unrepresentable by construction, which is the
/// strongest form of the AC-M-1 fail-closed guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum KemSuiteId {
    /// Pure ML-KEM-768.
    MlKem768 = 1,
    /// Pure ML-KEM-1024.
    MlKem1024 = 2,
    /// Hybrid X25519 + ML-KEM-768 (the NFR-PQ-1 default for transit).
    HybridX25519MlKem768 = 3,
    /// Hybrid X25519 + ML-KEM-1024.
    HybridX25519MlKem1024 = 4,
}

impl KemSuiteId {
    /// Stable string id.
    pub const fn as_str(self) -> &'static str {
        match self {
            KemSuiteId::MlKem768 => "ML-KEM-768",
            KemSuiteId::MlKem1024 => "ML-KEM-1024",
            KemSuiteId::HybridX25519MlKem768 => "X25519+ML-KEM-768",
            KemSuiteId::HybridX25519MlKem1024 => "X25519+ML-KEM-1024",
        }
    }

    /// Every enumerated suite embeds a post-quantum KEM.
    pub const fn is_post_quantum(self) -> bool {
        true
    }

    /// Encode the suite id as a single byte for canonical/transcript preimages.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decode a suite id from its byte tag (wire/at-rest). Unknown tags fail
    /// closed with `None` rather than mapping to a default.
    pub const fn from_u8(v: u8) -> Option<KemSuiteId> {
        match v {
            1 => Some(KemSuiteId::MlKem768),
            2 => Some(KemSuiteId::MlKem1024),
            3 => Some(KemSuiteId::HybridX25519MlKem768),
            4 => Some(KemSuiteId::HybridX25519MlKem1024),
            _ => None,
        }
    }

    /// Whether this suite additionally mixes a classical KEM (hybrid).
    pub const fn is_hybrid(self) -> bool {
        matches!(
            self,
            KemSuiteId::HybridX25519MlKem768 | KemSuiteId::HybridX25519MlKem1024
        )
    }

    /// The post-quantum component of this suite.
    pub const fn pq_component(self) -> KemId {
        match self {
            KemSuiteId::MlKem768 | KemSuiteId::HybridX25519MlKem768 => KemId::MlKem768,
            KemSuiteId::MlKem1024 | KemSuiteId::HybridX25519MlKem1024 => KemId::MlKem1024,
        }
    }

    /// The classical component, if any.
    pub const fn classical_component(self) -> Option<KemId> {
        match self {
            KemSuiteId::HybridX25519MlKem768 | KemSuiteId::HybridX25519MlKem1024 => {
                Some(KemId::X25519)
            }
            _ => None,
        }
    }
}

/// A signature algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SigId {
    /// ML-DSA-44 (FIPS 204).
    MlDsa44 = 1,
    /// ML-DSA-65 (FIPS 204).
    MlDsa65 = 2,
    /// ML-DSA-87 (FIPS 204).
    MlDsa87 = 3,
    /// SLH-DSA-SHA2-128s (FIPS 205).
    SlhDsaSha2_128s = 4,
}

impl SigId {
    /// Stable string id.
    pub const fn as_str(self) -> &'static str {
        match self {
            SigId::MlDsa44 => "ML-DSA-44",
            SigId::MlDsa65 => "ML-DSA-65",
            SigId::MlDsa87 => "ML-DSA-87",
            SigId::SlhDsaSha2_128s => "SLH-DSA-SHA2-128s",
        }
    }

    /// All enumerated signature algorithms are post-quantum.
    pub const fn is_post_quantum(self) -> bool {
        true
    }

    /// Encode the id as a single byte for canonical signing preimages.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decode an id from its byte tag.
    pub const fn from_u8(v: u8) -> Option<SigId> {
        match v {
            1 => Some(SigId::MlDsa44),
            2 => Some(SigId::MlDsa65),
            3 => Some(SigId::MlDsa87),
            4 => Some(SigId::SlhDsaSha2_128s),
            _ => None,
        }
    }
}
