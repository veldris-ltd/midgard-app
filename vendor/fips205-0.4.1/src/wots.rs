use crate::hashers::Hashers;
use crate::helpers::{base_2b, to_byte};
use crate::types::{Adrs, WotsPk, WotsSig, WOTS_PK, WOTS_PRF};


/// Algorithm 5: `chain(X, i, s, PK.seed, ADRS)` on page 18.
/// Chaining function used in WOTS+. The chain function takes as input an n-byte string `X` and integers `s` and `i`
/// and returns the result of iterating the hash function `F` on the input `s` times, starting from an index of `i`.
/// The chain function also requires as input PK.seed, which is part of the SLH-DSA public key, and an address `ADRS`.
/// The type in `ADRS` must be set to `WOTS_HASH`, and the layer address, tree address, key pair address, and chain
/// address must be set to the address of the chain being computed. The chain function updates the hash address in
/// `ADRS` with each iteration to specify the current position in the chain prior to ADRS’s use in `F`.
///
/// Input: Input string `X`, start index `i`, number of steps `s`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: Value of `F` iterated `s` times on `X`.
pub(crate) fn chain<const K: usize, const LEN: usize, const M: usize, const N: usize>(
    hashers: &Hashers<K, LEN, M, N>, cap_x: [u8; N], i: u32, s: u32, pk_seed: &[u8], adrs: &Adrs,
) -> [u8; N] {
    debug_assert!(i + s < u32::MAX);
    let mut adrs = adrs.clone();

    // Note this bounds check was only specified in the draft specification
    // (old)1: if (i + s) ≥ w then return NULL;
    // if (i + s) >= crate::W { return None; }

    // 1: tmp ← X
    let mut tmp = cap_x;

    // 2: for j from i to i + s − 1 do
    for j in i..(i + s) {
        //
        // 3: ADRS.setHashAddress(j)
        adrs.set_hash_address(j);

        // 4: tmp ← F(PK.seed, ADRS, tmp)
        tmp = (hashers.f)(pk_seed, &adrs, &tmp);

        // 5: end for
    }

    // 6: return tmp
    tmp
}


/// Algorithm 6: `wots_PKgen(SK.seed, PK.seed, ADRS)` on page 18.
/// Generates a WOTS+ public key. The `wots_PKgen` function generates WOTS+ public keys. It takes as input `SK.seed`
/// and `PK.seed` from the SLH-DSA private key and an address. The type in the address `ADRS` must be set to
/// `WOTS_HASH`, and the layer address, tree address, and key pair address must encode the address of the `WOTS+`
/// public key to be generated.
///
/// Input: Secret seed `SK.seed`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: WOTS+ public key `pk`.
#[allow(clippy::similar_names)] // pk_seed and sk_seed
pub(crate) fn wots_pkgen<const K: usize, const LEN: usize, const M: usize, const N: usize>(
    hashers: &Hashers<K, LEN, M, N>, sk_seed: &[u8], pk_seed: &[u8], adrs: &Adrs,
) -> WotsPk<N> {
    let len32 = u32::try_from(LEN).unwrap();
    let mut adrs = adrs.clone();
    let mut tmp = [[0u8; N]; LEN];

    // 1: skADRS ← ADRS    ▷ Copy address to create key generation key address
    let mut sk_adrs = adrs.clone();

    // 2: skADRS.setTypeAndClear(WOTS_PRF)
    sk_adrs.set_type_and_clear(WOTS_PRF);

    // 3: skADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    sk_adrs.set_key_pair_address(adrs.get_key_pair_address());

    // 4: for i from 0 to len − 1 do
    for i in 0..len32 {
        //
        // 5: skADRS.setChainAddress(i)
        sk_adrs.set_chain_address(i);

        // 6: sk ← PRF(PK.seed, SK.seed, skADRS)    ▷ Compute secret value for chain i
        let sk = (hashers.prf)(pk_seed, sk_seed, &sk_adrs);

        // 7: ADRS.setChainAddress(i)
        adrs.set_chain_address(i);

        // 8: tmp[i] ← chain(sk, 0, w − 1, PK.seed, ADRS)    ▷ Compute public value for chain i
        tmp[i as usize] = chain(hashers, sk, 0, crate::W - 1, pk_seed, &adrs);

        // 9: end for
    }

    // 10: wotspkADRS ← ADRS    ▷ Copy address to create WOTS+ public key address
    let mut wotspk_adrs = adrs.clone();

    // 11: wotspkADRS.setTypeAndClear(WOTS_PK)
    wotspk_adrs.set_type_and_clear(WOTS_PK);

    // 12: wotspkADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    wotspk_adrs.set_key_pair_address(adrs.get_key_pair_address());

    // 13: pk ← Tlen (PK.seed, wotspkADRS, tmp)    ▷ Compress public key
    let pk = (hashers.t_l)(pk_seed, &wotspk_adrs, &tmp);

    // 14: return pk
    WotsPk(pk)
}


/// Algorithm 7: `wots_sign(M, SK.seed, PK.seed, ADRS)` on page 20.
/// Generates a WOTS+ signature on an n-byte message.
///
/// Input: Message `M`, secret seed `SK.seed`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: WOTS+ signature sig.
#[allow(clippy::similar_names)] // pk_seed and sk_seed
pub(crate) fn wots_sign<const K: usize, const LEN: usize, const M: usize, const N: usize>(
    hashers: &Hashers<K, LEN, M, N>, m: &[u8], sk_seed: &[u8], pk_seed: &[u8], adrs: &Adrs,
) -> WotsSig<LEN, N> {
    let n32 = u32::try_from(N).unwrap();
    let mut adrs = adrs.clone();
    let mut sig: WotsSig<LEN, N> = WotsSig { data: [[0u8; N]; LEN] };

    // 1: csum ← 0
    let mut csum = 0_u32;

    // 2: msg ← base_2b(M, lgw, len1)    ▷ Convert message to base w
    let mut msg = [0u32; LEN]; // note: 3 entries left over, used step 10
    base_2b(m, crate::LGW, 2 * n32, &mut msg[0..(2 * N)]);

    // 3: for i from 0 to len1 − 1 do    ▷ Compute checksum
    for item in msg.iter().take(2 * N) {
        //
        // 4: csum ← csum + w − 1 − msg[i]
        csum += crate::W - 1 - *item;

        // 5: end for
    }

    // 6: csum ← csum ≪ ((8 − ((len2·lgw) mod 8)) mod 8)    ▷ For lgw = 4 left shift by 4
    csum <<= (8 - ((crate::LEN2 * crate::LGW) & 0x07)) & 0x07;

    // 7: msg ← msg ∥ base_2b(toByte(csum, ceil(len2·lgw/8)), lgw, len2)    ▷ Convert csum to base w
    base_2b(
        &to_byte(csum, (crate::LEN2 * crate::LGW + 7) / 8),
        crate::LGW,
        crate::LEN2,
        &mut msg[(2 * N)..],
    );

    // 8: skADRS ← ADRS
    let mut sk_addrs = adrs.clone();

    // 9: skADRS.setTypeAndClear(WOTS_PRF)
    sk_addrs.set_type_and_clear(WOTS_PRF);

    // 10: skADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    sk_addrs.set_key_pair_address(adrs.get_key_pair_address());

    // 11: for i from 0 to len − 1 do
    for (item, i) in msg.iter().zip(0u32..) {
        //
        // 12: skADRS.setChainAddress(i)
        sk_addrs.set_chain_address(i);

        // 13: sk ← PRF(PK.seed, SK.seed, skADRS)    ▷ Compute secret value for chain i
        let sk = (hashers.prf)(pk_seed, sk_seed, &sk_addrs);

        // 14: ADRS.setChainAddress(i)
        adrs.set_chain_address(i);

        // 15: sig[i] ← chain(sk, 0, msg[i], PK.seed, ADRS)    ▷ Compute signature value for chain i
        sig.data[i as usize] = chain(hashers, sk, 0, *item, pk_seed, &adrs);

        // 16: end for
    }

    // 17: return sig
    sig
}


/// Algorithm 8: `wots_PKFromSig(sig, M, PK.seed, ADRS)` on page 21.
/// Computes a WOTS+ public key from a message and its signature.
///
/// Input: WOTS+ signature `sig`, message `M`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: WOTS+ public key `pksig` derived from `sig`.
pub(crate) fn wots_pk_from_sig<const K: usize, const LEN: usize, const M: usize, const N: usize>(
    hashers: &Hashers<K, LEN, M, N>, sig: &WotsSig<LEN, N>, m: &[u8], pk_seed: &[u8], adrs: &Adrs,
) -> WotsPk<N> {
    let n32 = u32::try_from(N).unwrap();
    let mut adrs = adrs.clone();
    let mut tmp = [[0u8; N]; LEN];

    // 1: csum ← 0
    let mut csum = 0_u32;

    // 2: msg ← base_2b (M, lgw , len1 )    ▷ Convert message to base w
    let mut msg = [0u32; LEN];
    base_2b(m, crate::LGW, 2 * n32, &mut msg[0..(2 * N)]);

    // 3: for i from 0 to len1 − 1 do    ▷ Compute checksum
    for item in msg.iter().take(2 * N) {
        //
        // 4:   csum ← csum + w − 1 − msg[i]
        csum += crate::W - 1 - item;

        // 5: end for
    }

    // 6: csum ← csum ≪ ((8 − ((len2·lgw) mod 8)) mod 8)    ▷ For lgw = 4 left shift by 4
    csum <<= (8 - ((crate::LEN2 * crate::LGW) & 0x07)) & 0x07;

    // 7: msg ← msg ∥ base_2b(toByte(csum, ceil(len2·lgw/8)), lgw, len2)    ▷ Convert csum to base w
    base_2b(
        &to_byte(csum, (crate::LEN2 * crate::LGW + 7) / 8),
        crate::LGW,
        crate::LEN2,
        &mut msg[(2 * N)..],
    );

    // 8: for i from 0 to len − 1 do
    #[allow(clippy::cast_possible_truncation)] // steps 9 and 10
    for i in 0..LEN {
        //
        // 9: ADRS.setChainAddress(i)
        adrs.set_chain_address(i as u32);

        // 10: tmp[i] ← chain(sig[i], msg[i], w − 1 − msg[i], PK.seed, ADRS)
        tmp[i] = chain::<K, LEN, M, N>(
            hashers,
            sig.data[i],
            msg[i],
            crate::W - 1 - msg[i],
            pk_seed,
            &adrs,
        );

        // 11: end for
    }

    // 12: wotspkADRS ← ADRS
    let mut wotspk_adrs = adrs.clone();

    // 13: wotspkADRS.setTypeAndClear(WOTS_PK)
    wotspk_adrs.set_type_and_clear(WOTS_PK);

    // 14: wotspkADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    wotspk_adrs.set_key_pair_address(adrs.get_key_pair_address());

    // 15: pksig ← Tlen (PK.seed, wotspkADRS, tmp)
    let pk = (hashers.t_l)(pk_seed, &wotspk_adrs, &tmp);

    // 16: return pksig
    WotsPk(pk)
}
