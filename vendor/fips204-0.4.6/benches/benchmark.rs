use criterion::{criterion_group, criterion_main, Criterion};
use fips204::traits::{Signer, Verifier};
use fips204::{ml_dsa_44, ml_dsa_65, ml_dsa_87};
use rand_core::{CryptoRng, RngCore};


// Test RNG to supply incremented values when 'asked'
#[repr(align(8))]
struct TestRng {
    value: u32,
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, _out: &mut [u8]) { unimplemented!() }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        out.iter_mut().for_each(|b| *b = self.value.to_le_bytes()[0]);
        out[0..4].copy_from_slice(&self.value.to_be_bytes());
        self.value = self.value.wrapping_add(1);
        Ok(())
    }
}

impl CryptoRng for TestRng {}


pub fn criterion_benchmark(c: &mut Criterion) {
    let mut rng = TestRng { value: 0 };
    let msg = [0u8, 1, 2, 3, 4, 5, 6, 7];

    let (pk44, sk44) = ml_dsa_44::try_keygen_with_rng(&mut rng).unwrap();
    let sig44 = sk44.try_sign(&msg, &[0]).unwrap();

    let (pk65, sk65) = ml_dsa_65::try_keygen_with_rng(&mut rng).unwrap();
    let sig65 = sk65.try_sign(&msg, &[0]).unwrap();

    let (pk87, sk87) = ml_dsa_87::try_keygen_with_rng(&mut rng).unwrap();
    let sig87 = sk87.try_sign(&msg, &[0]).unwrap();

    c.bench_function("ml_dsa_44 keygen", |b| b.iter(|| ml_dsa_44::try_keygen_with_rng(&mut rng)));
    c.bench_function("ml_dsa_65 keygen", |b| b.iter(|| ml_dsa_65::try_keygen_with_rng(&mut rng)));
    c.bench_function("ml_dsa_87 keygen", |b| b.iter(|| ml_dsa_87::try_keygen_with_rng(&mut rng)));

    c.bench_function("ml_dsa_44 sk sign", |b| {
        b.iter(|| sk44.try_sign_with_rng(&mut rng, &msg, &[]))
    });
    c.bench_function("ml_dsa_65 sk sign", |b| {
        b.iter(|| sk65.try_sign_with_rng(&mut rng, &msg, &[]))
    });
    c.bench_function("ml_dsa_87 sk sign", |b| {
        b.iter(|| sk87.try_sign_with_rng(&mut rng, &msg, &[]))
    });

    c.bench_function("ml_dsa_44 pk verify", |b| b.iter(|| pk44.verify(&msg, &sig44, &[])));
    c.bench_function("ml_dsa_65 pk verify", |b| b.iter(|| pk65.verify(&msg, &sig65, &[])));
    c.bench_function("ml_dsa_87 pk verify", |b| b.iter(|| pk87.verify(&msg, &sig87, &[])));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
