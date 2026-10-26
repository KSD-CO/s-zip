//! Decompression limits that protect readers against ZIP bombs.
//!
//! Limits are enforced on the bytes actually produced by the decompressor, not
//! on the sizes an archive declares in its headers, so a crafted archive that
//! lies about `uncompressed_size` is still caught.
//!
//! ```no_run
//! use s_zip::{ExtractLimits, StreamingZipReader};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut reader = StreamingZipReader::open("upload.zip")?;
//! reader.set_limits(ExtractLimits::strict());
//! reader.extract_all("./out")?; // fails with SZipError::LimitExceeded on a ZIP bomb
//! # Ok(())
//! # }
//! ```

use crate::error::{Result, SZipError};
use crate::format::{ZipEntry, MAX_ENTRY_ALLOC};
use std::io;

/// Output size below which `max_ratio` is not enforced, so tiny highly
/// compressible files (e.g. a 4 KB file of spaces) are never rejected.
pub const RATIO_GRACE_BYTES: u64 = 1024 * 1024;

/// Limits applied when decompressing entries.
///
/// The default only caps in-memory reads (`read_entry`) at 2 GiB per entry.
/// Use [`ExtractLimits::strict`] (or set the fields yourself) when reading
/// archives from untrusted sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    /// Maximum decompressed size of a single entry.
    pub max_entry_size: Option<u64>,
    /// Maximum decompressed size of all entries combined (used by `extract_all`).
    pub max_total_size: Option<u64>,
    /// Maximum decompressed/compressed size ratio, enforced once an entry's
    /// output exceeds [`RATIO_GRACE_BYTES`].
    pub max_ratio: Option<u64>,
    /// Maximum number of entries an archive may contain (used by `extract_all`).
    pub max_entries: Option<usize>,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl ExtractLimits {
    /// No limits besides the 2 GiB cap on in-memory reads.
    pub const fn unlimited() -> Self {
        Self {
            max_entry_size: None,
            max_total_size: None,
            max_ratio: None,
            max_entries: None,
        }
    }

    /// Conservative limits for untrusted archives: 1 GiB per entry, 4 GiB in
    /// total, 1000:1 compression ratio, 100 000 entries.
    pub const fn strict() -> Self {
        Self {
            max_entry_size: Some(1024 * 1024 * 1024),
            max_total_size: Some(4 * 1024 * 1024 * 1024),
            max_ratio: Some(1000),
            max_entries: Some(100_000),
        }
    }

    /// Fail early if the archive has more entries than allowed.
    pub(crate) fn check_entry_count(&self, count: usize) -> Result<()> {
        match self.max_entries {
            Some(max) if count > max => Err(SZipError::LimitExceeded(format!(
                "archive has {} entries, limit is {}",
                count, max
            ))),
            _ => Ok(()),
        }
    }

    /// Build the output budget for one entry. `remaining_total` is the unused
    /// part of `max_total_size` when extracting a whole archive.
    pub(crate) fn budget(
        &self,
        entry: &ZipEntry,
        in_memory: bool,
        remaining_total: Option<u64>,
    ) -> Result<OutputBudget> {
        let mut max = self.max_entry_size;
        if in_memory {
            max = Some(max.map_or(MAX_ENTRY_ALLOC, |m| m.min(MAX_ENTRY_ALLOC)));
        }
        if let Some(rem) = remaining_total {
            max = Some(max.map_or(rem, |m| m.min(rem)));
        }
        let ratio_max = self.max_ratio.map(|r| {
            entry
                .compressed_size
                .saturating_mul(r)
                .max(RATIO_GRACE_BYTES)
        });

        let budget = OutputBudget {
            name: entry.name.clone(),
            max,
            ratio_max,
        };
        // Reject early when the declared size already exceeds the budget
        budget.check(entry.uncompressed_size)?;
        Ok(budget)
    }
}

/// Byte budget for a single entry's decompressed output.
#[derive(Debug, Clone)]
pub(crate) struct OutputBudget {
    name: String,
    max: Option<u64>,
    ratio_max: Option<u64>,
}

impl OutputBudget {
    pub(crate) fn is_unlimited(&self) -> bool {
        self.max.is_none() && self.ratio_max.is_none()
    }

    pub(crate) fn check(&self, produced: u64) -> Result<()> {
        if let Some(max) = self.max {
            if produced > max {
                return Err(SZipError::LimitExceeded(format!(
                    "entry '{}' decompresses to more than {} bytes",
                    self.name, max
                )));
            }
        }
        if let Some(ratio_max) = self.ratio_max {
            if produced > ratio_max {
                return Err(SZipError::LimitExceeded(format!(
                    "entry '{}' exceeds the maximum compression ratio",
                    self.name
                )));
            }
        }
        Ok(())
    }

    fn io_check(&self, produced: u64) -> io::Result<()> {
        self.check(produced)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// `Read` adapter that fails once the inner reader produces more than the budget.
pub(crate) struct LimitedRead<R> {
    inner: R,
    produced: u64,
    budget: OutputBudget,
}

impl<R> LimitedRead<R> {
    pub(crate) fn new(inner: R, budget: OutputBudget) -> Self {
        Self {
            inner,
            produced: 0,
            budget,
        }
    }
}

impl<R: io::Read> io::Read for LimitedRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.produced += n as u64;
        self.budget.io_check(self.produced)?;
        Ok(n)
    }
}

#[cfg(feature = "async")]
impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for LimitedRead<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        match std::pin::Pin::new(&mut this.inner).poll_read(cx, buf) {
            std::task::Poll::Ready(Ok(())) => {
                this.produced += (buf.filled().len() - before) as u64;
                std::task::Poll::Ready(this.budget.io_check(this.produced))
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn entry(compressed: u64, uncompressed: u64) -> ZipEntry {
        ZipEntry {
            name: "bomb.bin".to_string(),
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            compression_method: 8,
            offset: 0,
            crc32: 0,
            is_encrypted: false,
        }
    }

    #[test]
    fn test_declared_size_rejected_early() {
        let limits = ExtractLimits {
            max_entry_size: Some(100),
            ..ExtractLimits::unlimited()
        };
        let err = limits.budget(&entry(10, 1000), false, None).unwrap_err();
        assert!(matches!(err, SZipError::LimitExceeded(_)));
    }

    #[test]
    fn test_actual_output_enforced_when_header_lies() {
        let limits = ExtractLimits {
            max_entry_size: Some(100),
            ..ExtractLimits::unlimited()
        };
        // Header claims 10 bytes, data is 1000 bytes
        let budget = limits.budget(&entry(10, 10), false, None).unwrap();
        let data = vec![0u8; 1000];
        let mut out = Vec::new();
        let err = LimitedRead::new(&data[..], budget)
            .read_to_end(&mut out)
            .unwrap_err();
        assert!(matches!(SZipError::from(err), SZipError::LimitExceeded(_)));
    }

    #[test]
    fn test_ratio_grace_for_small_entries() {
        let limits = ExtractLimits {
            max_ratio: Some(10),
            ..ExtractLimits::unlimited()
        };
        // 1 byte compressed -> 4 KB output is fine (below grace threshold)
        let budget = limits.budget(&entry(1, 4096), false, None).unwrap();
        assert!(budget.check(4096).is_ok());
        assert!(budget.check(RATIO_GRACE_BYTES + 1).is_err());
    }

    #[test]
    fn test_in_memory_cap_always_applies() {
        let budget = ExtractLimits::unlimited()
            .budget(&entry(10, 10), true, None)
            .unwrap();
        assert!(!budget.is_unlimited());
        assert!(budget.check(MAX_ENTRY_ALLOC + 1).is_err());
    }
}
