#![feature(test)]

extern crate ethereum_models;
extern crate fnv;
extern crate seahash;
extern crate test;

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

fn hasher_benchmark<H>(b: &mut test::Bencher, mut hasher: H, len: usize)
    where H: Hasher
{
    let bytes: Vec<_> = (0..100).cycle().take(len).collect();
    b.bytes = bytes.len() as u64;
    b.iter(|| {
        hasher.write(&bytes);
        hasher.finish()
    });
}

fn fnvhash_bench(b: &mut test::Bencher, len: usize) {
    hasher_benchmark(b, <fnv::FnvHasher as Default>::default(), len)
}

fn seahash_bench(b: &mut test::Bencher, len: usize) {
    hasher_benchmark(b, <seahash::SeaHasher as Default>::default(), len)
}

fn siphash_bench(b: &mut test::Bencher, len: usize) {
    hasher_benchmark(b, DefaultHasher::new(), len)
}

#[bench]
fn fnvhash_32_byte(b: &mut test::Bencher) { fnvhash_bench(b, 32) }

#[bench]
fn fnvhash_16_byte(b: &mut test::Bencher) { fnvhash_bench(b, 16) }

#[bench]
fn fnvhash_8_byte(b: &mut test::Bencher) { fnvhash_bench(b, 8) }

#[bench]
fn fnvhash_4_byte(b: &mut test::Bencher) { fnvhash_bench(b, 4) }

#[bench]
fn fnvhash_1_byte(b: &mut test::Bencher) { fnvhash_bench(b, 1) }

#[bench]
fn fnvhash_0_byte(b: &mut test::Bencher) { fnvhash_bench(b, 0) }

#[bench]
fn seahash_32_byte(b: &mut test::Bencher) { seahash_bench(b, 32) }

#[bench]
fn seahash_16_byte(b: &mut test::Bencher) { seahash_bench(b, 16) }

#[bench]
fn seahash_8_byte(b: &mut test::Bencher) { seahash_bench(b, 8) }

#[bench]
fn seahash_4_byte(b: &mut test::Bencher) { seahash_bench(b, 4) }

#[bench]
fn seahash_1_byte(b: &mut test::Bencher) { seahash_bench(b, 1) }

#[bench]
fn seahash_0_byte(b: &mut test::Bencher) { seahash_bench(b, 0) }

#[bench]
fn siphash_32_byte(b: &mut test::Bencher) { siphash_bench(b, 32) }

#[bench]
fn siphash_16_byte(b: &mut test::Bencher) { siphash_bench(b, 16) }

#[bench]
fn siphash_8_byte(b: &mut test::Bencher) { siphash_bench(b, 8) }

#[bench]
fn siphash_4_byte(b: &mut test::Bencher) { siphash_bench(b, 4) }

#[bench]
fn siphash_1_byte(b: &mut test::Bencher) { siphash_bench(b, 1) }

#[bench]
fn siphash_0_byte(b: &mut test::Bencher) { siphash_bench(b, 0) }