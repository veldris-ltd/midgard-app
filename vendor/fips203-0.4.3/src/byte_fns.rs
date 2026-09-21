use crate::helpers::ensure;
use crate::types::Z;
use crate::Q;


// Note: Algorithms 1 and 2 are examples only, so have not been implemented. Algorithms
// 3 and 4 have been "optimized away" as they had a lot of overhead and made memory
// allocations tricky. The definitions of the latter two are left here for reference.

// /// Algorithm 3 `BitsToBytes(b)` on page 20.
// /// Converts a bit array (of a length that is a multiple of eight) into an array of bytes.
// ///
// /// Input: bit array `b ∈ {0,1}^{8·ℓ}` <br>
// /// Output: byte array `B ∈ B^ℓ`

// /// Algorithm 4 `BytesToBits(B)` on page 20.
// /// Performs the inverse of `BitsToBytes`, converting a byte array into a bit array.
// ///
// /// Input: byte array `B ∈ B^ℓ` <br>
// /// Output: bit array `b ∈ {0,1}^{8·ℓ}`


/// Algorithm 5 `ByteEncode_d(F)` on page 22.
/// Encodes an array of `d`-bit integers into a byte array, for `1 ≤ d ≤ 12`.
/// This is an optimized variant (which does not use individual bit functions).
///
/// Input: integer array `F ∈ Z^{256}_m`, where `m = 2^d if d < 12` and `m = q if d = 12` <br>
/// Output: byte array `B ∈ B^{32·d}`
pub(crate) fn byte_encode(d: u32, integers_f: &[Z; 256], bytes_b: &mut [u8]) {
    debug_assert_eq!(bytes_b.len(), 32 * d as usize, "Alg 5: bytes_b len is not 32 * d");
    debug_assert!(
        integers_f.iter().all(|f| f.get_u32() <= if d < 12 { 1 << d } else { u32::from(Q) }),
        "Alg 5: integers_f out of range"
    );

    // temp acts as a bit buffer that we gradually fill and extract bytes from
    let mut temp = 0u32;
    // track how many valid bits are currently in temp
    let mut bit_index = 0;
    // track where we're writing in the output byte array
    let mut byte_index = 0;

    // Process each d-bit integer into bytes
    for coeff in integers_f {
        // Mask off any bits above d (safety measure)
        let coeff = coeff.get_u32() & ((1 << d) - 1);

        // Accumulate bits into temp buffer
        temp |= coeff << bit_index;
        bit_index += d as usize;

        // Extract complete bytes (8 bits) whenever possible
        while bit_index > 7 {
            // Extract lowest byte from temp
            bytes_b[byte_index] = temp.to_le_bytes()[0];

            // Remove the extracted byte from temp and update counters
            temp >>= 8;
            byte_index += 1;
            bit_index -= 8;
        }
    }
    debug_assert_eq!(byte_index, bytes_b.len(), "Alg 5: left over bytes_b");
}


/// Algorithm 6 `ByteDecode_d(B)` on page 22.
/// Decodes a byte array into an array of d-bit integers, for 1 ≤ d ≤ 12.
/// This is an optimized variant (which does not use individual bit functions).
///
/// Input: byte array `B ∈ B^{32·d}` <br>
/// Output: integer array `F ∈ Z^256_m`, where `m = 2^d if d < 12` and `m = q if d = 12`
pub(crate) fn byte_decode(d: u32, bytes_b: &[u8]) -> Result<[Z; 256], &'static str> {
    let mut integers_f = [Z::default(); 256];
    debug_assert_eq!(bytes_b.len(), 32 * d as usize, "Alg 6: bytes len is not 32 * d");

    // temp acts as a bit buffer that we gradually fill and extract d-bit integers from
    let mut temp = 0u32;
    // track how many valid bits are currently in temp
    let mut bit_index = 0;
    // track where we're writing in the output integer array
    let mut int_index = 0;

    // Process each byte into d-bit integers
    for byte in bytes_b {
        // Accumulate bits into temp buffer
        temp |= u32::from(*byte) << bit_index;
        bit_index += 8;

        // Extract complete d-bit integers whenever possible
        #[allow(clippy::cast_possible_truncation)]
        while bit_index >= d {
            // Mask off d bits and convert to Z type
            let mut z = Z::default();
            z.set_u16((temp & ((1 << d) - 1)) as u16);
            integers_f[int_index] = z;

            // Remove the extracted bits from temp and update counters
            bit_index -= d;
            temp >>= d;
            int_index += 1;
        }
    }

    debug_assert_eq!(int_index, integers_f.len(), "Alg 6: left over integers");

    // Verify all integers are within valid range for the given bit width
    let m = if d < 12 { 1 << d } else { u32::from(Q) };
    ensure!(integers_f.iter().all(|e| e.get_u32() < m), "Alg 6: integers out of range");
    Ok(integers_f)
}


#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec;
    use alloc::vec::Vec;

    use rand::{Rng, SeedableRng};

    use crate::byte_fns::{byte_decode, byte_encode};
    use crate::types::Z;

    // Simple round trip tests...
    #[test]
    fn test_decode_and_encode() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
        //let mut integer_array = [Z::default(); 256];
        for num_bits in 2..12_u32 {
            for _i in 0..100 {
                let num_bytes = 32 * num_bits as usize;
                let mut bytes2 = vec![0u8; num_bytes];
                let bytes1: Vec<u8> = (0..num_bytes).map(|_| rng.gen()).collect();
                let integer_array = byte_decode(num_bits, &bytes1).unwrap();
                byte_encode(num_bits, &integer_array, &mut bytes2);
                assert_eq!(bytes1, bytes2);
            }
        }
    }

    #[test]
    fn test_result_errs() {
        let mut integer_array = [Z::default(); 256];
        let num_bits = 12;
        let num_bytes = 32 * num_bits as usize;
        let bytes1: Vec<u8> = (0..num_bytes).map(|_| 0xFF).collect();
        let ret = byte_decode(num_bits, &bytes1);
        assert!(ret.is_err());
        integer_array.iter_mut().for_each(|x| x.set_u16(u16::MAX));
    }
}
