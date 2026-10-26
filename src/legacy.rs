//! Read support for legacy ZIP compression methods (`legacy-codecs` feature):
//! bzip2 (12), LZMA (14) and XZ (95).
//!
//! These are only ever decoded, never written.

use crate::error::{Result, SZipError};
use std::io::{self, Read};

/// ZIP compression method: bzip2
pub const METHOD_BZIP2: u16 = 12;
/// ZIP compression method: LZMA (LZMA1 with a ZIP-specific properties header)
pub const METHOD_LZMA: u16 = 14;
/// ZIP compression method: XZ
pub const METHOD_XZ: u16 = 95;

/// General purpose bit 1 for LZMA entries: the stream ends with an end-of-stream marker.
const FLAG_LZMA_EOS: u16 = 0x02;

/// Whether `method` is one of the legacy codecs handled here.
pub fn is_legacy_method(method: u16) -> bool {
    matches!(method, METHOD_BZIP2 | METHOD_LZMA | METHOD_XZ)
}

/// Read the ZIP LZMA header (version, properties size, properties) from the
/// start of the compressed data and return the equivalent 13-byte `.lzma`
/// ("LZMA alone") header that liblzma's decoder understands.
pub(crate) fn lzma_alone_header(
    zip_header: &[u8; 9],
    flags: u16,
    uncompressed: u64,
) -> Result<[u8; 13]> {
    let props_len = u16::from_le_bytes([zip_header[2], zip_header[3]]);
    if props_len != 5 {
        return Err(SZipError::InvalidFormat(format!(
            "Unsupported LZMA properties size: {}",
            props_len
        )));
    }
    let mut header = [0u8; 13];
    header[..5].copy_from_slice(&zip_header[4..9]);
    // With an end-of-stream marker the size is "unknown" to the decoder
    let size = if flags & FLAG_LZMA_EOS != 0 {
        u64::MAX
    } else {
        uncompressed
    };
    header[5..].copy_from_slice(&size.to_le_bytes());
    Ok(header)
}

/// Wrap `inner` (the entry's compressed bytes) in a decoder for a legacy method.
///
/// `flags` are the entry's general purpose flags and `uncompressed` its
/// uncompressed size (both needed for LZMA).
pub(crate) fn decoder<'a, R: Read + Send + 'a>(
    method: u16,
    flags: u16,
    uncompressed: u64,
    mut inner: R,
) -> Result<Box<dyn Read + Send + 'a>> {
    match method {
        METHOD_BZIP2 => Ok(Box::new(bzip2::read::BzDecoder::new(inner))),
        METHOD_XZ => Ok(Box::new(liblzma::read::XzDecoder::new(inner))),
        METHOD_LZMA => {
            let mut zip_header = [0u8; 9];
            inner.read_exact(&mut zip_header)?;
            let header = lzma_alone_header(&zip_header, flags, uncompressed)?;
            let stream = liblzma::stream::Stream::new_lzma_decoder(u64::MAX)
                .map_err(|e| SZipError::Io(io::Error::other(e)))?;
            Ok(Box::new(liblzma::read::XzDecoder::new_stream(
                io::Cursor::new(header).chain(inner),
                stream,
            )))
        }
        other => Err(SZipError::UnsupportedCompression(other)),
    }
}

#[cfg(feature = "async")]
pub(crate) mod nonblocking {
    use super::*;
    use async_compression::tokio::bufread::{BzDecoder, LzmaDecoder, XzDecoder};
    use tokio::io::{AsyncRead, AsyncReadExt, BufReader};

    /// Async counterpart of [`decoder`](super::decoder).
    pub(crate) async fn decoder<'a, R: AsyncRead + Unpin + Send + 'a>(
        method: u16,
        flags: u16,
        uncompressed: u64,
        mut inner: R,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send + 'a>> {
        match method {
            METHOD_BZIP2 => Ok(Box::new(BzDecoder::new(BufReader::new(inner)))),
            METHOD_XZ => Ok(Box::new(XzDecoder::new(BufReader::new(inner)))),
            METHOD_LZMA => {
                let mut zip_header = [0u8; 9];
                inner.read_exact(&mut zip_header).await?;
                let header = lzma_alone_header(&zip_header, flags, uncompressed)?;
                let chained = AsyncReadExt::chain(std::io::Cursor::new(header), inner);
                Ok(Box::new(LzmaDecoder::new(BufReader::new(chained))))
            }
            other => Err(SZipError::UnsupportedCompression(other)),
        }
    }
}
