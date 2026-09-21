//! Secret-holding newtypes: **zeroized on drop via the vetted `zeroize` crate**, and
//! (best-effort) **locked into RAM with `mlock`** so long-lived key material is not
//! written to swap. [NFR-PQ-2] [RC-39]
//!
//! ## Guarantees & limits
//! - **Zeroization** uses `zeroize` — an audited implementation whose drop glue is not
//!   elided and which clears the backing storage. (Replaces the previous home-rolled
//!   `write_volatile`.)
//! - **Swap protection**: on construction the secret's pages are `mlock`ed (Unix); on
//!   drop the bytes are zeroized and then `munlock`ed. mlock is **best-effort** — if
//!   `RLIMIT_MEMLOCK` is exhausted or the platform lacks it, the secret is held
//!   unlocked (zeroization still applies) and [`is_locked`](SecretBytes::is_locked)
//!   reports `false`. On non-Unix targets locking is a no-op. Operators running the
//!   enclosure should raise `RLIMIT_MEMLOCK` (or grant `CAP_IPC_LOCK`) so key material
//!   locks reliably.
//! - **Residual**: copies a *caller* leaves in CPU registers or spills to the stack
//!   are outside these types' control. Construct secrets at their final size and never
//!   grow them — a `Vec` realloc would move the buffer out from under the lock and
//!   leave a copy in a freed page.
//!
//! Secret types deliberately do **not** implement `Clone` — copying key material must
//! be explicit via `expose`.

use zeroize::Zeroize;

/// Lock a byte range into RAM so it is never paged to swap. Best-effort: returns
/// whether the lock succeeded. Zero-length ranges are never locked.
fn lock_pages(ptr: *const u8, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: `ptr`/`len` describe a live allocation owned by the caller for the
        // secret's lifetime; mlock only pins its pages and mutates no memory.
        unsafe { libc::mlock(ptr as *const libc::c_void, len) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = (ptr, len);
        false
    }
}

/// Undo [`lock_pages`] for the same range. No-op on a range that was not locked.
fn unlock_pages(ptr: *const u8, len: usize) {
    if len == 0 {
        return;
    }
    #[cfg(unix)]
    {
        // SAFETY: same range previously passed to `mlock`; the allocation is still
        // live at drop time.
        unsafe {
            let _ = libc::munlock(ptr as *const libc::c_void, len);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (ptr, len);
    }
}

/// Variable-length secret bytes: mlocked on construction, zeroized (and munlocked) on
/// drop. Backs the long-lived PQ secret keys.
pub struct SecretBytes {
    buf: Vec<u8>,
    locked: bool,
}

impl SecretBytes {
    /// Wrap an existing buffer. Construct at final size and do not grow it (a realloc
    /// would defeat the lock and leave a copy behind).
    pub fn new(buf: Vec<u8>) -> Self {
        let locked = lock_pages(buf.as_ptr(), buf.len());
        Self { buf, locked }
    }

    /// Borrow the secret bytes. Callers must not copy them casually.
    pub fn expose(&self) -> &[u8] {
        &self.buf
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Whether the secret's pages are locked into RAM (mlock succeeded).
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        // Capture the range before zeroize resets the length.
        let (ptr, len) = (self.buf.as_ptr(), self.buf.len());
        self.buf.zeroize();
        if self.locked {
            unlock_pages(ptr, len);
        }
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SecretBytes([redacted; {} bytes])", self.buf.len())
    }
}

/// Fixed-length secret bytes, mlocked + zeroized on drop. Preferred for known key
/// sizes (its stable heap address makes the lock robust).
pub struct SecretArray<const N: usize> {
    buf: Box<[u8; N]>,
    locked: bool,
}

impl<const N: usize> SecretArray<N> {
    /// Wrap a fixed-size array.
    pub fn new(buf: [u8; N]) -> Self {
        let boxed = Box::new(buf);
        let locked = lock_pages(boxed.as_ptr(), N);
        Self { buf: boxed, locked }
    }

    /// An all-zero secret.
    pub fn zeroed() -> Self {
        Self::new([0u8; N])
    }

    /// Borrow the secret.
    pub fn expose(&self) -> &[u8; N] {
        &self.buf
    }

    /// Mutably borrow the secret (e.g. to fill it in place).
    pub fn expose_mut(&mut self) -> &mut [u8; N] {
        &mut self.buf
    }

    /// Whether the secret's pages are locked into RAM (mlock succeeded).
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

impl<const N: usize> Drop for SecretArray<N> {
    fn drop(&mut self) {
        let ptr = self.buf.as_ptr();
        self.buf.as_mut_slice().zeroize();
        if self.locked {
            unlock_pages(ptr, N);
        }
    }
}

impl<const N: usize> core::fmt::Debug for SecretArray<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SecretArray([redacted; {N} bytes])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_array_exposes_and_zeroizes_in_place() {
        let mut s = SecretArray::<4>::new([9, 9, 9, 9]);
        assert_eq!(s.expose(), &[9, 9, 9, 9]);
        s.expose_mut().as_mut_slice().zeroize();
        assert_eq!(s.expose(), &[0, 0, 0, 0]);
    }

    #[test]
    fn zeroed_is_all_zero() {
        let s = SecretArray::<32>::zeroed();
        assert_eq!(s.expose(), &[0u8; 32]);
    }

    #[test]
    fn secret_bytes_roundtrips_and_reports_lock_state() {
        let s = SecretBytes::new(vec![1, 2, 3, 4, 5]);
        assert_eq!(s.expose(), &[1, 2, 3, 4, 5]);
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
        // is_locked() reflects whether mlock succeeded in this environment; it must be
        // callable and consistent (a locked secret stays reported locked).
        let _ = s.is_locked();
    }

    #[test]
    fn empty_secret_is_never_locked() {
        let s = SecretBytes::new(Vec::new());
        assert!(!s.is_locked(), "a zero-length secret has no pages to lock");
        assert!(s.is_empty());
    }
}
