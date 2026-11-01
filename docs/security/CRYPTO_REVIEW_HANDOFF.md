# Cryptographic review hand-off

What the independent cryptographic review (ANUBIS WP-P5) is asked to look at: each
primitive and construction the platform relies on, where it is implemented, which
test files hold its known-answer vectors, and the questions left open for the
reviewer. Raised by ANUBIS WP-53 (VLD-ANB-MIDGARD-0024, VLD-ANB-MIDGARD-0059).

Nothing here is hand-written cryptography. Since MG-SEC-10 the hash, MAC and KDF
functions are thin wrappers over the vendored RustCrypto crates, and the
post-quantum algorithms come from the vendored `fips203`, `fips204` and `fips205`
crates. The vectors below prove the wrappers compute the standard functions; a
round-trip test cannot, because both ends would share any bug.

## Primitives and constructions under review

| Primitive | Implemented in | Vectors and tests |
|-----------|----------------|-------------------|
| SHA-256 (FIPS 180-4) | `crypto/pqc-agility/src/hash.rs` over `sha2` | `crypto/pqc-agility/tests/hash_kat.rs`: NIST CAVP short and long messages, the one-million-`a` message, block-boundary and clone cases |
| HMAC-SHA256 (RFC 2104) | `crypto/pqc-agility/src/hash.rs` over `hmac` | `crypto/pqc-agility/tests/hash_kat.rs::kat_hmac_sha256_rfc4231`: RFC 4231 test cases 1 to 7 (case 5 truncated to 128 bits, as the RFC gives it) |
| HKDF-SHA256 (RFC 5869) | `crypto/pqc-agility/src/hash.rs` (`hkdf_sha256_32`, L = 32) over `hkdf` | `crypto/pqc-agility/tests/hash_kat.rs::kat_hkdf_sha256_rfc5869`: RFC 5869 A.1 to A.3, PRK and the first 32 bytes of OKM |
| PBKDF2-HMAC-SHA256 (RFC 8018) | `crypto/pqc-agility/src/hash.rs` over `pbkdf2` | `crypto/pqc-agility/tests/hash_kat.rs::kat_pbkdf2_hmac_sha256`: RFC 6070 inputs with the published SHA-256 outputs and RFC 7914 section 11. Used only to verify legacy password entries |
| Constant-time compare (`ct_eq`) | `crypto/pqc-agility/src/hash.rs` over `subtle` | `crypto/pqc-agility/tests/hash_kat.rs::ct_eq_is_equality_and_refuses_different_lengths`. A length mismatch returns early: lengths are public |
| SHA3-256 (FIPS 202) | `sha3`, used only by the hybrid combiner | `crypto/pqc-agility/tests/hybrid_kat.rs::sha3_256_matches_fips_202` |
| Hybrid KEM combiner (X-Wing) | `crypto/pqc-agility/src/hybrid.rs` (`combine_hybrid`), called from `crypto/pqc-agility/src/backend/fips.rs` | `crypto/pqc-agility/tests/hybrid_kat.rs::combiner_pinned_vector` (expected values computed outside the crate with Python `hashlib.sha3_256`) and `changing_ct_or_pk_changes_the_secret`; `crypto/pqc-agility/src/hybrid.rs` unit tests; `crypto/pqc-agility/tests/fips_backend.rs::hybrid_refuses_an_all_zero_x25519_result` and `hybrid_secret_binds_the_x25519_ciphertext`; `crypto/pq-mtls/tests/real_fips_handshake.rs` |
| ML-KEM-768/1024, ML-DSA (FIPS 203, 204) | `fips203`, `fips204` behind `crypto/pqc-agility/src/backend/fips.rs` | `crypto/pqc-agility/tests/nist_acvp.rs`: the NIST ACVP vectors vendored with the crates |
| SLH-DSA (FIPS 205) | `fips205` behind `crypto/pqc-agility/src/backend/fips.rs` | `crypto/pqc-agility/tests/fips_backend.rs` round trips and tamper tests only |
| AES-256-GCM | `aes-gcm` behind `crypto/pqc-agility/src/aead.rs` | `crypto/pqc-agility/tests/aead_kat.rs`: McGrew and Viega test cases 13 and 14 |
| Argon2id (RFC 9106) password storage | `services/iam/src/password.rs` over `argon2` | `services/iam/tests/password_storage.rs::the_vendored_argon2id_matches_rfc_9106` and the cost-bound tests in the same file |

## The hybrid combiner

```text
ss = SHA3-256( ss_M || ss_X || ct_X || pk_X || label )
```

`ss_M` is the ML-KEM shared secret, `ss_X` the X25519 result, `ct_X` the ephemeral
X25519 public key and `pk_X` the recipient's X25519 public key. The label is
X-Wing's six bytes `\.//^\` for ML-KEM-768 + X25519, so that suite is X-Wing as
draft-connolly-cfrg-xwing-kem specifies it, and `midgard-xwing-mlkem1024-x25519-v1`
for ML-KEM-1024 + X25519. An all-zero X25519 result (a low-order or identity point,
RFC 7748 section 6.1) is refused on both sides before the combiner runs
(`reject_noncontributory_x25519`). The hybrid artefacts append the 32-byte X25519
half to the ML-KEM public key, secret key and ciphertext. Delivered by MG-SEC-09;
the pq-mtls handshake is protocol version 2 because of it.

The construction differs from the one the ANUBIS book proposed for WP-53
(HKDF-SHA256 with the ciphertexts hashed into `info`): the repository adopted
X-Wing itself, with SHA3-256, which the book had rejected only because no SHA-3 was
then available.

## Open questions for WP-P5

1. **X-Wing conformance.** The pinned vector is computed from the formula, not taken
   from the draft. X-Wing's published vectors start from a 32-byte seed that is
   expanded into both key pairs; this repository stores expanded keys, so the
   draft's end-to-end vectors are not run. Is formula-level conformance enough, or
   should key generation follow the draft's seed expansion?
2. **ML-KEM-1024 + X25519.** The draft covers ML-KEM-768 only. Is the same
   construction under its own label acceptable for the 1024 suite, given that
   ML-KEM's own secret already binds `ct_M` (FIPS 203's Fujisaki-Okamoto transform)?
3. **Error on a low-order ephemeral key.** Decapsulation refuses it with
   `CryptoError::InvalidKey`, the same error the encapsulating side returns, where
   the book asked for `DecapsulationFailed`. Does either choice give a peer a
   useful distinguisher in the pq-mtls handshake, which fails closed either way?
4. **NIST SP 800-227.** Confirm that the X-Wing combiner satisfies SP 800-227's
   requirements for combining an approved KEM with a classical one in this
   deployment, given that X25519 is not a FIPS-approved component.
5. **Session-resumption key schedule.** The `resume_traffic` salt and IKM order named
   in VLD-ANB-MIDGARD-0059 (`crypto/pq-mtls/src/resumption.rs`) is rewritten by
   ANUBIS WP-52; review the schedule as WP-52 delivers it.
6. **SLH-DSA vectors.** SLH-DSA has round-trip and tamper tests but no ACVP vectors
   in the repository; should they be added alongside the ML-KEM and ML-DSA sets?
7. **FIPS 140-3.** None of this replaces module certification, which remains an
   external laboratory activity and a GA blocker
   (`docs/compliance/RESIDUAL_RISK_REGISTER.md`, "FIPS KAT certification before GA").
