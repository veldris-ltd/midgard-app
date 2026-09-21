use criterion::{criterion_group, criterion_main, Criterion};
use fips203::traits::{Decaps, Encaps, KeyGen};
use fips203::{ml_kem_1024, ml_kem_512, ml_kem_768};
use rand_core::{CryptoRng, RngCore};


// Test RNG to regurgitate incremented values when 'asked'
struct TestRng {
    value: u32,
}

impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 { unimplemented!() }

    fn next_u64(&mut self) -> u64 { unimplemented!() }

    fn fill_bytes(&mut self, _out: &mut [u8]) { unimplemented!() }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        out.iter_mut().for_each(|b| *b = 0);
        out[0..4].copy_from_slice(&self.value.to_be_bytes());
        self.value = self.value.wrapping_add(1);
        Ok(())
    }
}

impl CryptoRng for TestRng {}


#[allow(clippy::redundant_closure)]
pub fn criterion_benchmark(c: &mut Criterion) {
    // Generate intermediate values needed for the actual benchmark functions
    let mut bench_rng = TestRng { value: 0 };
    let (ek_512, dk_512) = ml_kem_512::KG::try_keygen_with_rng(&mut bench_rng).unwrap();
    let (_, ct_512) = ek_512.try_encaps().unwrap();
    let (ek_768, dk_768) = ml_kem_768::KG::try_keygen_with_rng(&mut bench_rng).unwrap();
    let (_, ct_768) = ek_768.try_encaps().unwrap();
    let (ek_1024, dk_1024) = ml_kem_1024::KG::try_keygen_with_rng(&mut bench_rng).unwrap();
    let (_, ct_1024) = ek_1024.try_encaps().unwrap();

    c.bench_function("ml_kem_512  KeyGen", |b| {
        b.iter(|| ml_kem_512::KG::try_keygen_with_rng(&mut bench_rng))
    });
    c.bench_function("ml_kem_768  KeyGen", |b| {
        b.iter(|| ml_kem_768::KG::try_keygen_with_rng(&mut bench_rng))
    });
    c.bench_function("ml_kem_1024 KeyGen", |b| {
        b.iter(|| ml_kem_1024::KG::try_keygen_with_rng(&mut bench_rng))
    });

    c.bench_function("ml_kem_512  Encaps", |b| {
        b.iter(|| ek_512.try_encaps_with_rng(&mut bench_rng))
    });
    c.bench_function("ml_kem_768  Encaps", |b| {
        b.iter(|| ek_768.try_encaps_with_rng(&mut bench_rng))
    });
    c.bench_function("ml_kem_1024 Encaps", |b| {
        b.iter(|| ek_1024.try_encaps_with_rng(&mut bench_rng))
    });

    c.bench_function("ml_kem_512  Decaps", |b| b.iter(|| dk_512.try_decaps(&ct_512)));
    c.bench_function("ml_kem_768  Decaps", |b| b.iter(|| dk_768.try_decaps(&ct_768)));
    c.bench_function("ml_kem_1024 Decaps", |b| b.iter(|| dk_1024.try_decaps(&ct_1024)));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
