use crate::types::Adrs;
use crate::Ph;


// Holds hasher function references; constructed by each security parameter set wrapper
#[allow(clippy::type_complexity)]
pub(crate) struct Hashers<const K: usize, const LEN: usize, const M: usize, const N: usize> {
    pub(crate) h_msg: fn(&[u8], &[u8], &[u8], &[&[u8]]) -> [u8; M],
    pub(crate) prf: fn(&[u8], &[u8], &Adrs) -> [u8; N],
    pub(crate) prf_msg: fn(&[u8], &[u8], &[&[u8]]) -> [u8; N],
    pub(crate) f: fn(&[u8], &Adrs, &[u8]) -> [u8; N],
    pub(crate) h: fn(&[u8], &Adrs, &[u8], &[u8]) -> [u8; N],
    pub(crate) t_l: fn(&[u8], &Adrs, &[[u8; N]; LEN]) -> [u8; N],
    pub(crate) t_len: fn(&[u8], &Adrs, &[[u8; N]; K]) -> [u8; N],
}


#[cfg(any(
    feature = "slh_dsa_shake_128f",
    feature = "slh_dsa_shake_128s",
    feature = "slh_dsa_shake_192f",
    feature = "slh_dsa_shake_192s",
    feature = "slh_dsa_shake_256f",
    feature = "slh_dsa_shake_256s"
))]
pub(crate) mod shake {
    use crate::types::Adrs;
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    use sha3::Shake256;


    fn shake256(input: &[&[u8]], out: &mut [u8]) {
        let mut hasher = Shake256::default();
        input.iter().for_each(|item| hasher.update(item));
        let mut reader = hasher.finalize_xof();
        reader.read(out);
    }


    pub(crate) fn h_msg<const M: usize>(
        r: &[u8], pk_seed: &[u8], pk_root: &[u8], m: &[&[u8]],
    ) -> [u8; M] {
        let mut digest = [0u8; M];
        let mut inp = [r, pk_seed, pk_root, &[], &[], &[], &[], &[]];
        inp[3..3 + m.len()].copy_from_slice(m); // m can have up to 5 elements
        shake256(&inp, &mut digest);
        digest
    }


    #[allow(clippy::similar_names)] // pk_seed and sk_seed
    pub(crate) fn prf<const N: usize>(pk_seed: &[u8], sk_seed: &[u8], adrs: &Adrs) -> [u8; N] {
        let mut digest = [0u8; N];
        shake256(&[pk_seed, &adrs.to_32_bytes(), sk_seed], &mut digest); // Spec swaps order of last two params 557/997/1005
        digest
    }


    pub(crate) fn prf_msg<const N: usize>(sk_prf: &[u8], opt_rand: &[u8], m: &[&[u8]]) -> [u8; N] {
        let mut digest = [0u8; N];
        let mut inp = [sk_prf, opt_rand, &[], &[], &[], &[], &[]];
        inp[2..2 + m.len()].copy_from_slice(m); // m can have up to 5 elements
        shake256(&inp, &mut digest);
        digest
    }


    pub(crate) fn f<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        shake256(&[pk_seed, &adrs.to_32_bytes(), m1], &mut digest);
        digest
    }


    pub(crate) fn h<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8], m2: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        shake256(&[pk_seed, &adrs.to_32_bytes(), m1, m2], &mut digest);
        digest
    }


    // Perhaps there is a more elegant way to covert ml into list of bytes
    pub(crate) fn t_l<const X: usize, const Y: usize>(
        pk_seed: &[u8], adrs: &Adrs, ml: &[[u8; Y]; X],
    ) -> [u8; Y] {
        let mut hasher = Shake256::default();
        hasher.update(pk_seed);
        hasher.update(&adrs.to_32_bytes());
        ml.iter().for_each(|item| hasher.update(item));
        let mut reader = hasher.finalize_xof();
        let mut result = [0u8; Y];
        reader.read(&mut result);
        result
    }
}


#[cfg(any(feature = "slh_dsa_sha2_128f", feature = "slh_dsa_sha2_128s"))]
pub(crate) mod sha2_cat_1 {
    use crate::types::Adrs;
    use core::cmp::min;
    use sha2::{Digest, Sha256};


    fn sha2_256(input: &[&[u8]], out: &mut [u8]) {
        let mut hasher = Sha256::new();
        input.iter().for_each(|item| hasher.update(item));
        let result = hasher.finalize();
        out.copy_from_slice(&result[0..out.len()]);
    }


    pub(crate) fn h_msg<const M: usize>(
        r: &[u8], pk_seed: &[u8], pk_root: &[u8], m: &[&[u8]],
    ) -> [u8; M] {
        let mut digest1 = [0u8; 32];
        let mut inp = [r, pk_seed, pk_root, &[], &[], &[], &[], &[]];
        inp[3..3 + m.len()].copy_from_slice(m); // m can have up to 5 elements
        sha2_256(&inp, &mut digest1);
        let mut result = [0u8; M];
        let mut start = 0;
        let mut counter = 0u32;
        while start < M {
            let mut tmp = [0u8; 32];
            sha2_256(&[r, pk_seed, &digest1, &counter.to_be_bytes()], &mut tmp);
            let len = min(M - start, 32);
            result[start..start + len].copy_from_slice(&tmp[0..len]);
            start += 32;
            counter += 1;
        }
        result
    }


    #[allow(clippy::similar_names)] // pk_seed and sk_seed
    pub(crate) fn prf<const N: usize>(pk_seed: &[u8], sk_seed: &[u8], adrs: &Adrs) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 48];
        sha2_256(&[pk_seed, &zeros[0..(64 - N)], &adrs.to_22_bytes(), sk_seed], &mut digest); // Spec swaps order of last two params 557/997/1005
        digest
    }


    fn hmac_sha_256(key: &[u8], a0: &[u8], m: &[&[u8]]) -> [u8; 32] {
        let mut padding = [0x36; 64];
        for (p, &k) in padding.iter_mut().zip(key.iter()) {
            *p ^= k;
        }
        let mut inner_hasher = Sha256::new();
        inner_hasher.update(&padding[..]);
        inner_hasher.update(a0);
        m.iter().for_each(|item| inner_hasher.update(item));
        for p in &mut padding {
            *p ^= 0x6a;
        }
        let mut outer_hasher = Sha256::new();
        outer_hasher.update(&padding[..]);
        outer_hasher.update(inner_hasher.finalize());
        outer_hasher.finalize().into()
    }


    pub(crate) fn prf_msg<const N: usize>(sk_prf: &[u8], opt_rand: &[u8], m: &[&[u8]]) -> [u8; N] {
        let mut digest = [0u8; N];
        let full_digest = hmac_sha_256(sk_prf, opt_rand, m);
        digest.copy_from_slice(&full_digest[0..N]);
        digest
    }


    pub(crate) fn f<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 48];
        sha2_256(&[pk_seed, &zeros[0..(64 - N)], &adrs.to_22_bytes(), m1], &mut digest);
        digest
    }


    pub(crate) fn h<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8], m2: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 48];
        sha2_256(&[pk_seed, &zeros[0..(64 - N)], &adrs.to_22_bytes(), m1, m2], &mut digest);
        digest
    }


    pub(crate) fn t_l<const LEN: usize, const N: usize>(
        pk_seed: &[u8], adrs: &Adrs, ml: &[[u8; N]; LEN],
    ) -> [u8; N] {
        let mut result = [0u8; N];
        let zeros = [0u8; 48];
        let mut hasher = Sha256::new();
        hasher.update(pk_seed);
        hasher.update(&zeros[0..(64 - N)]);
        hasher.update(adrs.to_22_bytes());
        ml.iter().for_each(|item| hasher.update(item));
        let digest = hasher.finalize();
        result.copy_from_slice(&digest[0..N]);
        result
    }
}


#[cfg(any(
    feature = "slh_dsa_sha2_192f",
    feature = "slh_dsa_sha2_192s",
    feature = "slh_dsa_sha2_256f",
    feature = "slh_dsa_sha2_256s"
))]
pub(crate) mod sha2_cat_3_5 {
    use crate::types::Adrs;
    use core::cmp::min;
    use sha2::{Digest, Sha256, Sha512};


    fn sha2_256(input: &[&[u8]], out: &mut [u8]) {
        let mut hasher = Sha256::new();
        input.iter().for_each(|item| hasher.update(item));
        let result = hasher.finalize();
        out.copy_from_slice(&result[0..out.len()]);
    }


    fn sha2_512(input: &[&[u8]], out: &mut [u8]) {
        let mut hasher = Sha512::new();
        input.iter().for_each(|item| hasher.update(item));
        let result = hasher.finalize();
        out.copy_from_slice(&result[0..out.len()]);
    }


    pub(crate) fn h_msg<const M: usize>(
        r: &[u8], pk_seed: &[u8], pk_root: &[u8], m: &[&[u8]],
    ) -> [u8; M] {
        let mut digest1 = [0u8; 64];
        let mut inp = [r, pk_seed, pk_root, &[], &[], &[], &[], &[]];
        inp[3..3 + m.len()].copy_from_slice(m); // m can have up to 5 elements
        sha2_512(&inp, &mut digest1);
        let mut result = [0u8; M];
        let mut start = 0;
        let mut counter = 0u32;
        while start < M {
            let mut tmp = [0u8; 64];
            sha2_512(&[r, pk_seed, &digest1, &counter.to_be_bytes()], &mut tmp);
            let len = min(M - start, 64);
            result[start..start + len].copy_from_slice(&tmp[0..len]);
            start += 64;
            counter += 1;
        }
        result
    }


    #[allow(clippy::similar_names)] // pk_seed and sk_seed
    pub(crate) fn prf<const N: usize>(pk_seed: &[u8], sk_seed: &[u8], adrs: &Adrs) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 40];
        sha2_256(&[pk_seed, &zeros[0..(64 - N)], &adrs.to_22_bytes(), sk_seed], &mut digest); // Spec swaps order of last two params 557/997/1005
        digest
    }


    fn hmac_sha_512(key: &[u8], a0: &[u8], m: &[&[u8]]) -> [u8; 64] {
        let mut padding = [0x36; 128];
        for (p, &k) in padding.iter_mut().zip(key.iter()) {
            *p ^= k;
        }
        let mut inner_hasher = Sha512::new();
        inner_hasher.update(&padding[..]);
        inner_hasher.update(a0);
        m.iter().for_each(|item| inner_hasher.update(item));
        for p in &mut padding {
            *p ^= 0x6a;
        }
        let mut outer_hasher = Sha512::new();
        outer_hasher.update(&padding[..]);
        outer_hasher.update(inner_hasher.finalize());
        outer_hasher.finalize().into()
    }


    pub(crate) fn prf_msg<const N: usize>(sk_prf: &[u8], opt_rand: &[u8], m: &[&[u8]]) -> [u8; N] {
        let mut digest = [0u8; N];
        let full_digest = hmac_sha_512(sk_prf, opt_rand, m);
        digest.copy_from_slice(&full_digest[0..N]);
        digest
    }


    pub(crate) fn f<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 40];
        sha2_256(&[pk_seed, &zeros[0..(64 - N)], &adrs.to_22_bytes(), m1], &mut digest);
        digest
    }


    pub(crate) fn h<const N: usize>(pk_seed: &[u8], adrs: &Adrs, m1: &[u8], m2: &[u8]) -> [u8; N] {
        let mut digest = [0u8; N];
        let zeros = [0u8; 104];
        sha2_512(&[pk_seed, &zeros[0..(128 - N)], &adrs.to_22_bytes(), m1, m2], &mut digest);
        digest
    }


    pub(crate) fn t_l<const LEN: usize, const N: usize>(
        pk_seed: &[u8], adrs: &Adrs, ml: &[[u8; N]; LEN],
    ) -> [u8; N] {
        let mut result = [0u8; N];
        let zeros = [0u8; 104];
        let mut hasher = Sha512::new();
        hasher.update(pk_seed);
        hasher.update(&zeros[0..(128 - N)]);
        hasher.update(adrs.to_22_bytes());
        ml.iter().for_each(|item| hasher.update(item));
        let digest = hasher.finalize();
        result.copy_from_slice(&digest[0..N]);
        result
    }
}

pub(crate) fn hash_message(message: &[u8], ph: &Ph, phm: &mut [u8; 64]) -> ([u8; 11], usize) {
    use sha2::{Digest, Sha256, Sha512};
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    use sha3::{Shake128, Shake256};

    match ph {
        Ph::SHA256 => (
            [
                0x06u8, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
            ],
            {
                let mut hasher = Sha256::new();
                Digest::update(&mut hasher, message);
                phm[0..32].copy_from_slice(&hasher.finalize());
                32
            },
        ),
        Ph::SHA512 => (
            [
                0x06u8, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03,
            ],
            {
                let mut hasher = Sha512::new();
                Digest::update(&mut hasher, message);
                phm.copy_from_slice(&hasher.finalize());
                64
            },
        ),
        Ph::SHAKE128 => (
            [
                0x06u8, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x0B,
            ],
            {
                let mut hasher = Shake128::default();
                hasher.update(message);
                let mut reader = hasher.finalize_xof();
                reader.read(&mut phm[0..32]);
                32
            },
        ),
        Ph::SHAKE256 => (
            [
                0x06u8, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x0C,
            ],
            {
                let mut hasher = Shake256::default();
                hasher.update(message);
                let mut reader = hasher.finalize_xof();
                reader.read(phm);
                64
            },
        ),
    }
}
