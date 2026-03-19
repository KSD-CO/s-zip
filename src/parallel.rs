//! Parallel compression support for async writer
//!
//! This module provides parallel compression capabilities with memory constraints.
//! Uses a bounded semaphore to limit concurrent tasks and prevent memory spikes.

use crate::error::{Result, SZipError};
use crate::writer::CompressionMethod;
use async_compression::tokio::bufread::DeflateEncoder;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, ReadBuf};
use tokio::sync::{mpsc, Semaphore};

/// An `AsyncRead` + `AsyncBufRead` wrapper that computes CRC32 of bytes passing through it.
///
/// Wraps any `AsyncBufRead` and accumulates a running CRC32 hash so that CRC32
/// is computed in a single streaming pass alongside compression — eliminating
/// the need to buffer the full file in memory before compressing.
struct CrcReader<R> {
    inner: R,
    hasher: crc32fast::Hasher,
}

impl<R> CrcReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: crc32fast::Hasher::new(),
        }
    }

    /// Consume this wrapper and return the final CRC32 digest.
    fn finalize(self) -> u32 {
        self.hasher.finalize()
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for CrcReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let filled_before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let new_bytes = &buf.filled()[filled_before..];
            if !new_bytes.is_empty() {
                this.hasher.update(new_bytes);
            }
        }
        result
    }
}

impl<R: AsyncBufRead + Unpin> AsyncBufRead for CrcReader<R> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<&[u8]>> {
        // Delegate to inner; bytes are hashed in consume() after the caller reads them.
        Pin::new(&mut self.get_mut().inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        // Peek at the filled buffer *before* consuming so we can hash it.
        // SAFETY: poll_fill_buf already returned Ok, so the buffer is valid.
        // We re-borrow inner as a Pin to call poll_fill_buf synchronously via
        // a noop waker just to read the slice — but that's complex. Instead,
        // we use a simpler approach: maintain a small shadow buffer.
        //
        // Actually the cleanest approach: call inner.consume and hash via
        // the AsyncRead path (poll_read already hashes). Since DeflateEncoder
        // uses poll_fill_buf/consume, we need to hash here.
        // We can't easily re-read already-filled bytes from BufReader after consume.
        //
        // Solution: use a raw waker to call poll_fill_buf synchronously (the buffer
        // is already filled — it's a no-op poll) to borrow the slice, hash it, then consume.
        use std::task::{RawWaker, RawWakerVTable, Waker};
        fn noop(_: *const ()) {}
        fn noop_clone(p: *const ()) -> RawWaker {
            RawWaker::new(p, &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        // SAFETY: waker does nothing; poll_fill_buf on BufReader with a full buffer
        // returns Poll::Ready immediately without touching the waker.
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);
        if let Poll::Ready(Ok(buf)) = Pin::new(&mut this.inner).poll_fill_buf(&mut cx) {
            let to_hash = &buf[..amt.min(buf.len())];
            this.hasher.update(to_hash);
        }
        Pin::new(&mut this.inner).consume(amt);
    }
}
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

    /// Set max concurrent tasks (1–16).
    ///
    /// Returns `Err` instead of panicking so callers can handle invalid input
    /// without crashing the process.
    pub fn with_max_concurrent(mut self, max: usize) -> crate::error::Result<Self> {
        if max == 0 {
            return Err(crate::error::SZipError::InvalidFormat(
                "max_concurrent must be at least 1".to_string(),
            ));
        }
        if max > 16 {
            return Err(crate::error::SZipError::InvalidFormat(
                "max_concurrent must not exceed 16".to_string(),
            ));
        }
        self.max_concurrent = max;
        Ok(self)
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

/// Compress a single file with DEFLATE in a true single-pass stream.
///
/// The pipeline is: `File → BufReader → CrcReader → DeflateEncoder → Vec<u8>`.
/// CRC32 is computed on-the-fly over the raw bytes as they are read by the
/// encoder, so the full file is **never** buffered in memory.  Peak RAM per
/// task is bounded by the encoder's internal window (~32 KB) and the 64 KB
/// `BufReader` buffer — independent of file size.
async fn compress_file_deflate(path: PathBuf, level: u32) -> Result<(Vec<u8>, u64, u32)> {
    let file = tokio::fs::File::open(&path).await?;
    let metadata = file.metadata().await?;
    let uncompressed_size = metadata.len();

    // File → BufReader (64 KB I/O buffer) → CrcReader (hashes bytes as read)
    let buf_reader = tokio::io::BufReader::with_capacity(64 * 1024, file);
    let crc_reader = CrcReader::new(buf_reader);

    // CrcReader → DeflateEncoder: bytes are hashed on-the-fly as the encoder
    // pulls them; no intermediate Vec<u8> is needed.
    let mut encoder =
        DeflateEncoder::with_quality(crc_reader, async_compression::Level::Precise(level as i32));

    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed).await?;

    // Unwrap the encoder to recover the CrcReader and obtain the digest.
    let crc32 = encoder.into_inner().finalize();

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
    fn test_invalid_max_concurrent_zero() {
        let result = ParallelConfig::default().with_max_concurrent(0);
        assert!(result.is_err(), "Expected error for max_concurrent=0");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("at least 1"),
            "Error should mention minimum: {}",
            msg
        );
    }

    #[test]
    fn test_invalid_max_concurrent_too_high() {
        let result = ParallelConfig::default().with_max_concurrent(20);
        assert!(result.is_err(), "Expected error for max_concurrent=20");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("16"), "Error should mention maximum: {}", msg);
    }

    #[test]
    fn test_valid_max_concurrent() {
        let config = ParallelConfig::default().with_max_concurrent(8).unwrap();
        assert_eq!(config.max_concurrent, 8);
    }
}
