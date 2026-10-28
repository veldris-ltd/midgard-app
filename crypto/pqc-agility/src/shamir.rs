//! Shamir secret sharing over GF(2^8): split a secret into `n` shares so that any
//! `k` recover it and fewer than `k` reveal nothing about it.
//! [RT-35] [NFR-PQ-2]
//!
//! Used for key escrow: the release key ceremony seals its keystores under a
//! random 256-bit key and splits that key among named holders, so recovery needs
//! a quorum of people and no single drive or person holds the keys
//! (docs/ops/DR_PLAN.md, step 4). Splitting is information-theoretic, so it is
//! unaffected by quantum attack; what it protects is a 256-bit AES key.
//!
//! Each byte of the secret is the constant term of its own random polynomial of
//! degree `k - 1`; share `x` holds that polynomial's value at `x` (1..=255).
//! Field arithmetic is constant-time (no table lookups indexed by secret bytes),
//! and every intermediate holding secret material is zeroised on drop.
//!
//! **REVIEW REQUIRED (production replacement marker).** This is a fresh, small
//! implementation of a textbook construction, tested here but not yet
//! independently reviewed. It must be reviewed with the rest of the crypto core
//! before keys protected by it are relied on in production.

use zeroize::{Zeroize, Zeroizing};

use crate::rng::Rng;

/// One share: its evaluation point and the polynomial values at that point, one
/// byte per byte of the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Share {
    /// The evaluation point, 1..=255. Never 0: the value at 0 is the secret.
    pub x: u8,
    /// One byte per byte of the secret.
    pub y: Zeroizing<Vec<u8>>,
}

impl core::fmt::Debug for Share {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A share is secret material: never print its value.
        f.debug_struct("Share")
            .field("x", &self.x)
            .field("len", &self.y.len())
            .finish()
    }
}

/// Why a split or a recovery was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShamirError {
    /// `k` must be at least 2 (one share would be the secret itself) and at most `n`.
    BadThreshold { k: u8, n: u8 },
    /// An empty secret has nothing to protect.
    EmptySecret,
    /// Fewer shares than any threshold, or shares of different lengths.
    BadShares(&'static str),
    /// Two shares with the same evaluation point.
    DuplicateShare(u8),
}

impl core::fmt::Display for ShamirError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadThreshold { k, n } => write!(
                f,
                "a threshold of {k} of {n} is not allowed: need 2 <= k <= n <= 255"
            ),
            Self::EmptySecret => write!(f, "the secret is empty"),
            Self::BadShares(why) => write!(f, "the shares cannot be combined: {why}"),
            Self::DuplicateShare(x) => write!(f, "share {x} was given twice"),
        }
    }
}

impl std::error::Error for ShamirError {}

/// Split `secret` into `n` shares, any `k` of which recover it.
pub fn split(secret: &[u8], k: u8, n: u8, rng: &mut dyn Rng) -> Result<Vec<Share>, ShamirError> {
    if k < 2 || k > n {
        return Err(ShamirError::BadThreshold { k, n });
    }
    if secret.is_empty() {
        return Err(ShamirError::EmptySecret);
    }
    let mut shares: Vec<Share> = (1..=n)
        .map(|x| Share {
            x,
            y: Zeroizing::new(vec![0u8; secret.len()]),
        })
        .collect();
    // Coefficients 1..k of one byte's polynomial; coefficient 0 is the secret byte.
    let mut coeffs = Zeroizing::new(vec![0u8; usize::from(k) - 1]);
    for (i, &s) in secret.iter().enumerate() {
        rng.fill(&mut coeffs);
        for share in &mut shares {
            // Horner's rule, highest coefficient first.
            let mut acc = 0u8;
            for &c in coeffs.iter().rev() {
                acc = gf_mul(acc, share.x) ^ c;
            }
            share.y[i] = gf_mul(acc, share.x) ^ s;
            acc.zeroize();
        }
    }
    Ok(shares)
}

/// Recover the secret from shares. Any `k` or more shares of one split recover
/// it; fewer give an unrelated value, which is why callers check the result
/// (for instance by an AEAD tag) rather than trusting it.
pub fn combine(shares: &[Share]) -> Result<Zeroizing<Vec<u8>>, ShamirError> {
    if shares.len() < 2 {
        return Err(ShamirError::BadShares("at least two shares are needed"));
    }
    let len = shares[0].y.len();
    if len == 0 || shares.iter().any(|s| s.y.len() != len) {
        return Err(ShamirError::BadShares(
            "the shares are of different lengths",
        ));
    }
    for (i, a) in shares.iter().enumerate() {
        if a.x == 0 {
            return Err(ShamirError::BadShares("a share has evaluation point 0"));
        }
        if shares[..i].iter().any(|b| b.x == a.x) {
            return Err(ShamirError::DuplicateShare(a.x));
        }
    }
    // Lagrange basis at 0: l_j = prod_{m != j} x_m / (x_m - x_j); in GF(2^8)
    // subtraction is xor. The x values are public; only the y values are secret.
    let basis: Vec<u8> = shares
        .iter()
        .enumerate()
        .map(|(j, sj)| {
            let (mut num, mut den) = (1u8, 1u8);
            for (m, sm) in shares.iter().enumerate() {
                if m != j {
                    num = gf_mul(num, sm.x);
                    den = gf_mul(den, sm.x ^ sj.x);
                }
            }
            gf_mul(num, gf_inv(den))
        })
        .collect();
    let mut out = Zeroizing::new(vec![0u8; len]);
    for (share, &l) in shares.iter().zip(&basis) {
        for (o, &y) in out.iter_mut().zip(share.y.iter()) {
            *o ^= gf_mul(y, l);
        }
    }
    Ok(out)
}

/// Multiplication in GF(2^8) with the AES polynomial x^8 + x^4 + x^3 + x + 1,
/// in constant time: eight rounds whatever the operands, with masks in place of
/// branches.
fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    for _ in 0..8 {
        p ^= a & 0u8.wrapping_sub(b & 1);
        let carry = 0u8.wrapping_sub(a >> 7);
        a = (a << 1) ^ (0x1b & carry);
        b >>= 1;
    }
    p
}

/// The multiplicative inverse, as a^254 (a^255 = 1 for a != 0), in constant time.
/// Only ever applied to public values here, but constant time costs nothing.
fn gf_inv(a: u8) -> u8 {
    let mut result = 1u8;
    let mut base = a;
    let mut e = 254u8;
    for _ in 0..8 {
        let m = 0u8.wrapping_sub(e & 1);
        let prod = gf_mul(result, base);
        result = (prod & m) | (result & !m);
        base = gf_mul(base, base);
        e >>= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SeedRng;

    #[test]
    fn field_arithmetic_matches_the_aes_field() {
        // FIPS 197, section 4.2: {57} x {83} = {c1}, and {57} x {13} = {fe}.
        assert_eq!(gf_mul(0x57, 0x83), 0xc1);
        assert_eq!(gf_mul(0x57, 0x13), 0xfe);
        for a in 1..=255u8 {
            assert_eq!(gf_mul(a, gf_inv(a)), 1, "inverse of {a:#x}");
            assert_eq!(gf_mul(a, 1), a);
            assert_eq!(gf_mul(a, 0), 0);
        }
    }

    #[test]
    fn any_k_shares_recover_the_secret() {
        let mut rng = SeedRng::new(7);
        let secret: Vec<u8> = (0..32).map(|i| (i * 37 + 11) as u8).collect();
        let shares = split(&secret, 3, 5, &mut rng).unwrap();
        assert_eq!(shares.len(), 5);
        assert_eq!(
            shares.iter().map(|s| s.x).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        for a in 0..5 {
            for b in a + 1..5 {
                for c in b + 1..5 {
                    let pick = [shares[a].clone(), shares[b].clone(), shares[c].clone()];
                    assert_eq!(&combine(&pick).unwrap()[..], &secret[..], "{a} {b} {c}");
                }
            }
        }
        // More than k also recovers it.
        assert_eq!(&combine(&shares).unwrap()[..], &secret[..]);
    }

    #[test]
    fn fewer_than_k_shares_do_not_recover_it() {
        let mut rng = SeedRng::new(9);
        let secret = [0xA5u8; 32];
        let shares = split(&secret, 3, 5, &mut rng).unwrap();
        let two = combine(&shares[..2]).unwrap();
        assert_ne!(&two[..], &secret[..]);
    }

    #[test]
    fn a_share_alone_is_not_the_secret() {
        // With k >= 2 no share equals the secret (with overwhelming probability
        // for random coefficients; fixed here by the seed).
        let mut rng = SeedRng::new(3);
        let secret = [0x42u8; 32];
        for s in split(&secret, 2, 4, &mut rng).unwrap() {
            assert_ne!(&s.y[..], &secret[..]);
        }
    }

    #[test]
    fn refuses_bad_parameters_and_bad_shares() {
        let mut rng = SeedRng::new(1);
        assert_eq!(
            split(b"s", 1, 3, &mut rng).unwrap_err(),
            ShamirError::BadThreshold { k: 1, n: 3 }
        );
        assert!(split(b"s", 4, 3, &mut rng).is_err());
        assert_eq!(
            split(b"", 2, 3, &mut rng).unwrap_err(),
            ShamirError::EmptySecret
        );
        let shares = split(b"secret", 2, 3, &mut rng).unwrap();
        assert_eq!(
            combine(&[shares[0].clone(), shares[0].clone()]).unwrap_err(),
            ShamirError::DuplicateShare(1)
        );
        assert!(combine(&shares[..1]).is_err());
        let mut short = shares[1].clone();
        short.y.pop();
        assert!(combine(&[shares[0].clone(), short]).is_err());
    }

    #[test]
    fn debug_never_prints_share_values() {
        let mut rng = SeedRng::new(5);
        let s = &split(&[0xEE; 4], 2, 2, &mut rng).unwrap()[0];
        let shown = format!("{s:?}");
        assert!(
            !shown.contains("238") && !shown.to_lowercase().contains("ee"),
            "{shown}"
        );
    }
}
