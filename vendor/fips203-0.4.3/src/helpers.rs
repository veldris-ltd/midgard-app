use crate::ntt::multiply_ntts;
use crate::types::Z;
use crate::Q;
use sha3::digest::{ExtendableOutput, Update, XofReader};
use sha3::{Digest, Sha3_256, Sha3_512, Shake128, Shake256};


/// If the condition is not met, return an error message. Borrowed from the `anyhow` crate.
macro_rules! ensure {
    ($cond:expr, $msg:literal $(,)?) => {
        if !$cond {
            return Err($msg);
        }
    };
}

pub(crate) use ensure; // make available throughout crate


/// Vector addition; See commentary on 2.11 page 10: `z_hat` = `u_hat` + `v_hat`
///
/// # Arguments
/// * `vec_a` - First vector of size K×256
/// * `vec_b` - Second vector of size K×256
///
/// # Returns
/// Sum of the two vectors element-wise
#[must_use]
pub(crate) fn add_vecs<const K: usize>(
    vec_a: &[[Z; 256]; K], vec_b: &[[Z; 256]; K],
) -> [[Z; 256]; K] {
    core::array::from_fn(|k| core::array::from_fn(|n| vec_a[k][n].add(vec_b[k][n])))
}


/// Matrix by vector multiplication; See commentary on 2.12 page 10: `w_hat` = `A_hat` mul `u_hat`
///
/// # Arguments
/// * `a_hat` - Matrix of size K×K×256
/// * `u_hat` - Vector of size K×256
///
/// # Returns
/// Result of matrix multiplication `A_hat * u_hat`
#[must_use]
pub(crate) fn mul_mat_vec<const K: usize>(
    a_hat: &[[[Z; 256]; K]; K], u_hat: &[[Z; 256]; K],
) -> [[Z; 256]; K] {
    let mut w_hat = [[Z::default(); 256]; K];
    for i in 0..K {
        #[allow(clippy::needless_range_loop)] // alternative is harder to understand
        for j in 0..K {
            let tmp = multiply_ntts(&a_hat[i][j], &u_hat[j]);
            w_hat[i] = add_vecs(&[w_hat[i]], &[tmp])[0];
        }
    }
    w_hat
}


/// Matrix transpose by vector multiplication; See commentary on 2.13 page 10: `y_hat` = `A_hat^T` mul `u_hat`
///
/// # Arguments
/// * `a_hat` - Matrix of size K×K×256 to be transposed before multiplication
/// * `u_hat` - Vector of size K×256
///
/// # Returns
/// Result of matrix multiplication `A_hat^T * u_hat`, where `^T` denotes transpose
#[must_use]
pub(crate) fn mul_mat_t_vec<const K: usize>(
    a_hat: &[[[Z; 256]; K]; K], u_hat: &[[Z; 256]; K],
) -> [[Z; 256]; K] {
    let mut y_hat = [[Z::default(); 256]; K];
    #[allow(clippy::needless_range_loop)] // alternative is harder to understand
    for i in 0..K {
        #[allow(clippy::needless_range_loop)] // alternative is harder to understand
        for j in 0..K {
            let tmp = multiply_ntts(&a_hat[j][i], &u_hat[j]); // i,j swapped vs above fn
            y_hat[i] = add_vecs(&[y_hat[i]], &[tmp])[0];
        }
    }
    y_hat
}


/// Vector dot product; See commentary on 2.14 page 10: `z_hat` = `u_hat^T` mul `v_hat`
///
/// # Arguments
/// * `u_hat` - First vector of size K×256
/// * `v_hat` - Second vector of size K×256
///
/// # Returns
/// Dot product result as a 256-element array, computed as sum of element-wise products
#[must_use]
pub(crate) fn dot_t_prod<const K: usize>(u_hat: &[[Z; 256]; K], v_hat: &[[Z; 256]; K]) -> [Z; 256] {
    let mut result = [Z::default(); 256];
    for j in 0..K {
        let tmp = multiply_ntts(&u_hat[j], &v_hat[j]);
        result = add_vecs(&[result], &[tmp])[0];
    }
    result
}


/// Function PRF on page 18 (4.3).
/// Pseudorandom function that generates `ETA_64` bytes of output using SHAKE256
///
/// # Arguments
/// * `s` - 32-byte seed
/// * `b` - Single byte domain separator
#[must_use]
pub(crate) fn prf<const ETA_64: usize>(s: &[u8; 32], b: u8) -> [u8; ETA_64] {
    let mut hasher = Shake256::default();
    hasher.update(s);
    hasher.update(&[b]);
    let mut reader = hasher.finalize_xof();
    let mut result = [0u8; ETA_64];
    reader.read(&mut result);
    result
}


/// Function XOF on page 19 (4.6), used with 32-byte `rho`
/// Expandable output function based on SHAKE128 for generating matrix elements
///
/// # Arguments
/// * `rho` - 32-byte seed for randomness
/// * `i` - Row index for matrix generation
/// * `j` - Column index for matrix generation
///
/// # Returns
/// An extendable output reader that can generate arbitrary length output
#[must_use]
pub(crate) fn xof(rho: &[u8; 32], i: u8, j: u8) -> impl XofReader {
    //debug_assert_eq!(rho.len(), 32);
    let mut hasher = Shake128::default();
    hasher.update(rho);
    hasher.update(&[i]);
    hasher.update(&[j]);
    hasher.finalize_xof()
}


/// Function G on page 19 (4.5).
/// Hash function that produces two 32-byte outputs from variable input
///
/// # Arguments
/// * `bytes` - Slice of byte slices to be hashed together
///
/// # Returns
/// Tuple of two 32-byte arrays (tr, K) as specified in the protocol
pub(crate) fn g(bytes: &[&[u8]]) -> ([u8; 32], [u8; 32]) {
    let mut hasher = Sha3_512::new();
    bytes.iter().for_each(|b| Digest::update(&mut hasher, b));
    let digest = hasher.finalize();
    let a = digest[0..32].try_into().expect("g_a fail");
    let b = digest[32..64].try_into().expect("g_b fail");
    (a, b)
}


/// Function H on page 18 (4.4).
/// Hash function that produces a single 32-byte output
///
/// # Arguments
/// * `bytes` - Input bytes to hash (typically public key)
///
/// # Returns
/// 32-byte array representing the hash
#[must_use]
pub(crate) fn h(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();
    Digest::update(&mut hasher, bytes);
    let digest = hasher.finalize();
    digest.into()
}


/// Function J on page 18 (4.4).
/// XOF-based hash function for challenge generation
///
/// # Arguments
/// * `z` - 32-byte seed
/// * `ct` - Variable length ciphertext
///
/// # Returns
/// 32-byte challenge value derived from inputs
#[must_use]
pub(crate) fn j(z: &[u8; 32], ct: &[u8]) -> [u8; 32] {
    let mut hasher = Shake256::default();
    hasher.update(z);
    hasher.update(ct);
    let mut reader = hasher.finalize_xof();
    let mut result = [0u8; 32];
    reader.read(&mut result);
    result
}


/// Compress<d> from page 21 (4.7).
/// x → ⌈(2^d/q) · x⌋
///
/// This function compresses elements from `Z_q` to a smaller range by scaling them down.
/// The compression is lossy but maintains approximate ratios between elements.
///
/// # Arguments
/// * `d` - Compression parameter that determines output range (0 to 11)
/// * `inout` - Vector of elements to compress in-place
///
/// # Implementation Notes
/// * Works for all odd q values from 17 to 6307
/// * Input x must be in range 0 to q-1
/// * Uses pre-computed multiplier M to avoid floating-point arithmetic
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn compress_vector(d: u32, inout: &mut [Z]) {
    const M: u32 = (((1u64 << 36) + Q as u64 - 1) / Q as u64) as u32;
    for x_ref in &mut *inout {
        let y = (x_ref.get_u32() << d) + (u32::from(Q) >> 1);
        let result = (u64::from(y) * u64::from(M)) >> 36;
        x_ref.set_u16(result as u16);
    }
}


/// Decompress<d> from page 21 (4.8).
/// y → ⌈(q/2^d) · y⌋
///
/// Inverse operation of `compress_vector` that expands compressed elements back to `Z_q`.
/// While not perfect due to lossy compression, attempts to restore original ratios.
///
/// # Arguments
/// * `d` - Same compression parameter used in `compress_vector`
/// * `inout` - Vector of compressed elements to decompress in-place
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn decompress_vector(d: u32, inout: &mut [Z]) {
    for y_ref in &mut *inout {
        let qy = u32::from(Q) * y_ref.get_u32() + (1 << d) - 1;
        y_ref.set_u16((qy >> d) as u16);
    }
}
