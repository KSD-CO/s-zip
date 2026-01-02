use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
#[cfg(feature = "zstd-support")]
use s_zip::CompressionMethod;
use s_zip::{StreamingZipReader, StreamingZipWriter};
use std::io::Read;
use tempfile::NamedTempFile;

fn generate_compressible_data(size: usize) -> Vec<u8> {
    let pattern = b"The quick brown fox jumps over the lazy dog. ";
    let mut data = Vec::with_capacity(size);
    while data.len() < size {
        data.extend_from_slice(pattern);
    }
    data.truncate(size);
    data
}

fn generate_random_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let mut state = 0x12345678u32;
    for _ in 0..size {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        data.push((state >> 16) as u8);
    }
    data
}

fn create_test_zip_deflate(data: &[u8], level: u32) -> NamedTempFile {
    let temp = NamedTempFile::new().unwrap();
    let mut writer = StreamingZipWriter::with_compression(temp.path(), level).unwrap();
    writer.start_entry("test.bin").unwrap();
    writer.write_data(data).unwrap();
    writer.finish().unwrap();
    temp
}

#[cfg(feature = "zstd-support")]
fn create_test_zip_zstd(data: &[u8], level: u32) -> NamedTempFile {
    let temp = NamedTempFile::new().unwrap();
    let mut writer =
        StreamingZipWriter::with_method(temp.path(), CompressionMethod::Zstd, level).unwrap();
    writer.start_entry("test.bin").unwrap();
    writer.write_data(data).unwrap();
    writer.finish().unwrap();
    temp
}

fn bench_read_compressible_data(c: &mut Criterion) {
    let sizes = vec![
        100 * 1024,       // 100KB
        1024 * 1024,      // 1MB
        10 * 1024 * 1024, // 10MB
    ];

    for size in sizes {
        let mut group = c.benchmark_group(format!("read_compressible_{}", format_size(size)));
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_compressible_data(size);

        // Benchmark reading DEFLATE compressed data
        let zip_deflate = create_test_zip_deflate(&data, 6);
        group.bench_function(BenchmarkId::new("deflate_level_6", size), |b| {
            b.iter(|| {
                let mut reader = StreamingZipReader::open(zip_deflate.path()).unwrap();
                let entries: Vec<_> = reader.entries().to_vec();
                let mut buf = Vec::new();
                for entry in entries {
                    reader
                        .read_entry_streaming(&entry)
                        .unwrap()
                        .read_to_end(black_box(&mut buf))
                        .unwrap();
                    buf.clear();
                }
            });
        });

        // Benchmark reading Zstd compressed data
        #[cfg(feature = "zstd-support")]
        {
            let zip_zstd = create_test_zip_zstd(&data, 3);
            group.bench_function(BenchmarkId::new("zstd_level_3", size), |b| {
                b.iter(|| {
                    let mut reader = StreamingZipReader::open(zip_zstd.path()).unwrap();
                    let entries: Vec<_> = reader.entries().to_vec();
                    let mut buf = Vec::new();
                    for entry in entries {
                        reader
                            .read_entry_streaming(&entry)
                            .unwrap()
                            .read_to_end(black_box(&mut buf))
                            .unwrap();
                        buf.clear();
                    }
                });
            });
        }

        group.finish();
    }
}

fn bench_read_random_data(c: &mut Criterion) {
    let sizes = vec![100 * 1024, 1024 * 1024]; // 100KB, 1MB

    for size in sizes {
        let mut group = c.benchmark_group(format!("read_random_{}", format_size(size)));
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_random_data(size);

        let zip_deflate = create_test_zip_deflate(&data, 6);
        group.bench_function(BenchmarkId::new("deflate_level_6", size), |b| {
            b.iter(|| {
                let mut reader = StreamingZipReader::open(zip_deflate.path()).unwrap();
                let entries: Vec<_> = reader.entries().to_vec();
                let mut buf = Vec::new();
                for entry in entries {
                    reader
                        .read_entry_streaming(&entry)
                        .unwrap()
                        .read_to_end(black_box(&mut buf))
                        .unwrap();
                    buf.clear();
                }
            });
        });

        #[cfg(feature = "zstd-support")]
        {
            let zip_zstd = create_test_zip_zstd(&data, 3);
            group.bench_function(BenchmarkId::new("zstd_level_3", size), |b| {
                b.iter(|| {
                    let mut reader = StreamingZipReader::open(zip_zstd.path()).unwrap();
                    let entries: Vec<_> = reader.entries().to_vec();
                    let mut buf = Vec::new();
                    for entry in entries {
                        reader
                            .read_entry_streaming(&entry)
                            .unwrap()
                            .read_to_end(black_box(&mut buf))
                            .unwrap();
                        buf.clear();
                    }
                });
            });
        }

        group.finish();
    }
}

fn bench_read_multiple_entries(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_multiple_entries");

    let entry_count = 100;
    let entry_size = 10 * 1024; // 10KB per entry
    group.throughput(Throughput::Bytes((entry_count * entry_size) as u64));

    let data = generate_compressible_data(entry_size);

    // Create test ZIP with multiple entries
    let temp_deflate = NamedTempFile::new().unwrap();
    let mut writer = StreamingZipWriter::with_compression(temp_deflate.path(), 6).unwrap();
    for i in 0..entry_count {
        writer.start_entry(&format!("file_{}.txt", i)).unwrap();
        writer.write_data(&data).unwrap();
    }
    writer.finish().unwrap();

    group.bench_function("deflate_100_entries", |b| {
        b.iter(|| {
            let mut reader = StreamingZipReader::open(temp_deflate.path()).unwrap();
            let entries: Vec<_> = reader.entries().to_vec();
            let mut buf = Vec::new();
            for entry in entries {
                reader
                    .read_entry_streaming(&entry)
                    .unwrap()
                    .read_to_end(black_box(&mut buf))
                    .unwrap();
                buf.clear();
            }
        });
    });

    #[cfg(feature = "zstd-support")]
    {
        let temp_zstd = NamedTempFile::new().unwrap();
        let mut writer =
            StreamingZipWriter::with_method(temp_zstd.path(), CompressionMethod::Zstd, 3).unwrap();
        for i in 0..entry_count {
            writer.start_entry(&format!("file_{}.txt", i)).unwrap();
            writer.write_data(&data).unwrap();
        }
        writer.finish().unwrap();

        group.bench_function("zstd_100_entries", |b| {
            b.iter(|| {
                let mut reader = StreamingZipReader::open(temp_zstd.path()).unwrap();
                let entries: Vec<_> = reader.entries().to_vec();
                let mut buf = Vec::new();
                for entry in entries {
                    reader
                        .read_entry_streaming(&entry)
                        .unwrap()
                        .read_to_end(black_box(&mut buf))
                        .unwrap();
                    buf.clear();
                }
            });
        });
    }

    group.finish();
}

fn bench_read_streaming_vs_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_method_comparison");

    let size = 1024 * 1024; // 1MB
    let data = generate_compressible_data(size);
    group.throughput(Throughput::Bytes(size as u64));

    let zip_file = create_test_zip_deflate(&data, 6);

    group.bench_function("streaming_read", |b| {
        b.iter(|| {
            let mut reader = StreamingZipReader::open(zip_file.path()).unwrap();
            let entries: Vec<_> = reader.entries().to_vec();
            let mut buf = Vec::new();
            for entry in entries {
                reader
                    .read_entry_streaming(&entry)
                    .unwrap()
                    .read_to_end(black_box(&mut buf))
                    .unwrap();
                buf.clear();
            }
        });
    });

    group.bench_function("full_read", |b| {
        b.iter(|| {
            let mut reader = StreamingZipReader::open(zip_file.path()).unwrap();
            let entries: Vec<_> = reader.entries().to_vec();
            for entry in entries {
                let _ = black_box(reader.read_entry(&entry).unwrap());
            }
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
    bench_read_compressible_data,
    bench_read_random_data,
    bench_read_multiple_entries,
    bench_read_streaming_vs_full
);
criterion_main!(benches);
