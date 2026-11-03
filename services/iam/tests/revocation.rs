//! The token deny list: a revoked id is refused until the token it names has expired,
//! then purged, so the list holds no more than one token lifetime of sign-outs.
//! [NFR-SEC-2] [VLD-ANB-MIDGARD-0021]

use iam::RevocationList;

#[test]
fn revoke_then_is_revoked_until_purge() {
    let mut list = RevocationList::new();
    let (a, b) = ([1u8; 16], [2u8; 16]);
    assert!(!list.is_revoked(&a));

    list.revoke(a, 1_000);
    list.revoke(b, 2_000);
    assert!(list.is_revoked(&a) && list.is_revoked(&b));
    assert_eq!(list.len(), 2);

    // Before either token expires nothing is dropped.
    list.purge(999);
    assert_eq!(list.len(), 2);

    // At its expiry the first token is refused by its window anyway: its entry goes.
    list.purge(1_000);
    assert!(!list.is_revoked(&a));
    assert!(list.is_revoked(&b), "an unexpired revocation is kept");
    assert_eq!(list.len(), 1);

    list.purge(2_000);
    assert!(list.is_empty());
}
