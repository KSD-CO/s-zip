//! Async read throughput and archive open time with many entries.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use s_zip::{AsyncStreamingZipReader, StreamingZipWriter};
use tokio::io::AsyncReadExt;

fn data(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i * 7) % 251) as u8 ^ (i / 1024) as u8)
        .collect()
}

fn bench_async_read(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench.zip");
    let payload = data(10 * 1024 * 1024);
    let mut w = StreamingZipWriter::new(&path).unwrap();
    w.start_entry("data.bin").unwrap();
    w.write_data(&payload).unwrap();
    for i in 0..100 {
        w.start_entry(&format!("small_{i}.txt")).unwrap();
        w.write_data(&payload[..10 * 1024]).unwrap();
    }
    w.finish().unwrap();

    let mut group = c.benchmark_group("async_read_10mb");
    group.throughput(Throughput::Bytes(payload.len() as u64));
    group.bench_function("read_entry", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut r = AsyncStreamingZipReader::open(&path).await.unwrap();
                r.read_entry_by_name("data.bin").await.unwrap()
            })
        })
    });
    group.bench_function("streaming", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut r = AsyncStreamingZipReader::open(&path).await.unwrap();
                let mut s = r.read_entry_streaming_by_name("data.bin").await.unwrap();
                let mut out = Vec::with_capacity(payload.len());
                s.read_to_end(&mut out).await.unwrap();
                out
            })
        })
    });
    group.finish();

    let mut group = c.benchmark_group("async_read_100_small");
    group.bench_function("read_entry", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut r = AsyncStreamingZipReader::open(&path).await.unwrap();
                for i in 0..100 {
                    r.read_entry_by_name(&format!("small_{i}.txt"))
                        .await
                        .unwrap();
                }
            })
        })
    });
    group.finish();
}

fn bench_open_many_entries(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("many.zip");
    let mut w = StreamingZipWriter::new(&path).unwrap();
    for i in 0..20_000 {
        w.add_entry(&format!("dir/file_{i:05}.txt"), b"x").unwrap();
    }
    w.finish().unwrap();

    let mut group = c.benchmark_group("open_20k_entries");
    group.sample_size(10);
    group.bench_function("sync", |b| {
        b.iter(|| {
            s_zip::StreamingZipReader::open(&path)
                .unwrap()
                .entries()
                .len()
        })
    });
    group.bench_function("async", |b| {
        b.iter(|| {
            rt.block_on(async {
                AsyncStreamingZipReader::open(&path)
                    .await
                    .unwrap()
                    .entries()
                    .len()
            })
        })
    });
    group.finish();
}

criterion_group!(benches, bench_async_read, bench_open_many_entries);
criterion_main!(benches);
