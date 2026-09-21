use crate::hashers::Hashers;
use crate::helpers::base_2b;
use crate::types::{Adrs, Auth, ForsPk, ForsSig, FORS_PRF, FORS_ROOTS};


/// Algorithm 14: `fors_SKgen(SK.seed, PK.seed, ADRS, idx)` on page 29.
/// Generates a FORS private-key value.
///
/// Input: Secret seed `SK.seed`, public seed `PK.seed`, address `ADRS`, secret key index `idx`. <br>
/// Output: n-byte FORS private-key value.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn fors_sk_gen<const K: usize, const LEN: usize, const M: usize, const N: usize>(
    hashers: &Hashers<K, LEN, M, N>, sk_seed: &[u8], pk_seed: &[u8], adrs: &Adrs, idx: u32,
) -> [u8; N] {
    // 1: skADRS ← ADRS    ▷ Copy address to create key generation address
    let mut sk_adrs = adrs.clone();

    // 2: skADRS.setTypeAndClear(FORS_PRF)
    sk_adrs.set_type_and_clear(FORS_PRF);

    // 3: skADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    sk_adrs.set_key_pair_address(adrs.get_key_pair_address());

    // 4: skADRS.setTreeIndex(idx)
    sk_adrs.set_tree_index(idx);

    // 5: return PRF(PK.seed, SK.seed, skADRS)
    (hashers.prf)(pk_seed, sk_seed, &sk_adrs)
}


/// Algorithm 15: `fors_node(SK.seed, i, z, PK.seed, ADRS)` on page 30.
/// Computes the root of a Merkle subtree of FORS public values.
///
/// Input: Secret seed `SK.seed`, target node index `i`, target node height `z`, public seed `PK.seed`,
/// address `ADRS`. <br>
/// Output: n-byte root node.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn fors_node<
    const A: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, sk_seed: &[u8], i: u32, z: u32, pk_seed: &[u8], adrs: &Adrs,
) -> Result<[u8; N], &'static str> {
    let mut adrs = adrs.clone();

    // Note this bounds check was only specified in the draft specification
    // let (a32, k32) = (u32::try_from(A).unwrap(), u32::try_from(K).unwrap());
    // debug_assert!((z > a32) | (i > k32 * (1 << (a32 - z))), "Alg15 fails");

    // 1: if z = 0 then
    let node = if z == 0 {
        //
        // 2: sk ← fors_SKgen(SK.seed, PK.seed, ADRS, i)
        let sk = fors_sk_gen(hashers, sk_seed, pk_seed, &adrs, i);

        // 3: ADRS.setTreeHeight(0)
        adrs.set_tree_height(0);

        // 4: ADRS.setTreeIndex(i)
        adrs.set_tree_index(i);

        // 5: node ← F(PK.seed, ADRS, sk)
        (hashers.f)(pk_seed, &adrs, &sk)

        // 6: else
    } else {
        //
        // 7: lnode ← fors_node(SK.seed, 2i, z − 1, PK.seed, ADRS)
        let lnode = fors_node::<A, K, LEN, M, N>(hashers, sk_seed, 2 * i, z - 1, pk_seed, &adrs)?;

        // 8: rnode ← fors_node(SK.seed, 2i + 1, z − 1, PK.seed, ADRS)
        let rnode =
            fors_node::<A, K, LEN, M, N>(hashers, sk_seed, 2 * i + 1, z - 1, pk_seed, &adrs)?;

        // 9: ADRS.setTreeHeight(z)
        adrs.set_tree_height(z);

        // 10: ADRS.setTreeIndex(i)
        adrs.set_tree_index(i);

        // 11: node ← H(PK.seed, ADRS, lnode ∥ rnode)
        (hashers.h)(pk_seed, &adrs, &lnode, &rnode)

        // 12: end if
    };

    // 13: return node
    Ok(node)
}


/// Algorithm 16: `fors_sign(md, SK.seed, PK.seed, ADRS)` on page 31.
/// Generates a FORS signature.
///
/// Input: Message digest `md`, secret seed `SK.seed`, address `ADRS`, public seed `PK.seed`. <br>
/// Output: FORS signature `SIG_FORS`.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn fors_sign<
    const A: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, md: &[u8], sk_seed: &[u8], adrs: &Adrs, pk_seed: &[u8],
) -> Result<ForsSig<A, K, N>, &'static str> {
    let (a32, k32) = (u32::try_from(A).unwrap(), u32::try_from(K).unwrap());

    // 1: SIG_FORS = NULL    ▷ Initialize SIG_FORS as a zero-length byte string
    // Here, we manually initialize all zeros then overwrite its contents below
    let mut sig_fors = ForsSig {
        private_key_value: [[0u8; N]; K],
        auth: core::array::from_fn(|_| Auth { tree: [[0u8; N]; A] }),
    };

    // 2: indices ← base_2b(md, a, k)
    let mut indices = [0u32; K];
    base_2b(md, a32, k32, &mut indices);

    // 3: for i from 0 to k − 1 do    ▷ Compute signature elements
    for i in 0..k32 {
        //
        // 4: SIG_FORS ← SIG_FORS ∥ fors_SKgen(SK.seed, PK.seed, ADRS, i · 2^a + indices[i])
        sig_fors.private_key_value[i as usize] = fors_sk_gen::<K, LEN, M, N>(
            hashers,
            sk_seed,
            pk_seed,
            adrs,
            (i << a32) + indices[i as usize],
        );

        // 5: for j from 0 to a − 1 do    ▷ Compute auth path
        for j in 0..a32 {
            //
            // 6: s ← indices[i]/2^j xor 1
            let s = (indices[i as usize] >> j) ^ 1;

            // 7: AUTH[j] ← fors_node(SK.seed, i · 2^{a−j} + s, j, PK.seed, ADRS)
            sig_fors.auth[i as usize].tree[j as usize] = fors_node::<A, K, LEN, M, N>(
                hashers,
                sk_seed,
                (i << (a32 - j)) + s,
                j,
                pk_seed,
                adrs,
            )?;

            // 8: end for
        }

        // 9: SIG_FORS ← SIG_FORS ∥ AUTH
        // built within inner loop above (step 7)

        // 10: end for
    }

    // 11: return SIG_FORS
    Ok(sig_fors)
}


/// Algorithm 17: `fors_pkFromSig(SIG_FORS, md, PK.seed, ADRS)` on page 32.
/// Computes a FORS public key from a FORS signature.
///
/// Input: FORS signature `SIG_FORS`, message digest `md`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: FORS public key.
#[allow(clippy::similar_names)]
pub(crate) fn fors_pk_from_sig<
    const A: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, sig_fors: &ForsSig<A, K, N>, md: &[u8], pk_seed: &[u8],
    adrs: &Adrs,
) -> ForsPk<N> {
    let (a32, k32) = (u32::try_from(A).unwrap(), u32::try_from(K).unwrap());
    let mut adrs = adrs.clone();

    // 1: indices ← base_2b(md, a, k)
    let mut indices = [0u32; K];
    base_2b(md, a32, k32, &mut indices);

    // 2: for i from 0 to k − 1 do
    let mut root = [[0u8; N]; K];
    for i in 0..k32 {
        //
        // 3: sk ← SIG_FORS.getSK(i)    ▷ SIG_FORS [i · (a + 1) · n : (i · (a + 1) + 1) · n]
        let sk = sig_fors.private_key_value[i as usize];

        // 4: ADRS.setTreeHeight(0)    ▷ Compute leaf
        adrs.set_tree_height(0);

        // 5: ADRS.setTreeIndex(i · 2^a + indices[i])
        adrs.set_tree_index((i << a32) + indices[i as usize]);

        // 6: node[0] ← F(PK.seed, ADRS, sk)
        let mut node_0 = (hashers.f)(pk_seed, &adrs, &sk);

        // 7: auth ← SIGFORS.getAUTH(i)    ▷ SIGFORS [(i · (a + 1) + 1) · n : (i + 1) · (a + 1) · n]
        let auth = sig_fors.auth[i as usize].clone();

        // 8: for j from 0 to a − 1 do    ▷ Compute root from leaf and AUTH
        for j in 0..a32 {
            //
            // 9: ADRS.setTreeHeight(j + 1)
            adrs.set_tree_height(j + 1);

            // 10: if indices[i]/2^j is even then
            let node_1 = if ((indices[i as usize] >> j) & 0x01) == 0 {
                //
                // 11: ADRS.setTreeIndex(ADRS.getTreeIndex()/2)
                let tmp = adrs.get_tree_index() / 2;
                adrs.set_tree_index(tmp);

                // 12: node[1] ← H(PK.seed, ADRS, node[0] ∥ auth[j])
                (hashers.h)(pk_seed, &adrs, &node_0, &auth.tree[j as usize])

                // 13: else
            } else {
                //
                // 14: ADRS.setTreeIndex((ADRS.getTreeIndex() − 1)/2)
                let tmp = (adrs.get_tree_index() - 1) / 2;
                adrs.set_tree_index(tmp);

                // 15: node[1] ← H(PK.seed, ADRS, auth[j] ∥ node[0])
                (hashers.h)(pk_seed, &adrs, &auth.tree[j as usize], &node_0)

                // 16: end if
            };

            // 17: node[0] ← node[1]
            node_0 = node_1;

            // 18: end for
        }

        // 19: root[i] ← node[0]
        root[i as usize] = node_0;

        // 20: end for
    }

    // 21: forspkADRS ← ADRS    ▷ Compute the FORS public key from the Merkle tree roots
    let mut fors_pk_adrs = adrs.clone();

    // 22: forspkADRS.setTypeAndClear(FORS_ROOTS)
    fors_pk_adrs.set_type_and_clear(FORS_ROOTS);

    // 23: forspkADRS.setKeyPairAddress(ADRS.getKeyPairAddress())
    fors_pk_adrs.set_key_pair_address(adrs.get_key_pair_address());

    // 24: pk ← Tk(PK.seed, forspkADRS, root)   ▷ compute the FORS public key
    let pk = (hashers.t_len)(pk_seed, &fors_pk_adrs, &root);

    // 25: return pk;
    ForsPk { key: pk }
}
