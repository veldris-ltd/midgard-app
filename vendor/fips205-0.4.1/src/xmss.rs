use crate::hashers::Hashers;
use crate::types::{Adrs, WotsSig, XmssSig, TREE, WOTS_HASH};
use crate::wots;


/// Algorithm 9: `xmss_node(SK.seed, i, z, PK.seed, ADRS)` on page 22.
/// Computes the root of a Merkle subtree of WOTS+ public keys.
///
/// Input: Secret seed `SK.seed`, target node index `i`, target node height `z`, public seed `PK.seed`,
/// `address ADRS`. <br>
/// Output: n-byte root `node`.
#[allow(clippy::similar_names, clippy::let_and_return)] // sk_seed and pk_seed, clarity
pub(crate) fn xmss_node<
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, sk_seed: &[u8], i: u32, z: u32, pk_seed: &[u8], adrs: &Adrs,
) -> [u8; N] {
    let mut adrs = adrs.clone();

    // Note this bounds check was only specified in the draft specification
    // (old)1: if z > h′ or i ≥ 2^{h −z} then
    // if (z > hp32) | (u64::from(i) >= (1 << (hp32 - z))) { return Err("Alg8: fail"); }

    // 1: if z = 0 then
    let node = if z == 0 {
        //
        // 2: ADRS.setTypeAndClear(WOTS_HASH)
        adrs.set_type_and_clear(WOTS_HASH);

        // 3: ADRS.setKeyPairAddress(i)
        adrs.set_key_pair_address(i);

        // 4: node ← wots_PKgen(SK.seed, PK.seed, ADRS)
        wots::wots_pkgen::<K, LEN, M, N>(hashers, sk_seed, pk_seed, &adrs).0

        // 5: else
    } else {
        //
        // 6: lnode ← xmss_node(SK.seed, 2 * i, z − 1, PK.seed, ADRS)
        let lnode =
            xmss_node::<H, HP, K, LEN, M, N>(hashers, sk_seed, 2 * i, z - 1, pk_seed, &adrs);

        // 7: rnode ← xmss_node(SK.seed, 2 * i + 1, z − 1, PK.seed, ADRS)
        let rnode =
            xmss_node::<H, HP, K, LEN, M, N>(hashers, sk_seed, 2 * i + 1, z - 1, pk_seed, &adrs);

        // 8: ADRS.setTypeAndClear(TREE)
        adrs.set_type_and_clear(TREE);

        // 9: ADRS.setTreeHeight(z)
        adrs.set_tree_height(z);

        // 10: ADRS.setTreeIndex(i)
        adrs.set_tree_index(i);

        // 11: node ← H(PK.seed, ADRS, lnode ∥ rnode)
        (hashers.h)(pk_seed, &adrs, &lnode, &rnode)

        // 12: end if
    };

    // 13: return node
    node
}


/// Algorithm 10: `xmss_sign(M, SK.seed, idx, PK.seed, ADRS)` on page 23.
/// Generates an XMSS signature.
///
/// Input: n-byte message `M`, secret seed `SK.seed`, index `idx`, public seed `PK.seed`, address `ADRS`. <br>
/// Output: XMSS signature SIGXMSS = (sig ∥ AUTH).
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn xmss_sign<
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, m: &[u8], sk_seed: &[u8], idx: u32, pk_seed: &[u8],
    adrs: &Adrs,
) -> XmssSig<HP, LEN, N> {
    let hp32 = u32::try_from(HP).unwrap();
    let mut adrs = adrs.clone();
    let mut sig_xmss = XmssSig {
        sig_wots: WotsSig { data: [[0u8; N]; LEN] },
        auth: [[0u8; N]; HP],
    };

    // 1: for j from 0 to h′-1 do    ▷ Build authentication path
    for j in 0..hp32 {
        //
        // 2: k ← idx/2 ^j xor 1
        let k = (idx >> j) ^ 1;

        // 3: AUTH[j] ← xmss_node(SK.seed, k, j, PK.seed, ADRS)
        sig_xmss.auth[j as usize] =
            xmss_node::<H, HP, K, LEN, M, N>(hashers, sk_seed, k, j, pk_seed, &adrs);

        // 4: end for
    }

    // 5: ADRS.setTypeAndClear(WOTS_HASH)
    adrs.set_type_and_clear(WOTS_HASH);

    // 6: ADRS.setKeyPairAddress(idx)
    adrs.set_key_pair_address(idx);

    // 7: sig ← wots_sign(M, SK.seed, PK.seed, ADRS)
    sig_xmss.sig_wots = wots::wots_sign::<K, LEN, M, N>(hashers, m, sk_seed, pk_seed, &adrs);

    // 8: SIG_XMSS ← sig ∥ AUTH
    // struct built above

    // 9: return SIG_XMSS
    sig_xmss
}


/// Algorithm 11: `xmss_PKFromSig(idx, SIG_XMSS, M, PK.seed, ADRS)`
/// Computes an XMSS public key from an XMSS signature.
///
/// Input: Index `idx`, XMSS signature `SIG_XMSS = (sig ∥ AUTH)`, n-byte message `M`, public seed `PK.seed`,
/// address `ADRS`. <br>
/// Output: n-byte root value `node[0]`.
pub(crate) fn xmss_pk_from_sig<
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, idx: u32, sig_xmss: &XmssSig<HP, LEN, N>, m: &[u8],
    pk_seed: &[u8], adrs: &Adrs,
) -> [u8; N] {
    let hp32 = u32::try_from(HP).unwrap();
    let mut adrs = adrs.clone();

    // 1: ADRS.setTypeAndClear(WOTS_HASH)    ▷ Compute WOTS+ pk from WOTS+ sig
    adrs.set_type_and_clear(WOTS_HASH);

    // 2: ADRS.setKeyPairAddress(idx)
    adrs.set_key_pair_address(idx);

    // 3: sig ← SIG_XMSS.getWOTSSig()    ▷ SIG_XMSS [0 : len · n]
    let sig = sig_xmss.get_wots_sig();

    // 4: AUTH ← SIG_XMSS.getXMSSAUTH()    ▷ SIG_XMSS [len · n : (len + h′) · n]
    let auth = sig_xmss.get_xmss_auth();

    // 5: node[0] ← wots_PKFromSig(sig, M, PK.seed, ADRS)
    let mut node_0 = wots::wots_pk_from_sig::<K, LEN, M, N>(hashers, sig, m, pk_seed, &adrs).0;

    // 6: ADRS.setTypeAndClear(TREE)    ▷ Compute root from WOTS+ pk and AUTH
    adrs.set_type_and_clear(TREE);

    // 7: ADRS.setTreeIndex(idx)
    adrs.set_tree_index(idx);

    // 8: for k from 0 to h′ − 1 do
    for k in 0..hp32 {
        //
        // 9: ADRS.setTreeHeight(k + 1)
        adrs.set_tree_height(k + 1);

        // 10: if idx/2^k is even then
        let node_1 = if ((idx >> k) & 1) == 0 {
            //
            // 11: ADRS.setTreeIndex(ADRS.getTreeIndex()/2)
            let tmp = adrs.get_tree_index() / 2;
            adrs.set_tree_index(tmp);

            // 12: node[1] ← H(PK.seed, ADRS, node[0] ∥ AUTH[k])
            (hashers.h)(pk_seed, &adrs, &node_0, &auth[k as usize])

            // 13: else
        } else {
            //
            // 14: ADRS.setTreeIndex((ADRS.getTreeIndex() − 1)/2)
            let tmp = (adrs.get_tree_index() - 1) / 2;
            adrs.set_tree_index(tmp);

            // 15: node[1] ← H(PK.seed, ADRS, AUTH[k] ∥ node[0])
            (hashers.h)(pk_seed, &adrs, &auth[k as usize], &node_0)

            // 16: end if
        };

        // 17: node[0] ← node[1]
        node_0 = node_1;

        // 18: end for
    }

    // 19: return node[0]
    node_0
}
