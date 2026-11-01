//! Source-text guard over secret hygiene outside `SecretBytes`: the functions that
//! derive or return session keys, resumption secrets, data-encryption keys and ML-KEM
//! shared secrets must hand them back in a wiping type (`Zeroizing` or `SecretArray`),
//! never as a plain `[u8; 32]` that outlives its use on the stack, and the FIPS backend
//! must never grow a secret key `Vec` after writing key bytes into it (a realloc leaves
//! the old buffer in freed memory). [AC-51.2] [NFR-PQ-2] [VLD-ANB-MIDGARD-0036]

use std::path::{Path, PathBuf};

fn source(rel: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    (path, text)
}

/// Every signature of `fn <name>` in `src`, from the name to the opening brace, so a
/// return type on a later line of a wrapped signature is still seen.
fn signatures<'a>(src: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("fn {name}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find(&needle) {
        let start = from + i;
        let after = start + needle.len();
        // `fn binder_key` must not match `fn binder_key_v2`.
        let next = src[after..].chars().next();
        if matches!(next, Some('(') | Some('<')) {
            let end = src[after..].find('{').map_or(src.len(), |j| after + j);
            out.push(&src[start..end]);
        }
        from = after;
    }
    out
}

/// `(file, functions)` pairs: each function must exist in its file and none may return
/// a bare `[u8; 32]`.
const GUARDED: &[(&str, &[&str])] = &[
    ("src/backend/fips.rs", &["mlkem_encaps", "mlkem_decaps"]),
    ("../pq-mtls/src/handshake.rs", &["derive_traffic"]),
    ("../pq-mtls/src/session.rs", &["resumption_secret"]),
    (
        "../pq-mtls/src/resumption.rs",
        &["binder_key", "resume_traffic"],
    ),
    ("../../services/kms/src/lib.rs", &["derive_dek"]),
];

#[test]
fn derived_secrets_are_never_returned_as_plain_arrays() {
    for (file, names) in GUARDED {
        let (path, src) = source(file);
        for name in *names {
            let sigs = signatures(&src, name);
            assert!(
                !sigs.is_empty(),
                "{}: fn {name} not found; update this guard if it moved",
                path.display()
            );
            for sig in sigs {
                let compact: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(
                    !compact.contains("-> [u8; 32]"),
                    "{}: fn {name} returns a plain [u8; 32]; wrap it in Zeroizing or \
                     SecretArray: {compact}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn fips_backend_never_grows_a_secret_key() {
    let (path, src) = source("src/backend/fips.rs");
    assert!(
        !src.contains("sk.extend_from_slice"),
        "{}: a secret key Vec is grown after holding key bytes; allocate it at its final \
         size and copy into it",
        path.display()
    );
}
