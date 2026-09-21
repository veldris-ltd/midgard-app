//! Randomness injection. The trait is object-safe (`&mut dyn Rng`) so backends
//! and the handshake never carry a generic `R: Rng` parameter.

/// A byte-oriented random source.
pub trait Rng {
    /// Fill `buf` with random bytes.
    fn fill(&mut self, buf: &mut [u8]);
}

/// A deterministic, seedable RNG (splitmix64) for tests and the INSECURE
/// test-backend. Never use for production key generation.
#[cfg(any(test, feature = "test-backend"))]
pub struct SeedRng {
    state: u64,
}

#[cfg(any(test, feature = "test-backend"))]
impl SeedRng {
    /// Create a deterministic RNG from a seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(any(test, feature = "test-backend"))]
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            let bytes = self.next_u64().to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&bytes[..take]);
            i += take;
        }
    }
}

/// The production OS randomness source, backed by the platform CSPRNG via
/// `getrandom` (a syscall — no network, works air-gapped). This is the RNG the
/// FIPS backend's key generation and the handshake draw from in production.
/// [NFR-PRV-1]
#[cfg(feature = "backend-fips")]
pub struct OsRng;

#[cfg(feature = "backend-fips")]
impl OsRng {
    /// Construct an `OsRng` after **probing** the OS CSPRNG, so a service can
    /// detect an entropy failure at startup and **degrade gracefully** instead of
    /// discovering it via a panic mid-keygen. Returns [`CryptoError::RngFailure`]
    /// if the CSPRNG is unavailable. [RT-51] [RC-51]
    pub fn try_new() -> Result<Self, crate::error::CryptoError> {
        let mut probe = [0u8; 16];
        let mut rng = OsRng;
        rng.try_fill(&mut probe)?;
        Ok(rng)
    }

    /// Fallible fill: draws from the OS CSPRNG and returns
    /// [`CryptoError::RngFailure`] on an entropy failure rather than panicking, so
    /// the **caller can recover** (retry, back off, or refuse the operation
    /// cleanly). The infallible [`Rng::fill`] is this call's fail-closed sibling —
    /// use `try_fill` wherever a crypto failure should surface as an error. [RC-51]
    pub fn try_fill(&mut self, buf: &mut [u8]) -> Result<(), crate::error::CryptoError> {
        use rand_core::RngCore;
        rand_core::OsRng
            .try_fill_bytes(buf)
            .map_err(|_| crate::error::CryptoError::RngFailure)
    }
}

#[cfg(feature = "backend-fips")]
impl Rng for OsRng {
    fn fill(&mut self, buf: &mut [u8]) {
        // `rand_core::OsRng` pulls from the OS CSPRNG (getrandom). It is
        // infallible in practice; on the rare OS-entropy failure we must not
        // proceed with weak randomness, so we fail closed rather than emit key
        // material from a degraded source. Callers that want to RECOVER from an
        // entropy failure use [`OsRng::try_fill`] / [`OsRng::try_new`] instead of
        // this panicking path. [AC-M-1] [RT-51]
        self.try_fill(buf)
            .expect("OS CSPRNG (getrandom) failed — refusing to generate keys from weak entropy");
    }
}
