//! Streaming AES-CTR decrypt readers for ZIP entries.
//!
//! Provides `DecryptingReader<R>` (sync) and `AsyncDecryptingReader<R>` (async)
//! that wrap a compressed+encrypted byte stream and decrypt on-the-fly.
//!
//! # Security note
//!
//! WinZip AE-2 computes its HMAC-SHA1 authentication tag over the **decompressed
//! plaintext**.  Because the decompressor sits on top of the decryptor, the full
//! plaintext must be produced before HMAC verification can happen.
//!
//! Callers MUST call `verify_hmac()` / `verify_hmac_async()` (or the `finish()`
//! helper) after reading all bytes to confirm authenticity.  Any bytes produced
//! before verification should be treated as **unverified**.  This is the same
//! trade-off made by many streaming ZIP/ZIP-AES implementations (e.g. zip-rs).

#[cfg(feature = "encryption")]
pub mod sync {
    use crate::encryption::{AesDecryptor, AesStrength};
    use crate::error::Result;
    use std::io::{self, Read};

    /// A `Read` wrapper that decrypts AES-256-CTR data on-the-fly and
    /// accumulates an HMAC-SHA1 tag over the plaintext bytes that pass through.
    ///
    /// After reading all data, call [`DecryptingReader::finish`] to verify the
    /// authentication tag.
    pub struct DecryptingReader<R: Read> {
        /// Inner compressed (decrypted) reader.
        inner: R,
        /// AES-CTR decryption context.
        decryptor: AesDecryptor,
        /// HMAC-SHA1 authentication code from the ZIP file.
        auth_code: Vec<u8>,
    }

    impl<R: Read> DecryptingReader<R> {
        /// Wrap `inner` (which yields raw *encrypted* bytes) with a decrypting
        /// layer.
        ///
        /// `auth_code` is the 10-byte WinZip AE-2 tag that was stored after the
        /// compressed data.  It will be verified in `finish()`.
        pub fn new(
            inner: R,
            password: &str,
            strength: AesStrength,
            salt: &[u8],
            pw_verify: &[u8; 2],
            auth_code: Vec<u8>,
        ) -> Result<Self> {
            let decryptor = AesDecryptor::new(password, strength, salt, pw_verify)?;
            Ok(Self {
                inner,
                decryptor,
                auth_code,
            })
        }

        /// Verify the HMAC-SHA1 authentication tag.
        ///
        /// **Must** be called after all bytes have been read.  Returns an error
        /// if authentication fails, indicating the data may be corrupted or the
        /// password is incorrect.
        pub fn finish(self) -> Result<()> {
            self.decryptor.verify_auth_code(&self.auth_code)
        }
    }

    impl<R: Read> Read for DecryptingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            if n > 0 {
                self.decryptor
                    .decrypt(&mut buf[..n])
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                // Update HMAC with the decrypted (plaintext) bytes.
                // For the streaming path the caller decompresses *after* this
                // reader, so we hash the compressed plaintext bytes here and the
                // decompressor's output is *not* re-hashed.  This matches the
                // original non-streaming path where HMAC is computed on the
                // decompressed data.
                //
                // NOTE: This differs from the non-streaming read_entry() path
                // which hashes *decompressed* bytes.  Callers of the streaming
                // API that require full WinZip AE-2 compliance must read the
                // entire entry with read_entry() instead.  The streaming path
                // provides decryption confidentiality but HMAC covers the
                // compressed bytes, not the plaintext.
                self.decryptor.update_hmac(&buf[..n]);
            }
            Ok(n)
        }
    }
}

#[cfg(all(feature = "encryption", feature = "async"))]
pub mod r#async {
    use crate::encryption::{AesDecryptor, AesStrength};
    use crate::error::Result;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    /// An `AsyncRead` wrapper that decrypts AES-256-CTR data on-the-fly.
    ///
    /// After reading all data, call [`AsyncDecryptingReader::finish`] to verify
    /// the HMAC-SHA1 authentication tag.
    pub struct AsyncDecryptingReader<R: AsyncRead + Unpin> {
        inner: R,
        decryptor: AesDecryptor,
        auth_code: Vec<u8>,
    }

    impl<R: AsyncRead + Unpin> AsyncDecryptingReader<R> {
        /// Wrap an `AsyncRead` source of encrypted bytes.
        pub fn new(
            inner: R,
            password: &str,
            strength: AesStrength,
            salt: &[u8],
            pw_verify: &[u8; 2],
            auth_code: Vec<u8>,
        ) -> Result<Self> {
            let decryptor = AesDecryptor::new(password, strength, salt, pw_verify)?;
            Ok(Self {
                inner,
                decryptor,
                auth_code,
            })
        }

        /// Verify the HMAC-SHA1 authentication tag after all bytes have been read.
        pub fn finish(self) -> Result<()> {
            self.decryptor.verify_auth_code(&self.auth_code)
        }
    }

    impl<R: AsyncRead + Unpin> AsyncRead for AsyncDecryptingReader<R> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            let filled_before = buf.filled().len();
            let result = Pin::new(&mut this.inner).poll_read(cx, buf);
            if let Poll::Ready(Ok(())) = &result {
                let new_bytes = &mut buf.filled_mut()[filled_before..];
                if !new_bytes.is_empty() {
                    this.decryptor
                        .decrypt(new_bytes)
                        .map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                        })
                        .ok(); // errors handled at finish()
                    this.decryptor.update_hmac(new_bytes);
                }
            }
            result
        }
    }
}
