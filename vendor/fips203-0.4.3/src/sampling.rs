use crate::types::Z;
use crate::Q;
use sha3::digest::XofReader;


/// Algorithm 7 `SampleNTT(B)` on page 23.
/// Takes a 32-byte seed and two indices as input and outputs a pseudorandom element of `𝑇_𝑞`.
/// This implementation takes the `XofReader` directly.
///
/// Input: byte stream `B ∈ B^{34}`     ▷ a 32-byte seed along with two indices <br>
/// Output: array `a_hat ∈ Z^{256}_q`    ▷ the coefficients of the NTT of a polynomial
pub(crate) fn sample_ntt(mut xof_reader: impl XofReader) -> [Z; 256] {
    //
    let mut array_a_hat = [Z::default(); 256];
    let mut c = [0u8; 3]; // Space for 3 random (byte) draws

    // Not needed as XofReader is passed into function.
    // 1: ctx ← XOF.Init()
    // 2: ctx ← XOF.Absorb(ctx, 𝐵)    ▷ input the given byte array into XOF

    // 3: j ← 0
    let mut j = 0usize;

    // This rejection sampling loop is solely dependent upon rho which crosses a trust boundary
    // in the clear. Thus, it does not need to be constant time.
    // 4: while j < 256 do
    while j < 256 {
        //
        // 5: (ctx, 𝐶) ← XOF.Squeeze(ctx, 3)    ▷ get a fresh 3-byte array 𝐶 from XOF
        xof_reader.read(&mut c); // Draw 3 bytes

        // 6: 𝑑1 ← 𝐶[0] + 256 ⋅ (𝐶[1] mod 16)    ▷ 0 ≤ 𝑑1 < 2^{12}
        let d1 = u16::from(c[0]) + 256 * (u16::from(c[1]) & 0x0F);

        // 7: 𝑑2 ← ⌊𝐶[1]/16⌋ + 16 ⋅ 𝐶[2]    ▷ 0 ≤ 𝑑2 < 2^{12}
        let d2 = (u16::from(c[1]) >> 4) + 16 * u16::from(c[2]);

        // 8: if d1 < q then
        if d1 < Q {
            //
            // 9: a_hat[j] ← d1         ▷ a_hat ∈ Z256
            array_a_hat[j].set_u16(d1);

            // 10: j ← j + 1
            j += 1;

            // 11: end if
        }

        // 12: if d2 < q and j < 256 then
        if (d2 < Q) & (j < 256) {
            //
            // 13: a_hat[j] ← d2
            array_a_hat[j].set_u16(d2);

            // 14: j ← j + 1
            j += 1;

            // 15: end if
        }

        // 16: end while
    }

    // 17: return a_hat
    array_a_hat
}


/// Algorithm 8 `SamplePolyCBD_η(B)` on page 23.
/// Takes a seed as input and outputs a pseudorandom sample from the distribution `D_𝜂(𝑅_𝑞)`. <br>
/// This function is an optimized version that avoids the `BytesToBits` function (algorithm 4).
///
/// Input: byte array `B ∈ B^{64·η}` <br>
/// Output: array `f ∈ Z^{256}_q`
#[must_use]
pub(crate) fn sample_poly_cbd(byte_array_b: &[u8]) -> [Z; 256] {
    let eta = u32::try_from(byte_array_b.len()).unwrap() >> 6;
    debug_assert_eq!(byte_array_b.len(), 64 * eta as usize, "Alg 8: byte array not 64 * eta");
    let mut array_f: [Z; 256] = [Z::default(); 256];
    let mut temp = 0;
    let mut int_index = 0;
    let mut bit_index = 0;
    for byte in byte_array_b {
        temp |= u32::from(*byte) << bit_index;
        bit_index += 8;
        while bit_index >= 2 * (eta as usize) {
            let tmask_x = temp & ((1 << eta) - 1);
            let x = count_ones(tmask_x);
            let tmask_y = (temp >> eta) & ((1 << eta) - 1);
            let y = count_ones(tmask_y);
            let (mut xx, mut yy) = (Z::default(), Z::default());
            xx.set_u16(x);
            yy.set_u16(y);
            array_f[int_index] = xx.sub(yy);
            bit_index -= 2 * (eta as usize);
            temp >>= 2 * (eta as usize);
            int_index += 1;
        }
    }
    array_f
}


// Count u8 ones in constant time (u32 helps perf)
#[allow(clippy::cast_possible_truncation)] // return x as u16
fn count_ones(x: u32) -> u16 {
    let x = (x & 0x5555_5555) + ((x >> 1) & 0x5555_5555);
    let x = (x & 0x3333_3333) + ((x >> 2) & 0x3333_3333);
    let x = (x & 0x0F0F_0F0F) + ((x >> 4) & 0x0F0F_0F0F);
    x as u16
}


// The original pseudocode for Algorithm 8 follows...
// Algorithm 8 `SamplePolyCBDη(B)` on page 23.
// Takes a seed as input and outputs a pseudorandom sample from the distribution `D_𝜂(𝑅_𝑞)`.
//
// Input: byte array B ∈ B^{64·η}
// Output: array f ∈ Z^{256}_q
// 1: b ← BytesToBits(B)
// 2: for (i ← 0; i < 256; i ++)
// 3:   x ← ∑_{j=0}^{η-1} b[2iη + j] //
// 4:   y ← ∑_{j=0}^{η-1} b[2iη + η + j]
// 5:   f [i] ← x − y mod q
// 6: end for
// 7: return f
// }
