//! Async buffered cursor for streaming SCALE decode.
//!
//! Same API as [`Cursor`](super::cursor::Cursor) but reads from an
//! `embedded_io_async::Read` source, buffering internally. This enables
//! parsing metadata from a network stream without holding the full blob.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use embedded_io_async::Read;

use crate::Error;

const BUF_SIZE: usize = 1024;

/// Async streaming cursor over SCALE-encoded data.
///
/// Reads from any `embedded_io_async::Read` source, keeping a small
/// internal buffer. Methods mirror [`Cursor`](super::cursor::Cursor).
pub struct StreamCursor<R> {
    reader: R,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
}

impl<R: Read> StreamCursor<R> {
    pub fn new(reader: R) -> Self {
        let buf = vec![0u8; BUF_SIZE];
        Self {
            reader,
            buf,
            pos: 0,
            len: 0,
        }
    }

    /// Ensure at least `n` bytes are available in the buffer.
    async fn fill(&mut self, n: usize) -> Result<(), Error> {
        let available = self.len - self.pos;
        if available >= n {
            return Ok(());
        }

        // Shift remaining data to the front
        if self.pos > 0 {
            self.buf.copy_within(self.pos..self.len, 0);
            self.len = available;
            self.pos = 0;
        }

        // Grow buffer if needed
        if n > self.buf.len() {
            self.buf.resize(n, 0);
        }

        // Read until we have enough
        while self.len < n {
            let read = self
                .reader
                .read(&mut self.buf[self.len..])
                .await
                .map_err(|_| Error::BadInput("stream read error".into()))?;
            if read == 0 {
                return Err(Error::BadInput("unexpected end of stream".into()));
            }
            self.len += read;
        }
        Ok(())
    }

    pub async fn read_byte(&mut self) -> Result<u8, Error> {
        self.fill(1).await?;
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub async fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, Error> {
        self.fill(n).await?;
        let data = self.buf[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(data)
    }

    pub async fn skip_bytes(&mut self, mut n: usize) -> Result<(), Error> {
        while n > 0 {
            let available = self.len - self.pos;
            if available >= n {
                self.pos += n;
                return Ok(());
            }
            // Skip what's buffered, read more
            n -= available;
            self.pos = 0;
            self.len = 0;
            self.fill(n.min(BUF_SIZE)).await?;
        }
        Ok(())
    }

    pub async fn read_u32_le(&mut self) -> Result<u32, Error> {
        self.fill(4).await?;
        let b = &self.buf[self.pos..self.pos + 4];
        let val = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        self.pos += 4;
        Ok(val)
    }

    pub async fn read_compact_u32(&mut self) -> Result<u32, Error> {
        let b = self.read_byte().await?;
        let mode = b & 0x03;
        match mode {
            0 => Ok(u32::from(b) >> 2),
            1 => {
                let b2 = self.read_byte().await?;
                Ok(u32::from(u16::from_le_bytes([b, b2])) >> 2)
            }
            2 => {
                self.fill(3).await?;
                let rest = &self.buf[self.pos..self.pos + 3];
                let val = u32::from_le_bytes([b, rest[0], rest[1], rest[2]]);
                self.pos += 3;
                Ok(val >> 2)
            }
            _ => {
                let n = ((b >> 2) + 4) as usize;
                let bytes = self.read_bytes(n).await?;
                let mut buf = [0u8; 4];
                let len = n.min(4);
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(u32::from_le_bytes(buf))
            }
        }
    }

    pub async fn read_string(&mut self) -> Result<String, Error> {
        let len = self.read_compact_u32().await? as usize;
        let bytes = self.read_bytes(len).await?;
        core::str::from_utf8(&bytes)
            .map(|s| s.into())
            .map_err(|_| Error::BadInput("invalid utf8 in metadata".into()))
    }

    pub async fn skip_string(&mut self) -> Result<(), Error> {
        let len = self.read_compact_u32().await? as usize;
        self.skip_bytes(len).await
    }

    pub async fn skip_vec_string(&mut self) -> Result<(), Error> {
        let count = self.read_compact_u32().await?;
        for _ in 0..count {
            self.skip_string().await?;
        }
        Ok(())
    }

    pub async fn skip_option_compact_u32(&mut self) -> Result<(), Error> {
        if self.read_byte().await? != 0 {
            self.read_compact_u32().await?;
        }
        Ok(())
    }
}
