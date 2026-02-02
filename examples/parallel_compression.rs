/// Example demonstrating parallel compression with bounded memory usage
///
/// Compresses multiple files simultaneously while maintaining memory constraints
/// through semaphore-based concurrency limiting.
///
/// Run with: cargo run --example parallel_compression --features async
use s_zip::{AsyncStreamingZipWriter, ParallelConfig, ParallelEntry};
use std::io::Write;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Parallel Compression Demo ===\n");

    // Create test files
    let temp_dir = std::env::temp_dir().join("s_zip_parallel_test");
    std::fs::create_dir_all(&temp_dir)?;
    println!("Creating test files in: {:?}", temp_dir);

    let mut test_files = Vec::new();

    // Create 12 test files of different sizes
    for i in 0..12 {
        let file_path = temp_dir.join(format!("test_file_{}.txt", i));
        let size = match i % 3 {
            0 => 1_000_000,  // 1MB
            1 => 5_000_000,  // 5MB
            _ => 10_000_000, // 10MB
        };

        let mut file = std::fs::File::create(&file_path)?;
        let data = format!("Test data for file {} ", i).repeat(size / 20);
        file.write_all(data.as_bytes())?;

        test_files.push((file_path.clone(), format!("file_{}.txt", i)));
        println!(
            "  Created: {} ({:.1} MB)",
            file_path.display(),
            size as f64 / 1_000_000.0
        );
    }

    println!("\nTotal test data: ~66 MB\n");

    // Test different configurations
    let configs = vec![
        (
            "Sequential (1 thread)",
            ParallelConfig::default().with_max_concurrent(1),
        ),
        ("Conservative (2 threads)", ParallelConfig::conservative()),
        ("Balanced (4 threads)", ParallelConfig::balanced()),
        ("Aggressive (8 threads)", ParallelConfig::aggressive()),
    ];

    for (name, config) in configs {
        println!("--- {} ---", name);
        println!("Max concurrent: {}", config.max_concurrent);
        println!(
            "Estimated peak memory: ~{} MB",
            config.estimated_peak_memory_mb()
        );

        let output_path = temp_dir.join(format!("output_{}.zip", config.max_concurrent));

        let start = Instant::now();

        let mut writer = AsyncStreamingZipWriter::new(&output_path).await?;

        let entries: Vec<ParallelEntry> = test_files
            .iter()
            .map(|(path, name)| ParallelEntry::new(name.clone(), path.clone()))
            .collect();

        writer.write_entries_parallel(entries, config).await?;
        writer.finish().await?;

        let elapsed = start.elapsed();
        let output_size = std::fs::metadata(&output_path)?.len();

        println!("Time: {:.2} seconds", elapsed.as_secs_f64());
        println!("Output size: {:.2} MB", output_size as f64 / 1_000_000.0);
        println!("Throughput: {:.1} MB/s\n", 66.0 / elapsed.as_secs_f64());
    }

    // Demonstrate memory-safe usage patterns
    println!("=== Memory-Safe Usage Patterns ===\n");

    println!("1. Low-memory systems (< 1GB available):");
    println!("   let config = ParallelConfig::conservative(); // 2 threads, ~8MB");

    println!("\n2. Normal systems (2-8GB):");
    println!("   let config = ParallelConfig::balanced(); // 4 threads, ~16MB");

    println!("\n3. High-performance systems (16GB+):");
    println!("   let config = ParallelConfig::aggressive(); // 8 threads, ~32MB");

    println!("\n4. Custom configuration:");
    println!("   let config = ParallelConfig::default()");
    println!("       .with_max_concurrent(6)");
    println!("       .with_compression_level(9);");

    println!("\n=== Performance Analysis ===\n");
    println!("Expected speedup vs sequential:");
    println!("  2 threads: 1.5-1.8x");
    println!("  4 threads: 2.5-3.5x");
    println!("  8 threads: 3.5-5.0x");
    println!("\nDiminishing returns beyond 8 threads due to I/O bottleneck");

    println!("\n=== Memory Guarantees ===\n");
    println!("Peak memory formula: max_concurrent × ~4MB");
    println!("Memory is bounded by semaphore - prevents spikes");
    println!("Files are streamed from disk, not pre-loaded");
    println!("Compressed data is written immediately");

    // Cleanup
    println!("\nCleaning up test files...");
    std::fs::remove_dir_all(&temp_dir)?;

    println!("\n✅ Demo complete!");

    Ok(())
}
