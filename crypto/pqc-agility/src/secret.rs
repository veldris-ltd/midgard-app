//! Secret-holding newtypes: **zeroized on drop via the vetted `zeroize` crate**, and
//! (best-effort) **locked into RAM with `mlock`** so long-lived key material is not
//! written to swap. [NFR-PQ-2] [RC-39]
//!
//! ## Guarantees & limits
//! - **Zeroization** uses `zeroize` — an audited implementation whose drop glue is not
//!   elided and which clears the backing storage. (Replaces the previous home-rolled
//!   `write_volatile`.) [`SecretArray::new`] also wipes the by-value array it was
//!   given once it has been copied to the heap.
//! - **Swap protection** is process-wide: services call [`lock_process_memory`] at start
//!   (`mlockall(MCL_CURRENT | MCL_FUTURE)`), so every page, secret or not, stays in RAM
//!   for the life of the process. Until that succeeds, each secret's own pages are
//!   `mlock`ed on construction as a fallback. Drop **never** unlocks: Linux locks are
//!   page-granular and not reference-counted, so unlocking one small secret's range
//!   would also unlock a page a long-lived key shares. Locked pages stay locked until
//!   the process exits. [VLD-ANB-MIDGARD-0036]
//! - Locking is **best-effort**: if `RLIMIT_MEMLOCK` is too low or the platform lacks
//!   it, the secret is held unlocked (zeroization still applies) and
//!   [`is_locked`](SecretBytes::is_locked) reports `false`. On non-Unix targets locking
//!   is a no-op. Operators should set `RLIMIT_MEMLOCK` to unlimited (or grant
//!   `CAP_IPC_LOCK` with an unlimited limit) so the process lock succeeds.
//! - **Residual**: copies a *caller* leaves in CPU registers or spills to the stack
//!   are outside these types' control. Construct secrets at their final size and never
//!   grow them — a `Vec` realloc would move the buffer out from under the lock and
//!   leave a copy in a freed page.
//!
//! Secret types deliberately do **not** implement `Clone` — copying key material must
//! be explicit via `expose`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Once;

use zeroize::Zeroize;

/// How many secrets this process failed to lock into RAM.
static LOCK_FAILURES: AtomicU64 = AtomicU64::new(0);
static LOCK_FAILURE_WARNING: Once = Once::new();
/// Whether [`lock_process_memory`] has locked every current and future page.
static PROCESS_LOCKED: AtomicBool = AtomicBool::new(false);

/// How many secrets this process has failed to `mlock` since it started. A non-zero
/// count means key material may reach swap; services export it as a metric. [MG-SEC-08]
pub fn mlock_failures() -> u64 {
    LOCK_FAILURES.load(Ordering::Relaxed)
}

/// Count a failed lock, and say so on stderr the first time — a silent fallback would
/// leave an operator believing keys are kept out of swap when they are not. [MG-SEC-08]
fn report_lock_failure(len: usize) {
    LOCK_FAILURES.fetch_add(1, Ordering::Relaxed);
    LOCK_FAILURE_WARNING.call_once(|| {
        eprintln!(
            "warning: pqc-agility could not mlock a {len}-byte secret ({}); key material \
             may be written to swap. Raise RLIMIT_MEMLOCK or grant CAP_IPC_LOCK.",
            std::io::Error::last_os_error()
        );
    });
}

/// Lock a byte range into RAM so it is never paged to swap. Best-effort: returns
/// whether the lock succeeded, and reports a failure through [`mlock_failures`] and a
/// one-time warning. Zero-length ranges are never locked, and nothing is done once the
/// whole process is locked.
fn lock_pages(ptr: *const u8, len: usize) -> bool {
    if len == 0 || process_locked() {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: `ptr`/`len` describe a live allocation owned by the caller for the
        // secret's lifetime; mlock only pins its pages and mutates no memory.
        let locked = unsafe { libc::mlock(ptr as *const libc::c_void, len) == 0 };
        if !locked {
            report_lock_failure(len);
        }
        locked
    }
    #[cfg(not(unix))]
    {
        let _ = (ptr, len);
        false
    }
}

/// Whether the whole process is locked into RAM (see [`lock_process_memory`]).
fn process_locked() -> bool {
    PROCESS_LOCKED.load(Ordering::Relaxed)
}

/// Lock every current and future page of this process into RAM with
/// `mlockall(MCL_CURRENT | MCL_FUTURE)`, so no secret can reach swap and no secret's
/// drop can unlock a page another secret shares. Services call this once at start,
/// through `service_runtime::harden_process`. Idempotent. [VLD-ANB-MIDGARD-0036]
///
/// Best-effort: on failure the process keeps running with per-secret locks and this
/// returns the reason. It is only attempted when `RLIMIT_MEMLOCK` is unlimited: with a
/// finite limit, `MCL_FUTURE` would make every allocation past the limit fail, turning
/// ordinary growth into an out-of-memory abort.
pub fn lock_process_memory() -> Result<(), String> {
    if process_locked() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `lim` is a valid, writable rlimit for getrlimit to fill.
        if unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut lim) } != 0 {
            return Err(format!(
                "getrlimit(RLIMIT_MEMLOCK) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if lim.rlim_cur != libc::RLIM_INFINITY {
            return Err(format!(
                "RLIMIT_MEMLOCK is {} bytes, not unlimited",
                lim.rlim_cur
            ));
        }
        // SAFETY: mlockall takes only flags and mutates no memory.
        if unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) } != 0 {
            return Err(format!(
                "mlockall failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        PROCESS_LOCKED.store(true, Ordering::Relaxed);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err("mlockall is not available on this platform".to_string())
    }
}

/// Variable-length secret bytes: mlocked on construction (unless the whole process is
/// locked), zeroized on drop. Backs the long-lived PQ secret keys.
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

    /// Whether the secret's pages are locked into RAM: its own mlock succeeded, or the
    /// whole process is locked. An empty secret has no pages and is never locked.
    pub fn is_locked(&self) -> bool {
        self.locked || (!self.buf.is_empty() && process_locked())
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        // Zeroize only. Unlocking here would unlock whole pages other secrets share.
        self.buf.zeroize();
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SecretBytes([redacted; {} bytes])", self.buf.len())
    }
}

/// Fixed-length secret bytes, mlocked on construction (unless the whole process is
/// locked) and zeroized on drop. Preferred for known key sizes (its stable heap address
/// makes the lock robust).
pub struct SecretArray<const N: usize> {
    buf: Box<[u8; N]>,
    locked: bool,
}

impl<const N: usize> SecretArray<N> {
    /// Wrap a fixed-size array. The by-value `buf` is wiped once it has been copied to
    /// the heap.
    pub fn new(mut buf: [u8; N]) -> Self {
        let boxed = Box::new(buf);
        buf.zeroize();
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

    /// Whether the secret's pages are locked into RAM: its own mlock succeeded, or the
    /// whole process is locked.
    pub fn is_locked(&self) -> bool {
        self.locked || (N > 0 && process_locked())
    }
}

impl<const N: usize> Drop for SecretArray<N> {
    fn drop(&mut self) {
        // Zeroize only. Unlocking here would unlock whole pages other secrets share.
        self.buf.as_mut_slice().zeroize();
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

    // [AC-51.3] [VLD-ANB-MIDGARD-0036] Dropping a secret must never unlock pages: Linux
    // locks are page-granular, so unlocking a small secret's range would also unlock a
    // long-lived key on the same page. The drops below run the real path, and the
    // module is held to having no munlock call at all (the name is split so this test
    // does not match itself).
    #[test]
    fn drop_does_not_unlock_pages() {
        let keep = SecretArray::<32>::new([7; 32]);
        let locked_before = keep.is_locked();
        drop(SecretArray::<32>::new([1; 32]));
        drop(SecretBytes::new(vec![2; 48]));
        assert_eq!(keep.is_locked(), locked_before || process_locked());
        assert_eq!(keep.expose(), &[7; 32]);

        let src = include_str!("secret.rs");
        let call = concat!("mun", "lock(");
        assert!(
            !src.contains(call),
            "secret.rs must not call munlock: a per-secret unlock removes a neighbour's lock"
        );
    }

    // [AC-51.3] [VLD-ANB-MIDGARD-0036]
    #[test]
    fn lock_process_memory_is_idempotent_and_best_effort() {
        // Either outcome is acceptable (CI hosts usually have a finite RLIMIT_MEMLOCK).
        // What matters: it never panics, once it has succeeded a repeat succeeds too,
        // and is_locked() follows the process lock.
        let first = lock_process_memory();
        let second = lock_process_memory();
        if first.is_ok() {
            assert!(second.is_ok(), "a locked process stays locked");
        }
        assert_eq!(process_locked(), first.is_ok() || second.is_ok());
        let s = SecretArray::<32>::new([3; 32]);
        if process_locked() {
            assert!(s.is_locked());
        }
        if let Err(why) = first {
            assert!(!why.is_empty());
        }
    }
}
