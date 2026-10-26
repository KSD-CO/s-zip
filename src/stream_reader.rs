//! Sequential ZIP reader for non-seekable sources (HTTP bodies, pipes, stdin).
//!
//! [`ZipStreamReader`] walks the archive front to back using local file
//! headers, so it only needs `AsyncRead` — no `AsyncSeek`, no temporary file.
//! The central directory at the end of the archive is never read.
//!
//! ```no_run
//! use s_zip::ZipStreamReader;
//! use tokio::io::AsyncReadExt;
//!
//! # async fn example(body: impl tokio::io::AsyncRead + Unpin + Send) -> Result<(), Box<dyn std::error::Error>> {
//! let mut zip = ZipStreamReader::new(body);
//! while let Some(entry) = zip.next_entry().await? {
//!     let mut data = Vec::new();
//!     zip.entry_reader().read_to_end(&mut data).await?;
//!     println!("{}: {} bytes", entry.name, data.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Limitations
//!
//! - Encrypted entries are not supported (use `GenericAsyncZipReader`).
//! - Stored (uncompressed) entries written with a data descriptor and no size
//!   in the local header cannot be delimited without the central directory and
//!   are rejected. DEFLATE and Zstd entries are self-delimiting and work in
//!   all cases.
//! - Entries are listed in archive order as found in local headers; entries
//!   that exist only in the central directory are not visible.
//! - CRC-32 and sizes are verified when the entry is fully consumed — by
//!   [`ZipStreamReader::next_entry`] or [`ZipStreamReader::read_entry_to_end`].

use crate::error::{Result, SZipError};
use crate::format::{
    decode_file_name, parse_zip64_extra_field, ZipEntry, CENTRAL_DIRECTORY_SIGNATURE,
    END_OF_CENTRAL_DIRECTORY_SIGNATURE, LOCAL_FILE_HEADER_SIGNATURE,
    ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE,
};
use crate::limits::{into_io, ExtractLimits, OutputBudget};
use async_compression::tokio::bufread::DeflateDecoder;
#[cfg(feature = "async-zstd")]
use async_compression::tokio::bufread::ZstdDecoder;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, BufReader, ReadBuf, Take};

const DATA_DESCRIPTOR_SIGNATURE: u32 = 0x08074b50;

/// Where the compressed bytes of the current entry are read from.
enum Body<R> {
    Idle(BufReader<R>),
    Stored(Take<BufReader<R>>),
    Deflate(DeflateDecoder<Take<BufReader<R>>>),
    #[cfg(feature = "async-zstd")]
    Zstd(ZstdDecoder<Take<BufReader<R>>>),
    /// Transient state while switching variants
    Empty,
}

impl<R: AsyncRead + Unpin> Body<R> {
    fn take_inner(&mut self) -> Take<BufReader<R>> {
        match std::mem::replace(self, Body::Empty) {
            Body::Stored(t) => t,
            Body::Deflate(d) => d.into_inner(),
            #[cfg(feature = "async-zstd")]
            Body::Zstd(d) => d.into_inner(),
            Body::Idle(_) | Body::Empty => unreachable!("no entry body"),
        }
    }

    /// Compressed bytes still allowed by the `Take` limit.
    fn remaining(&self) -> u64 {
        match self {
            Body::Stored(t) => t.limit(),
            Body::Deflate(d) => d.get_ref().limit(),
            #[cfg(feature = "async-zstd")]
            Body::Zstd(d) => d.get_ref().limit(),
            Body::Idle(_) | Body::Empty => 0,
        }
    }
}

/// State of the entry currently being read.
struct Current {
    entry: ZipEntry,
    /// Header declared sizes (false when deferred to a data descriptor)
    sizes_known: bool,
    has_descriptor: bool,
    zip64: bool,
    take_limit: u64,
    hasher: crc32fast::Hasher,
    produced: u64,
    budget: OutputBudget,
}

/// Sequential ZIP reader over any `AsyncRead` (no seeking required).
///
/// See the [module docs](self) for an example and limitations.
pub struct ZipStreamReader<R> {
    body: Body<R>,
    current: Option<Current>,
    limits: ExtractLimits,
    remaining_total: Option<u64>,
    entries_seen: usize,
    /// Bytes consumed from the source so far (used for `ZipEntry::offset`)
    position: u64,
    done: bool,
}

impl<R: AsyncRead + Unpin + Send> ZipStreamReader<R> {
    /// Create a reader over `reader`.
    pub fn new(reader: R) -> Self {
        Self {
            body: Body::Idle(BufReader::new(reader)),
            current: None,
            limits: ExtractLimits::default(),
            remaining_total: None,
            entries_seen: 0,
            position: 0,
            done: false,
        }
    }

    /// Set decompression limits (ZIP bomb protection). Call before the first
    /// `next_entry()`; `max_total_size` and `max_entries` count from there.
    pub fn set_limits(&mut self, limits: ExtractLimits) -> &mut Self {
        self.limits = limits;
        self.remaining_total = limits.max_total_size;
        self
    }

    /// Advance to the next entry, returning `None` at the end of the archive.
    ///
    /// Any unread data of the previous entry is drained and its CRC-32 and
    /// sizes are verified first.
    pub async fn next_entry(&mut self) -> Result<Option<ZipEntry>> {
        self.finish_current().await?;
        if self.done {
            return Ok(None);
        }

        let header_offset = self.position;
        let reader = self.idle_reader();
        let signature = match read_u32(reader).await {
            Ok(sig) => sig,
            // A stream that ends right after the last entry (no central directory)
            Err(SZipError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                self.done = true;
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        match signature {
            LOCAL_FILE_HEADER_SIGNATURE => {}
            CENTRAL_DIRECTORY_SIGNATURE
            | END_OF_CENTRAL_DIRECTORY_SIGNATURE
            | ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE => {
                self.done = true;
                return Ok(None);
            }
            other => {
                return Err(SZipError::InvalidFormat(format!(
                    "Unexpected signature 0x{:08x} at offset {}",
                    other, header_offset
                )))
            }
        }

        let mut fixed = [0u8; 26];
        reader.read_exact(&mut fixed).await?;
        let u16_at = |i: usize| u16::from_le_bytes([fixed[i], fixed[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(fixed[i..i + 4].try_into().unwrap());
        let flags = u16_at(2);
        let method = u16_at(4);
        let crc32 = u32_at(10);
        let compressed_32 = u32_at(14) as u64;
        let uncompressed_32 = u32_at(18) as u64;
        let name_len = u16_at(22) as usize;
        let extra_len = u16_at(24) as usize;

        let mut name_buf = vec![0u8; name_len];
        reader.read_exact(&mut name_buf).await?;
        let mut extra = vec![0u8; extra_len];
        reader.read_exact(&mut extra).await?;
        self.position += 30 + name_len as u64 + extra_len as u64;

        let name = decode_file_name(&name_buf, flags);
        if flags & 0x01 != 0 {
            return Err(SZipError::InvalidFormat(format!(
                "Entry '{}' is encrypted; ZipStreamReader does not support encryption",
                name
            )));
        }

        let zip64 = compressed_32 == 0xFFFFFFFF || uncompressed_32 == 0xFFFFFFFF;
        let (uncompressed_size, compressed_size, _) =
            parse_zip64_extra_field(&extra, compressed_32, uncompressed_32, 0);
        let has_descriptor = flags & 0x08 != 0;
        let sizes_known = !has_descriptor || compressed_size != 0;

        if !sizes_known && method == 0 {
            return Err(SZipError::InvalidFormat(format!(
                "Stored entry '{}' has no size in its local header; \
                 it cannot be read without the central directory",
                name
            )));
        }

        self.entries_seen += 1;
        self.limits.check_entry_count(self.entries_seen)?;

        let entry = ZipEntry {
            name,
            compressed_size,
            uncompressed_size,
            compression_method: method,
            offset: header_offset,
            crc32,
            is_encrypted: false,
        };
        let budget = self.limits.budget(&entry, false, self.remaining_total)?;

        let take_limit = if sizes_known {
            compressed_size
        } else {
            u64::MAX
        };
        let reader = match std::mem::replace(&mut self.body, Body::Empty) {
            Body::Idle(r) => r,
            _ => unreachable!("body is idle between entries"),
        };
        let take = reader.take(take_limit);
        self.body = match method {
            0 => Body::Stored(take),
            8 => Body::Deflate(DeflateDecoder::new(take)),
            #[cfg(feature = "async-zstd")]
            93 => Body::Zstd(ZstdDecoder::new(take)),
            other => {
                self.body = Body::Idle(take.into_inner());
                return Err(SZipError::UnsupportedCompression(other));
            }
        };

        self.current = Some(Current {
            entry: entry.clone(),
            sizes_known,
            has_descriptor,
            zip64,
            take_limit,
            hasher: crc32fast::Hasher::new(),
            produced: 0,
            budget,
        });
        Ok(Some(entry))
    }

    /// Reader over the decompressed data of the current entry.
    ///
    /// Returns EOF immediately if there is no current entry.
    pub fn entry_reader(&mut self) -> EntryReader<'_, R> {
        EntryReader { zip: self }
    }

    /// Read the rest of the current entry into memory and verify it.
    pub async fn read_entry_to_end(&mut self) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        self.entry_reader().read_to_end(&mut data).await?;
        self.finish_current().await?;
        Ok(data)
    }

    fn idle_reader(&mut self) -> &mut BufReader<R> {
        match &mut self.body {
            Body::Idle(r) => r,
            _ => unreachable!("body is idle between entries"),
        }
    }

    /// Drain the current entry, read its data descriptor and verify it.
    async fn finish_current(&mut self) -> Result<()> {
        if self.current.is_none() {
            return Ok(());
        }
        let mut sink = [0u8; 16 * 1024];
        while self.entry_reader().read(&mut sink).await? != 0 {}

        let cur = self.current.take().expect("checked above");
        let consumed = cur.take_limit - self.body.remaining();
        let take = self.body.take_inner();
        self.body = Body::Idle(take.into_inner());
        self.position += consumed;
        let name = &cur.entry.name;

        if cur.sizes_known && consumed != cur.entry.compressed_size {
            return Err(SZipError::InvalidFormat(format!(
                "Entry '{}' ended after {} of {} compressed bytes",
                name, consumed, cur.entry.compressed_size
            )));
        }

        let (expected_crc, expected_size) = if cur.has_descriptor {
            self.read_data_descriptor(&cur, consumed).await?
        } else {
            (cur.entry.crc32, cur.entry.uncompressed_size)
        };

        let actual_crc = cur.hasher.finalize();
        if actual_crc != expected_crc {
            return Err(SZipError::InvalidFormat(format!(
                "CRC-32 mismatch for '{}': expected {:#010x}, got {:#010x}",
                name, expected_crc, actual_crc
            )));
        }
        if cur.produced != expected_size {
            return Err(SZipError::InvalidFormat(format!(
                "Size mismatch for '{}': expected {} bytes, got {}",
                name, expected_size, cur.produced
            )));
        }
        if let Some(rem) = self.remaining_total.as_mut() {
            *rem = rem.saturating_sub(cur.produced);
        }
        Ok(())
    }

    /// Read the data descriptor that follows the entry data and return
    /// `(crc32, uncompressed_size)`. Sizes are 8 bytes for ZIP64 entries, or
    /// when either actual size does not fit in 32 bits.
    async fn read_data_descriptor(&mut self, cur: &Current, consumed: u64) -> Result<(u32, u64)> {
        let reader = self.idle_reader();
        let mut len = 4u64;
        let mut crc = read_u32(reader).await?;
        if crc == DATA_DESCRIPTOR_SIGNATURE {
            crc = read_u32(reader).await?;
            len += 4;
        }
        let wide = cur.zip64 || consumed > u32::MAX as u64 || cur.produced > u32::MAX as u64;
        let (compressed, uncompressed) = if wide {
            len += 16;
            (read_u64(reader).await?, read_u64(reader).await?)
        } else {
            len += 8;
            (
                read_u32(reader).await? as u64,
                read_u32(reader).await? as u64,
            )
        };
        self.position += len;

        if compressed != consumed {
            return Err(SZipError::InvalidFormat(format!(
                "Data descriptor of '{}' declares {} compressed bytes, found {}",
                cur.entry.name, compressed, consumed
            )));
        }
        Ok((crc, uncompressed))
    }

    fn poll_entry_read(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let Some(cur) = self.current.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let before = buf.filled().len();
        let poll = match &mut self.body {
            Body::Stored(r) => Pin::new(r).poll_read(cx, buf),
            Body::Deflate(r) => Pin::new(r).poll_read(cx, buf),
            #[cfg(feature = "async-zstd")]
            Body::Zstd(r) => Pin::new(r).poll_read(cx, buf),
            Body::Idle(_) | Body::Empty => Poll::Ready(Ok(())),
        };
        if let Poll::Ready(Ok(())) = poll {
            let new = &buf.filled()[before..];
            cur.hasher.update(new);
            cur.produced += new.len() as u64;
            let consumed = cur.take_limit - self.body.remaining();
            if let Err(e) = cur.budget.check_with_compressed(cur.produced, consumed) {
                // AsyncRead contract: nothing may be filled when returning an error
                buf.set_filled(before);
                return Poll::Ready(Err(into_io(e)));
            }
        }
        poll
    }
}

/// `AsyncRead` over the decompressed data of the current entry.
///
/// Created by [`ZipStreamReader::entry_reader`].
pub struct EntryReader<'a, R> {
    zip: &'a mut ZipStreamReader<R>,
}

impl<R: AsyncRead + Unpin + Send> AsyncRead for EntryReader<'_, R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.zip.poll_entry_read(cx, buf)
    }
}

async fn read_u32<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).await?;
    Ok(u32::from_le_bytes(buf))
}

async fn read_u64<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).await?;
    Ok(u64::from_le_bytes(buf))
}
