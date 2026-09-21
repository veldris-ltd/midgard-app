//! The crypto provider: a runtime registry of KEM/signature backends with
//! enable/disable (hot-swap) and preference ordering. Because algorithm ids
//! travel with every artefact, enabling or disabling an algorithm requires **no
//! data migration** (AC-M-8). [NFR-PQ-4]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use crate::algorithm::{KemSuiteId, SigId};
use crate::error::CryptoError;
use crate::kem::Kem;
use crate::negotiate::Capabilities;
use crate::sign::SignatureScheme;

/// The canonical KEM preference order: hybrids first (NFR-PQ-1 transit default),
/// then higher security levels.
pub const DEFAULT_KEM_PREFERENCE: [KemSuiteId; 4] = [
    KemSuiteId::HybridX25519MlKem768,
    KemSuiteId::HybridX25519MlKem1024,
    KemSuiteId::MlKem1024,
    KemSuiteId::MlKem768,
];

/// The canonical signature preference order.
pub const DEFAULT_SIG_PREFERENCE: [SigId; 4] = [
    SigId::MlDsa87,
    SigId::MlDsa65,
    SigId::MlDsa44,
    SigId::SlhDsaSha2_128s,
];

/// A registry of algorithm backends with agility controls.
pub struct CryptoProvider {
    kems: BTreeMap<KemSuiteId, Box<dyn Kem>>,
    sigs: BTreeMap<SigId, Box<dyn SignatureScheme>>,
    enabled_kems: BTreeSet<KemSuiteId>,
    enabled_sigs: BTreeSet<SigId>,
    kem_pref: Vec<KemSuiteId>,
    sig_pref: Vec<SigId>,
}

impl Default for CryptoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoProvider {
    /// An empty provider seeded with the canonical preference orders.
    pub fn new() -> Self {
        Self {
            kems: BTreeMap::new(),
            sigs: BTreeMap::new(),
            enabled_kems: BTreeSet::new(),
            enabled_sigs: BTreeSet::new(),
            kem_pref: DEFAULT_KEM_PREFERENCE.to_vec(),
            sig_pref: DEFAULT_SIG_PREFERENCE.to_vec(),
        }
    }

    /// Register (and enable) a KEM backend. If its suite is not already in the
    /// preference list it is appended (lowest preference).
    pub fn register_kem(&mut self, kem: Box<dyn Kem>) {
        let suite = kem.suite();
        self.kems.insert(suite, kem);
        self.enabled_kems.insert(suite);
        if !self.kem_pref.contains(&suite) {
            self.kem_pref.push(suite);
        }
    }

    /// Register (and enable) a signature backend.
    pub fn register_sig(&mut self, scheme: Box<dyn SignatureScheme>) {
        let alg = scheme.id();
        self.sigs.insert(alg, scheme);
        self.enabled_sigs.insert(alg);
        if !self.sig_pref.contains(&alg) {
            self.sig_pref.push(alg);
        }
    }

    /// Enable a registered KEM suite (hot-swap).
    pub fn enable_kem(&mut self, suite: KemSuiteId) {
        if self.kems.contains_key(&suite) {
            self.enabled_kems.insert(suite);
        }
    }

    /// Disable a KEM suite without unregistering it (hot-swap).
    pub fn disable_kem(&mut self, suite: KemSuiteId) {
        self.enabled_kems.remove(&suite);
    }

    /// Enable a registered signature algorithm.
    pub fn enable_sig(&mut self, alg: SigId) {
        if self.sigs.contains_key(&alg) {
            self.enabled_sigs.insert(alg);
        }
    }

    /// Disable a signature algorithm.
    pub fn disable_sig(&mut self, alg: SigId) {
        self.enabled_sigs.remove(&alg);
    }

    /// Replace the KEM preference order.
    pub fn set_kem_preference(&mut self, order: Vec<KemSuiteId>) {
        self.kem_pref = order;
    }

    /// Borrow the enabled KEM backend for a suite. Fails closed if the suite is
    /// disabled or unregistered.
    pub fn kem_for(&self, suite: KemSuiteId) -> Result<&dyn Kem, CryptoError> {
        if !self.enabled_kems.contains(&suite) {
            return Err(if self.kems.contains_key(&suite) {
                CryptoError::AlgorithmDisabled(suite.as_str())
            } else {
                CryptoError::UnknownAlgorithm(suite.as_str())
            });
        }
        self.kems
            .get(&suite)
            .map(|b| b.as_ref())
            .ok_or(CryptoError::UnknownAlgorithm(suite.as_str()))
    }

    /// Borrow the enabled signature backend for an algorithm.
    pub fn sig_for(&self, alg: SigId) -> Result<&dyn SignatureScheme, CryptoError> {
        if !self.enabled_sigs.contains(&alg) {
            return Err(if self.sigs.contains_key(&alg) {
                CryptoError::AlgorithmDisabled(alg.as_str())
            } else {
                CryptoError::UnknownAlgorithm(alg.as_str())
            });
        }
        self.sigs
            .get(&alg)
            .map(|b| b.as_ref())
            .ok_or(CryptoError::UnknownAlgorithm(alg.as_str()))
    }

    /// The currently-offerable capabilities: preference order filtered to
    /// registered-and-enabled algorithms.
    pub fn capabilities(&self) -> Capabilities {
        let kem_suites = self
            .kem_pref
            .iter()
            .copied()
            .filter(|s| self.enabled_kems.contains(s) && self.kems.contains_key(s))
            .collect();
        let sig_algs = self
            .sig_pref
            .iter()
            .copied()
            .filter(|a| self.enabled_sigs.contains(a) && self.sigs.contains_key(a))
            .collect();
        Capabilities {
            kem_suites,
            sig_algs,
        }
    }
}

/// A thread-safe, hot-swappable provider handle. Live algorithm changes take
/// effect behind this lock without restarting consumers.
pub type SharedProvider = Arc<RwLock<CryptoProvider>>;

/// Wrap a provider in a [`SharedProvider`].
pub fn shared(provider: CryptoProvider) -> SharedProvider {
    Arc::new(RwLock::new(provider))
}
