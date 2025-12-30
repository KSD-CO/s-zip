use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use s_zip::{AsyncStreamingZipWriter, StreamingZipWriter};
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

fn bench_async_vs_sync(c: &mut Criterion) {
    let sizes = vec![
        10 * 1024,       // 10KB
        100 * 1024,      // 100KB
        1024 * 1024,     // 1MB
        5 * 1024 * 1024, // 5MB
    ];

    for size in sizes {
        let mut group = c.benchmark_group(format!("async_vs_sync_{}", format_size(size)));
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_compressible_data(size);

        // Sync version
        group.bench_with_input(BenchmarkId::new("sync", size), &data, |b, data| {
            b.iter(|| {
                let temp = NamedTempFile::new().unwrap();
                let mut writer = StreamingZipWriter::with_compression(temp.path(), 6).unwrap();
                writer.start_entry("test.bin").unwrap();
                writer.write_data(black_box(data)).unwrap();
                writer.finish().unwrap();
            });
        });

        // Async version
        group.bench_with_input(BenchmarkId::new("async", size), &data, |b, data| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            b.iter(|| {
                rt.block_on(async {
                    let temp = NamedTempFile::new().unwrap();
                    let mut writer = AsyncStreamingZipWriter::with_compression(temp.path(), 6)
                        .await
                        .unwrap();
                    writer.start_entry("test.bin").await.unwrap();
                    writer.write_data(black_box(data)).await.unwrap();
                    writer.finish().await.unwrap();
                })
            });
        });

        group.finish();
    }
}

fn bench_async_multiple_entries(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_multiple_entries");

    let entry_count = 50;
    let entry_size = 10 * 1024; // 10KB per entry
    group.throughput(Throughput::Bytes((entry_count * entry_size) as u64));

    let data = generate_compressible_data(entry_size);

    // Sync version
    group.bench_function("sync_50_entries", |b| {
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

    // Async version
    group.bench_function("async_50_entries", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let temp = NamedTempFile::new().unwrap();
                let mut writer = AsyncStreamingZipWriter::with_compression(temp.path(), 6)
                    .await
                    .unwrap();
                for i in 0..entry_count {
                    writer
                        .start_entry(&format!("file_{}.txt", i))
                        .await
                        .unwrap();
                    writer.write_data(black_box(&data)).await.unwrap();
                }
                writer.finish().await.unwrap();
            })
        });
    });

    group.finish();
}

fn bench_async_in_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("in_memory_operations");

    let size = 100 * 1024; // 100KB
    group.throughput(Throughput::Bytes(size as u64));

    let data = generate_compressible_data(size);

    // Sync version with Vec
    group.bench_with_input(BenchmarkId::new("sync_memory", size), &data, |b, data| {
        b.iter(|| {
            let buffer = Vec::new();
            let cursor = std::io::Cursor::new(buffer);
            let mut writer = StreamingZipWriter::from_writer_with_compression(cursor, 6).unwrap();
            writer.start_entry("test.bin").unwrap();
            writer.write_data(black_box(data)).unwrap();
            let cursor = writer.finish().unwrap();
            black_box(cursor.into_inner());
        });
    });

    // Async version with Vec
    group.bench_with_input(BenchmarkId::new("async_memory", size), &data, |b, data| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let buffer = Vec::new();
                let cursor = std::io::Cursor::new(buffer);
                let mut writer = AsyncStreamingZipWriter::from_writer_with_compression(cursor, 6);
                writer.start_entry("test.bin").await.unwrap();
                writer.write_data(black_box(data)).await.unwrap();
                let cursor = writer.finish().await.unwrap();
                black_box(cursor.into_inner());
            })
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
    bench_async_vs_sync,
    bench_async_multiple_entries,
    bench_async_in_memory
);
criterion_main!(benches);
