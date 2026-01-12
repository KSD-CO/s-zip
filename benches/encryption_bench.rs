use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use s_zip::StreamingZipWriter;
use std::io::Cursor;

fn generate_data(size: usize) -> Vec<u8> {
    // Generate compressible data (repeating pattern)
    let mut data = Vec::with_capacity(size);
    let pattern = b"This is a test pattern that repeats. Lorem ipsum dolor sit amet. ";
    for _ in 0..(size / pattern.len() + 1) {
        data.extend_from_slice(pattern);
    }
    data.truncate(size);
    data
}

#[cfg(feature = "encryption")]
fn bench_encryption(c: &mut Criterion) {
    let sizes = vec![1024, 10 * 1024, 100 * 1024, 1024 * 1024]; // 1KB, 10KB, 100KB, 1MB

    let mut group = c.benchmark_group("encryption_overhead");

    for size in sizes {
        let data = generate_data(size);

        // Benchmark without encryption
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("no_encryption", size), &data, |b, data| {
            b.iter(|| {
                let buffer = Vec::new();
                let cursor = Cursor::new(buffer);
                let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
                writer.start_entry("test.txt").unwrap();
                writer.write_data(black_box(data)).unwrap();
                writer.finish().unwrap()
            });
        });

        // Benchmark with AES-256 encryption
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("aes256_encryption", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let buffer = Vec::new();
                    let cursor = Cursor::new(buffer);
                    let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
                    writer.set_password("benchmark_password_123");
                    writer.start_entry("test.txt").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap()
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "encryption")]
fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");

    // Test with 10MB file to see memory footprint
    let large_data = generate_data(10 * 1024 * 1024);

    group.bench_function("10mb_no_encryption", |b| {
        b.iter(|| {
            let buffer = Vec::new();
            let cursor = Cursor::new(buffer);
            let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
            writer.start_entry("large.txt").unwrap();
            writer.write_data(black_box(&large_data)).unwrap();
            writer.finish().unwrap()
        });
    });

    group.bench_function("10mb_aes256_encryption", |b| {
        b.iter(|| {
            let buffer = Vec::new();
            let cursor = Cursor::new(buffer);
            let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
            writer.set_password("test_password");
            writer.start_entry("large.txt").unwrap();
            writer.write_data(black_box(&large_data)).unwrap();
            writer.finish().unwrap()
        });
    });

    group.finish();
}

#[cfg(feature = "encryption")]
fn bench_pbkdf2_overhead(c: &mut Criterion) {
    use s_zip::encryption::{AesEncryptor, AesStrength};

    let mut group = c.benchmark_group("key_derivation");

    group.bench_function("pbkdf2_1000_iterations", |b| {
        b.iter(|| AesEncryptor::new(black_box("test_password_123"), AesStrength::Aes256).unwrap());
    });

    group.finish();
}

#[cfg(not(feature = "encryption"))]
fn bench_encryption(_c: &mut Criterion) {}

#[cfg(not(feature = "encryption"))]
fn bench_memory_usage(_c: &mut Criterion) {}

#[cfg(not(feature = "encryption"))]
fn bench_pbkdf2_overhead(_c: &mut Criterion) {}

criterion_group!(
    benches,
    bench_encryption,
    bench_memory_usage,
    bench_pbkdf2_overhead
);
criterion_main!(benches);
