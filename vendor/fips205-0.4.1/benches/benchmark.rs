use criterion::{criterion_group, criterion_main, Criterion};
use fips205::traits::{KeyGen, Signer, Verifier};
use fips205::{
    slh_dsa_sha2_128f, slh_dsa_sha2_128s, slh_dsa_sha2_192f, slh_dsa_sha2_192s, slh_dsa_sha2_256f,
    slh_dsa_sha2_256s, slh_dsa_shake_128f, slh_dsa_shake_128s, slh_dsa_shake_192f,
    slh_dsa_shake_192s, slh_dsa_shake_256f, slh_dsa_shake_256s,
};


#[allow(clippy::redundant_closure)]
pub fn criterion_benchmark(c: &mut Criterion) {
    let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    let hedged = false;

    let (pk_sha2_128s, sk_sha2_128s) = slh_dsa_sha2_128s::KG::try_keygen().unwrap();
    let (pk_sha2_128f, sk_sha2_128f) = slh_dsa_sha2_128f::KG::try_keygen().unwrap();
    let (pk_sha2_192s, sk_sha2_192s) = slh_dsa_sha2_192s::KG::try_keygen().unwrap();
    let (pk_sha2_192f, sk_sha2_192f) = slh_dsa_sha2_192f::KG::try_keygen().unwrap();
    let (pk_sha2_256s, sk_sha2_256s) = slh_dsa_sha2_256s::KG::try_keygen().unwrap();
    let (pk_sha2_256f, sk_sha2_256f) = slh_dsa_sha2_256f::KG::try_keygen().unwrap();
    let (pk_shake_128s, sk_shake_128s) = slh_dsa_shake_128s::KG::try_keygen().unwrap();
    let (pk_shake_128f, sk_shake_128f) = slh_dsa_shake_128f::KG::try_keygen().unwrap();
    let (pk_shake_192s, sk_shake_192s) = slh_dsa_shake_192s::KG::try_keygen().unwrap();
    let (pk_shake_192f, sk_shake_192f) = slh_dsa_shake_192f::KG::try_keygen().unwrap();
    let (pk_shake_256s, sk_shake_256s) = slh_dsa_shake_256s::KG::try_keygen().unwrap();
    let (pk_shake_256f, sk_shake_256f) = slh_dsa_shake_256f::KG::try_keygen().unwrap();

    let sig_sha2_128s = sk_sha2_128s.try_sign(&message, b"context", hedged).unwrap();
    let sig_sha2_128f = sk_sha2_128f.try_sign(&message, b"context", hedged).unwrap();
    let sig_sha2_192s = sk_sha2_192s.try_sign(&message, b"context", hedged).unwrap();
    let sig_sha2_192f = sk_sha2_192f.try_sign(&message, b"context", hedged).unwrap();
    let sig_sha2_256s = sk_sha2_256s.try_sign(&message, b"context", hedged).unwrap();
    let sig_sha2_256f = sk_sha2_256f.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_128s = sk_shake_128s.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_128f = sk_shake_128f.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_192s = sk_shake_192s.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_192f = sk_shake_192f.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_256s = sk_shake_256s.try_sign(&message, b"context", hedged).unwrap();
    let sig_shake_256f = sk_shake_256f.try_sign(&message, b"context", hedged).unwrap();

    c.bench_function("sha2_128f  keygen", |b| b.iter(|| slh_dsa_sha2_128f::KG::try_keygen()));
    c.bench_function("sha2_192f  keygen", |b| b.iter(|| slh_dsa_sha2_192f::KG::try_keygen()));
    c.bench_function("sha2_256f  keygen", |b| b.iter(|| slh_dsa_sha2_256f::KG::try_keygen()));
    c.bench_function("shake_128f keygen", |b| b.iter(|| slh_dsa_shake_128f::KG::try_keygen()));
    c.bench_function("shake_192f keygen", |b| b.iter(|| slh_dsa_shake_192f::KG::try_keygen()));
    c.bench_function("shake_256f keygen", |b| b.iter(|| slh_dsa_shake_256f::KG::try_keygen()));
    c.bench_function("sha2_128s  keygen", |b| b.iter(|| slh_dsa_sha2_128s::KG::try_keygen()));
    c.bench_function("sha2_192s  keygen", |b| b.iter(|| slh_dsa_sha2_192s::KG::try_keygen()));
    c.bench_function("sha2_256s  keygen", |b| b.iter(|| slh_dsa_sha2_256s::KG::try_keygen()));
    c.bench_function("shake_128s keygen", |b| b.iter(|| slh_dsa_shake_128s::KG::try_keygen()));
    c.bench_function("shake_192s keygen", |b| b.iter(|| slh_dsa_shake_192s::KG::try_keygen()));
    c.bench_function("shake_256s keygen", |b| b.iter(|| slh_dsa_shake_256s::KG::try_keygen()));
    //
    c.bench_function("sha2_128f  sign  ", |b| {
        b.iter(|| sk_sha2_128f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("sha2_192f  sign  ", |b| {
        b.iter(|| sk_sha2_192f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("sha2_256f  sign  ", |b| {
        b.iter(|| sk_sha2_256f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_128f sign  ", |b| {
        b.iter(|| sk_shake_128f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_192f sign  ", |b| {
        b.iter(|| sk_shake_192f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_256f sign  ", |b| {
        b.iter(|| sk_shake_256f.try_sign(&message, b"context", hedged))
    });
    c.bench_function("sha2_128s  sign  ", |b| {
        b.iter(|| sk_sha2_128s.try_sign(&message, b"context", hedged))
    });
    c.bench_function("sha2_192s  sign  ", |b| {
        b.iter(|| sk_sha2_192s.try_sign(&message, b"context", hedged))
    });
    c.bench_function("sha2_256s  sign  ", |b| {
        b.iter(|| sk_sha2_256s.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_128s sign  ", |b| {
        b.iter(|| sk_shake_128s.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_192s sign  ", |b| {
        b.iter(|| sk_shake_192s.try_sign(&message, b"context", hedged))
    });
    c.bench_function("shake_256s sign  ", |b| {
        b.iter(|| sk_shake_256s.try_sign(&message, b"context", hedged))
    });
    //
    c.bench_function("sha2_128f  verify", |b| {
        b.iter(|| pk_sha2_128f.verify(&message, &sig_sha2_128f, b"context"))
    });
    c.bench_function("sha2_192f  verify", |b| {
        b.iter(|| pk_sha2_192f.verify(&message, &sig_sha2_192f, b"context"))
    });
    c.bench_function("sha2_256f  verify", |b| {
        b.iter(|| pk_sha2_256f.verify(&message, &sig_sha2_256f, b"context"))
    });
    c.bench_function("shake_128f verify", |b| {
        b.iter(|| pk_shake_128f.verify(&message, &sig_shake_128f, b"context"))
    });
    c.bench_function("shake_192f verify", |b| {
        b.iter(|| pk_shake_192f.verify(&message, &sig_shake_192f, b"context"))
    });
    c.bench_function("shake_256f verify", |b| {
        b.iter(|| pk_shake_256f.verify(&message, &sig_shake_256f, b"context"))
    });
    c.bench_function("sha2_128s  verify", |b| {
        b.iter(|| pk_sha2_128s.verify(&message, &sig_sha2_128s, b"context"))
    });
    c.bench_function("sha2_192s  verify", |b| {
        b.iter(|| pk_sha2_192s.verify(&message, &sig_sha2_192s, b"context"))
    });
    c.bench_function("sha2_256s  verify", |b| {
        b.iter(|| pk_sha2_256s.verify(&message, &sig_sha2_256s, b"context"))
    });
    c.bench_function("shake_128s verify", |b| {
        b.iter(|| pk_shake_128s.verify(&message, &sig_shake_128s, b"context"))
    });
    c.bench_function("shake_192s verify", |b| {
        b.iter(|| pk_shake_192s.verify(&message, &sig_shake_192s, b"context"))
    });
    c.bench_function("shake_256s verify", |b| {
        b.iter(|| pk_shake_256s.verify(&message, &sig_shake_256s, b"context"))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
