use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
#[cfg(feature = "zstd-support")]
use s_zip::CompressionMethod;
use s_zip::StreamingZipWriter;
use tempfile::NamedTempFile;

fn generate_compressible_data(size: usize) -> Vec<u8> {
    // Pattern that compresses well
    let pattern = b"The quick brown fox jumps over the lazy dog. ";
    let mut data = Vec::with_capacity(size);
    while data.len() < size {
        data.extend_from_slice(pattern);
    }
    data.truncate(size);
    data
}

fn generate_random_data(size: usize) -> Vec<u8> {
    // Pseudo-random data that doesn't compress well
    let mut data = Vec::with_capacity(size);
    let mut state = 0x12345678u32;
    for _ in 0..size {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        data.push((state >> 16) as u8);
    }
    data
}

fn bench_compression_methods(c: &mut Criterion) {
    let sizes = vec![
        1024,             // 1KB
        10 * 1024,        // 10KB
        100 * 1024,       // 100KB
        1024 * 1024,      // 1MB
        10 * 1024 * 1024, // 10MB
    ];

    for size in sizes {
        let mut group = c.benchmark_group(format!("write_compressible_{}", format_size(size)));
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_compressible_data(size);

        // Benchmark DEFLATE compression
        group.bench_with_input(
            BenchmarkId::new("deflate_level_6", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer = StreamingZipWriter::with_compression(temp.path(), 6).unwrap();
                    writer.start_entry("test.bin").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap();
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("deflate_level_9", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer = StreamingZipWriter::with_compression(temp.path(), 9).unwrap();
                    writer.start_entry("test.bin").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap();
                });
            },
        );

        // Benchmark Zstd compression if feature is enabled
        #[cfg(feature = "zstd-support")]
        {
            group.bench_with_input(BenchmarkId::new("zstd_level_3", size), &data, |b, data| {
                b.iter(|| {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer =
                        StreamingZipWriter::with_method(temp.path(), CompressionMethod::Zstd, 3)
                            .unwrap();
                    writer.start_entry("test.bin").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap();
                });
            });

            group.bench_with_input(BenchmarkId::new("zstd_level_10", size), &data, |b, data| {
                b.iter(|| {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer =
                        StreamingZipWriter::with_method(temp.path(), CompressionMethod::Zstd, 10)
                            .unwrap();
                    writer.start_entry("test.bin").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap();
                });
            });
        }

        group.finish();
    }
}

fn bench_random_data_compression(c: &mut Criterion) {
    let sizes = vec![100 * 1024, 1024 * 1024]; // 100KB, 1MB

    for size in sizes {
        let mut group = c.benchmark_group(format!("write_random_{}", format_size(size)));
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_random_data(size);

        group.bench_with_input(
            BenchmarkId::new("deflate_level_6", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer = StreamingZipWriter::with_compression(temp.path(), 6).unwrap();
                    writer.start_entry("random.bin").unwrap();
                    writer.write_data(black_box(data)).unwrap();
                    writer.finish().unwrap();
                });
            },
        );

        #[cfg(feature = "zstd-support")]
        group.bench_with_input(BenchmarkId::new("zstd_level_3", size), &data, |b, data| {
            b.iter(|| {
                let temp = NamedTempFile::new().unwrap();
                let mut writer =
                    StreamingZipWriter::with_method(temp.path(), CompressionMethod::Zstd, 3)
                        .unwrap();
                writer.start_entry("random.bin").unwrap();
                writer.write_data(black_box(data)).unwrap();
                writer.finish().unwrap();
            });
        });

        group.finish();
    }
}

fn bench_multiple_entries(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_multiple_entries");

    let entry_count = 100;
    let entry_size = 10 * 1024; // 10KB per entry
    group.throughput(Throughput::Bytes((entry_count * entry_size) as u64));

    let data = generate_compressible_data(entry_size);

    group.bench_function("deflate_100_entries", |b| {
        b.iter(|| {
            let temp = NamedTempFile::new().unwrap();
            let mut writer = StreamingZipWriter::with_compression(temp.path(), 6).unwrap();
            for i in 0..entry_count {
                writer.start_entry(&format!("file_{}.txt", i)).unwrap();
                writer.write_data(black_box(&data)).unwrap();
            }
            writer.finish().unwrap();
        });
    });

    #[cfg(feature = "zstd-support")]
    group.bench_function("zstd_100_entries", |b| {
        b.iter(|| {
            let temp = NamedTempFile::new().unwrap();
            let mut writer =
                StreamingZipWriter::with_method(temp.path(), CompressionMethod::Zstd, 3).unwrap();
            for i in 0..entry_count {
                writer.start_entry(&format!("file_{}.txt", i)).unwrap();
                writer.write_data(black_box(&data)).unwrap();
            }
            writer.finish().unwrap();
        });
    });

    group.finish();
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}MB", bytes / (1024 * 1024))
    }
}

criterion_group!(
    benches,
    bench_compression_methods,
    bench_random_data_compression,
    bench_multiple_entries
);
criterion_main!(benches);
