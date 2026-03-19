/// Peak memory test for encryption multi-chunk write path.
///
/// Writes a 50MB entry in 5×10MB chunks with AES-256 encryption enabled.
/// Expected: peak RSS stays bounded (~20-30MB), not proportional to 50MB input.
///
/// Run with: cargo run --example memory_test_encryption --features encryption --release
#[cfg(feature = "encryption")]
fn main() {
    use s_zip::StreamingZipWriter;
    use std::io::Cursor;

    fn vmrss_mb() -> f64 {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("VmRSS:"))
                    .map(|l| l.to_string())
            })
            .and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse::<f64>().ok())
            })
            .unwrap_or(0.0)
            / 1024.0
    }

    println!("=== Encryption Multi-Chunk Memory Test ===");
    println!("Writing 5 × 10MB chunks (50MB total) with AES-256 encryption");
    println!("Baseline RSS:  {:.1} MB", vmrss_mb());

    // 10MB chunk — same pattern as real large-file writes
    let chunk = vec![0xAAu8; 10 * 1024 * 1024];

    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);
    let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
    writer.set_password("perf_test_password_123");
    writer.start_entry("large_encrypted.bin").unwrap();

    let mut peak = 0.0f64;
    for i in 0..5 {
        writer.write_data(&chunk).unwrap();
        let rss = vmrss_mb();
        peak = peak.max(rss);
        println!("  After chunk {:1}: {:5.1} MB RSS", i + 1, rss);
    }

    let result = writer.finish().unwrap();
    let zip_bytes = result.into_inner().len();
    let final_rss = vmrss_mb();
    peak = peak.max(final_rss);

    println!();
    println!("Final RSS:     {:.1} MB", final_rss);
    println!("Peak RSS:      {:.1} MB", peak);
    println!(
        "ZIP output:    {:.2} MB ({} bytes)",
        zip_bytes as f64 / 1024.0 / 1024.0,
        zip_bytes
    );
    println!();

    // Sanity check: peak should be well under 50MB (the uncompressed input size)
    if peak < 50.0 {
        println!("PASS: peak memory ({:.1} MB) < 50 MB input size", peak);
    } else {
        println!(
            "WARN: peak memory ({:.1} MB) >= 50 MB — check for buffering issues",
            peak
        );
    }
}

#[cfg(not(feature = "encryption"))]
fn main() {
    eprintln!("This example requires the 'encryption' feature.");
    eprintln!(
        "Run with: cargo run --example memory_test_encryption --features encryption --release"
    );
}
