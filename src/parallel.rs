//! Parallel compression support for async writer
//!
//! This module provides parallel compression capabilities with memory constraints.
//! Uses a bounded semaphore to limit concurrent tasks and prevent memory spikes.

use crate::error::{Result, SZipError};
use crate::writer::CompressionMethod;
use async_compression::tokio::bufread::DeflateEncoder;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Semaphore};

/// Configuration for parallel compression
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Maximum number of concurrent compression tasks (default: 4, max: 16)
    pub max_concurrent: usize,
    /// Compression level (default: 6)
    pub compression_level: u32,
    /// Compression method (default: Deflate)
    pub compression_method: CompressionMethod,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            compression_level: 6,
            compression_method: CompressionMethod::Deflate,
        }
    }
}

impl ParallelConfig {
    /// Create a conservative config for low-memory systems
    pub fn conservative() -> Self {
        Self {
            max_concurrent: 2,
            compression_level: 6,
            compression_method: CompressionMethod::Deflate,
        }
    }

    /// Create a balanced config for normal systems
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Create an aggressive config for high-memory systems
    pub fn aggressive() -> Self {
        Self {
            max_concurrent: 8,
            compression_level: 6,
            compression_method: CompressionMethod::Deflate,
        }
    }

    /// Set max concurrent tasks
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        assert!(max > 0, "max_concurrent must be at least 1");
        assert!(max <= 16, "max_concurrent should not exceed 16");
        self.max_concurrent = max;
        self
    }

    /// Set compression level
    pub fn with_compression_level(mut self, level: u32) -> Self {
        self.compression_level = level;
        self
    }

    /// Estimate peak memory usage in MB
    pub fn estimated_peak_memory_mb(&self) -> usize {
        // Each task uses approximately:
        // - Input buffer: varies by file size
        // - Compression buffer: ~2MB
        // - Output buffer: ~2MB
        // Conservative estimate: 4MB per task
        self.max_concurrent * 4
    }
}

/// A file entry to be compressed in parallel
pub struct ParallelEntry {
    /// Entry name in ZIP
    pub name: String,
    /// File path to read from
    pub path: PathBuf,
}

impl ParallelEntry {
    /// Create a new parallel entry
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }
}

/// Result of parallel compression
pub(crate) struct CompressedEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub uncompressed_size: u64,
    pub crc32: u32,
}

/// Compress a single file with DEFLATE
async fn compress_file_deflate(path: PathBuf, level: u32) -> Result<(Vec<u8>, u64, u32)> {
    // Read file
    let data = tokio::fs::read(&path).await?;

    let uncompressed_size = data.len() as u64;

    // Calculate CRC32
    let crc32 = crc32fast::hash(&data);

    // Compress
    let cursor = Cursor::new(data);
    let mut encoder =
        DeflateEncoder::with_quality(cursor, async_compression::Level::Precise(level as i32));

    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed).await?;

    Ok((compressed, uncompressed_size, crc32))
}

/// Compress multiple files in parallel with bounded concurrency
pub(crate) async fn compress_entries_parallel(
    entries: Vec<ParallelEntry>,
    config: ParallelConfig,
) -> Result<Vec<CompressedEntry>> {
    let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
    let (tx, mut rx) = mpsc::channel(config.max_concurrent);

    // Spawn compression tasks (bounded by semaphore)
    let handles: Vec<_> = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let semaphore = semaphore.clone();
            let tx = tx.clone();
            let config = config.clone();

            tokio::task::spawn(async move {
                // Acquire semaphore permit (blocks if max concurrent reached)
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|_e| SZipError::InvalidFormat("Semaphore error".to_string()))?;

                // Compress file
                let (compressed, uncompressed_size, crc32) = match config.compression_method {
                    CompressionMethod::Deflate => {
                        compress_file_deflate(entry.path, config.compression_level).await?
                    }
                    _ => {
                        return Err(SZipError::InvalidFormat(
                            "Only DEFLATE supported in parallel compression".to_string(),
                        ));
                    }
                };

                let result = CompressedEntry {
                    name: entry.name,
                    data: compressed,
                    uncompressed_size,
                    crc32,
                };

                // Send result with index to maintain order
                tx.send((index, result))
                    .await
                    .map_err(|_e| SZipError::InvalidFormat("Channel send error".to_string()))?;

                // Permit is automatically dropped here
                Ok::<_, SZipError>(())
            })
        })
        .collect();

    // Drop sender so receiver knows when all tasks are done
    drop(tx);

    // Collect results maintaining original order
    let mut results = Vec::new();
    while let Some((index, entry)) = rx.recv().await {
        results.push((index, entry));
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle
            .await
            .map_err(|_e| SZipError::InvalidFormat("Task join error".to_string()))??;
    }

    // Sort by original index and extract entries
    results.sort_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, entry)| entry).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ParallelConfig::default();
        assert_eq!(config.max_concurrent, 4);
        assert_eq!(config.compression_level, 6);
    }

    #[test]
    fn test_config_presets() {
        let conservative = ParallelConfig::conservative();
        assert_eq!(conservative.max_concurrent, 2);

        let aggressive = ParallelConfig::aggressive();
        assert_eq!(aggressive.max_concurrent, 8);
    }

    #[test]
    fn test_memory_estimation() {
        let config = ParallelConfig::balanced();
        let estimated = config.estimated_peak_memory_mb();
        assert_eq!(estimated, 16); // 4 concurrent × 4MB
    }

    #[test]
    #[should_panic(expected = "max_concurrent must be at least 1")]
    fn test_invalid_max_concurrent_zero() {
        ParallelConfig::default().with_max_concurrent(0);
    }

    #[test]
    #[should_panic(expected = "max_concurrent should not exceed 16")]
    fn test_invalid_max_concurrent_too_high() {
        ParallelConfig::default().with_max_concurrent(20);
    }
}
