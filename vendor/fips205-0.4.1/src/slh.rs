use crate::hashers::Hashers;
use crate::types::{Adrs, SlhDsaSig, SlhPrivateKey, SlhPublicKey};
use crate::types::{Auth, ForsSig, HtSig, WotsSig, XmssSig, FORS_TREE};
use crate::{fors, helpers, hypertree, xmss};
use rand_core::CryptoRngCore;


/// Algorithm 21: `slh_keygen()` on page 37.
/// Generates an SLH-DSA key pair.
///
/// Input: (none) <br>
/// Output: SLH-DSA key pair `(SK, PK)`.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn slh_keygen_with_rng<
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    rng: &mut impl CryptoRngCore, hashers: &Hashers<K, LEN, M, N>,
) -> Result<(SlhPrivateKey<N>, SlhPublicKey<N>), &'static str> {
    //
    // 1: SK.seed ←$ B^n    ▷ Set SK.seed, SK.prf, and PK.seed to random n-byte
    let mut sk_seed = [0u8; N];
    rng.try_fill_bytes(&mut sk_seed).map_err(|_| "Alg17: rng failed1")?;

    // 2: SK.prf ←$ B^n    ▷ strings using an approved random bit generator
    let mut sk_prf = [0u8; N];
    rng.try_fill_bytes(&mut sk_prf).map_err(|_| "Alg17: rng failed2")?;

    // 3: PK.seed ←$ B^n
    let mut pk_seed = [0u8; N];
    rng.try_fill_bytes(&mut pk_seed).map_err(|_| "Alg17: rng failed3")?;

    // 4/5/6: implemented by ? operator on the above steps; not timing/order sensitive

    // 7:
    Ok(slh_keygen_internal::<D, H, HP, K, LEN, M, N>(hashers, sk_seed, sk_prf, pk_seed))
}


/// Algorithm 18: `slh_keygen_internal()` on page 34.
/// Generates an SLH-DSA key pair. Note: this function **is not** exported.
///
/// Input: Secret seed `SK.seed`, PRF key `SK.prf`, public seed `PK.seed` <br>
/// Output: SLH-DSA key pair `(SK, PK)`.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn slh_keygen_internal<
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, sk_seed: [u8; N], sk_prf: [u8; N], pk_seed: [u8; N],
) -> (SlhPrivateKey<N>, SlhPublicKey<N>) {
    let (d32, hp32) = (u32::try_from(D).unwrap(), u32::try_from(HP).unwrap());
    //
    // 1: ADRS ← toByte(0, 32)    ▷ Generate the public key for the top-level XMSS tree
    let mut adrs = Adrs::default();

    // 2: ADRS.setLayerAddress(d − 1)
    adrs.set_layer_address(d32 - 1);

    // 3: PK.root ← xmss_node(SK.seed, 0, h′, PK.seed, ADRS)
    let pk_root =
        xmss::xmss_node::<H, HP, K, LEN, M, N>(hashers, &sk_seed, 0, hp32, &pk_seed, &adrs);

    // 4: return ( (SK.seed, SK.prf, PK.seed, PK.root), (PK.seed, PK.root) )
    let pk = SlhPublicKey { pk_seed, pk_root };
    let sk = SlhPrivateKey { sk_seed, sk_prf, pk_seed, pk_root };
    (sk, pk)
}


/// Algorithm 22: `slh_sign(M, SK)` on page 39.
/// Generates a pure SLH-DSA signature. Note that the collection of M' elements is done in the
/// calling function, and this collection proceeds down into the hasher (to help avoid memory
/// allocation, buffer copies, etc).
///
/// Input: Message `M`, context string `ctx`, private key `SK`. `hedged` == hedged variant <br>
/// Output: SLH-DSA signature `SIG`.
#[allow(clippy::similar_names)]
#[allow(clippy::cast_possible_truncation)] // temporary, investigating idx_leaf int sizes
pub(crate) fn slh_sign_with_rng<
    const A: usize,
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    rng: &mut impl CryptoRngCore, hashers: &Hashers<K, LEN, M, N>, mp: &[&[u8]],
    sk: &SlhPrivateKey<N>, hedged: bool,
) -> Result<SlhDsaSig<A, D, HP, K, LEN, N>, &'static str> {
    //
    // 1: if |𝑐𝑡𝑥| > 255 then
    // 2:   return ⊥    ▷ return an error indication if the context string is too long
    // 3: end if
    // The ctx length is checked in both calling functions (where it is a bit more
    // visible and immediate): `try_sign_with_rng()` and `try_hash_sign_with_rng()`

    // 4: 𝑎𝑑𝑑𝑟𝑛𝑑 ←− 𝔹𝑛     ▷ skip lines 4 through 7 for the deterministic variant
    let mut opt_rand = sk.pk_seed;

    // 5: if 𝑎𝑑𝑑𝑟𝑛𝑑 = NULL then
    // 6:   return ⊥
    if hedged {
        //
        rng.try_fill_bytes(&mut opt_rand).map_err(|_| "Alg17: rng failed")?;

        // 7: end if
    }

    // 8: 𝑀′ ← toByte(0, 1) ∥ toByte(|𝑐𝑡𝑥|, 1) ∥ 𝑐𝑡𝑥 ∥ 𝑀
    // The collection of M' elements is done in the calling function, and this collection proceeds
    // down into the hasher as `mp` (to help avoid memory allocation, buffer copies, etc).

    // 9: SIG ← slh_sign_internal(𝑀′, SK, 𝑎𝑑𝑑𝑟𝑛𝑑)    ▷ omit 𝑎𝑑𝑑𝑟𝑛𝑑 for the deterministic variant
    slh_sign_internal::<A, D, H, HP, K, LEN, M, N>(hashers, mp, sk, opt_rand)
}


/// Algorithm 19: `slh_sign_internal(M, SK, addrnd)` on page 35.
/// Generate an SLH-DSA signature.
///
/// Input: Message `M`, private key `SK = (SK.seed, SK.prf, PK.seed, PK.root)`,
/// (optional) additional randomness 𝑎𝑑𝑑𝑟𝑛𝑑.<br>
/// Output: SLH-DSA signature `SIG`.
#[allow(clippy::similar_names)]
#[allow(clippy::cast_possible_truncation)] // temporary, investigating idx_leaf int sizes
pub(crate) fn slh_sign_internal<
    const A: usize,
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, m: &[&[u8]], sk: &SlhPrivateKey<N>, opt_rand: [u8; N],
) -> Result<SlhDsaSig<A, D, HP, K, LEN, N>, &'static str> {
    let (d32, h32) = (u32::try_from(D).unwrap(), u32::try_from(H).unwrap());
    //
    // 1: ADRS ← toByte(0, 32)
    let mut adrs = Adrs::default();

    // 2: 𝑜𝑝𝑡_𝑟𝑎𝑛𝑑 ← 𝑎𝑑𝑑𝑟𝑛𝑑    ▷ substitute 𝑜𝑝𝑡_𝑟𝑎𝑛𝑑 ← PK.seed for the deterministic variant
    // This is handled in the calling function

    // 3: R ← PRF_msg(SK.prf, opt_rand, M)    ▷ Generate hedgedr
    let r = (hashers.prf_msg)(&sk.sk_prf, &opt_rand, m);

    // 4: SIG ← R
    let mut sig = SlhDsaSig {
        randomness: r, // here!
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

    // 5: digest ← H_msg(R, PK.seed, PK.root, M)    ▷ Compute message digest
    let digest = (hashers.h_msg)(&r, &sk.pk_seed, &sk.pk_root, m);

    // 6: md ← digest[0 : ceil(k·a/8)]    ▷ first ceil(k·a/8) bytes
    let index1 = (K * A + 7) / 8;
    let md = &digest[0..index1];

    // 7: tmp_idx_tree ← digest[ceil(k·a/8) : ceil(k·a/8) + ceil((h-h/d)/8)]    ▷ next ceil((h-h/d)/8) bytes
    let index2 = index1 + (H - H / D + 7) / 8;
    let tmp_idx_tree = &digest[index1..index2];

    // 8: tmp_idx_leaf ← digest[ceil(k·a/8) + ceil((h-h/d)/8) : ceil(k·a/8) + ceil((h-h/d)/8) + ceil(h/8d)]    ▷ next ceil(h/8d) bytes
    let index3 = index2 + (H + 8 * D - 1) / (8 * D);
    let tmp_idx_leaf = &digest[index2..index3];

    // 9: idx_tree ← toInt(tmp_idx_tree, ceil((h-h/d)/8)) mod 2^{h−h/d}
    let idx_tree = helpers::to_int(tmp_idx_tree, (h32 - h32 / d32 + 7) / 8)
        & (u64::MAX >> (64 - (h32 - h32 / d32)));

    // 10: idx_leaf ← toInt(tmp_idx_leaf, ceil(h/8d) mod 2^{h/d}
    let idx_leaf = helpers::to_int(tmp_idx_leaf, (h32 + 8 * d32 - 1) / (8 * d32))
        & (u64::MAX >> (64 - h32 / d32));

    // 11: ADRS.setTreeAddress(idx_tree)
    adrs.set_tree_address(idx_tree);

    // 12: ADRS.setTypeAndClear(FORS_TREE)
    adrs.set_type_and_clear(FORS_TREE);

    // 13: ADRS.setKeyPairAddress(idxleaf)
    adrs.set_key_pair_address(idx_leaf as u32);

    // 14: SIG_FORS ← fors_sign(md, SK.seed, PK.seed, ADRS)
    // 15: SIG ← SIG ∥ SIG_FORS
    sig.fors_sig = fors::fors_sign(hashers, md, &sk.sk_seed, &adrs, &sk.pk_seed)?;

    // 16: PK_FORS ← fors_pkFromSig(SIG_FORS , md, PK.seed, ADRS)    ▷ Get FORS key
    let pk_fors =
        fors::fors_pk_from_sig::<A, K, LEN, M, N>(hashers, &sig.fors_sig, md, &sk.pk_seed, &adrs);

    // 17: SIG_HT ← ht_sign(PK_FORS , SK.seed, PK.seed, idx_tree, idx_leaf)
    // 18: SIG ← SIG ∥ SIG_HT
    sig.ht_sig = hypertree::ht_sign::<D, H, HP, K, LEN, M, N>(
        hashers,
        &pk_fors.key,
        &sk.sk_seed,
        &sk.pk_seed,
        idx_tree,
        idx_leaf as u32,
    )?;

    // 19: return SIG
    Ok(sig)
}


/// Algorithm 19: `slh_verify(M, SIG, ctx, PK)` on page 41.
/// Verifies a pure SLH-DSA signature. Note that the collection of M' elements is done in the
/// calling function, and this collection proceeds down into the hasher (to help avoid memory
/// allocation, buffer copies, etc).
///
/// Input: Message `M`, signature `SIG`, context string `ctx`, public key `PK = (PK.seed, PK.root)`. <br>
/// Output: Boolean.
#[allow(clippy::similar_names)]
pub(crate) fn slh_verify<
    const A: usize,
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, mp: &[&[u8]], sig: &SlhDsaSig<A, D, HP, K, LEN, N>,
    pk: &SlhPublicKey<N>,
) -> bool {
    // 1: if |𝑐𝑡𝑥| > 255 then
    // 2:   return false
    // 3: end if
    // The ctx length is checked in both calling functions (where it is a bit more
    // visible and immediate): `verify()` and `hash_verify()`


    // 4: 𝑀 ′ ← toByte(0, 1) ∥ toByte(|𝑐𝑡𝑥|, 1) ∥ 𝑐𝑡𝑥 ∥ 𝑀
    // The collection of M' elements is done in the calling function, and this collection proceeds
    // down into the hasher as `mp` (to help avoid memory allocation, buffer copies, etc).

    // 5: return slh_verify_internal(𝑀′, SIG, PK)
    slh_verify_internal::<A, D, H, HP, K, LEN, M, N>(hashers, mp, sig, pk)
}


/// Algorithm 20: `slh_verify(M, SIG, PK)` on page 36.
/// Verifies an SLH-DSA signature.
///
/// Input: Message `M`, signature `SIG`, public key `PK = (PK.seed, PK.root)`. <br>
/// Output: Boolean.
#[allow(clippy::similar_names)]
pub(crate) fn slh_verify_internal<
    const A: usize,
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, m: &[&[u8]], sig: &SlhDsaSig<A, D, HP, K, LEN, N>,
    pk: &SlhPublicKey<N>,
) -> bool {
    let (d32, h32) = (u32::try_from(D).unwrap(), u32::try_from(H).unwrap());

    // 1: if |SIG| != (1 + k(1 + a) + h + d · len) · n then
    // 2:   return false
    // 3: end if
    // The above size check is performed in the wrapper/adapter deserialize function

    // 4: ADRS ← toByte(0, 32)
    let mut adrs = Adrs::default();

    // 5: R ← SIG.getR()    ▷ SIG[0 : n]
    let r = &sig.randomness;

    // 6: SIG_FORS ← SIG.getSIG_FORS()    ▷ SIG[n : (1 + k(1 + a)) · n]
    let sig_fors = &sig.fors_sig;

    // 7: SIG_HT ← SIG.getSIG_HT()    ▷ SIG[(1 + k(1 + a)) · n : (1 + k(1 + a) + h + d · len) · n]
    let sig_ht = &sig.ht_sig;

    // 8: digest ← Hmsg(R, PK.seed, PK.root, M)    ▷ Compute message digest
    let digest = (hashers.h_msg)(r, &pk.pk_seed, &pk.pk_root, m);

    // 9: md ← digest[0 : ceil(k·a/8)]    ▷ first ceil(k·a/8) bytes
    let index1 = (K * A + 7) / 8;
    let md = &digest[0..index1];

    // 10: tmp_idx_tree ← digest[ceil(k·a/8) : ceil(k·a/8) + ceil((h - h/d)/8)]     ▷ next ceil((h - h/d)/8) bytes
    let index2 = index1 + (H - H / D + 7) / 8;
    let tmp_idx_tree = &digest[index1..index2];

    // 11: tmp_idx_leaf ← digest[ceil(k·a/8) + ceil((h - h/d)/8) : ceil(k·a/8) + ceil((h - h/d)/8) + ceil(h/8d)]  ▷ next ceil(h/8d) bytes
    let index3 = index2 + (H + 8 * D - 1) / (8 * D);
    let tmp_idx_leaf = &digest[index2..index3];

    // 12: idx_tree ← toInt(tmp_idx_tree, ceil((h - h/d)/8)) mod 2^{h−h/d}
    let idx_tree = helpers::to_int(tmp_idx_tree, (h32 - h32 / d32 + 7) / 8)
        & (u64::MAX >> (64 - (h32 - h32 / d32)));

    // 13: idx_leaf ← toInt(tmp_idx_leaf, ceil(h/8d) mod 2^{h/d}
    let idx_leaf = helpers::to_int(tmp_idx_leaf, (h32 + 8 * d32 - 1) / (8 * d32))
        & (u64::MAX >> (64 - h32 / d32));

    // 14: ADRS.setTreeAddress(idx_tree)    ▷ Compute FORS public key
    adrs.set_tree_address(idx_tree);

    // 15: ADRS.setTypeAndClear(FORS_TREE)
    adrs.set_type_and_clear(FORS_TREE);

    // 16: ADRS.setKeyPairAddress(idx_leaf)
    let Ok(idx_leaf_u32) = u32::try_from(idx_leaf) else { return false };  // should never fail
    adrs.set_key_pair_address(idx_leaf_u32);

    // 17: PK_FORS ← fors_pkFromSig(SIG_FORS, md, PK.seed, ADRS)
    let pk_fors =
        fors::fors_pk_from_sig::<A, K, LEN, M, N>(hashers, sig_fors, md, &pk.pk_seed, &adrs);

    // 18: return ht_verify(PK_FORS, SIG_HT, PK.seed, idx_tree , idx_leaf, PK.root)
    hypertree::ht_verify::<D, HP, K, LEN, M, N>(
        hashers,
        &pk_fors.key,
        sig_ht,
        &pk.pk_seed,
        idx_tree,
        idx_leaf_u32,
        &pk.pk_root,
    )
}
