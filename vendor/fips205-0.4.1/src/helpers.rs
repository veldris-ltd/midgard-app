use crate::types::{Adrs, Auth, ForsSig, HtSig, SlhDsaSig, WotsSig, XmssSig};


/// Algorithm 2: `toInt(X, n)` on page 15.
/// Converts a byte string to an integer.
///
/// Input: n-byte string `X`, string length `n`. <br>
/// Output: Integer value of `X`.
pub(crate) fn to_int(x: &[u8], n: u32) -> u64 {
    debug_assert_eq!(x.len(), n as usize);
    debug_assert!(n <= 8);

    // 1: total ← 0
    let mut total = 0;

    // 2: for i from 0 to n − 1 do
    for item in x.iter().take(n as usize) {
        //
        // 3: total ← 256 · total + X[i]
        total = (total << 8) + u64::from(*item);

        // 4: end for
    }

    // 5: return total
    total
}


/// Algorithm 3: `toByte(x, n)` on page 15.
/// Converts an integer to a byte string.
///
/// Input: Integer `x`, string length `n`. <br>
/// Output: Byte string of length `n` containing binary representation of `x` in big-endian byte-order.
pub(crate) fn to_byte(x: u32, n: u32) -> [u8; ((crate::LEN2 * crate::LGW + 7) / 8) as usize] {
    let mut s = [0u8; ((crate::LEN2 * crate::LGW + 7) / 8) as usize]; // Size fixed across all profiles (2)
    debug_assert_eq!(n, ((crate::LEN2 * crate::LGW + 7) / 8)); // just in case life changes
    debug_assert_eq!(n, 2); // optimize: this resolves into a two-byte (be) write!

    // 1: total ← x
    let mut total = x;

    // 2: for i from 0 to n − 1 do
    for i in 0..n {
        //
        // 3: S[n − 1 − i] ← total mod 256    ▷ Least significant 8 bits of total
        s[(n - 1 - i) as usize] = total.to_le_bytes()[0];

        // 4: total ← total ≫ 8
        total >>= 8;

        // 5: end for
    }

    // 6: return S
    s
}


/// Algorithm 4: `base_2^b(X, b, out_len)` on page 16.
/// Computes the base 2^b representation of X.
///
/// Input: Byte string `X` of length at least `ceil(out_len·b/8)`, integer `b`, output length `out_len`. <br>
/// Output: Array of `out_len` integers in the range `[0, . . . , 2^b − 1]`.
pub(crate) fn base_2b(x: &[u8], b: u32, out_len: u32, baseb: &mut [u32]) {
    debug_assert!(x.len() >= ((out_len * b + 7) / 8) as usize);
    debug_assert!(b < 16);
    debug_assert_eq!(out_len as usize, baseb.len());

    // 1: in ← 0
    let mut inn = 0;

    // 2: bits ← 0
    let mut bits = 0;

    // 3: total ← 0
    let mut total = 0;

    // 4: for out from 0 to out_len − 1 do
    for item in baseb.iter_mut() {
        //
        // 5: while bits < b do
        while bits < b {
            //
            // 6: total ← (total ≪ 8) + X[in]
            total = (total << 8) + u32::from(x[inn]);

            // 7: in ← in + 1
            inn += 1;

            // 8: bits ← bits + 8
            bits += 8;

            // 9: end while
        }

        // 10: bits ← bits − b
        bits -= b;

        // 11: baseb[out] ← (total ≫ bits) mod 2^b
        *item = (total >> bits) & (u32::MAX >> (32 - b));

        // 12: end for
    }

    // 13: return baseb  (mutable parameter)
}


impl<
        const A: usize,
        const D: usize,
        const HP: usize,
        const K: usize,
        const LEN: usize,
        const N: usize,
    > SlhDsaSig<A, D, HP, K, LEN, N>
{
    pub(crate) fn serialize<const SIG_LEN: usize>(self) -> [u8; SIG_LEN] {
        let mut out = [0u8; SIG_LEN];
        debug_assert_eq!(
            out.len(),
            N +  // randomness
                N * K + K * A * N + // ForsSig
                D * (HP * N + LEN * N)
        );
        out[0..N].copy_from_slice(&self.randomness);
        let mut start = N;
        for k in 0..K {
            out[start..(start + N)].copy_from_slice(&self.fors_sig.private_key_value[k]);
            start += N;
            for a in 0..A {
                out[start..(start + N)].copy_from_slice(&self.fors_sig.auth[k].tree[a]);
                start += N;
            }
        }
        for d in 0..D {
            for len in 0..LEN {
                out[start..(start + N)]
                    .copy_from_slice(&self.ht_sig.xmss_sigs[d].sig_wots.data[len]);
                start += N;
            }
            for hp in 0..HP {
                out[start..(start + N)].copy_from_slice(&self.ht_sig.xmss_sigs[d].auth[hp]);
                start += N;
            }
        }
        debug_assert_eq!(start, out.len());
        out
    }

    pub(crate) fn deserialize(bytes: &[u8]) -> Self {
        debug_assert_eq!(
            bytes.len(),
            N +  // randomness
                N * K + K * A * N + // ForsSig
                D * (HP * N + LEN * N)
        );
        let mut output = SlhDsaSig {
            randomness: [0u8; N],
            fors_sig: ForsSig {
                private_key_value: [[0u8; N]; K],
                auth: core::array::from_fn(|_| Auth { tree: [[0u8; N]; A] }),
            },
            ht_sig: HtSig {
                xmss_sigs: core::array::from_fn(|_| XmssSig {
                    sig_wots: WotsSig { data: [[0u8; N]; LEN] },
                    auth: [[0u8; N]; HP],
                }),
            },
        };
        output.randomness.copy_from_slice(&bytes[0..N]);
        let mut start = N;
        for k in 0..K {
            output.fors_sig.private_key_value[k].copy_from_slice(&bytes[start..(start + N)]);
            start += N;
            for a in 0..A {
                output.fors_sig.auth[k].tree[a].copy_from_slice(&bytes[start..(start + N)]);
                start += N;
            }
        }
        for d in 0..D {
            for len in 0..LEN {
                output.ht_sig.xmss_sigs[d].sig_wots.data[len]
                    .copy_from_slice(&bytes[start..(start + N)]);
                start += N;
            }
            for hp in 0..HP {
                output.ht_sig.xmss_sigs[d].auth[hp].copy_from_slice(&bytes[start..(start + N)]);
                start += N;
            }
        }
        debug_assert_eq!(start, bytes.len());
        output
    }
}


impl Adrs {
    pub(crate) fn set_layer_address(&mut self, la: u32) { self.f0 = la.to_be_bytes() }

    pub(crate) fn get_key_pair_address(&self) -> u32 { u32::from_be_bytes(self.f5) }

    pub(crate) fn set_key_pair_address(&mut self, kp_addr: u32) { self.f5 = kp_addr.to_be_bytes(); }

    pub(crate) fn set_chain_address(&mut self, i: u32) { self.f6 = i.to_be_bytes(); }

    pub(crate) fn set_type_and_clear(&mut self, type_t: u32) {
        self.f4 = type_t.to_be_bytes();
        self.f5 = 0u32.to_be_bytes();
        self.f6 = 0u32.to_be_bytes();
        self.f7 = 0u32.to_be_bytes();
    }

    pub(crate) fn set_tree_address(&mut self, t: u64) {
        let bytes = t.to_be_bytes();
        self.f2.copy_from_slice(&bytes[..4]); // = ((t >> 32) as u32).to_be_bytes();
        self.f3.copy_from_slice(&bytes[4..]); // = (t as u32).to_be_bytes();
    }

    pub(crate) fn set_hash_address(&mut self, addr: u32) { self.f7 = addr.to_be_bytes() }

    pub(crate) fn set_tree_height(&mut self, z: u32) { self.f6 = z.to_be_bytes() }

    pub(crate) fn get_tree_index(&mut self) -> u32 { u32::from_be_bytes(self.f7) }

    pub(crate) fn set_tree_index(&mut self, i: u32) { self.f7 = i.to_be_bytes() }

    #[cfg(any(
        feature = "slh_dsa_shake_128f",
        feature = "slh_dsa_shake_128s",
        feature = "slh_dsa_shake_192f",
        feature = "slh_dsa_shake_192s",
        feature = "slh_dsa_shake_256f",
        feature = "slh_dsa_shake_256s"
    ))]
    pub(crate) fn to_32_bytes(&self) -> [u8; 32] {
        let mut ret = [0u8; 32];
        let mut start = 0;
        for sl in [
            self.f0, self.f1, self.f2, self.f3, self.f4, self.f5, self.f6, self.f7,
        ] {
            ret[start..start + 4].copy_from_slice(&sl);
            start += 4;
        }
        ret
    }

    #[cfg(any(
        feature = "slh_dsa_sha2_128f",
        feature = "slh_dsa_sha2_128s",
        feature = "slh_dsa_sha2_192f",
        feature = "slh_dsa_sha2_192s",
        feature = "slh_dsa_sha2_256f",
        feature = "slh_dsa_sha2_256s"
    ))]
    pub(crate) fn to_22_bytes(&self) -> [u8; 22] {
        let mut ret = [0u8; 22];
        ret[0] = self.f0[3];
        ret[1..5].copy_from_slice(&self.f2);
        ret[5..9].copy_from_slice(&self.f3);
        ret[9] = self.f4[3];
        ret[10..14].copy_from_slice(&self.f5);
        ret[14..18].copy_from_slice(&self.f6);
        ret[18..22].copy_from_slice(&self.f7);
        ret
    }
}
