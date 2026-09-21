use crate::Q;
use zeroize::{Zeroize, ZeroizeOnDrop};


/// Correctly sized encapsulation key specific to the target security parameter set.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[repr(align(8))]
pub struct EncapsKey<const EK_LEN: usize>(pub(crate) [u8; EK_LEN]);


/// Correctly sized decapsulation key specific to the target security parameter set.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[repr(align(8))]
pub struct DecapsKey<const DK_LEN: usize>(pub(crate) [u8; DK_LEN]);


/// Correctly sized ciphertext specific to the target security parameter set.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[repr(align(8))]
pub struct CipherText<const CT_LEN: usize>(pub(crate) [u8; CT_LEN]);


// While Z is simple and correct, the performance is somewhat suboptimal.
// This will be addressed (particularly in matrix operations etc) over
// the medium-term - potentially using 256-entry rows.

/// Stored as u16 for space, but arithmetic as u32 for perf
#[derive(Clone, Copy, Default)]
pub(crate) struct Z(pub(crate) u16);


#[allow(clippy::inline_always)]
impl Z {
    pub(crate) fn get_u32(self) -> u32 { u32::from(self.0) }

    pub(crate) fn set_u16(&mut self, a: u16) { self.0 = a }

    #[inline(always)]
    #[allow(clippy::cast_possible_truncation)] // rem as u16; for perf
    pub(crate) fn add(self, other: Self) -> Self {
        debug_assert!(self.0 < Q);
        debug_assert!(other.0 < Q);
        let res = u32::from(self.0) + u32::from(other.0); // + debug=strict, release=wrapping
        let res = res.wrapping_sub(u32::from(Q));
        let res = res.wrapping_add((res >> 16) & (u32::from(Q)));
        debug_assert!(res < u32::from(Q));
        Self(res as u16)
    }

    #[inline(always)]
    #[allow(clippy::cast_possible_truncation)] // res as u16; for perf
    pub(crate) fn sub(self, other: Self) -> Self {
        debug_assert!(self.0 < Q);
        debug_assert!(other.0 < Q);
        let res = u32::from(self.0).wrapping_sub(u32::from(other.0));
        let res = res.wrapping_add((res >> 16) & (u32::from(Q)));
        debug_assert!(res < u32::from(Q));
        Self(res as u16)
    }

    #[inline(always)]
    #[allow(clippy::cast_possible_truncation)] // rem as u16; for perf
    pub(crate) fn mul(self, other: Self) -> Self {
        const M: u64 = ((1u64 << 36) + Q as u64 - 1) / Q as u64;
        debug_assert!(self.0 < Q);
        debug_assert!(other.0 < Q);
        let prod = u32::from(self.0) * u32::from(other.0); // * debug=strict, release=wrapping
        let quot = ((u64::from(prod) * M) >> 36) as u32;
        let rem = prod - quot * u32::from(Q); // further reduction is not needed
        debug_assert!(rem < u32::from(Q));
        Self(rem as u16)
    }

    #[inline(always)]
    #[allow(clippy::cast_possible_truncation)] // rem as u16; for perf
    pub(crate) fn base_mul(self, a1: Self, b0: Self, b1: Self, gamma: Self) -> Self {
        // 1: c0 ← a0 · b0 + a1 · b1 · γ    ▷ steps 1-2 done modulo q
        const M: u128 = ((1u128 << 100) + Q as u128 - 1) / Q as u128;
        debug_assert!(self.0 < Q);
        debug_assert!(a1.0 < Q);
        debug_assert!(b0.0 < Q);
        debug_assert!(b1.0 < Q);
        debug_assert!(gamma.0 < Q);
        let prod = u64::from(self.0) * u64::from(b0.0)
            + u64::from(a1.0) * u64::from(b1.0) * u64::from(gamma.0);
        let quot = (u128::from(prod) * M) >> 100;
        let rem = u128::from(prod) - quot * u128::from(Q); // further reduction is not needed
        debug_assert!(rem < u128::from(Q));
        Self(rem as u16)
    }

    #[inline(always)]
    #[allow(clippy::cast_possible_truncation)] // rem as u16; for perf
    pub(crate) fn base_mul2(self, a1: Self, b0: Self, b1: Self) -> Self {
        // 2: c1 ← a0 · b1 + a1 · b0
        const M: u64 = ((1u64 << 36) + Q as u64 - 1) / Q as u64;
        debug_assert!(self.0 < Q);
        debug_assert!(a1.0 < Q);
        debug_assert!(b0.0 < Q);
        debug_assert!(b1.0 < Q);
        let prod = u32::from(self.0) * u32::from(b1.0) + u32::from(a1.0) * u32::from(b0.0); // * debug=strict, release=wrapping
        let quot = ((u64::from(prod) * M) >> 36) as u32;
        let rem = prod - quot * u32::from(Q); // further reduction is not needed
        debug_assert!(rem < u32::from(Q));
        Self(rem as u16)
    }
}
