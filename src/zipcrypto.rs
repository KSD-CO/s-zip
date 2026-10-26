//! Traditional PKWARE encryption ("ZipCrypto"), read-only.
//!
//! ZipCrypto is cryptographically broken and s-zip never writes it; it is
//! decoded only so that legacy archives (e.g. created with `zip -P`) can be
//! opened. Prefer AES for anything new.

use crate::error::{Result, SZipError};
use std::io::{self, Read};

/// Size of the encryption header that precedes the encrypted data.
pub const HEADER_LEN: u64 = 12;

/// ZipCrypto key state.
#[derive(Clone)]
pub(crate) struct Keys {
    k0: u32,
    k1: u32,
    k2: u32,
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc32_update(crc: u32, b: u8) -> u32 {
    CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8)
}

impl Keys {
    fn new(password: &[u8]) -> Self {
        let mut keys = Keys {
            k0: 0x1234_5678,
            k1: 0x2345_6789,
            k2: 0x3456_7890,
        };
        for &b in password {
            keys.update(b);
        }
        keys
    }

    fn update(&mut self, b: u8) {
        self.k0 = crc32_update(self.k0, b);
        self.k1 = self
            .k1
            .wrapping_add(self.k0 & 0xFF)
            .wrapping_mul(134_775_813)
            .wrapping_add(1);
        self.k2 = crc32_update(self.k2, (self.k1 >> 24) as u8);
    }

    fn decrypt_byte(&mut self, c: u8) -> u8 {
        let t = (self.k2 | 2) as u16;
        let p = c ^ (t.wrapping_mul(t ^ 1) >> 8) as u8;
        self.update(p);
        p
    }

    pub(crate) fn decrypt(&mut self, data: &mut [u8]) {
        for b in data {
            *b = self.decrypt_byte(*b);
        }
    }

    /// Initialise keys from `password` and consume the 12-byte encryption
    /// header. `check_byte` is the high byte of the CRC-32, or of the DOS
    /// modification time when the entry uses a data descriptor.
    pub(crate) fn from_header(password: &str, header: &[u8; 12], check_byte: u8) -> Result<Self> {
        let mut keys = Keys::new(password.as_bytes());
        let mut header = *header;
        keys.decrypt(&mut header);
        if header[11] != check_byte {
            return Err(SZipError::IncorrectPassword);
        }
        Ok(keys)
    }
}

/// The byte the last decrypted header byte must match.
pub(crate) fn check_byte(flags: u16, crc32: u32, mod_time: u16) -> u8 {
    if flags & 0x08 != 0 {
        (mod_time >> 8) as u8
    } else {
        (crc32 >> 24) as u8
    }
}

/// `Read` adapter that decrypts ZipCrypto data on the fly.
pub(crate) struct ZipCryptoReader<R> {
    inner: R,
    keys: Keys,
}

impl<R: Read> ZipCryptoReader<R> {
    /// Read the encryption header from `inner` and verify the password.
    pub(crate) fn new(mut inner: R, password: &str, check_byte: u8) -> Result<Self> {
        let mut header = [0u8; 12];
        inner.read_exact(&mut header)?;
        let keys = Keys::from_header(password, &header, check_byte)?;
        Ok(Self { inner, keys })
    }
}

impl<R: Read> Read for ZipCryptoReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.keys.decrypt(&mut buf[..n]);
        Ok(n)
    }
}

#[cfg(feature = "async")]
pub(crate) mod nonblocking {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

    /// `AsyncRead` adapter that decrypts ZipCrypto data on the fly.
    pub(crate) struct ZipCryptoReader<R> {
        inner: R,
        keys: Keys,
    }

    impl<R: AsyncRead + Unpin> ZipCryptoReader<R> {
        pub(crate) async fn new(mut inner: R, password: &str, check_byte: u8) -> Result<Self> {
            let mut header = [0u8; 12];
            inner.read_exact(&mut header).await?;
            let keys = Keys::from_header(password, &header, check_byte)?;
            Ok(Self { inner, keys })
        }
    }

    impl<R: AsyncRead + Unpin> AsyncRead for ZipCryptoReader<R> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let before = buf.filled().len();
            let this = &mut *self;
            let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
            if let Poll::Ready(Ok(())) = poll {
                this.keys.decrypt(&mut buf.filled_mut()[before..]);
            }
            poll
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc_table_matches_crc32fast() {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in b"123456789" {
            crc = crc32_update(crc, b);
        }
        assert_eq!(!crc, crc32fast::hash(b"123456789"));
    }

    #[test]
    fn test_wrong_password_rejected() {
        // Encrypt a header with one password, try to open it with another
        let mut enc = Keys::new(b"right");
        let plain_header = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0xAB];
        let mut cipher = [0u8; 12];
        for (c, &p) in cipher.iter_mut().zip(&plain_header) {
            let t = (enc.k2 | 2) as u16;
            *c = p ^ (t.wrapping_mul(t ^ 1) >> 8) as u8;
            enc.update(p);
        }
        assert!(Keys::from_header("right", &cipher, 0xAB).is_ok());
        assert!(matches!(
            Keys::from_header("wrong", &cipher, 0xAB),
            Err(SZipError::IncorrectPassword)
        ));
    }
}
