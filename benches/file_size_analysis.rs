use s_zip::{CompressionMethod, StreamingZipWriter};
use std::fs;
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

fn test_compression(
    name: &str,
    data: &[u8],
    method_name: &str,
    create_writer: impl FnOnce(&std::path::Path) -> s_zip::Result<StreamingZipWriter>,
) {
    let temp = NamedTempFile::new().unwrap();
    let path = temp.path();

    let mut writer = create_writer(path).unwrap();
    writer.start_entry("test.bin").unwrap();
    writer.write_data(data).unwrap();
    writer.finish().unwrap();

    let compressed_size = fs::metadata(path).unwrap().len();
    let original_size = data.len() as u64;
    let ratio = (compressed_size as f64 / original_size as f64) * 100.0;

    println!(
        "{:<20} | {:<15} | {:>12} | {:>12} | {:>8.2}%",
        name,
        method_name,
        format_bytes(original_size),
        format_bytes(compressed_size),
        ratio
    );
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn main() {
    println!("\n╔════════════════════════════════════════════════════════════════════════════╗");
    println!("║                    s-zip File Size Analysis                                ║");
    println!("╚════════════════════════════════════════════════════════════════════════════╝\n");

    println!(
        "{:<20} | {:<15} | {:>12} | {:>12} | {:>8}",
        "Data Type", "Method", "Original", "Compressed", "Ratio"
    );
    println!(
        "{:-<20}-+-{:-<15}-+-{:->12}-+-{:->12}-+-{:->8}",
        "", "", "", "", ""
    );

    // Test 1MB compressible data
    let compressible_1mb = generate_compressible_data(1024 * 1024);

    test_compression(
        "Compressible 1MB",
        &compressible_1mb,
        "DEFLATE lvl 1",
        |p| StreamingZipWriter::with_compression(p, 1),
    );
    test_compression(
        "Compressible 1MB",
        &compressible_1mb,
        "DEFLATE lvl 6",
        |p| StreamingZipWriter::with_compression(p, 6),
    );
    test_compression(
        "Compressible 1MB",
        &compressible_1mb,
        "DEFLATE lvl 9",
        |p| StreamingZipWriter::with_compression(p, 9),
    );

    #[cfg(feature = "zstd-support")]
    {
        test_compression("Compressible 1MB", &compressible_1mb, "Zstd lvl 1", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 1)
        });
        test_compression("Compressible 1MB", &compressible_1mb, "Zstd lvl 3", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 3)
        });
        test_compression("Compressible 1MB", &compressible_1mb, "Zstd lvl 10", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 10)
        });
        test_compression("Compressible 1MB", &compressible_1mb, "Zstd lvl 21", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 21)
        });
    }

    println!();

    // Test 1MB random data
    let random_1mb = generate_random_data(1024 * 1024);

    test_compression("Random 1MB", &random_1mb, "DEFLATE lvl 6", |p| {
        StreamingZipWriter::with_compression(p, 6)
    });
    test_compression("Random 1MB", &random_1mb, "DEFLATE lvl 9", |p| {
        StreamingZipWriter::with_compression(p, 9)
    });

    #[cfg(feature = "zstd-support")]
    {
        test_compression("Random 1MB", &random_1mb, "Zstd lvl 3", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 3)
        });
        test_compression("Random 1MB", &random_1mb, "Zstd lvl 10", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 10)
        });
    }

    println!();

    // Test 10MB compressible data
    let compressible_10mb = generate_compressible_data(10 * 1024 * 1024);

    test_compression(
        "Compressible 10MB",
        &compressible_10mb,
        "DEFLATE lvl 6",
        |p| StreamingZipWriter::with_compression(p, 6),
    );
    test_compression(
        "Compressible 10MB",
        &compressible_10mb,
        "DEFLATE lvl 9",
        |p| StreamingZipWriter::with_compression(p, 9),
    );

    #[cfg(feature = "zstd-support")]
    {
        test_compression("Compressible 10MB", &compressible_10mb, "Zstd lvl 3", |p| {
            StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 3)
        });
        test_compression(
            "Compressible 10MB",
            &compressible_10mb,
            "Zstd lvl 10",
            |p| StreamingZipWriter::with_method(p, CompressionMethod::Zstd, 10),
        );
    }

    println!("\n");
}
