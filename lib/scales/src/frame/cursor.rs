//! Lightweight SCALE binary cursor for no_std decoding.
//!
//! Reads SCALE primitives (compact integers, strings, vecs) from a byte slice
//! without allocating — only advancing a position. Used by [`decode_meta`](super::decode_meta).

use alloc::string::String;

use crate::Error;

/// A zero-copy cursor over a SCALE-encoded byte slice.
pub(super) struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn read_byte(&mut self) -> Result<u8, Error> {
        if self.pos >= self.data.len() {
            return Err(Error::BadInput("unexpected end of input".into()));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.pos + n > self.data.len() {
            return Err(Error::BadInput("unexpected end of input".into()));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    pub fn read_u32_le(&mut self) -> Result<u32, Error> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn read_compact_u32(&mut self) -> Result<u32, Error> {
        let b = self.read_byte()?;
        let mode = b & 0x03;
        match mode {
            0 => Ok(u32::from(b) >> 2),
            1 => {
                let b2 = self.read_byte()?;
                Ok(u32::from(u16::from_le_bytes([b, b2])) >> 2)
            }
            2 => {
                let rest = self.read_bytes(3)?;
                let val = u32::from_le_bytes([b, rest[0], rest[1], rest[2]]);
                Ok(val >> 2)
            }
            _ => {
                let n = ((b >> 2) + 4) as usize;
                let bytes = self.read_bytes(n)?;
                let mut buf = [0u8; 4];
                let len = n.min(4);
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(u32::from_le_bytes(buf))
            }
        }
    }

    pub fn read_string(&mut self) -> Result<String, Error> {
        let len = self.read_compact_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        core::str::from_utf8(bytes)
            .map(|s| s.into())
            .map_err(|_| Error::BadInput("invalid utf8 in metadata".into()))
    }

    pub fn skip_string(&mut self) -> Result<(), Error> {
        let len = self.read_compact_u32()? as usize;
        self.read_bytes(len)?;
        Ok(())
    }

    pub fn skip_vec_string(&mut self) -> Result<(), Error> {
        let count = self.read_compact_u32()?;
        for _ in 0..count {
            self.skip_string()?;
        }
        Ok(())
    }

    pub fn skip_option_compact_u32(&mut self) -> Result<(), Error> {
        if self.read_byte()? != 0 {
            self.read_compact_u32()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_single_byte() {
        // 0 in compact = 0x00
        let mut c = Cursor::new(&[0x00]);
        assert_eq!(c.read_compact_u32().unwrap(), 0);

        // 1 in compact = 0x04
        let mut c = Cursor::new(&[0x04]);
        assert_eq!(c.read_compact_u32().unwrap(), 1);

        // 63 in compact = 0xFC
        let mut c = Cursor::new(&[0xFC]);
        assert_eq!(c.read_compact_u32().unwrap(), 63);
    }

    #[test]
    fn compact_two_byte() {
        // 64 in compact = 0x01 0x01
        let mut c = Cursor::new(&[0x01, 0x01]);
        assert_eq!(c.read_compact_u32().unwrap(), 64);

        // 16383 in compact = 0xFD 0xFF
        let mut c = Cursor::new(&[0xFD, 0xFF]);
        assert_eq!(c.read_compact_u32().unwrap(), 16383);
    }

    #[test]
    fn compact_four_byte() {
        // 16384 in compact = 0x02 0x00 0x01 0x00
        let mut c = Cursor::new(&[0x02, 0x00, 0x01, 0x00]);
        assert_eq!(c.read_compact_u32().unwrap(), 16384);
    }

    #[test]
    fn read_string_basic() {
        // "hi" = compact(2) + b"hi"
        let mut c = Cursor::new(&[0x08, b'h', b'i']);
        assert_eq!(c.read_string().unwrap(), "hi");
    }

    #[test]
    fn skip_string_basic() {
        let mut c = Cursor::new(&[0x08, b'h', b'i', 0xFF]);
        c.skip_string().unwrap();
        assert_eq!(c.pos, 3);
        assert_eq!(c.read_byte().unwrap(), 0xFF);
    }

    #[test]
    fn skip_vec_string() {
        // Vec of 2 strings: "a", "bc"
        let mut c = Cursor::new(&[
            0x08, // compact(2) — count
            0x04, b'a', // compact(1) + "a"
            0x08, b'b', b'c', // compact(2) + "bc"
            0xFF,
        ]);
        c.skip_vec_string().unwrap();
        assert_eq!(c.read_byte().unwrap(), 0xFF);
    }

    #[test]
    fn eof_error() {
        let mut c = Cursor::new(&[]);
        assert!(c.read_byte().is_err());
    }
}
