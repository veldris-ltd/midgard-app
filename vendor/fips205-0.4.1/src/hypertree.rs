use crate::hashers::Hashers;
use crate::types::{Adrs, HtSig, WotsSig, XmssSig};
use crate::xmss;


/// Algorithm 12: `ht_sign(M, SK.seed, PK.seed, idx_tree, idx_leaf)` on page 27.
/// Generates a hypertree signature.
///
/// Input: Message `M`, private seed `SK.seed`, public seed `PK.seed`, tree index `idx_tree`, leaf
/// index `idx_leaf`. <br>
/// Output: HT signature `SIG_HT`.
#[allow(clippy::similar_names)] // sk_seed and pk_seed
pub(crate) fn ht_sign<
    const D: usize,
    const H: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, m: &[u8], sk_seed: &[u8], pk_seed: &[u8], idx_tree: u64,
    idx_leaf: u32,
) -> Result<HtSig<D, HP, LEN, N>, &'static str> {
    let mut idx_tree = idx_tree;
    let (d32, hp32) = (u32::try_from(D).unwrap(), u32::try_from(HP).unwrap());
    //
    // 1: ADRS ← toByte(0, 32)
    let mut adrs = Adrs::default();

    // 2: ADRS.setTreeAddress(idxtree)
    adrs.set_tree_address(idx_tree);

    // 3: SIG_tmp ← xmss_sign(M, SK.seed, idxleaf, PK.seed, ADRS)
    let mut sig_tmp =
        xmss::xmss_sign::<H, HP, K, LEN, M, N>(hashers, m, sk_seed, idx_leaf, pk_seed, &adrs);

    // 4: SIG_HT ← SIG_tmp
    let mut sig_ht = HtSig {
        xmss_sigs: core::array::from_fn(|_| XmssSig {
            sig_wots: WotsSig { data: [[0u8; N]; LEN] },
            auth: [[0u8; N]; HP],
        }),
    };
    sig_ht.xmss_sigs[0] = sig_tmp.clone();

    // 5: root ← xmss_PKFromSig(idx_leaf, SIG_tmp, M, PK.seed, ADRS)
    let mut root =
        xmss::xmss_pk_from_sig::<HP, K, LEN, M, N>(hashers, idx_leaf, &sig_tmp, m, pk_seed, &adrs);

    // 6: for j from 1 to d − 1 do
    for j in 1..d32 {
        //
        // 7: idx_leaf ← idx_tree mod 2^{h′}    ▷ h′ least significant bits of idx_tree
        let idx_leaf =
            u32::try_from(idx_tree & ((1 << hp32) - 1)).map_err(|_| "Alg11: oversized idx leaf")?;

        // 8: idx_tree ← idx_tree ≫ h′    ▷ Remove least significant h′ bits from idx_tree
        idx_tree >>= hp32;

        // 9: ADRS.setLayerAddress(j)
        adrs.set_layer_address(j);

        // 10: ADRS.setTreeAddress(idx_tree)
        adrs.set_tree_address(idx_tree);

        // 11: SIG_tmp ← xmss_sign(root, SK.seed, idx_leaf, PK.seed, ADRS)
        sig_tmp = xmss::xmss_sign::<H, HP, K, LEN, M, N>(
            hashers, &root, sk_seed, idx_leaf, pk_seed, &adrs,
        );

        // 12: SIG_HT ← SIG_HT ∥ SIG_tmp
        sig_ht.xmss_sigs[j as usize] = sig_tmp.clone();

        // 13: if j < d − 1 then
        if j < (d32 - 1) {
            //
            // 14: root ← xmss_PKFromSig(idx_leaf, SIG_tmp, root, PK.seed, ADRS)
            root = xmss::xmss_pk_from_sig::<HP, K, LEN, M, N>(
                hashers, idx_leaf, &sig_tmp, &root, pk_seed, &adrs,
            );

            // 15: end if
        }

        // 16: end for
    }

    // 17: return SIGHT
    Ok(sig_ht)
}


/// Algorithm 13: `ht_verify(M, SIG_HT, PK.seed, idx_tree, idx_leaf, PK.root)` on page 28.
/// Verifies a hypertree signature.
///
/// Input: Message `M`, signature `SIG_HT`, public seed `PK.seed`, tree index `idx_tree`, leaf index `idx_leaf`,
/// HT public key `PK.root`. <br>
/// Output: Boolean.
pub(crate) fn ht_verify<
    const D: usize,
    const HP: usize,
    const K: usize,
    const LEN: usize,
    const M: usize,
    const N: usize,
>(
    hashers: &Hashers<K, LEN, M, N>, m: &[u8], sig_ht: &HtSig<D, HP, LEN, N>, pk_seed: &[u8],
    idx_tree: u64, idx_leaf: u32, pk_root: &[u8; N],
) -> bool {
    let mut idx_tree = idx_tree;
    let (d32, hp32) = (u32::try_from(D).unwrap(), u32::try_from(HP).unwrap());
    //
    // 1: ADRS ← toByte(0, 32)
    let mut adrs = Adrs::default();

    // 2: ADRS.setTreeAddress(idx_tree)
    adrs.set_tree_address(idx_tree);

    // 3: SIG_tmp ← SIG_HT.getXMSSSignature(0)    ▷ SIG_HT [0 : (h′ + len) · n]
    let sig_tmp = sig_ht.xmss_sigs[0].clone();

    // 4: node ← xmss_PKFromSig(idx_leaf, SIG_tmp, M, PK.seed, ADRS)
    let mut node = xmss::xmss_pk_from_sig(hashers, idx_leaf, &sig_tmp, m, pk_seed, &adrs);

    // 5: for j from 1 to d − 1 do
    for j in 1..d32 {
        //
        // 6: idx_leaf ← idx_tree mod 2^{h′}    ▷ h′ least significant bits of idx_tree
        let idx_leaf = u32::try_from(idx_tree & ((1 << hp32) - 1));

        if idx_leaf.is_err() {
            return false;
        };
        let idx_leaf = idx_leaf.unwrap();

        // 7: idx_tree ← idx_tree ≫ h′    ▷ Remove least significant h′ bits from idx_tree
        idx_tree >>= hp32;

        // 8: ADRS.setLayerAddress(j)
        adrs.set_layer_address(j);

        // 9: ADRS.setTreeAddress(idx_tree)
        adrs.set_tree_address(idx_tree);

        // 10: SIG_tmp ← SIG_HT.getXMSSSignature(j)     ▷ SIGHT [ j · (h′ + len) · n : ( j + 1)(h′ + len) · n]
        let sig_tmp = sig_ht.xmss_sigs[j as usize].clone();

        // 11: node ← xmss_PKFromSig(idx_leaf, SIG_tmp, node, PK.seed, ADRS)
        node = xmss::xmss_pk_from_sig(hashers, idx_leaf, &sig_tmp, &node, pk_seed, &adrs);

        // 12: end for
    }

    // 13: if node = PK.root then
    // 14:   return true
    // 15: else
    // 16:   return false
    // 17: end if
    node == *pk_root // Public data, thus no CT eq required
}
